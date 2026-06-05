//! The swap engine — the heart of claude-switch.
//!
//! A switch is an atomic **paired write**: the Keychain token blob *and*
//! `.claude.json`'s `oauthAccount` move together (DESIGN §15). Ordering is
//! chosen so an interruption is always recoverable from the pre-swap backup.

use anyhow::{bail, Context, Result};
use chrono::Utc;

use crate::{claude, config, index::Index, keychain, paths, safety::Backup};

/// Result of a swap, for the caller's messaging.
pub struct SwapOutcome {
    pub already_active: bool,
    pub outgoing_label: Option<String>,
}

/// Make `target_label` the live account, syncing the outgoing account first.
/// Assumes the caller holds the process lock.
pub fn swap_to(index: &mut Index, target_label: &str) -> Result<SwapOutcome> {
    // Target must be a known, vaulted account.
    let target = index
        .find(target_label)
        .with_context(|| format!("no saved account labeled '{target_label}'"))?
        .clone();
    let target_uuid = target.uuid().map(str::to_owned);

    let live_blob = keychain::read(paths::LIVE_SERVICE)
        .context("reading the live credential from the Keychain")?;
    let live_oauth = config::read_oauth_account()?;
    let live_uuid = live_oauth
        .as_ref()
        .and_then(|o| o.get("accountUuid"))
        .and_then(|v| v.as_str())
        .map(str::to_owned);

    // Already the active account? Nothing to move.
    if live_uuid.is_some() && live_uuid == target_uuid {
        touch_last_used(index, target_label)?;
        return Ok(SwapOutcome {
            already_active: true,
            outgoing_label: None,
        });
    }

    // --- 1. Sync the OUTGOING account back into its vault (tokens rotate). ---
    let mut outgoing_label = None;
    if let (Some(blob), Some(uuid)) = (live_blob.as_ref(), live_uuid.as_ref()) {
        if let Some(outgoing) = index.find_by_uuid(uuid).cloned() {
            sync_outgoing(index, &outgoing.label, blob, live_oauth.as_ref())?;
            outgoing_label = Some(outgoing.label);
        } else {
            eprintln!(
                "ℹ  The current live account isn't managed by claude-switch. \
                 It's saved to the recovery backup before switching."
            );
        }
    }

    // Load the incoming token. Without it there is nothing to swap in.
    let target_service = paths::vault_service(target_label);
    let target_blob = keychain::read(&target_service)?.with_context(|| {
        format!("no saved credential for '{target_label}'. Re-add it with `claude-switch add`.")
    })?;

    // --- 2. Backup the live slot before mutating it. ---
    if let Some(blob) = live_blob.as_ref() {
        Backup::capture(blob)?;
    }

    // --- 3. Paired write: token blob, then identity (the commit point). ---
    keychain::write(paths::LIVE_SERVICE, &target_blob)
        .context("writing the incoming credential into the live slot")?;
    verify_written(paths::LIVE_SERVICE, &target_blob)?;

    if let Err(e) = config::write_oauth_account(&target.oauth_account) {
        // Identity write failed after the token write — roll the token back.
        rollback(live_blob.as_deref(), live_oauth.as_ref());
        return Err(e).context("stamping the new identity into ~/.claude.json");
    }

    // --- 4. Verify the live session reflects the target. ---
    if let Err(e) = claude::verify_status(target.email()) {
        rollback(live_blob.as_deref(), live_oauth.as_ref());
        return Err(e).context("post-swap verification failed; rolled back");
    }

    // --- 5. Commit complete: record usage and drop the backup. ---
    touch_last_used(index, target_label)?;
    Backup::clear()?;

    Ok(SwapOutcome {
        already_active: false,
        outgoing_label,
    })
}

/// Sync the *current* live account into its vault entry, if it's one we manage.
/// Returns its label. Used by `add` before clearing the slot for a fresh login.
pub fn sync_live(index: &mut Index) -> Result<Option<String>> {
    let live_blob = keychain::read(paths::LIVE_SERVICE)?;
    let live_oauth = config::read_oauth_account()?;
    if let (Some(blob), Some(oauth)) = (live_blob.as_ref(), live_oauth.as_ref()) {
        if let Some(uuid) = oauth.get("accountUuid").and_then(|v| v.as_str()) {
            if let Some(acct) = index.find_by_uuid(uuid).cloned() {
                sync_outgoing(index, &acct.label, blob, Some(oauth))?;
                return Ok(Some(acct.label));
            }
        }
    }
    Ok(None)
}

/// Copy the live blob into the outgoing account's vault entry (read-back
/// verified) and refresh its stored identity.
fn sync_outgoing(
    index: &mut Index,
    label: &str,
    live_blob: &str,
    live_oauth: Option<&serde_json::Value>,
) -> Result<()> {
    let service = paths::vault_service(label);
    keychain::write(&service, live_blob)
        .with_context(|| format!("syncing outgoing credential for '{label}'"))?;
    verify_written(&service, live_blob)?;

    if let (Some(acct), Some(oauth)) = (index.find_mut(label), live_oauth) {
        acct.oauth_account = oauth.clone();
    }
    index.save()?;
    Ok(())
}

/// Read an entry back and confirm it matches what we just wrote.
fn verify_written(service: &str, expected: &str) -> Result<()> {
    match keychain::read(service)? {
        Some(got) if got == expected => Ok(()),
        Some(_) => bail!("read-back mismatch on Keychain entry '{service}'"),
        None => bail!("read-back found no entry for '{service}' right after writing it"),
    }
}

/// Best-effort restore of the live slot from the in-memory pre-swap state.
fn rollback(live_blob: Option<&str>, live_oauth: Option<&serde_json::Value>) {
    if let Some(blob) = live_blob {
        let _ = keychain::write(paths::LIVE_SERVICE, blob);
    }
    match live_oauth {
        Some(o) => {
            let _ = config::write_oauth_account(o);
        }
        None => {
            let _ = config::clear_oauth_account();
        }
    }
}

fn touch_last_used(index: &mut Index, label: &str) -> Result<()> {
    if let Some(acct) = index.find_mut(label) {
        acct.last_used_at = Some(Utc::now());
    }
    index.save()
}
