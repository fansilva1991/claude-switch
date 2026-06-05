//! Orchestrate the real `claude` CLI. We never mint tokens ourselves — login
//! and logout are delegated to Claude Code; we only move the resulting blobs.

use anyhow::{bail, Context, Result};
use std::process::Command;

use crate::index::AccountType;

/// Run `claude auth login <flag> --email <email>` interactively, inheriting the
/// terminal so the user completes the real OAuth flow.
pub fn auth_login(account_type: AccountType, email: &str) -> Result<()> {
    let status = Command::new("claude")
        .args(["auth", "login", account_type.login_flag(), "--email", email])
        .status()
        .context("launching `claude auth login` (is the `claude` CLI on PATH?)")?;
    if !status.success() {
        bail!("`claude auth login` exited with a non-zero status");
    }
    Ok(())
}

/// Run `claude auth logout` to revoke the active session server-side.
pub fn auth_logout() -> Result<()> {
    let status = Command::new("claude")
        .args(["auth", "logout"])
        .status()
        .context("launching `claude auth logout`")?;
    if !status.success() {
        bail!("`claude auth logout` exited with a non-zero status");
    }
    Ok(())
}

/// Verify the active session: `claude auth status` succeeds and (if an expected
/// email is given) the readout mentions it. Returns the raw stdout for context.
pub fn verify_status(expected_email: Option<&str>) -> Result<()> {
    let out = Command::new("claude")
        .args(["auth", "status", "--json"])
        .output()
        .context("launching `claude auth status --json`")?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        bail!("`claude auth status` reported no active session: {}", stderr.trim());
    }
    if let Some(email) = expected_email {
        let stdout = String::from_utf8_lossy(&out.stdout);
        if !stdout.contains(email) {
            bail!(
                "post-swap verification mismatch: `claude auth status` did not report {email}"
            );
        }
    }
    Ok(())
}
