#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Record raw HID traffic from the ELAN digitizer with hid-recorder (hid-tools),
# plus the kernel's decoded view via evtest when available. Records what the
# chip sends while the pen hovers, touches, presses buttons and rests.
#
# Run this yourself (it uses sudo for /dev/hidraw). Nothing is written to the
# device — this is read-only capture.
#
# Usage (from the repo root):  tools/capture/linux/01-digitizer-hid-recorder.sh [outdir]
# Output: <outdir>/digitizer-<ts>.hidrec  (+ evtest-<ts>.log if evtest exists)
set -euo pipefail

outdir="${1:-captures/linux}"
mkdir -p "$outdir"
ts="$(date +%Y%m%d-%H%M%S)"

node=""
for u in /sys/class/hidraw/hidraw*/device/uevent; do
  [ -f "$u" ] || continue
  if grep -q 'ELAN9008' "$u"; then
    node="/dev/$(basename "$(dirname "$(dirname "$u")")")"
    break
  fi
done
if [ -z "$node" ]; then
  echo "No ELAN9008 hidraw node found (is the digitizer bound?)" >&2
  exit 1
fi

echo "Digitizer node: $node"
echo "Recording to $outdir/digitizer-$ts.hidrec"
echo
echo "Press Enter to start, then with the pen:"
echo "  1. hover without touching, ~5 s"
echo "  2. one slow stroke, light -> maximum pressure"
echo "  3. tilt sweep upright -> near-flat and back"
echo "  4. rotate the pen around its axis while drawing"
echo "  5. eraser end strokes"
echo "  6. barrel button held while drawing"
echo "  7. fast zig-zag, 5 s"
echo "  8. pen away from the screen, idle 10 s"
echo "Stop with Ctrl+C."
read -r

sudo hid-recorder "$node" | tee "$outdir/digitizer-$ts.hidrec"

if command -v evtest >/dev/null 2>&1; then
  ev=""
  ev="$(grep -B4 'Name=.*ELAN9008.*Stylus' /proc/bus/input/devices \
        | grep -o 'event[0-9]*' | head -1 || true)"
  if [ -n "$ev" ]; then
    echo "Also recording the kernel's decoded events from /dev/input/$ev for 60 s"
    echo "(repeat a hover + draw + buttons sequence now)."
    sudo timeout 60 evtest "/dev/input/$ev" > "$outdir/evtest-$ts.log" || true
  fi
fi
echo "Done: $outdir/digitizer-$ts.hidrec"
