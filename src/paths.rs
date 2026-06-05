//! Filesystem locations and well-known Keychain service names.

use anyhow::{Context, Result};
use std::path::PathBuf;

/// The Keychain service that Claude Code itself uses for the live credential.
pub const LIVE_SERVICE: &str = "Claude Code-credentials";

/// Prefix for our own per-account vault entries: `claude-switch:<label>`.
pub const VAULT_PREFIX: &str = "claude-switch:";

/// Reserved vault entry holding a pre-swap snapshot of the live credential.
pub const BACKUP_SERVICE: &str = "claude-switch:_backup_live";

/// Build the Keychain service name for a saved account.
pub fn vault_service(label: &str) -> String {
    format!("{VAULT_PREFIX}{label}")
}

/// `~/.claude.json` — holds the active `oauthAccount` identity.
pub fn claude_json() -> Result<PathBuf> {
    Ok(home()?.join(".claude.json"))
}

/// `~/.claude-switch/` — our state directory.
pub fn state_dir() -> Result<PathBuf> {
    Ok(home()?.join(".claude-switch"))
}

/// `~/.claude-switch/accounts.json` — the plaintext, non-secret index.
pub fn index_file() -> Result<PathBuf> {
    Ok(state_dir()?.join("accounts.json"))
}

/// `~/.claude-switch/lock` — serializes concurrent invocations.
pub fn lock_file() -> Result<PathBuf> {
    Ok(state_dir()?.join("lock"))
}

fn home() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME environment variable is not set")
}
