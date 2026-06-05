//! Keychain adapter — shells out to `/usr/bin/security`.
//!
//! We deliberately avoid linking the Security framework: the CLI is more stable
//! across macOS versions and keeps the secret-handling auditable. Secrets are
//! never logged; the only place a token appears is the `security` argv (visible
//! to the same user only) and stdout when reading back.

use anyhow::{bail, Context, Result};
use std::process::Command;

const SECURITY: &str = "/usr/bin/security";

/// The Keychain "account" attribute we tag entries with. Claude Code itself
/// uses the OS username on the live entry; we mirror that for consistency.
fn account_attr() -> String {
    std::env::var("USER").unwrap_or_else(|_| "claude".to_string())
}

/// Read a generic-password secret by service name. Returns `None` if no such
/// entry exists. The protected live entry may trigger a one-time ACL prompt.
pub fn read(service: &str) -> Result<Option<String>> {
    let out = Command::new(SECURITY)
        .args(["find-generic-password", "-s", service, "-w"])
        .output()
        .with_context(|| format!("running security find-generic-password -s {service}"))?;

    if out.status.success() {
        // `-w` prints the raw secret followed by a trailing newline.
        let mut s = String::from_utf8(out.stdout)
            .context("credential was not valid UTF-8")?;
        if s.ends_with('\n') {
            s.pop();
        }
        Ok(Some(s))
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr);
        // Exit 44 / "could not be found" == no such entry: a normal absence.
        if stderr.contains("could not be found") || out.status.code() == Some(44) {
            Ok(None)
        } else {
            bail!("security failed reading '{service}': {}", stderr.trim());
        }
    }
}

/// Create or overwrite a generic-password entry (`-U` updates in place).
pub fn write(service: &str, secret: &str) -> Result<()> {
    let account = account_attr();
    let out = Command::new(SECURITY)
        .args([
            "add-generic-password",
            "-U",
            "-s",
            service,
            "-a",
            &account,
            "-w",
            secret,
        ])
        .output()
        .with_context(|| format!("running security add-generic-password -s {service}"))?;

    if out.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr);
        bail!("security failed writing '{service}': {}", stderr.trim());
    }
}

/// Delete a generic-password entry. Returns `Ok(false)` if it didn't exist.
pub fn delete(service: &str) -> Result<bool> {
    let out = Command::new(SECURITY)
        .args(["delete-generic-password", "-s", service])
        .output()
        .with_context(|| format!("running security delete-generic-password -s {service}"))?;

    if out.status.success() {
        Ok(true)
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr);
        if stderr.contains("could not be found") || out.status.code() == Some(44) {
            Ok(false)
        } else {
            bail!("security failed deleting '{service}': {}", stderr.trim());
        }
    }
}

/// True if an entry exists without surfacing its secret to a prompt where
/// possible (still uses `-w`, but callers use this only for our own entries).
pub fn exists(service: &str) -> Result<bool> {
    Ok(read(service)?.is_some())
}
