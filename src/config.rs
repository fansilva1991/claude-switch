//! Read/write the active identity in `~/.claude.json`.
//!
//! The spike (DESIGN §15) proved `oauthAccount` here drives the *displayed*
//! identity, while the Keychain holds the token. A correct swap writes both.

use anyhow::{Context, Result};
use serde_json::Value;
use std::fs;

use crate::paths;

/// The current `oauthAccount` object, or `None` if logged out / absent.
pub fn read_oauth_account() -> Result<Option<Value>> {
    let path = paths::claude_json()?;
    let body = match fs::read_to_string(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let root: Value =
        serde_json::from_str(&body).with_context(|| format!("parsing {}", path.display()))?;
    Ok(root.get("oauthAccount").filter(|v| !v.is_null()).cloned())
}

/// Stamp `oauthAccount` into `.claude.json`, preserving every other key.
/// This is the swap commit point. Written atomically (temp + rename).
pub fn write_oauth_account(oauth_account: &Value) -> Result<()> {
    set_oauth_account(Some(oauth_account))
}

/// Remove `oauthAccount` entirely (used when clearing the live slot).
pub fn clear_oauth_account() -> Result<()> {
    set_oauth_account(None)
}

fn set_oauth_account(value: Option<&Value>) -> Result<()> {
    let path = paths::claude_json()?;
    let mut root: Value = match fs::read_to_string(&path) {
        Ok(b) => serde_json::from_str(&b)
            .with_context(|| format!("parsing {}", path.display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Value::Object(Default::default()),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };

    let obj = root
        .as_object_mut()
        .context("~/.claude.json is not a JSON object")?;
    match value {
        Some(v) => {
            obj.insert("oauthAccount".to_string(), v.clone());
        }
        None => {
            obj.remove("oauthAccount");
        }
    }

    // Claude Code writes this file compactly; match that to minimize churn.
    let body = serde_json::to_string(&root)?;
    let tmp = path.with_extension("json.cswtmp");
    fs::write(&tmp, body).with_context(|| format!("writing {}", tmp.display()))?;
    fs::rename(&tmp, &path).with_context(|| format!("renaming into {}", path.display()))?;
    Ok(())
}
