#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# First live haptics run. These are the project's first writes to a pen,
# so nothing leaves the USB/air without a per-step yes: every effect is
# first printed as the exact output-report bytes (docs/protocol/haptics.md
# §2.7 layout), and only after a "y" does oppd send it.
#
# Prerequisite: the pen is already paired (see 04-tap-to-pair.sh) and
# awake — touch the tip to the screen right before each send, the BLE
# link drops while idle. Needs write access to the pen hidraw node
# (input group membership).
#
# Usage (from the repo root, on the PX13):
#   cargo build -p oppd
#   tools/capture/linux/05-first-haptics.sh [outdir]
#
# What each step should feel like, from the Windows-side wire session
# (findings/006 §4): one crisp click in the air; the success chirp
# (145 ms) rising; the ink texture NOT in the air but like writing on
# paper while the tip draws (it auto-triggers on the tip switch, lasts
# about 2 s per send); Stop cutting a playing texture short — Stop
# itself is the one entry never exercised on this pen, so step 5 is the
# experiment.
set -euo pipefail

outdir="${1:-captures/linux}"
dir="$outdir/first-haptics-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$dir"
oppd="target/debug/oppd"
if [ ! -x "$oppd" ]; then
  echo "build the daemon first: cargo build -p oppd" >&2
  exit 1
fi
log="$dir/run.log"

step() {
  local label="$1"; shift
  echo | tee -a "$log"
  echo "== $label ==" | tee -a "$log"
  "$oppd" haptic "$@" | tee -a "$log"
  printf 'send these bytes to the pen? [y/N] '
  local answer
  read -r answer
  if [ "$answer" = "y" ]; then
    "$oppd" haptic "$@" --send | tee -a "$log"
    printf 'felt it? describe in one line (empty ok): '
    read -r felt
    echo "felt: $felt" | tee -a "$log"
  else
    echo "skipped" | tee -a "$log"
  fi
}

echo "First haptics run — dump in $dir (personal: keep it private)."
echo "Wake the pen (tip to screen) right before each yes."

step "1/5 one click in the air (the byte-exact verified send)" click
step "2/5 success chirp, low strength (25 % slider)" success --intensity 25
step "3/5 stop (never exercised — the experiment)" stop

echo | tee -a "$log"
echo "== 4/5 ink texture ==" | tee -a "$log"
"$oppd" haptic ink --cutoff-ms 2000 | tee -a "$log"
printf 'send, then immediately draw a few strokes: [y/N] '
read -r answer
if [ "$answer" = "y" ]; then
  "$oppd" haptic ink --cutoff-ms 2000 --send | tee -a "$log"
  echo "draw now; press Enter when done."
  read -r
  printf 'texture while touching, nothing in the air? (one line): '
  read -r felt
  echo "felt: $felt" | tee -a "$log"
else
  echo "skipped" | tee -a "$log"
fi

echo | tee -a "$log"
echo "== 5/5 stop mid-texture (needs step 4 sent) ==" | tee -a "$log"
printf 'send ink, draw, then stop mid-way? [y/N] '
read -r answer
if [ "$answer" = "y" ]; then
  "$oppd" haptic ink --cutoff-ms 2000 --send | tee -a "$log"
  echo "draw; press Enter to send Stop."
  read -r
  "$oppd" haptic stop --send | tee -a "$log"
  printf 'did the texture stop at once? (one line): '
  read -r felt
  echo "felt: $felt" | tee -a "$log"
else
  echo "skipped" | tee -a "$log"
fi

echo
echo "Done. Log in $dir/run.log."
echo "Success looks like: step 1 one crisp click, step 2 felt (even at"
echo "25 %), step 4 texture only while the tip touches, step 5 showing"
echo "whether Stop works on this firmware."
