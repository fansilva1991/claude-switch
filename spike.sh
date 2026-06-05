#!/usr/bin/env bash
# claude-switch verification spike.
# De-risks the core assumption before any real code is written.
#
# SAFETY:
#   - Never echoes your credential secret.
#   - Phase 1 only backs up/edits ~/.claude.json (restored automatically).
#   - Phase 2 backs the live credential into a recovery Keychain entry FIRST,
#     installs an EXIT trap that always restores it, then tests delete+restore.
#   - macOS will pop a Keychain dialog ("claude-switch/security wants to use...").
#     Click "Always Allow" (or "Allow"). That prompt is expected, not an error.
#
# If anything goes wrong you can always recover manually with:  claude auth login
set -uo pipefail

SVC="Claude Code-credentials"
BACKUP_SVC="claude-switch:_spike_backup"
CJSON="$HOME/.claude.json"
CJSON_BAK="$HOME/.claude.json.spike.bak"

line() { printf '\n\033[1m== %s ==\033[0m\n' "$1"; }
ok()   { printf '\033[32m✓ %s\033[0m\n' "$1"; }
warn() { printf '\033[33m! %s\033[0m\n' "$1"; }

active_account() {
  # Print just the email/account from auth status (no secrets).
  claude auth status --json 2>/dev/null \
    | grep -oE '"(email|account|accountUuid|organizationName)"[^,}]*' || true
}

line "Baseline"
echo "auth status (active account identity only):"
active_account
echo

# ---------------------------------------------------------------------------
line "PHASE 1 — is ~/.claude.json identity the source of truth?"
if [[ ! -f "$CJSON" ]]; then
  warn "$CJSON not found — skipping Phase 1."
else
  cp -p "$CJSON" "$CJSON_BAK" && ok "backed up ~/.claude.json"
  # Blank out oauthAccount.email if present, leave the rest intact.
  if command -v python3 >/dev/null 2>&1; then
    python3 - "$CJSON" <<'PY'
import json,sys
p=sys.argv[1]
d=json.load(open(p))
oa=d.get("oauthAccount")
if isinstance(oa,dict):
    oa["emailAddress"]=oa.get("emailAddress","")  # touch
    # corrupt the email so we can tell if status reads from here
    if "emailAddress" in oa: oa["emailAddress"]="spike-tampered@example.invalid"
json.dump(d,open(p,"w"),indent=2)
print("oauthAccount.emailAddress tampered (if it existed)")
PY
  else
    warn "python3 missing — cannot safely edit JSON; skipping the tamper test."
  fi
  echo "auth status AFTER tampering ~/.claude.json:"
  active_account
  echo
  echo ">>> INTERPRET:"
  echo "    - If the email above is UNCHANGED (your real one) => Keychain is the"
  echo "      source of truth; .claude.json sync is only belt-and-suspenders."
  echo "    - If it shows 'spike-tampered@...' or logs out => .claude.json identity"
  echo "      MATTERS and must be synced on every swap."
  cp -p "$CJSON_BAK" "$CJSON" && rm -f "$CJSON_BAK" && ok "restored ~/.claude.json"
fi

# ---------------------------------------------------------------------------
line "PHASE 2 — can we back up, delete, and restore the live credential?"
echo "This proves the swap mechanism. Press ENTER to continue, or Ctrl-C to stop."
read -r _

# Discover the account attribute of the live entry (not the secret).
ORIG_ACCT="$(security find-generic-password -s "$SVC" 2>/dev/null \
  | awk -F'"' '/"acct"/{print $4}')"
ORIG_ACCT="${ORIG_ACCT:-$USER}"
echo "live entry account attr: ${ORIG_ACCT}"

# Capture the secret into a variable WITHOUT printing it. (Keychain prompt here.)
if ! SECRET="$(security find-generic-password -w -s "$SVC" 2>/dev/null)"; then
  warn "Could not read live credential (denied or not found). Aborting Phase 2."
  exit 1
fi
[[ -n "$SECRET" ]] && ok "read live credential into memory (not shown)"

# Save a recovery copy into our own entry.
security add-generic-password -U -s "$BACKUP_SVC" -a "$ORIG_ACCT" -w "$SECRET" \
  && ok "wrote recovery backup entry ($BACKUP_SVC)"

# Always try to restore on exit, no matter what.
restore() {
  security add-generic-password -U -s "$SVC" -a "$ORIG_ACCT" -w "$SECRET" 2>/dev/null \
    && ok "restored live credential" \
    || warn "RESTORE FAILED — run:  claude auth login   (recovery copy in Keychain: $BACKUP_SVC)"
}
trap restore EXIT

# Delete the live entry and observe.
security delete-generic-password -s "$SVC" >/dev/null 2>&1 && ok "deleted live entry"
echo "auth status with NO live credential:"
active_account || true
echo ">>> Expect: logged out / no account. If it STILL shows your account, identity"
echo "    is cached elsewhere (e.g. .claude.json) and Phase 1's result explains it."
echo

# Restore happens via trap on exit; do it now explicitly and verify.
trap - EXIT
restore
echo "auth status AFTER restore:"
active_account || true
echo ">>> Expect: your original account is back, fully working."

# Clean up the recovery copy.
security delete-generic-password -s "$BACKUP_SVC" >/dev/null 2>&1 \
  && ok "removed recovery backup entry"

line "Spike complete"
echo "Report back: Phase 1 verdict (did the email change?) and Phase 2 verdict"
echo "(did status log out on delete and recover on restore?)."
