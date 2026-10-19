#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Pair the pen by touch, the way Windows does, as one standalone run.

This is the host side of Microsoft's "Seamless Pairing for Windows Pens
Implementation Guide", driven by hand against the installed daemon's
digitizer node and stock BlueZ. It is the reference run behind
docs/protocol/seamless-pairing.md and findings/008:

1. SET FEATURE 0x56 (Host Bluetooth Report): this host's LE address,
   little-endian, and its type (0 public, 1 random).
2. SET FEATURE 0x70 (Device Capability Report): seamless capable = 1, then
   GET FEATURE 0x70.
3. Start an LE scan through an interactive bluetoothctl whose default agent
   is KeyboardOnly. The scan must already run when the pen is touched: the
   pen's advertisement follows the announcement within milliseconds.
4. Wait for input 0x6E (Device Bluetooth Report): pen address, address
   type (bit 0) and Host Pairing Exists (bit 1).
5. Write output 0x6F (Pairing Key Report): the announced address, its type,
   and a 16-byte key of zeros. This is what makes the pen advertise
   (connectable, as "ASUS Pen", under the identity of its bond for this
   host, which need not be the announced address).
6. Pair that advertiser through BlueZ and answer the passkey request with
   0. The pen requires MITM protection and reports KeyboardOnly; with both
   sides KeyboardOnly the method is passkey entry, and a pen with nothing to
   type with enters 0. Linux cannot take the out-of-band path Windows uses:
   the kernel has no LE legacy out-of-band key support (see the protocol
   document), so the 16-byte key itself is not used by this pairing.

Usage (from the repo root, with the package installed, the pen NOT in its
button pairing mode):
  python3 tools/capture/linux/06-seamless-pairing.py
Then touch the pen to the screen when asked. A pen that just failed a
pairing may skip one touch; touch again a few seconds later.

Environment:
  SEAMLESS_WAIT=<s>             how long to wait for the touch (default 60)
  SEAMLESS_HOST=<addr>          register this host address instead of the
  SEAMLESS_HOST_TYPE=random     adapter's (e.g. a random static identity,
                                to test a host the pen has never paired)
  SEAMLESS_EXTERNAL_BTMON=1     do not start btmon (the caller records it)
  SEAMLESS_KEEP_PAUSED=1        leave the daemon's own listener paused

