#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Privacy gate. Fails when anything private is about to be published:
#   1. files that must never be tracked: captures, Windows binaries, and anything
#      matched by the local, untracked .git/info/exclude
#   2. Bluetooth/MAC addresses, private IPv4 addresses, key material
#   3. any term from the local, untracked .privacy-denylist (one regex per
#      line: hostnames, serial numbers, names). The list itself is never
#      committed, so the terms it protects never reach the repository.
#
# Usage: tools/privacy/check.sh            check all tracked files
#        tools/privacy/check.sh --staged   check what is staged (pre-commit)
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"
fail=0
err() { echo "  FAIL: $1"; fail=1; }

if [ "${1:-}" = "--staged" ]; then
  files=$(git diff --cached --name-only --diff-filter=ACMR)
  grep_src=(git grep --cached -n -I -E)
else
  files=$(git ls-files)
  grep_src=(git grep -n -I -E)
fi

echo "== privacy gate =="

# 1. Forbidden paths
forbidden='^(captures/|artifacts/)|\.(btsnoop|pcapng?|etl|esd|wim|exe|msi|msix|msixbundle|cab|sys)$'
bad=$(printf '%s\n' "$files" | grep -E "$forbidden" || true)
[ -z "$bad" ] || err "forbidden files tracked: $(echo $bad)"
if [ -f .git/info/exclude ]; then
  bad=$(git ls-files -ci --exclude-from=.git/info/exclude || true)
  [ -z "$bad" ] || err "tracked files match .git/info/exclude: $(echo $bad)"
fi

[ -n "$files" ] || { echo "Privacy gate passed (nothing to check)."; exit 0; }

# 2. Generic patterns. LICENSE is upstream text and is skipped.
patterns=(
  '([0-9A-Fa-f]{2}:){5}[0-9A-Fa-f]{2}'                        # MAC / BLE address
  '\b(10|192\.168|172\.(1[6-9]|2[0-9]|3[01]))\.[0-9]{1,3}\.[0-9]{1,3}\b'  # private IPv4
  'BEGIN [A-Z ]*PRIVATE KEY'
  '\b(sk-[A-Za-z0-9]{20,}|gh[pousr]_[A-Za-z0-9]{30,})'         # API tokens
)
# Lines that only document the patterns (this script) are exempt.
for p in "${patterns[@]}"; do
  hits=$(printf '%s\n' "$files" | grep -v -E '^(LICENSE|tools/privacy/check\.sh)$' \
         | xargs -r "${grep_src[@]}" "$p" -- 2>/dev/null || true)
  [ -z "$hits" ] || err "pattern /$p/ matched:"$'\n'"$hits"
done

# 3. Local denylist
if [ -f "$root/.privacy-denylist" ]; then
  while IFS= read -r term; do
    [ -z "$term" ] || [ "${term#\#}" != "$term" ] && continue
    hits=$(printf '%s\n' "$files" | xargs -r "${grep_src[@]}" -i "$term" -- 2>/dev/null || true)
    [ -z "$hits" ] || err "denylisted term matched (see .privacy-denylist):"$'\n'"$hits"
  done < "$root/.privacy-denylist"
else
  echo "  note: no .privacy-denylist (fine in CI; keep one locally)"
fi

echo
if [ "$fail" -ne 0 ]; then
  echo "Privacy gate FAILED. Remove the private data before committing."
  exit 1
fi
echo "Privacy gate passed."
