# claude-switch — Design Spec

A macOS CLI to manage multiple Claude Code accounts (e.g. personal + company),
switch the active one, and launch Claude as that account. Built because Claude
Code holds only **one** active credential at a time.

> Status: design locked via grilling session. **Verification spike done (see §15)** —
> results promote `.claude.json` sync from optional to **mandatory**. Ready to build.

---

## 1. Core model

Claude Code keeps exactly one live credential:
- **macOS Keychain** generic password, service `Claude Code-credentials` (the secret).
- `~/.claude.json` → `oauthAccount` (active account identity: email, org, UUID).

`claude-switch` maintains its own **vault** of saved accounts and swaps one in/out
of that single live slot, then launches Claude.

## 2. Storage model — *Keychain per account + plaintext index* (Q1)

- One Keychain entry **per saved account**: service `claude-switch:<label>`.
- Plaintext **index** at `~/.claude-switch/accounts.json` holding only non-secret
  metadata: label, type (sub/console/sso), timestamps, **and the full
  `oauthAccount` object** (email, org, UUID) needed to stamp `.claude.json` on swap
  (§15 proved this is identity metadata, not a secret).
- **Hard constraint:** never write a token to plaintext disk. Secrets live only in
  the Keychain.

> A vaulted account = **one Keychain blob** (the secret token, swapped into the live
> slot) **+ one `oauthAccount` object** in the index (the identity, stamped into
> `.claude.json`). The two are a matched pair and must always move together (§15).

## 3. Token staleness — *sync-outgoing-before-swap* (Q2)

Claude refreshes tokens in the background and **rotates refresh tokens**. A stale
vault snapshot can be dead. Therefore:
- On every switch, **first copy the current live slot back into the outgoing
  account's vault entry** (so the vault always holds the freshest credential).
- The vault entry for the *currently active* account is treated as a stale cache,
  refreshed at switch time. Then swap the incoming account into the live slot.

## 4. Active-account identity — *UUID primary key + adopt* (Q3)

- Read the live account via `claude auth status --json` (source of truth).
- Match to a vault entry by **stable account UUID** (email is just the display label).
- If the live account's UUID matches **no** vault entry → it's **unmanaged**
  (user logged in via plain `claude`). Do **not** clobber a random entry — offer to
  **adopt** it into the vault.

## 5. Add / authenticate — *orchestrate `claude auth login`, never mint* (Q4)

The tool never speaks OAuth itself. `add` choreography:
1. Sync current live account into its vault entry (§3).
2. Clear the live slot so Claude prompts a fresh login.
3. Run the real `claude auth login` interactively with the chosen mode flag (§12),
   pre-filling email: `claude auth login <--claudeai|--console|--sso> --email <e>`.
4. Capture resulting live credential + read new `oauthAccount`; create a vault
   entry, prompting for a friendly label.
5. Live slot now holds the new account — you're switched in.

## 6. Concurrency model — *sequential, one account live at a time* (Q5)

- One active account at a time (matches the single Keychain slot). No simultaneous
  parallel sessions in v1.
- v1 shares one config dir (history/settings shared across accounts).
- **Future option C:** per-account `CLAUDE_CONFIG_DIR` to isolate history/settings.

## 7. Disconnect — *two verbs* (Q6)

- `remove <label>` — **safe default**: delete the vault Keychain entry + index row.
  Token stays valid server-side; reversible by re-adding. No session kill.
- `logout <label>` — **destructive**: switch that account in, run `claude auth
  logout` (revokes server-side), then remove. **Requires a confirmation prompt
  naming the account.**
- Guard: cannot `remove`/`logout` the **currently active** account without first
  switching away (avoid orphaning the live slot). Warn and handle.

## 8. Launch behavior — *switch then `exec claude`* (Q7)

- Default: swap credential, then **`exec claude`** (replace process → straight into
  a session as the chosen account).
- **Passthrough args:** `claude-switch <account> [...claude args]` forwards verbatim
  (e.g. `claude-switch company --resume`).
- **Escape hatch:** `--no-launch` (or `use <label>`) swaps without launching.

## 9. Command grammar / UX — *built-in picker, no `fzf` dep* (Q8)

- `claude-switch` (bare) → arrow-key picker (label + email + type + active marker).
  If exactly **one** account, skip the menu and launch it. Falls back to a numbered
  list when not a TTY.
- `claude-switch <label>` → switch + launch directly (tab-completable).
- `claude-switch add` → login choreography (§5).
- `claude-switch remove <label>` / `logout <label>` → §7.
- `claude-switch list` (`ls`) → print accounts + active marker, no action.
- `claude-switch current` → print just the active account (for shell prompts/scripts).

