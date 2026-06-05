# claude-switch

A macOS CLI to manage multiple Claude Code accounts (e.g. personal + company),
switch the active one, and launch Claude as that account. Claude Code holds only
**one** live credential at a time; `claude-switch` keeps a vault of saved
accounts and swaps one in/out of that single slot.

See [`DESIGN.md`](./DESIGN.md) for the full design and the verification spike
that established the swap mechanism.

## How it works

A Claude session's identity lives in two places, and a correct switch is an
atomic **paired write** of both (proven by the spike, DESIGN §15):

- **macOS Keychain** entry `Claude Code-credentials` — the actual token.
- `~/.claude.json` → `oauthAccount` — the displayed identity (email/org/UUID).

`claude-switch` stores each saved account as its own Keychain entry
(`claude-switch:<label>`) plus a non-secret row in `~/.claude-switch/accounts.json`
(label, type, timestamps, and the full `oauthAccount`). **Tokens never touch
plaintext disk.**

## Usage

```
claude-switch                    pick an account, switch, and launch
claude-switch <label> [args...]  switch to <label> and launch (args forwarded to claude)
claude-switch use <label>        switch without launching
claude-switch add                add & authenticate a new account
claude-switch remove <label>     forget an account locally (its session stays valid)
claude-switch logout <label>     revoke an account server-side, then forget it
claude-switch list | ls          show saved accounts
claude-switch current            print the active account
```

Add `--no-launch` to any switch to update the credential without starting Claude.

## Install

Homebrew (macOS):

```
brew install fansilva1991/tap/claude-switch
```

Or with Cargo (needs the Rust toolchain):

```
cargo install --git https://github.com/fansilva1991/claude-switch
```

## Build from source

```
cargo build --release   # → target/release/claude-switch
cargo test
```

macOS only for v1. Requires the `claude` CLI on your `PATH`.

### First run

The first time the binary reads the protected `Claude Code-credentials` entry,
macOS shows a one-time Keychain permission dialog — choose **Always Allow**.
This is expected, not an error.

## Releasing

Releases are automated by `.github/workflows/release.yml`. To ship a new version:

```
# 1. bump the version in Cargo.toml (e.g. 0.1.0 -> 0.2.0), commit it
# 2. tag the matching version and push the tag:
git tag v0.2.0 && git push origin v0.2.0
```

The workflow then verifies the tag matches `Cargo.toml`, runs the tests, creates
the GitHub Release, and bumps `url` + `sha256` in the Homebrew tap formula.

**One-time setup:** the workflow needs a repo secret `HOMEBREW_TAP_TOKEN` — a
Personal Access Token with **Contents: read & write** on `fansilva1991/homebrew-tap`.
Create a fine-grained PAT (GitHub → Settings → Developer settings → Fine-grained
tokens), scope it to that repo, then:

```
gh secret set HOMEBREW_TAP_TOKEN -R fansilva1991/claude-switch
```

## Safety

- A pre-swap backup of the live credential + read-back-verified writes mean an
  interrupted swap is recoverable; the next run reconciles automatically.
- A lockfile (`~/.claude-switch/lock`) serializes concurrent invocations.
- Destructive `logout` requires an interactive confirmation.
