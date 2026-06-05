//! Crash-safety primitives: a process lock and pre-swap backup/reconcile.

use anyhow::{bail, Context, Result};
use std::fs::{self, OpenOptions};
use std::io::Write;

use crate::{config, keychain, paths};

/// An exclusive lock that serializes concurrent `claude-switch` invocations.
/// Released (file removed) on drop.
pub struct Lock {
    path: std::path::PathBuf,
}

impl Lock {
    /// Acquire the lock, failing fast if another invocation holds it.
    pub fn acquire() -> Result<Lock> {
        let dir = paths::state_dir()?;
        fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        let path = paths::lock_file()?;
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut f) => {
                let _ = writeln!(f, "{}", std::process::id());
                Ok(Lock { path })
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                bail!(
                    "another claude-switch operation is in progress (lock at {}). \
                     If you're sure none is running, delete that file.",
                    path.display()
                )
            }
            Err(e) => Err(e).with_context(|| format!("creating lock {}", path.display())),
        }
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Snapshot of the live slot taken just before a swap mutates anything.
pub struct Backup;

impl Backup {
    /// Copy the live Keychain blob into the reserved backup entry. No-op if the
    /// live slot is already empty (e.g. logged out).
    pub fn capture(live_blob: &str) -> Result<()> {
        keychain::write(paths::BACKUP_SERVICE, live_blob)
            .context("capturing pre-swap backup of the live credential")
    }

    /// Discard the backup once a swap has fully committed.
    pub fn clear() -> Result<()> {
        keychain::delete(paths::BACKUP_SERVICE)?;
        Ok(())
    }

    pub fn exists() -> Result<bool> {
        keychain::exists(paths::BACKUP_SERVICE)
    }
}

/// On start, detect a leftover backup from an interrupted swap. We surface it
/// but do not auto-restore: the live slot may already hold a valid newer
/// account. The caller decides (DESIGN §4 — adopt rather than clobber).
pub fn reconcile() -> Result<()> {
    if !Backup::exists()? {
        return Ok(());
    }

    // If the live slot currently has a coherent session, the swap likely
    // completed and only backup-cleanup was interrupted: drop the stale backup.
    let live_ok = config::read_oauth_account()?.is_some()
        && keychain::exists(paths::LIVE_SERVICE).unwrap_or(false);

    if live_ok {
        Backup::clear()?;
    } else {
        eprintln!(
            "⚠  A previous switch was interrupted and the live session looks broken.\n   \
             A backup of your prior credential is saved. Run `claude-switch` and pick an\n   \
             account to recover, or `claude auth login` to start fresh."
        );
    }
    Ok(())
}
