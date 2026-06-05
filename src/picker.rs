//! Interactive prompts. Every choice explains itself inline (DESIGN §9).
//! Arrow-key UI on a TTY; numbered-list fallback otherwise.

use anyhow::{bail, Context, Result};
use std::fmt;
use std::io::{self, IsTerminal, Write};

use inquire::{Confirm, Select, Text};

use crate::index::{Account, AccountType, Index};

/// One row in the account picker.
struct Row<'a> {
    account: &'a Account,
    active_uuid: Option<&'a str>,
}

impl fmt::Display for Row<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let active = self.account.uuid().is_some() && self.account.uuid() == self.active_uuid;
        let marker = if active { "● " } else { "  " };
        let email = self.account.email().unwrap_or("(unknown email)");
        write!(
            f,
            "{marker}{label}  {email} [{badge}]",
            label = self.account.label,
            badge = self.account.account_type.badge(),
        )
    }
}

/// Pick an account to switch to. `active_uuid` marks the current one.
/// Returns the chosen label, or `None` if the user aborts.
pub fn select_account(index: &Index, active_uuid: Option<&str>) -> Result<Option<String>> {
    if index.accounts.is_empty() {
        bail!("no saved accounts yet — run `claude-switch add` to add one");
    }
    if index.accounts.len() == 1 {
        return Ok(Some(index.accounts[0].label.clone()));
    }

    if !io::stdin().is_terminal() {
        return select_numbered(index);
    }

    let rows: Vec<Row> = index
        .accounts
        .iter()
        .map(|account| Row {
            account,
            active_uuid,
        })
        .collect();

    match Select::new("Switch to which account?", rows)
        .with_help_message("↑↓ to move, enter to select, esc to cancel")
        .prompt()
    {
        Ok(row) => Ok(Some(row.account.label.clone())),
        Err(inquire::InquireError::OperationCanceled)
        | Err(inquire::InquireError::OperationInterrupted) => Ok(None),
        Err(e) => Err(e).context("account picker failed"),
    }
}

/// Numbered-list fallback for non-interactive stdin.
fn select_numbered(index: &Index) -> Result<Option<String>> {
    let mut out = io::stdout();
    writeln!(out, "Saved accounts:")?;
    for (i, a) in index.accounts.iter().enumerate() {
        writeln!(
            out,
            "  {}) {}  {} [{}]",
            i + 1,
            a.label,
            a.email().unwrap_or("(unknown)"),
            a.account_type.badge()
        )?;
    }
    write!(out, "Select a number: ")?;
    out.flush()?;

    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    let n: usize = line.trim().parse().context("expected a number")?;
    let acct = index
        .accounts
        .get(n.checked_sub(1).context("selection out of range")?)
        .context("selection out of range")?;
    Ok(Some(acct.label.clone()))
}

/// Account-type prompt with inline explanations (DESIGN §9/§13).
pub fn select_account_type() -> Result<AccountType> {
    let options = vec![
        TypeChoice {
            ty: AccountType::Sub,
            text: "Claude subscription  — personal/Pro/Max plan, billed via your Claude subscription",
        },
        TypeChoice {
            ty: AccountType::Console,
            text: "Anthropic Console     — API usage billing (console.anthropic.com), pay-per-token",
        },
        TypeChoice {
            ty: AccountType::Sso,
            text: "SSO                   — company single sign-on (Google/Okta/etc.)",
        },
    ];
    let choice = Select::new("What kind of account is this?", options)
        .prompt()
        .context("account-type prompt failed")?;
    Ok(choice.ty)
}

struct TypeChoice {
    ty: AccountType,
    text: &'static str,
}

impl fmt::Display for TypeChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text)
    }
}

/// Prompt for the email to pre-fill into `claude auth login`.
pub fn prompt_email() -> Result<String> {
    let email = Text::new("Email for this account:")
        .with_help_message("used to pre-fill the Claude login")
        .prompt()
        .context("email prompt failed")?;
    let email = email.trim().to_string();
    if email.is_empty() {
        bail!("an email is required");
    }
    Ok(email)
}

/// Prompt for a friendly label, defaulting to a suggestion.
pub fn prompt_label(default: &str) -> Result<String> {
    let label = Text::new("Label for this account:")
        .with_default(default)
        .with_help_message("short name you'll type to switch, e.g. `personal` or `company`")
        .prompt()
        .context("label prompt failed")?;
    let label = label.trim().to_string();
    if label.is_empty() {
        bail!("a label is required");
    }
    Ok(label)
}

/// Destructive-action confirmation naming the account.
pub fn confirm(message: &str) -> Result<bool> {
    if !io::stdin().is_terminal() {
        // Refuse to assume "yes" for a destructive op without a TTY.
        bail!("refusing a destructive action without an interactive confirmation");
    }
    Confirm::new(message)
        .with_default(false)
        .prompt()
        .context("confirmation prompt failed")
}
