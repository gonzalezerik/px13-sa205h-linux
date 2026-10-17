#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Import the Windows capture sessions from the dual-boot partition into
# captures/windows/. Mounts the NTFS partition read-only, copies
# C:\mpp-captures\* , records SHA-256s, unmounts. Needs root; run from the
# repo root after rebooting back to NixOS.
set -euo pipefail

DEV="${WIN_DEV:-/dev/nvme0n1p3}"   # the Windows partition; override with WIN_DEV=...
DEST='captures/windows'

[ "$(id -u)" -eq 0 ] || { echo "needs root: sudo $0" >&2; exit 1; }
cd "$(dirname "$0")/../.."
[ -d captures ] || { echo "run from the repo root (no captures/ here)" >&2; exit 1; }
[ -b "$DEV" ] || { echo "no such partition: $DEV (override with WIN_DEV=/dev/nvmeXnYpZ)" >&2; exit 1; }

MNT=$(mktemp -d /run/win-ro.XXXXXX)
cleanup() { umount "$MNT" 2>/dev/null || true; rmdir "$MNT" 2>/dev/null || true; }
trap cleanup EXIT

if ! mount -t ntfs3 -o ro "$DEV" "$MNT" 2>/dev/null; then
  mount -t ntfs-3g -o ro "$DEV" "$MNT"
fi

SRC="$MNT/mpp-captures"
[ -d "$SRC" ] || { echo "no C:\\mpp-captures on $DEV — run a capture session first" >&2; exit 1; }

echo "importing $DEV:/mpp-captures -> $DEST/"
mkdir -p "$DEST"
cp -a "$SRC/." "$DEST/"
cleanup   # unmount before hashing so the manifest only covers local files

# hand the files back to the invoking user, then pin everything
if [ -n "${SUDO_UID:-}" ]; then
  chown -R "${SUDO_UID}:${SUDO_GID:-0}" "$DEST"
fi
( cd "$DEST" && find . -type f ! -name SHA256SUMS -print0 | sort -z | xargs -0 sha256sum > SHA256SUMS )

n=$(find "$DEST" -type f ! -name SHA256SUMS | wc -l)
echo "done: $n files under $DEST/, manifest at $DEST/SHA256SUMS"