### UX principle — explain every option inline (Q12 follow-up)
Every interactive choice shows a one-line explanation of what it means, not just a
label. E.g. the account-type prompt:
```
? What kind of account is this?
  > Claude subscription  — personal/Pro/Max plan, billed via your Claude subscription
    Anthropic Console     — API usage billing (console.anthropic.com), pay-per-token
    SSO                   — company single sign-on (Google/Okta/etc.)
```
Same treatment for destructive confirmations and the picker legend.

## 10. Implementation & distribution — *Rust, macOS-only v1* (Q9)

- **Rust**, single self-contained binary.
- Keychain via shelling to **`/usr/bin/security`** (more robust across macOS
  versions than linking the framework; auditable).
- Crates: `serde_json`, an interactive prompt lib (`inquire`/`dialoguer`),
  `std::process` for spawn/exec.
- Distribute via Homebrew tap or prebuilt binary.
- macOS-only for v1. (Linux/WSL would swap the Keychain layer for file-based
  `~/.claude/.credentials.json`.)

## 11. Crash safety — *backup + verified ordered writes + lockfile + reconcile* (Q10)

- **Pre-swap backup:** snapshot live credential to `claude-switch:_backup_live`
  before any mutation; detect incomplete swaps on next run and offer restore.
- **Ordered, verified writes:** save outgoing fully (read-back verified) *before*
  overwriting live; treat the `.claude.json` update as the commit point.
- **Lockfile** `~/.claude-switch/lock` to serialize concurrent invocations.
- **Reconcile on start:** every run checks live-vs-recorded consistency; on
  mismatch, adopt rather than clobber (ties to §4).

## 12. Keychain prompts & sufficiency (Q11)

- **One-time ACL prompt** expected when the binary first touches the protected
  `Claude Code-credentials` entry ("Always Allow"). Surface a friendly heads-up
  before it appears; don't treat it as an error. Our own `claude-switch:*` entries
  are owned by us → no prompt.
- **MANDATORY `.claude.json` sync** (upgraded from "defensive" by §15): the
  displayed/effective identity comes from `.claude.json`'s `oauthAccount`, while the
  Keychain blob is the actual token. A correct swap is a **paired write** — overwrite
  the Keychain blob **and** replace `oauthAccount` — then verify with
  `claude auth status --json`. Swapping only the Keychain would leave the old email
  showing against the new token (mismatch).

## 13. Account types — *capture & persist per account* (Q12)

- At `add`, prompt for type (default **Claude subscription**; also **Console**,
  **SSO**) with inline explanations (§9), pass the matching flag to
  `claude auth login`, and **persist the type in the index**.
- Show the type as a badge in `list`/picker (`company [sso]`, `personal [sub]`).
- Re-auth (when a refresh token finally dies) reuses stored type + email
  automatically.

---

## 14. Verification spike plan — ✅ DONE (results in §15)

A ~15–30 min throwaway to de-risk the core assumption:
1. Note current active account: `claude auth status --json`.
2. Read the live `Claude Code-credentials` Keychain blob; copy it to a
   `claude-switch:test` entry.
3. (Carefully) overwrite the live slot with a *second* account's blob.
4. Run `claude auth status --json` — **does it report the swapped account without
   editing `.claude.json`?**
   - If yes → Keychain blob is self-contained; `.claude.json` sync is just
     belt-and-suspenders.
   - If no → identity also lives in `.claude.json`; sync is mandatory. Adjust §3/§12.
5. Restore original live credential from backup.

Outcome decides the minimal correct swap, then build proceeds.

## 15. Spike results (2026-06-05) — RAN, decisive

- **Phase 1 — `.claude.json` drives displayed identity.** Tampering
  `oauthAccount.emailAddress` made `claude auth status` report the fake email. So
  identity is read from `.claude.json`, **not** the Keychain blob → syncing it on
  swap is **mandatory** (§12).
- **Phase 2 — Keychain holds the actual credential.** Deleting the live
  `Claude Code-credentials` entry logged the session out (no email); restoring the
  exact blob brought the account fully back. The backup→delete→restore round-trip
  works faithfully → **the swap mechanism is proven.**
- **Conclusion:** a switch = atomic **paired write** of (Keychain blob + `.claude.json`
  `oauthAccount`). The `oauthAccount` object is non-secret and lives in the index;
  the token stays in the Keychain. Account-attr on the live entry was `fansilva1991`.
- Spike left **no residue** (recovery entry removed, `.claude.json` restored).

## Open / deferred
- Per-account `CLAUDE_CONFIG_DIR` isolation (§6 option C) — future.
- Linux/WSL support (file-based credentials) — future.
- Shell-prompt integration helper using `current` — nice-to-have.
