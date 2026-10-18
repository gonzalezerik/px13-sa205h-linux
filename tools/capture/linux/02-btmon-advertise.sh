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
echo "Press Enter to start: btmon records and a 45 s LE scan runs by itself."
echo "While it runs, touch the pen tip to the screen and hover nearby."
echo "Do NOT pair anything in this session."
read -r

sudo btmon -w "$outdir/btmon-$ts.log" &
btmon_pid=$!
trap 'sudo kill "$btmon_pid" 2>/dev/null || true' EXIT

# --timeout ends the scan on its own; </dev/null keeps bluetoothctl from
# grabbing this terminal (an interactive scan on on a pipe just hangs).
bluetoothctl --timeout 45 scan on >/dev/null 2>&1 </dev/null &
sleep 47
sudo kill "$btmon_pid" 2>/dev/null || true
trap - EXIT
echo "Done: $outdir/btmon-$ts.log"
