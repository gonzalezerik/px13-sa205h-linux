#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# One manual pairing of the pen, typed by you, while btmon records; then dump
# everything Linux learned about the pen: BlueZ device + GATT tree, the BT HID
# report descriptor, and the input devices that appeared.
#
# This script does NOT pair anything itself — bluetoothctl stays in your
# hands. It only records and dumps.
#
# Usage (from the repo root):
#   tools/capture/linux/03-manual-pair.sh <BT-ADDRESS> [outdir]
# Output: <outdir>/pair-<ts>/ (btmon log, bluez dumps, rdesc if readable)
set -euo pipefail

addr_raw="${1:?usage: 03-manual-pair.sh <BT-ADDRESS> [outdir]}"
outdir="${2:-captures/linux}"
addr="$(echo "$addr_raw" | tr '[:lower:]' '[:upper:]')"
ts="$(date +%Y%m%d-%H%M%S)"
dir="$outdir/pair-$ts"
mkdir -p "$dir"
dev_path="/org/bluez/hci0/dev_${addr//:/_}"

cp /proc/bus/input/devices "$dir/input-devices-before.txt"

echo "btmon -> $dir/btmon.log (Ctrl+C in this terminal when pairing is done)"
sudo btmon -w "$dir/btmon.log" &
btmon_pid=$!
trap 'sudo kill "$btmon_pid" 2>/dev/null || true' EXIT

echo
echo "Now, in ANOTHER terminal, pair the pen (you type these):"
echo "  bluetoothctl"
echo "  power on"
echo "  scan on          # wait until the pen shows up, note its name"
echo "  pair $addr"
echo "  trust $addr"
echo "  quit"
echo "Press Enter here once the pen is paired (or type 'skip' + Enter to abort pairing)."
read -r ans
sudo kill "$btmon_pid" 2>/dev/null || true
trap - EXIT
if [ "${ans:-}" = "skip" ]; then exit 0; fi

sleep 2
bluetoothctl show > "$dir/bluetoothctl-show.txt" 2>&1 || true
bluetoothctl devices > "$dir/bluetoothctl-devices.txt" 2>&1 || true
bluetoothctl info "$addr" > "$dir/bluetoothctl-info.txt" 2>&1 || true
gdbus introspect --system --dest org.bluez --object-path "$dev_path" \
  --recurse > "$dir/bluez-gatt-introspect.txt" 2>&1 || true

cp /proc/bus/input/devices "$dir/input-devices-after.txt"
diff -u "$dir/input-devices-before.txt" "$dir/input-devices-after.txt" \
  > "$dir/input-devices.diff" || true

# BT HID report descriptor: copy every debugfs rdesc (they are small; the pen's
# is the one whose directory name appeared in input-devices.diff), and try
# hidrd-recv on pen-named hidraw nodes. The btmon log also contains the
# GET_DESCRIPTOR exchange as a fallback.
sudo bash -c 'd="'"$dir"'"; mkdir -p "$d/rdescs"
for r in /sys/kernel/debug/hid/*/rdesc; do
  [ -f "$r" ] && cat "$r" > "$d/rdescs/$(basename "$(dirname "$r")").rdesc" 2>/dev/null
done' || true
for u in /sys/class/hidraw/hidraw*/device/uevent; do
  [ -f "$u" ] || continue
  grep -qiE 'pen|stylus' "$u" 2>/dev/null || continue
  n="/dev/$(basename "$(dirname "$(dirname "$u")")")"
  if command -v hidrd-recv >/dev/null 2>&1; then
    sudo hidrd-recv "$n" > "$dir/pen-rdesc-hidrd-$n.txt" 2>/dev/null || true
  fi
done
for u in /sys/class/hidraw/hidraw*/device/uevent; do
  [ -f "$u" ] && cat "$u"
done > "$dir/hidraw-uevents.txt" 2>/dev/null || true

echo "Done. Dump in $dir — send the whole folder back as capture evidence."
