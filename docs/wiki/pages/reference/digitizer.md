# Digitizer reference — ELAN9008 `04F3:4631`

The screen-side controller: an ELAN9008 on I2C-HID (`AMDI0010:00`/`i2c-0`),
USB identity `04F3:4631`, a 1 219-byte "Windows 8 certified" report
descriptor, bound to the kernel's `hid-multitouch`. [VERIFIED-HW — the
descriptor fixture in `fixtures/px13-hn7306/`]

It receives the pen's 2.4 GHz bursts, turns them into HID reports, and
hosts the seamless-pairing collection. The full static analysis with
safety classification is `docs/protocol/digitizer-tlc.md`; this page is its
reference view.

## Reports at a glance

From the descriptor [STATIC]; per-collection byte layouts are given only
where a capture confirmed them.

| Report | Collection | Direction | Size | What it is |
|---|---|---|---|---|
| 1 | Touch Screen (0x0D/0x04) | Input | 60 B | up to 5 fingers; `hid-multitouch` owns it |
| 7 | Pen (0x0D/0x02) | Input | 24 B | **the pen report** — see below |
| 34 | 0xFF0F/0x50 | Input | 16 B | pen-presence telemetry; carries the transducer serial |
| 2 / 3 | 0x01FF / 0xFF00/0x01 | Input / **Output** | 64 / 32 B | maintenance response/command pair — **never written** |
| 23 | 0xFF00/0x81 | Input | 64 B | silent status channel (negative result in capture) |
| 25 | 0xFF0F/0x60 | Output + Input | 4 + 4 B | smallest command pair; consumer unknown |
| 68 | 0xFF00/0xC5 | Feature | 256 B | unproven blob — do not read |
| 41–45, 46–54, 72 | 0xFF0B/0x0B | Feature + Input | up to 7 487 B | block/flash pattern — **untouchable** |
| 86, 110, 111, 112 | 0xFFF4/0x01 | Feature + Input + Output | 1–23 B | seamless pairing — see [pairing](../flows/pairing.md) |

## Report 7 — the pen report

Usages and ranges [STATIC: descriptor; VERIFIED-HW: a Linux capture carried
13 284 report-7 frames covering hover, pressure ramps, tilt, eraser and
barrel presses (findings/006 §3)]:

| Usage (0x0D) | Name | Range / values |
|---|---|---|
| 0x32 | In Range | bit; hover = set, Tip Switch clear |
| 0x42 | Tip Switch | bit |
| 0x44 | Barrel Switch | bit |
| 0x3C / 0x45 | Invert / Eraser | bits (eraser model: invert) |
| 0x5A | Secondary Barrel Switch | bit |
| 0x30/0x31 | X / Y | 16-bit, 0..17664 × 0..10496 |
| 0x30/0x0D | Tip Pressure | 0–4096 |
| 0x3B | Battery Strength | raw 0–255; percentage mapping [HYPOTHESIS] |
| 0x3D/0x3E | X/Y Tilt | signed ±90.00° |
| 0x3F | Azimuth | 0–36000 (0.01°) |
| 0x56 | Scan Time | |
| 0x5B | **Transducer Serial Number** | 32-bit, little-endian |
| 0x91 | **Transducer Vendor ID** | 16-bit; `0x330A` here |

Byte positions confirmed from the Linux capture [VERIFIED-HW, findings/006
§3 — report id as byte 0], in the finding's own interval notation (open at
the end):

```
byte  0      : Report ID = 07
bytes 17..21 : transducer serial (32-bit, little-endian; valid while In Range)
bytes 21..23 : transducer vendor id, 0x330A (wire bytes 0A 33)
other offsets: not recorded per-field; parse the descriptor fixture
```

Report 34's first field repeats the same serial [VERIFIED-HW]; report 34
precedes every pairing announcement by milliseconds (see
[reconnect](../flows/reconnect.md)).

## Vendor collections and the safety buckets

The project's hard rule: no writes to vendor report sets, no firmware
channels, reads only where classified. The classification [STATIC +
VERIFIED-HW, `docs/protocol/digitizer-tlc.md` §3]:

| Channel | Bucket | Why |
|---|---|---|
| R2/R3 (`0xFF00/0x01` + `0x01FF`) | **dangerous** | command/response pair; the Windows WoT service writes six known commands through R3 (enable/disable, IC sleep/wake, IC reset, FW query — findings/005). Passive read of R2 is harmless; nothing is initiated |
| `0xFF0B` features/inputs | **dangerous** | flash-page block sizes (15 → 7 487 B): the firmware-update shape. Never read, never written |
| R68 (256 B feature) | command-adjacent | unproven; GET_FEATURE needs a proven-safe case first |
| R25 (4+4 B pair) | command | consumer unknown (and *not* wake-on-touch — disproved, findings/005) |
| R23 (64 B input) | safe-to-read | passive; stayed silent through an 84 s capture of hover/ink/erase/barrel |
| R34 (16 B input) | safe-to-read | pen-presence telemetry (serial + three zero words) |
| `0xFFF4` pairing set | command (pairing only) | the one write (`0x6F`) is allowed only inside the daemon's pairing flow |

## Node map

Linux sees seven input nodes: the touchscreen, one "Stylus", and five
"UNKNOWN" nodes for the vendor collections. One collection (0xFF0B) appears
to register no node — its 7 487-byte input exceeds the I2C-HID buffer —
while still consuming an input number [HYPOTHESIS, tlc §3.1]. None of this
matters to the daemon: `hidraw` exposes the whole device either way.
