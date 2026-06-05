//! Command handlers — one per verb in the grammar (DESIGN §9).

use anyhow::{bail, Context, Result};
use chrono::Utc;
use std::os::unix::process::CommandExt;
use std::process::Command;

use crate::{
    claude, config, keychain, paths, picker,
    index::{Account, Index},
    safety::{Backup, Lock},
    swap,
};

/// Read the currently active account UUID from `.claude.json`, if any.
fn active_uuid() -> Result<Option<String>> {
    Ok(config::read_oauth_account()?
        .and_then(|o| o.get("accountUuid").and_then(|v| v.as_str()).map(str::to_owned)))
}

/// `claude-switch <label> [...args]` and the bare picker both land here.
pub fn switch_and_launch(label: &str, passthrough: &[String], launch: bool) -> Result<()> {
    let _lock = Lock::acquire()?;
    let mut index = Index::load()?;

    let outcome = swap::swap_to(&mut index, label)?;
    if outcome.already_active {
        eprintln!("✓ '{label}' is already the active account.");
    } else {
        match &outcome.outgoing_label {
            Some(prev) => eprintln!("✓ Switched from '{prev}' to '{label}'."),
            None => eprintln!("✓ Switched to '{label}'."),
        }
    }

    drop(_lock);
    if launch {
        exec_claude(passthrough)?; // replaces this process; only returns on error
    }
    Ok(())
}

/// Bare invocation: pick an account, then switch (+ launch).
pub fn pick_and_launch(passthrough: &[String], launch: bool) -> Result<()> {
    let index = Index::load()?;
    let active = active_uuid()?;
    match picker::select_account(&index, active.as_deref())? {
        Some(label) => switch_and_launch(&label, passthrough, launch),
        None => {
            eprintln!("Cancelled.");
            Ok(())
        }
    }
}

/// `claude-switch add` — orchestrate a real `claude auth login` (DESIGN §5).
pub fn add() -> Result<()> {
    let _lock = Lock::acquire()?;
    let mut index = Index::load()?;

    // 1. Preserve the current live account into its vault entry.
    swap::sync_live(&mut index)?;

    // Back up the live slot, then clear it so Claude prompts a fresh login.
    if let Some(blob) = keychain::read(paths::LIVE_SERVICE)? {
        Backup::capture(&blob)?;
    }
    keychain::delete(paths::LIVE_SERVICE)?;
    config::clear_oauth_account()?;

    // 2. Gather type + email, then run the real login.
    let account_type = picker::select_account_type()?;
    let email = picker::prompt_email()?;

    eprintln!(
        "\nℹ  Claude will now open its login flow. macOS may ask permission to use \
         the Keychain — choose \"Always Allow\".\n"
    );
    if let Err(e) = claude::auth_login(account_type, &email) {
        // Login failed: restore the prior live account from backup.
        restore_backup();
        return Err(e);
    }

    // 3. Capture the freshly-minted credential + identity.
    let blob = keychain::read(paths::LIVE_SERVICE)?
        .context("login completed but no credential landed in the Keychain")?;
    let oauth = config::read_oauth_account()?
        .context("login completed but ~/.claude.json has no oauthAccount")?;

    // 4. Label and vault it.
    let default_label = oauth
        .get("emailAddress")
        .and_then(|v| v.as_str())
        .and_then(|e| e.split('@').next())
        .unwrap_or("account")
        .to_string();
    let label = picker::prompt_label(&default_label)?;
    if index.find(&label).is_some() {
        bail!("an account labeled '{label}' already exists");
    }

    keychain::write(&paths::vault_service(&label), &blob)
        .context("saving the new credential to the vault")?;

    index.insert(Account {
        label: label.clone(),
        account_type,
        added_at: Utc::now(),
        last_used_at: Some(Utc::now()),
        oauth_account: oauth,
    })?;
    index.save()?;
    Backup::clear()?;

    eprintln!("✓ Added '{label}' and switched to it.");
    Ok(())
}

