//! claude-switch — manage and switch between multiple Claude Code accounts.

mod claude;
mod commands;
mod config;
mod index;
mod keychain;
mod paths;
mod picker;
mod safety;
mod swap;

use anyhow::Result;
use clap::Parser;

/// Switch between multiple Claude Code accounts and launch Claude as one.
///
/// Usage:
///   claude-switch                    pick an account, switch, and launch
///   claude-switch <label> [args...]  switch to <label> and launch (args go to claude)
///   claude-switch use <label>        switch without launching
///   claude-switch add                add & authenticate a new account
///   claude-switch remove <label>     forget an account locally (keeps its session)
///   claude-switch logout <label>     revoke an account server-side and forget it
///   claude-switch list | ls          show saved accounts
///   claude-switch current            print the active account
#[derive(Parser, Debug)]
#[command(name = "claude-switch", version, verbatim_doc_comment)]
struct Cli {
    /// Switch the account but do not launch Claude.
    #[arg(long, global = true)]
    no_launch: bool,

    /// A label/subcommand followed by any args to pass through to `claude`.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    rest: Vec<String>,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    // Detect & clean up any interrupted prior swap before doing anything.
    safety::reconcile()?;

    let launch = !cli.no_launch;
    let (head, tail) = cli
        .rest
        .split_first()
        .map(|(h, t)| (Some(h.as_str()), t))
        .unwrap_or((None, &[]));

    match head {
        None => commands::pick_and_launch(&[], launch),
        Some("add") => commands::add(),
        Some("remove") | Some("rm") => {
            let label = require_label(tail, "remove")?;
            commands::remove(&label)
        }
        Some("logout") => {
            let label = require_label(tail, "logout")?;
            commands::logout(&label)
        }
        Some("list") | Some("ls") => commands::list(),
        Some("current") => commands::current(),
        Some("use") => {
            let label = require_label(tail, "use")?;
            commands::switch_and_launch(&label, &[], false)
        }
        // Anything else is a label to switch to; the remainder goes to claude.
        Some(label) => commands::switch_and_launch(label, tail, launch),
    }
}

fn require_label(tail: &[String], verb: &str) -> Result<String> {
    tail.first().cloned().ok_or_else(|| {
        anyhow::anyhow!("`{verb}` needs an account label, e.g. `claude-switch {verb} personal`")
    })
}
