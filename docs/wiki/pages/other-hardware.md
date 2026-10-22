# Your laptop, your pen — a different machine than the one documented here

This project has only been tested on one pairing: the ProArt PX13 with the SA205H pen
([devices](devices.md)). Other machines — Surface devices, HP and Lenovo convertibles,
other ASUS models such as the ROG Flow Z13 — haven't been tried yet. The daemon only
recognises the digitizer it was built for (`04F3:4631`, listed in
`crates/px13pen-core/src/lib.rs`), so on other hardware it stays idle rather than guessing.

If you have a different pen-capable machine, this page walks you through finding out what
it has using read-only checks. From there you can build support yourself, or send the
maintainer a hardware report to work from. For a Flow Z13 or similar, steps A–D below take
about an evening and never write anything to a device.

## A. What already works without anything installed

Inking (position, pressure, tilt, eraser, barrel) rides the kernel, not this project:
find the pen's evdev node and watch it.

```
sudo evtest            # pick the "Stylus" node; hover and draw
```

`tools/pen-test.html` opens in any browser and shows hover, pressure, tilt, azimuth,
eraser and barrel live — a quick read of what actually reaches userspace.

Ink arrives → the kernel path works for your digitizer, and everything below concerns
only the Bluetooth extras (pairing, haptics, battery, buttons). Nothing arrives → stop;
your digitizer needs kernel work first, which this project is not.

## B. Identify the hardware (all read-only)

```
sudo dmesg | grep -iE "i2c_hid|hid-multitouch"     # the digitizer on I2C-HID?
cat /sys/bus/hid/devices/*/modalias | grep -i i2c  # vendor:product, e.g. 04F3:xxxx
sudo dmidecode -s system-product-name
sudo dmidecode -s bios-version
```

Write these answers down — they are the first half of any hardware report.

## C. Capture what your pair actually exposes

The scripts in `tools/capture/linux/` were written for this project's laptop but most
are generic recorders — read each one's header comment first, they are read-only and ask
before anything that writes:

| Run | What it answers about *your* hardware |
|---|---|
| `01-digitizer-hid-recorder.sh` | does your digitizer report a pen at all, in what reports — and its full report descriptor, saved inside the capture |
| `02-btmon-advertise.sh` | does your pen advertise on its own (this pen never does), and with what name/appearance |
| `03-manual-pair.sh` | pair it by hand once; dumps the pen's GATT tree and Bluetooth HID descriptor — where haptics (`0x0E` usage page), battery and identity would live |

Decode step C's descriptor (the `hid-decode` tool ships with `nix develop`) and compare
against the [digitizer reference](reference/digitizer.md): **does your descriptor contain
a Usage Page `0xFFF4` collection** with reports `0x56`/`0x6E`/`0x6F`/`0x70`? That
collection is Microsoft's published seamless-pairing contract
([pairing](flows/pairing.md)) — its presence or absence splits the world cleanly:

- **0xFFF4 present** — touch-to-pair is *available in principle* on your machine, exactly
  as here; the rest is ID matching and quirks.
- **absent** — no touch-to-pair; the fallback path is your pen's button pairing mode plus
  ordinary Bluetooth pairing, and the extras reduce to whatever step C's GATT dump shows
  (a `0x0E` Simple Haptic Controller means haptics are at least reachable as plain HID
  writes).

## D. Decide

Everything step C recorded with the values that identify *your* devices is
**sensitive** — raw captures contain pairing keys, Bluetooth addresses and pen serials.
Before any of it leaves your machine, run it through the sanitizer (§ F); it replaces
every identifier it recognises with a freshly random value and tells you what it did.

## E. Build it yourself

> **Your hardware, your risk.** Nothing here is vendor guidance and there is no warranty
> (see the repository README's disclaimer). Exploration on an unfamiliar device must be
> read-only until *you* have proven what a channel is; the safety buckets in the
> [digitizer reference](reference/digitizer.md#vendor-collections-and-the-safety-buckets)
> exist because one of this device's collections is shaped exactly like a firmware-update
> channel. If a device starts misbehaving, stop and restart it — don't "try one more
> write".

The wiki is written for exactly this: the [conventions](conventions.md) labels tell you
which of its claims are hardware-specific, the [flows](flows/pairing.md) pages describe
the host-side sequences independent of *this* pen, and `tools/wiki/dissect.py` decodes
frames against the layout database (`docs/wiki/layouts.json`). A new machine is usually a
new digitizer ID plus new byte positions: keep the safety rule absolutely — vendor
collections other than the published `0xFFF4` set stay untouched until you have proven
what they are ([safety buckets](reference/digitizer.md#vendor-collections-and-the-safety-buckets)) —
and never flash firmware. `CONTRIBUTING.md` §6 states the same rules for contributors.

## F. Ask the maintainer instead — a hardware report

Open a **new issue** with the "Hardware report" template and fill it in; issues (not PRs)
are how the maintainer wants other hardware reported, because the useful material is data,
not code. Minimum useful contents:

1. step B's three lines;
2. step A's verdict (ink yes/no) and step C's descriptor — decoded text, or the `0xFFF4`
   section of it plus any `0x0E` collection you found;
3. the GATT dump from `03-manual-pair.sh`;
4. one or two example frames (announcement, battery notification…) as hex.

**Sanitize everything first** — `tools/capture/sanitize.py <file> -o <out>`: it randomises
Bluetooth addresses, long hex keys and anything you name with `--serial`, refuses raw
captures (`.hidrec`, `.pcap*`) and base64 blobs outright rather than pass them through,
never writes over the input, and prints a report of every substitution it made. Attach
only files it produced (or text it passed through — its report says which). If the report
says it randomised something inside a frame, say so in the issue; a synthetic example is
fine to discuss, a real identifier never is.

What happens next is honest and slow: the maintainer has only this laptop and this pen,
so a report can become wiki notes about your hardware family and, if it looks tractable,
an issue of its own — but code can only be verified when someone with that hardware runs
it. Expect that you may be asked to test.
