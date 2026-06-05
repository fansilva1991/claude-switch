# CLAUDE.md

Guidance for working in this repo. Read `DESIGN.md` first — it is the locked
source of truth (design resolved via a grilling session + a decisive verification
spike). Do not re-derive its decisions.

## What this is

`claude-switch` — a macOS-only Rust CLI that vaults multiple Claude Code accounts
and swaps one into Claude's **single** live credential slot, then launches Claude.

## The one invariant that matters

A Claude session's identity lives in **two** places and they must always move
together (proven by the spike, DESIGN §15):

1. **macOS Keychain** entry `Claude Code-credentials` — the actual token (secret).
2. `~/.claude.json` → `oauthAccount` — the displayed identity (email/org/UUID).

A correct switch is an **atomic paired write** of both. Writing only the Keychain
leaves the old email showing against the new token. **Never** break this pairing.

**Hard rule:** tokens live only in the Keychain. Never write a token to plaintext
disk. The index (`~/.claude-switch/accounts.json`) holds only non-secret metadata
(including the `oauthAccount` object, which is identity, not a secret).

## Architecture (module = responsibility)

- `paths.rs` — all filesystem locations + Keychain service-name constants.
- `keychain.rs` — the **only** place that shells to `/usr/bin/security`.
- `index.rs` — `accounts.json` model + load/save (atomic temp+rename). Has unit tests.
- `config.rs` — the **only** place that reads/writes `.claude.json` `oauthAccount`.
- `claude.rs` — the **only** place that orchestrates the real `claude` CLI
  (`auth login`/`logout`/`status`). We never mint tokens ourselves.
- `swap.rs` — the swap engine. The paired write lives here; keep it the single
  authority on swap ordering.
- `safety.rs` — `Lock` (process lockfile), `Backup` (pre-swap snapshot), `reconcile`.
- `picker.rs` — all interactive prompts (`inquire`), with inline option explanations.
- `commands.rs` — one handler per verb; the only place that `exec`s `claude`.
- `main.rs` — CLI parsing + manual dispatch (keeps `<label> [...claude args]`
  passthrough working alongside reserved subcommands).

### Swap ordering (don't reorder casually — it's the crash-safety contract)

sync outgoing token back to its vault (tokens rotate) → backup live slot → write
incoming token → **read-back verify** → stamp `oauthAccount` (commit point) →
`claude auth status` verify → on any failure, **rollback** → drop backup.

## Conventions

- Errors: `anyhow`; add `.context(...)` at each boundary. User-facing status
  lines go to **stderr** with `✓`/`ℹ`/`⚠` prefixes; machine-readable output
  (`list`, `current`) goes to **stdout**.
- Every interactive choice must explain each option inline (DESIGN §9) — match
  the existing prompt style in `picker.rs`.
- Identity matching is by **account UUID**, never email (email is a display label).
- Destructive actions (`logout`) require an interactive confirm and refuse to run
  without a TTY.
- Never clobber an **unmanaged** live account — back it up / offer adopt (DESIGN §4).

## Build / test

```
cargo build            # debug
cargo build --release  # → target/release/claude-switch (stripped, LTO)
cargo test             # pure-logic unit tests (index/serde)
cargo clippy           # keep it warning-clean
```

## Testing limits (important)

The destructive flows (`add`, `remove`, `logout`, real `swap_to`) touch the
protected `Claude Code-credentials` Keychain entry. An AI agent is blocked from
reading it (credential-exploration guard) and a wrong move can break the user's
real login. **Do not run those flows yourself** — verify pure logic with
`cargo test`, and have the user run `claude-switch add` etc. The first real
Keychain read triggers a one-time macOS "Always Allow" prompt (expected).

## Out of scope for v1 (see DESIGN "Open")

Per-account `CLAUDE_CONFIG_DIR` isolation, Linux/WSL (file-based credentials),
shell-prompt integration helper.
