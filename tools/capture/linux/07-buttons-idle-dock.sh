#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Three short read-only observations, in one sitting:
#
#   A. what evdev events each pen button produces — the top button's codes
#      have never been named (docs/dbus.md §9 is waiting on them);
#   B. how long an untouched pen stays connected off its charger, and that
#      the plain idle sleep really carries no "Charge State:" line
#      (findings/010 §4 open point) — two runs, this sets the tray's
#      grace period;
#   C. that Docked flips true on the charger and false again after a lift,
#      seen through the running daemon — plus a parallel evdev capture to
#      see whether dock/undock also arrives as keyboard keys.
#
# Nothing is ever written to the pen: the log reader only enables
# notifications (findings/010), and the button watcher only reads evdev.
#
# Prerequisites: the pen is paired (04-tap-to-pair.sh) and trusted; the
# daemon's D-Bus service is running for step C (the packaged px13pend
# serve, or `result/bin/px13pend serve` in another terminal); uaccess
# access to the pen's input nodes (docs/packaging.md).
#
# Usage (from the repo root, on the PX13):
#   nix build .                     # or: cargo build --release -p px13pend
#   tools/capture/linux/07-buttons-idle-dock.sh [outdir]
# Set PX13PEND=/path/to/px13pend to use a different daemon build, and
# PX13PENCTL=/path/to/px13penctl to talk to a different daemon.
#
# The log is personal (it carries the pen's own identifiers): keep it
# private, never commit it.
set -euo pipefail

outdir="${1:-captures/linux}"
dir="$outdir/buttons-idle-dock-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$dir"
px13pend="${PX13PEND:-result/bin/px13pend}"
if [ ! -x "$px13pend" ]; then
  echo "build the daemon first: nix build . (result/bin/px13pend) or" >&2
  echo "cargo build --release -p px13pend — or point PX13PEND at a built binary" >&2
  exit 1
fi
px13penctl="${PX13PENCTL:-px13penctl}"
log="$dir/run.log"

echo "Buttons, idle sleep and dock — log in $dir (personal: keep it private)."
echo "Every step waits for Enter; nothing is timed out on you."
echo "Wake the pen (tip to screen) whenever it has gone quiet."

# Run a command in the background until Enter, with its output in $1.
watch_until_enter() {
  local file="$1"
  shift
  read -r
  "$@" >"$file" 2>&1 &
  local pid=$!
  read -r
  kill "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
  sed 's/^/  | /' "$file" | tee -a "$log"
}

echo | tee -a "$log"
echo "== A. buttons: one gesture per capture ==" | tee -a "$log"
for gesture in top-single top-double top-long barrel-single barrel-double; do
  echo | tee -a "$log"
  echo "-- $gesture: wake the pen. Enter starts the watcher — do exactly"
  echo "   that one gesture, then Enter again to stop it. --"
  watch_until_enter "$dir/buttons-$gesture.log" "$px13pend" watch-buttons
  printf 'what did the watcher print for it? (one line, empty ok): '
  read -r named
  echo "$gesture: $named" | tee -a "$log"
done

echo | tee -a "$log"
echo "== B. idle sleep, two runs ==" | tee -a "$log"
echo "Each run: pen awake, OFF the charger, set down completely still."
echo "Start the log reader, leave it alone until the reader prints"
echo "'link down', then Enter. Watch the clock for the idle minutes."
for run in 1 2; do
  echo | tee -a "$log"
  echo "-- run $run: wake the pen, hold it a moment, then Enter to start"
  echo "   the reader. Note the clock, set the pen down still and off the"
  echo "   charger, and Enter again when the reader prints 'link down'. --"
  watch_until_enter "$dir/idle-run$run.log" "$px13pend" pen-log
  printf 'minutes from last touch to link drop: '
  read -r minutes
  printf 'PEN ENTER SLEEP appeared with NO Charge State line before it? [y/N] '
  read -r plain
  echo "idle run $run: ${minutes:-?} min, plain sleep (no Charge State): ${plain:-?}" | tee -a "$log"
done

echo | tee -a "$log"
echo "== C. dock and lift, through the running daemon ==" | tee -a "$log"
if ! command -v "$px13penctl" >/dev/null 2>&1; then
  echo "px13penctl not on PATH (set PX13PENCTL); skipping C." | tee -a "$log"
else
  echo "Watching the daemon and the buttons together. Put the pen on its"
  echo "charger; when the daemon shows it on the charger, Enter. Then lift"
  echo "it, wait for the flag to clear, Enter."
  "$px13pend" watch-buttons >"$dir/dock-buttons.log" 2>&1 &
  buttons_pid=$!
  "$px13penctl" watch >"$dir/dock-watch.log" 2>&1 &
  watch_pid=$!
  echo "on the charger now; Enter when the daemon shows it:"
  read -r
  echo "lifted; Enter when it clears:"
  read -r
  kill "$watch_pid" "$buttons_pid" 2>/dev/null || true
  wait "$watch_pid" "$buttons_pid" 2>/dev/null || true
  echo "-- daemon --" | tee -a "$log"
  sed 's/^/  | /' "$dir/dock-watch.log" | tee -a "$log"
  echo "-- buttons (dock keys?) --" | tee -a "$log"
  sed 's/^/  | /' "$dir/dock-buttons.log" | tee -a "$log"
  printf 'Docked went true on the charger and false after the lift? [y/N] '
  read -r docked
  printf 'any key events during dock/undock? (one line): '
  read -r keys
  echo "dock: flipped=${docked:-?}, keys=${keys:-none}" | tee -a "$log"
fi

echo
echo "Done. Log in $dir/run.log."
echo "Success looks like: A names a distinct code/value per gesture;"
echo "B shows PEN ENTER SLEEP with no Charge State line, link down a"
echo "similar number of minutes both runs; C shows the flag flip both"
echo "ways, and the buttons log answers whether dock keys exist on Linux."
