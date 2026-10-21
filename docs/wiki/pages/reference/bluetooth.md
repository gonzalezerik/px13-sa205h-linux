# Bluetooth reference — the pen's GATT side

What the pen exposes over Bluetooth LE once connected: standard services
plus a HID report set and four vendor services. HID reports ride HID over
GATT; the daemon reaches everything through BlueZ or the pen's `hidraw`
node. [VERIFIED-HW — findings/006, findings/010 §1]

## Services

| Service | UUID | Used by this project |
|---|---|---|
| Generic Access / Attribute, HID (`0x1812`), Battery, Device Information (`0x180A`) | standard | yes |
| Nordic UART Service | `6e400001-b5a3-f393-e0a9-e50e24dcca9e` | **read side only** — TX notifies the [debug log](../flows/battery.md); RX is never written |
| vendor | `a6ed0401-…` | never touched: notify + write-without-response characteristics, silent in every session |
| vendor | `58480000-5a6a-1122-3300-004095112200` | never touched: single write/notify characteristic, silent |
| vendor | `a6ed0801-…` | never touched: notify + write/indicate, silent |

The three silent vendor services look like command/response channels
(notify/indicate beside write); what they do is unknown [HYPOTHESIS:
update/test/config]. The standing rule keeps them untouched: notifications
and plain reads only, writes never. [VERIFIED-HW: "silent in any session",
findings/010 §1, §4]

Device Information answers with plain reads [VERIFIED-HW findings/010]:
firmware revision string (with build date) from `0x2A26`, PnP ID naming
vendor `0x330A` product `0x0001`.

## HID reports (report IDs and the Linux transport quirk)

| Report | Kind | Direction | What |
|---|---|---|---|
| `0x43` | Input | pen→host | top-button gestures; one distinct report per gesture — the pen classifies single/double/hold itself [VERIFIED-HW findings/006 §5] |
| `0x41` | Output | host→pen | haptic playback, 7 bytes — see below |
| `0x41` | Feature | read | current playback state (repeat 0, intensity `FF`, auto-trigger 1) [VERIFIED-HW findings/006 §4] |
| `0x42` | Feature | read | capabilities: waveform list, durations, auto-trigger source = Tip Switch [VERIFIED-HW findings/006 §4] |
| `0x40` | Feature | read | pen identity — see below |

**The quirk:** the pen exports **no Report Reference descriptors at all**,
so on Linux a HID *feature get* has nothing to map a report ID onto and is
rejected. The identity report answers instead as a plain readable
characteristic on the HID service (Bluetooth SIG's HID Report characteristic,
`0x2A4D`). [VERIFIED-HW findings/006 §2, docs/decisions.md]

The battery percentage arrives as a notification on its own fixed handle
(the same value Windows' battery UI showed) [VERIFIED-HW findings/006];
details in [battery](../flows/battery.md).

## Report layouts

### `0x40` — identity [VERIFIED-HW findings/006 §2]

```
byte 0     : Report ID = 40
bytes 1..5 : transducer serial, 32-bit little-endian
bytes 5..7 : vendor id, 0x330A (wire 0A 33)
bytes 7..9 : 0x0C0F   (constant so far — meaning unknown [HYPOTHESIS])
bytes 9..11: 0x0003   (constant so far — meaning unknown [HYPOTHESIS])
remainder  : zero padding
```

The serial is byte-for-byte the number the digitizer's reports 7 and 34
stream — the one identifier both links agree on, and what the daemon
matches pens by. On Linux read it from `0x2A4D` (ten bytes, serial then
vendor) because of the quirk above.

### `0x41` output — haptic playback [VERIFIED-HW findings/006 §4, findings/009]

```
byte 0   : Report ID = 41
byte 1   : repeat count (extra plays after the first)
byte 2   : intensity 0–0xFF (0xFF = full; scaled by user setting)
byte 3   : waveform ORDINAL (1 = None — the real stop, 2 = Stop (inert on
           this firmware), 3+ = from the feature-0x42 list)
byte 4   : cutoff ceiling, units of 10 ms (00 = none)
bytes 5–6: retrigger period, ms, little-endian (0000 = none)
```

One write per effect; five HID usages (0x0E:0x21/0x23/0x24/0x25/0x28)
compose into it, mirroring what `ISM.dll` writes on Windows [STATIC
docs/protocol/haptics.md §2.2]. Windows' exact per-effect field values
were never captured (the trace truncates) — everything verified here uses
the shape `41 01 FF <ordinal> 00 00 00`. [VERIFIED-HW §2.9]

### Feature `0x42` — capabilities [VERIFIED-HW findings/006 §4]

Auto-trigger control names **Tip Switch** as the start condition; the
waveform list is `0x1003, 0x1006–0x1011` (click, press, release, hover,
success, error + continuous ink/pencil/marker/chisel/brush/eraser/sparkle)
at ordinals 3–15, with firmware durations
`[45, 60, 60, 35, 145, 185, 1995, 1995, 1995, 1995, 1980, 1995, 1980]` ms —
continuous effects carry ≈2 s durations, not the spec's 0. No
Collide/Align/Step/Grow on this pen.

Machine-readable copies of every layout on the two reference pages live
with the source: `docs/wiki/layouts.json`, and
`tools/wiki/dissect.py` prints the named fields of any frame you paste
into it (`--list` shows what it knows).
