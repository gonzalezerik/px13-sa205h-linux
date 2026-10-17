#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Download every file listed in docs/artifacts.tsv into artifacts/ and verify its SHA-256.
# Files already present with the right hash are skipped. Plain-HTTP URLs (Microsoft's CDN
# only serves http) are safe here because the hash is checked before the file is kept.
#   tools/fetch-artifacts.sh            all
#   tools/fetch-artifacts.sh asus/      only paths starting with a prefix
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
prefix="${1:-}"
fail=0
while IFS=$'\t' read -r path sha url; do
  case "$path" in ''|'#'*) continue ;; esac
  [[ "$path" == "$prefix"* ]] || continue
  dest="$root/artifacts/$path"
  if [ -f "$dest" ] && echo "$sha  $dest" | sha256sum -c --quiet 2>/dev/null; then
    echo "ok       $path"; continue
  fi
  mkdir -p "$(dirname "$dest")"
  echo "fetch    $path"
  curl -fL --retry 3 -C - -o "$dest.part" "$url"
  if echo "$sha  $dest.part" | sha256sum -c --quiet; then
    mv "$dest.part" "$dest"; echo "verified $path"
  else
    echo "HASH MISMATCH $path (kept as $dest.part)" >&2; fail=1
  fi
done < "$root/docs/artifacts.tsv"
exit $fail
