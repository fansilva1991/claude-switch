//! The plaintext, non-secret account index (`~/.claude-switch/accounts.json`).
//!
//! Stores everything *except* the token: label, account type, timestamps, and
//! the full `oauthAccount` identity object stamped into `.claude.json` on swap.
//! Hard rule: no secrets ever land here.

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;

use crate::paths;

/// How the account authenticates — captured at `add`, reused on re-auth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccountType {
    /// Personal/Pro/Max plan billed via a Claude subscription.
    Sub,
    /// Anthropic Console — pay-per-token API billing.
    Console,
    /// Company single sign-on (Google/Okta/etc.).
    Sso,
}

impl AccountType {
    /// The matching `claude auth login` flag.
    pub fn login_flag(&self) -> &'static str {
        match self {
            AccountType::Sub => "--claudeai",
            AccountType::Console => "--console",
            AccountType::Sso => "--sso",
        }
    }

    /// Short badge shown in `list`/picker.
    pub fn badge(&self) -> &'static str {
        match self {
            AccountType::Sub => "sub",
            AccountType::Console => "console",
            AccountType::Sso => "sso",
        }
    }
}

/// One saved account. The token lives in the Keychain; this is everything else.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub label: String,
    #[serde(rename = "type")]
    pub account_type: AccountType,
    pub added_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
    /// Full `oauthAccount` object from `.claude.json` (non-secret identity).
    pub oauth_account: Value,
}

impl Account {
    /// Stable primary key: the account UUID from `oauthAccount`.
    pub fn uuid(&self) -> Option<&str> {
        self.oauth_account.get("accountUuid").and_then(Value::as_str)
    }

    /// Display email from `oauthAccount`, if present.
    pub fn email(&self) -> Option<&str> {
        self.oauth_account
            .get("emailAddress")
            .and_then(Value::as_str)
    }

    /// Organization name from `oauthAccount`, if present.
    pub fn organization(&self) -> Option<&str> {
        self.oauth_account
            .get("organizationName")
            .and_then(Value::as_str)
    }
}

/// On-disk shape of `accounts.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Index {
    pub version: u32,
    pub accounts: Vec<Account>,
}

impl Default for Index {
    fn default() -> Self {
        Index {
            version: 1,
            accounts: Vec::new(),
        }
    }
}

impl Index {
    /// Load the index, returning an empty one if the file doesn't exist yet.
    pub fn load() -> Result<Index> {
        let path = paths::index_file()?;
        match fs::read_to_string(&path) {
            Ok(s) => serde_json::from_str(&s)
                .with_context(|| format!("parsing {}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Index::default()),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }

    /// Atomically persist the index (write temp + rename).
    pub fn save(&self) -> Result<()> {
        let dir = paths::state_dir()?;
        fs::create_dir_all(&dir)
            .with_context(|| format!("creating {}", dir.display()))?;
        let path = paths::index_file()?;
        let tmp = path.with_extension("json.tmp");
        let body = serde_json::to_string_pretty(self)?;
        fs::write(&tmp, body).with_context(|| format!("writing {}", tmp.display()))?;
        fs::rename(&tmp, &path).with_context(|| format!("renaming into {}", path.display()))?;
        Ok(())
    }

    pub fn find(&self, label: &str) -> Option<&Account> {
        self.accounts.iter().find(|a| a.label == label)
    }

    pub fn find_mut(&mut self, label: &str) -> Option<&mut Account> {
        self.accounts.iter_mut().find(|a| a.label == label)
    }

    pub fn find_by_uuid(&self, uuid: &str) -> Option<&Account> {
        self.accounts
            .iter()
            .find(|a| a.uuid() == Some(uuid))
    }

    /// Insert a new account, rejecting a duplicate label.
    pub fn insert(&mut self, account: Account) -> Result<()> {
        if self.find(&account.label).is_some() {
            bail!("an account labeled '{}' already exists", account.label);
        }
        self.accounts.push(account);
        Ok(())
    }

    /// Remove an account by label, returning it if present.
    pub fn remove(&mut self, label: &str) -> Option<Account> {
        if let Some(pos) = self.accounts.iter().position(|a| a.label == label) {
            Some(self.accounts.remove(pos))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample(label: &str, uuid: &str, email: &str) -> Account {
        Account {
            label: label.to_string(),
            account_type: AccountType::Sub,
            added_at: Utc::now(),
            last_used_at: None,
            oauth_account: json!({
                "accountUuid": uuid,
                "emailAddress": email,
                "organizationName": "Acme",
            }),
        }
    }

    #[test]
    fn type_flags_and_badges() {
        assert_eq!(AccountType::Sub.login_flag(), "--claudeai");
        assert_eq!(AccountType::Console.login_flag(), "--console");
        assert_eq!(AccountType::Sso.login_flag(), "--sso");
        assert_eq!(AccountType::Sso.badge(), "sso");
    }

    #[test]
    fn accessors_read_oauth_fields() {
        let a = sample("personal", "uuid-1", "me@example.com");
        assert_eq!(a.uuid(), Some("uuid-1"));
        assert_eq!(a.email(), Some("me@example.com"));
        assert_eq!(a.organization(), Some("Acme"));
    }

    #[test]
    fn insert_rejects_duplicate_label() {
        let mut idx = Index::default();
        idx.insert(sample("personal", "u1", "a@x.com")).unwrap();
        let err = idx.insert(sample("personal", "u2", "b@x.com")).unwrap_err();
        assert!(err.to_string().contains("already exists"));
        assert_eq!(idx.accounts.len(), 1);
    }

    #[test]
    fn find_by_uuid_and_label() {
        let mut idx = Index::default();
        idx.insert(sample("personal", "u1", "a@x.com")).unwrap();
        idx.insert(sample("company", "u2", "b@x.com")).unwrap();
        assert_eq!(idx.find_by_uuid("u2").unwrap().label, "company");
        assert_eq!(idx.find("personal").unwrap().uuid(), Some("u1"));
        assert!(idx.find_by_uuid("nope").is_none());
    }

    #[test]
    fn remove_returns_and_drops() {
        let mut idx = Index::default();
        idx.insert(sample("personal", "u1", "a@x.com")).unwrap();
        let removed = idx.remove("personal").unwrap();
        assert_eq!(removed.label, "personal");
        assert!(idx.accounts.is_empty());
        assert!(idx.remove("personal").is_none());
    }

    #[test]
    fn serde_round_trip_preserves_oauth() {
        let mut idx = Index::default();
        idx.insert(sample("personal", "u1", "a@x.com")).unwrap();
        let s = serde_json::to_string(&idx).unwrap();
        let back: Index = serde_json::from_str(&s).unwrap();
        assert_eq!(back.version, 1);
        assert_eq!(back.accounts[0].account_type, AccountType::Sub);
        assert_eq!(back.accounts[0].email(), Some("a@x.com"));
        // The full oauthAccount object survives the round trip.
        assert_eq!(
            back.accounts[0].oauth_account.get("organizationName"),
            Some(&json!("Acme"))
        );
    }

    #[test]
    fn type_serializes_lowercase() {
        let s = serde_json::to_string(&AccountType::Console).unwrap();
        assert_eq!(s, "\"console\"");
    }
}
