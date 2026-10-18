#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# One live tap-to-pair run with the daemon: `oppd watch-pair` bonds the pen
# while btmon records the HCI side. Before and after, the script captures
# the digitizer's seamless-pairing feature flag, and afterwards dumps
# everything BlueZ learned about the pen: device info, GATT tree (includes
# the battery service) and the input devices that appeared.
#
# This is the decisive run for the open question in
# docs/protocol/seamless-pairing.md §5 item 4: the daemon pairs with plain
# LE Just Works and writes no vendor output reports, so if this bonds, the
# digitizer key-relay output (0x6F) is optional.
#
# Usage (from the repo root, on the PX13):
#   cargo build -p oppd
#   tools/capture/linux/04-tap-to-pair.sh [outdir]
# Needs: read access to the digitizer hidraw node (input group membership)
# and the system D-Bus. sudo is only used for btmon.
#
# The dumps contain pairing material — treat the output folder like a
# password; never commit it.
set -euo pipefail

outdir="${1:-captures/linux}"
dir="$outdir/tap-to-pair-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$dir"
oppd="target/debug/oppd"
if [ ! -x "$oppd" ]; then
  echo "build the daemon first: cargo build -p oppd" >&2
  exit 1
fi

cp /proc/bus/input/devices "$dir/input-devices-before.txt"
"$oppd" pairing-state > "$dir/pairing-state-before.txt" 2>&1 || true

echo "btmon -> $dir/btmon.log"
sudo btmon -w "$dir/btmon.log" &
btmon_pid=$!
trap 'sudo kill "$btmon_pid" 2>/dev/null || true' EXIT

"$oppd" watch-pair > "$dir/watch-pair.log" 2>&1 &
oppd_pid=$!

echo
echo "The listener is running (watch it with: tail -f $dir/watch-pair.log)."
echo "Touch the pen tip to the screen and hold for a second. One bonding"
echo "cycle is enough. Then come back here and press Enter to stop."
read -r

kill "$oppd_pid" 2>/dev/null || true
sudo kill "$btmon_pid" 2>/dev/null || true
trap - EXIT
sleep 1

"$oppd" pairing-state > "$dir/pairing-state-after.txt" 2>&1 || true
bluetoothctl show > "$dir/bluetoothctl-show.txt" 2>&1 || true
bluetoothctl devices > "$dir/bluetoothctl-devices.txt" 2>&1 || true

# Per pen-looking device: BlueZ info plus the full GATT tree, which carries
# the battery percentage and the HID report map characteristics.
for d in $(bluetoothctl devices 2>/dev/null | awk 'tolower($0) ~ /pen|stylus|sa205/ {print $2}'); do
  bluetoothctl info "$d" > "$dir/info-$d.txt" 2>&1 || true
  gdbus introspect --system --dest org.bluez \
    --object-path "/org/bluez/hci0/dev_${d//:/_}" --recurse \
    > "$dir/gatt-$d.txt" 2>&1 || true
done

cp /proc/bus/input/devices "$dir/input-devices-after.txt"
diff -u "$dir/input-devices-before.txt" "$dir/input-devices-after.txt" \
  > "$dir/input-devices.diff" || true

echo "Done. Dump in $dir — it is capture evidence, keep it private."
echo "Success looks like: watch-pair.log shows 'paired and trusted',"
echo "info-*.txt shows Paired: yes Trusted: yes, gatt-*.txt shows Battery1."
