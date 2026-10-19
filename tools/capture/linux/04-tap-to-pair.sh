#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# One live touch-to-pair run against the installed daemon: the session's
# own `px13pend` (started with the graphical session) bonds the pen while btmon
# records the HCI side. Before and after, the script captures the
# digitizer's seamless-pairing feature flag, and afterwards dumps
# everything BlueZ learned about the pen: device info, GATT tree (includes
# the battery service) and the input devices that appeared.
#
# This is the decisive run for the open question in
# docs/protocol/seamless-pairing.md §5 item 4: the daemon pairs with plain
# LE Just Works and writes no vendor output reports, so if this bonds, the
# digitizer key-relay output (0x6F) is optional.
#
# Usage (from the repo root, with the package installed — on NixOS the
# module, services.px13-sa205h-linux.enable — and logged in again since):
#   tools/capture/linux/04-tap-to-pair.sh [outdir]
# Keep `px13penctl watch` open in a second terminal to follow it live; a new
# bond also raises a desktop notification. Needs: the installed package
# (uaccess rule, user service, pairing helper) and sudo for btmon only.
#
# The dumps contain pairing material — treat the output folder like a
# password; never commit it.
set -euo pipefail

outdir="${1:-captures/linux}"
dir="$outdir/tap-to-pair-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$dir"

daemon() {
  busctl --user call io.github.gonzalezerik.Px13Pen.Daemon1 /io/github/gonzalezerik/Px13Pen/Daemon \
    io.github.gonzalezerik.Px13Pen.Daemon1 "$@"
}

if ! systemctl --user is-active --quiet px13pend.service; then
  echo "px13pend is not running — starting it (it normally starts with the session)"
  systemctl --user start px13pend.service
fi
systemctl --user status px13pend.service --no-pager > "$dir/px13pend-status.txt" 2>&1 || true
journalctl --user -u px13pend.service -f -o short-precise > "$dir/px13pend.log" 2>&1 &
journal_pid=$!
px13penctl watch --json > "$dir/watch.jsonl" 2>&1 &
watch_pid=$!
trap 'kill "$journal_pid" "$watch_pid" 2>/dev/null || true' EXIT

cp /proc/bus/input/devices "$dir/input-devices-before.txt"
px13pend pairing-state > "$dir/pairing-state-before.txt" 2>&1 || true

echo
echo "Dry round first — the daemon's listener is paused and nothing touches"
echo "Bluetooth; this only checks that the digitizer announces the pen."
daemon StopPairing > /dev/null
px13pend watch-pair --dry-run > "$dir/watch-pair-dry.log" 2>&1 &
dry_pid=$!
echo "Hover the pen over the screen (or touch it) once, then press Enter."
read -r
kill "$dry_pid" 2>/dev/null || true
echo "--- dry round saw: ---"
head -n 12 "$dir/watch-pair-dry.log" || true

echo
echo "btmon needs sudo; asking now, before anything runs in the background:"
sudo -v
echo "btmon -> $dir/btmon.log"
sudo btmon -w "$dir/btmon.log" > /dev/null &
btmon_pid=$!
trap 'sudo kill "$btmon_pid" 2>/dev/null || true; kill "$journal_pid" "$watch_pid" 2>/dev/null || true' EXIT

daemon StartPairing > /dev/null
echo
echo "The daemon is listening again. Bring the pen near the screen and keep it"
echo "there a few seconds: px13penctl watch shows it being found and paired, and a"
echo "notification says the pen is paired. Then press Enter here to stop."
read -r

sudo -v
sudo kill "$btmon_pid" 2>/dev/null || true
kill "$journal_pid" "$watch_pid" 2>/dev/null || true
trap - EXIT
sleep 1

px13pend pairing-state > "$dir/pairing-state-after.txt" 2>&1 || true
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
echo "Success looks like: px13pend.log shows 'paired and trusted', a 'paired'"
echo "notification appeared, info-*.txt shows Paired: yes Trusted: yes, and"
echo "gatt-*.txt shows Battery1."
