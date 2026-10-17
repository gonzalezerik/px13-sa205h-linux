#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Capture Bluetooth traffic while the pen advertises, with btmon.
# Run this yourself (sudo for btmon). Read-only: nothing is paired or sent.
#
# Usage (from the repo root):  tools/capture/linux/02-btmon-advertise.sh [outdir]
# Output: <outdir>/btmon-<ts>.log
set -euo pipefail

outdir="${1:-captures/linux}"
mkdir -p "$outdir"
ts="$(date +%Y%m%d-%H%M%S)"

echo "Capturing to $outdir/btmon-$ts.log"
echo
echo "Press Enter to start btmon, then in ANOTHER terminal:"
echo "  bluetoothctl power on"
echo "  bluetoothctl scan le        # keep it running"
echo "Then touch the pen tip to the screen and hover for ~30 s."
echo "Stop btmon here with Ctrl+C, and 'bluetoothctl power off' if you like."
echo "Do NOT pair anything in this session."
read -r

sudo btmon -w "$outdir/btmon-$ts.log"
echo "Done: $outdir/btmon-$ts.log"
