#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Accessibility policy gate. Fails CI when the a11y contract is broken:
#   1. the standard (docs/a11y.md) and its CONTRIBUTING section must exist
#   2. no UI code may exist before its platform's automated a11y checks are
#      wired into CI (phase-gated, so this is honest today and strict later)
#   3. a README accessibility section must exist
#
# Run from anywhere; resolves paths relative to the repo root.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
fail=0
note() { echo "  ok: $1"; }
err()  { echo "  FAIL: $1"; fail=1; }

echo "== accessibility policy gate =="

# 1. The standard itself
[ -f "$root/docs/a11y.md" ] && note "docs/a11y.md present" \
  || err "docs/a11y.md missing (the WCAG 2.2 AA standard)"
grep -q "WCAG 2.2" "$root/CONTRIBUTING.md" 2>/dev/null \
  && note "CONTRIBUTING.md states the a11y rule" \
  || err "CONTRIBUTING.md lost its Accessibility section"

# 2. UI code must not outrun its a11y checks. The settings app (GTK4 +
#    libadwaita, crates/opp-settings) must ship an automated AT-SPI2 check
#    (any file or dir named *a11y*) before it lands.
for dir in "$root"/crates/*settings* "$root"/apps/*; do
  [ -d "$dir" ] || continue
  if find "$dir" -type f -name '*.rs' | grep -q .; then
    if find "$dir" -iname '*a11y*' -not -path '*/target/*' | grep -q .; then
      note "$(basename "$dir") has source and an a11y harness"
    else
      err "$(basename "$dir") has source but no a11y test harness (add a *a11y* test)"
    fi
  fi
done

# 3. README promises users accessibility exists
grep -qi "accessib" "$root/README.md" 2>/dev/null \
  && note "README mentions accessibility" \
  || err "README should state the WCAG 2.2 AA commitment"

echo
if [ "$fail" -ne 0 ]; then
  echo "Accessibility gate FAILED — see docs/a11y.md"
  exit 1
fi
echo "Accessibility gate passed."