/// `claude-switch remove <label>` — safe, local forget (DESIGN §7).
pub fn remove(label: &str) -> Result<()> {
    let _lock = Lock::acquire()?;
    let mut index = Index::load()?;

    let acct = index
        .find(label)
        .with_context(|| format!("no account labeled '{label}'"))?
        .clone();
    guard_not_active(&acct)?;

    keychain::delete(&paths::vault_service(label))?;
    index.remove(label);
    index.save()?;
    eprintln!("✓ Removed '{label}' from claude-switch. Its server-side session is still valid.");
    Ok(())
}

/// `claude-switch logout <label>` — switch in, revoke server-side, remove (§7).
pub fn logout(label: &str) -> Result<()> {
    let _lock = Lock::acquire()?;
    let mut index = Index::load()?;

    let acct = index
        .find(label)
        .with_context(|| format!("no account labeled '{label}'"))?
        .clone();

    let email = acct.email().unwrap_or("(unknown email)");
    let proceed = picker::confirm(&format!(
        "Log out '{label}' ({email})? This revokes its session server-side and removes it locally."
    ))?;
    if !proceed {
        eprintln!("Cancelled.");
        return Ok(());
    }

    // Make it the active account so `claude auth logout` revokes the right one.
    swap::swap_to(&mut index, label)?;
    claude::auth_logout()?;

    keychain::delete(&paths::vault_service(label))?;
    keychain::delete(paths::LIVE_SERVICE)?;
    config::clear_oauth_account()?;
    index.remove(label);
    index.save()?;

    eprintln!("✓ Logged out and removed '{label}'.");
    Ok(())
}

/// `claude-switch list` — print accounts with the active marker.
pub fn list() -> Result<()> {
    let index = Index::load()?;
    if index.accounts.is_empty() {
        println!("No saved accounts. Run `claude-switch add` to add one.");
        return Ok(());
    }
    let active = active_uuid()?;
    for a in &index.accounts {
        let marker = if a.uuid().is_some() && a.uuid().map(str::to_owned) == active {
            "●"
        } else {
            " "
        };
        let org = a.organization().map(|o| format!(" ({o})")).unwrap_or_default();
        println!(
            "{marker} {label:<16} {email}{org} [{badge}]",
            label = a.label,
            email = a.email().unwrap_or("(unknown email)"),
            badge = a.account_type.badge(),
        );
    }
    Ok(())
}

/// `claude-switch current` — print just the active account (for prompts/scripts).
pub fn current() -> Result<()> {
    let index = Index::load()?;
    let active = active_uuid()?;
    match active.as_deref().and_then(|u| index.find_by_uuid(u)) {
        Some(a) => {
            println!("{} ({})", a.label, a.email().unwrap_or("unknown"));
            Ok(())
        }
        None => match config::read_oauth_account()? {
            Some(o) => {
                let email = o
                    .get("emailAddress")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                println!("(unmanaged) {email}");
                Ok(())
            }
            None => {
                println!("(logged out)");
                Ok(())
            }
        },
    }
}

/// Refuse to remove/logout the live account without switching away first.
fn guard_not_active(acct: &Account) -> Result<()> {
    let active = active_uuid()?;
    if acct.uuid().is_some() && acct.uuid().map(str::to_owned) == active {
        bail!(
            "'{}' is the active account — switch to another account first, then remove it.",
            acct.label
        );
    }
    Ok(())
}

/// Restore the pre-operation live credential from the backup entry.
fn restore_backup() {
    if let Ok(Some(blob)) = keychain::read(paths::BACKUP_SERVICE) {
        let _ = keychain::write(paths::LIVE_SERVICE, &blob);
        let _ = Backup::clear();
    }
}

/// Replace this process with `claude`, forwarding passthrough args verbatim.
fn exec_claude(passthrough: &[String]) -> Result<()> {
    let err = Command::new("claude").args(passthrough).exec();
    Err(err).context("failed to exec `claude` (is it on your PATH?)")
}