sudo is asked for once, for btmon. The output folder holds pairing
material; keep it private.
"""
import array
import fcntl
import os
import re
import select
import subprocess
import sys
import time

ESCAPES = re.compile(r"\x1b\[[0-9;]*m")


def hidioc(nr, size, direction):
    return (direction << 30) | (size << 16) | (ord("H") << 8) | nr


def set_feature(fd, data):
    buf = array.array("B", data)
    fcntl.ioctl(fd, hidioc(0x06, len(data), 3), buf, True)


def get_feature(fd, report_id, size=64):
    buf = array.array("B", [report_id] + [0] * (size - 1))
    n = fcntl.ioctl(fd, hidioc(0x07, size, 3), buf, True)
    return bytes(buf[:n])


def hexs(data):
    return " ".join(f"{b:02x}" for b in data)


def find_digitizer():
    for node in sorted(os.listdir("/sys/class/hidraw")):
        dev = os.path.basename(os.path.realpath(f"/sys/class/hidraw/{node}/device"))
        if dev.upper().startswith("0018:04F3:4631"):
            return f"/dev/{node}"
    sys.exit("no ELAN 04F3:4631 digitizer hidraw node")


def adapter_address():
    out = subprocess.run(["bluetoothctl", "show"], capture_output=True, text=True,
                         stdin=subprocess.DEVNULL).stdout
    m = re.search(r"Controller ((?:[0-9A-F]{2}:){5}[0-9A-F]{2}) \(public\)", out)
    if not m:
        sys.exit("no public controller address in `bluetoothctl show`")
    return m.group(1)


def daemon(method):
    subprocess.run(["busctl", "--user", "call", "org.openpenprotocol.Daemon1",
                    "/org/openpenprotocol/Daemon", "org.openpenprotocol.Daemon1", method],
                   capture_output=True)


def main():
    wait = int(os.environ.get("SEAMLESS_WAIT", "60"))
    outdir = time.strftime("captures/linux/seamless-pairing-%Y%m%d-%H%M%S")
    os.makedirs(outdir, exist_ok=True)
    log = open(f"{outdir}/run.log", "w")

    def say(line):
        now = time.time()
        stamp = time.strftime("%H:%M:%S", time.localtime(now)) + f".{int(now * 1000) % 1000:03d}"
        print(f"[{stamp}] {line}", flush=True)
        log.write(f"[{stamp}] {line}\n")
        log.flush()

    btmon = None
    if not os.environ.get("SEAMLESS_EXTERNAL_BTMON"):
        print("btmon needs sudo:")
        subprocess.run(["sudo", "-v"], check=True)
        btmon = subprocess.Popen(["sudo", "btmon", "-w", f"{outdir}/btmon.log"],
                                 stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    daemon("StopPairing")
    say("the daemon's own pairing listener is paused for this run")

    path = find_digitizer()
    fd = os.open(path, os.O_RDWR | os.O_NONBLOCK)
    host = os.environ.get("SEAMLESS_HOST") or adapter_address()
    host_type = 1 if os.environ.get("SEAMLESS_HOST_TYPE") == "random" else 0
    host_wire = bytes(int(b, 16) for b in host.split(":"))[::-1]
    say(f"digitizer {path}; registering host address {host} (type {host_type})")
    for report in ([0x56, *host_wire, host_type], [0x70, 0x01]):
        try:
            set_feature(fd, report)
            say(f"SET FEATURE {report[0]:#04x}: {hexs(report)}")
        except OSError as error:
            say(f"SET FEATURE {report[0]:#04x} failed: {error}")
    try:
        say(f"GET FEATURE 0x70: {hexs(get_feature(fd, 0x70))}")
    except OSError as error:
        say(f"GET FEATURE 0x70 failed: {error}")

    # Forget unpaired pens BlueZ still lists, so the next advertisement
    # shows up as a "[NEW] Device <addr> ASUS Pen" line with its name.
    listed = subprocess.run(["bluetoothctl", "devices"], stdin=subprocess.DEVNULL,
                            capture_output=True, text=True).stdout
    paired = subprocess.run(["bluetoothctl", "devices", "Paired"], stdin=subprocess.DEVNULL,
                            capture_output=True, text=True).stdout
    for line in listed.splitlines():
        m = re.search(r"Device ((?:[0-9A-F]{2}:){5}[0-9A-F]{2}) ASUS Pen", line)
        if m and m.group(1) not in paired:
            subprocess.run(["bluetoothctl", "remove", m.group(1)], stdin=subprocess.DEVNULL,
                           capture_output=True)
            say(f"forgot a stale unpaired pen entry {m.group(1)}")

    # bluetoothctl registers its agent at start-up, so the capability goes on
    # the command line; "agent <cap>" later is refused as already registered.
    ctl_log = f"{outdir}/bluetoothctl.txt"
    ctl_proc = subprocess.Popen(["bluetoothctl", "--agent", "KeyboardOnly"],
                                stdin=subprocess.PIPE, stdout=open(ctl_log, "w"),
                                stderr=subprocess.STDOUT, text=True, bufsize=1)

    def ctl(command):
        ctl_proc.stdin.write(command + "\n")
        ctl_proc.stdin.flush()

    def ctl_text():
        return ESCAPES.sub("", open(ctl_log).read())

    time.sleep(1)
    ctl("default-agent")
    time.sleep(0.5)
    ctl("scan le")
    time.sleep(1)
    say("bluetoothctl ready: KeyboardOnly default agent, scanning")

    print("\nNow touch the pen to the screen.", flush=True)
    deadline = time.time() + wait
    note = None
    while time.time() < deadline and note is None:
        ready, _, _ = select.select([fd], [], [], 1)
        if not ready:
            continue
        try:
            frame = os.read(fd, 64)
        except BlockingIOError:
            continue
        if len(frame) == 8 and frame[0] == 0x6E:
            note = frame
    if note is None:
        say(f"no announcement (0x6E) within {wait} s")
        return finish(fd, btmon, ctl, ctl_proc, say)
    pen_wire = note[1:7]
    pen = ":".join(f"{b:02X}" for b in pen_wire[::-1])
    flags = note[7]
    say(f"0x6E: {hexs(note)} -> pen {pen}, random={bool(flags & 1)}, "
        f"host pairing exists={bool(flags & 2)}")

    key_report = bytes([0x6F]) + pen_wire + bytes([flags & 0x01]) + bytes(16)
    try:
        os.write(fd, key_report)
        say(f"OUTPUT 0x6F: {hexs(key_report[:8])} + 16-byte key (zeros)")
    except OSError as error:
        say(f"OUTPUT 0x6F failed: {error}")

    found = None
    for _ in range(2500):
        text = ctl_text()
        m = re.search(r"Device ((?:[0-9A-F]{2}:){5}[0-9A-F]{2}) ASUS Pen", text) or \
            re.search(rf"Device ({re.escape(pen)})\b", text)
        if m:
            found = m.group(1)
            break
        time.sleep(0.01)
    if not found:
        say("the pen did not advertise within 25 s")
        return finish(fd, btmon, ctl, ctl_proc, say)
    say(f"pen advertising as {found} (announced as {pen}); pairing")

    ctl(f"pair {found}")
    answered = False
    outcome = "no answer in 25 s"
    for _ in range(2500):
        text = ctl_text()
        if not answered and "Enter passkey" in text:
            ctl("0")
            answered = True
            say("passkey requested; entered 0")
        if "Pairing successful" in text:
            outcome = "Pairing successful"
            break
        m = re.search(r"Failed to pair: \S+", text)
        if m:
            outcome = m.group(0)
            break
        time.sleep(0.01)
    say(f"bluetoothctl: {outcome}")
    if outcome == "Pairing successful":
        ctl(f"trust {found}")
        time.sleep(2)
        info = subprocess.run(["bluetoothctl", "info", found], stdin=subprocess.DEVNULL,
                              capture_output=True, text=True).stdout
        for line in info.splitlines():
            if re.search(r"Name|Paired|Bonded|Trusted|Connected|Battery", line):
                say(line.strip())
    return finish(fd, btmon, ctl, ctl_proc, say)


def finish(fd, btmon, ctl, ctl_proc, say):
    os.close(fd)
    ctl("scan off")
    ctl("quit")
    try:
        ctl_proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        ctl_proc.terminate()
    if btmon is not None:
        subprocess.run(["sudo", "kill", str(btmon.pid)], capture_output=True)
    if not os.environ.get("SEAMLESS_KEEP_PAUSED"):
        daemon("StartPairing")
        say("the daemon's listener is back on")
    say("done; the capture folder holds this log, the bluetoothctl session and btmon")


if __name__ == "__main__":
    main()
