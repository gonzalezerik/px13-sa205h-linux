# How an MPP pen connects

The pen lives on two independent links at once. Almost everything that makes it feel
like a pen runs on the first one and needs no pairing at all; Bluetooth carries only
the extras — and the unusual part is how the host learns a Bluetooth address to
connect to. Each fact below is marked **[public]** (a published specification or
vendor documentation) or **[ours]** (reverse-engineered here; see the findings
documents cited at the end).

```
PEN
 ├─ link 1 (always on, no pairing): MPP air protocol over 2.4 GHz
 │     ► ELAN9008 digitizer controller ► HID over I2C ► host
 │     hover · ink · pressure · tilt · barrel · eraser · serial · battery strength
 │                    │
 │                    └─ report 0x6E on the "ELAN Seamless Pairing" collection:
 │                       the pen's BLE address, handed up on tip touch [ours]
 │
 └─ link 2 (Bluetooth LE): the pen NEVER advertises [ours]
       ► host connects to the address from report 0x6E
       HID over GATT: top button (0x43) · haptics (0x41 out) ·
       battery notifications · identity/capability features [public transport]
```

## Link 1: the screen link — everything works before any pairing

An active MPP pen talks to the touch sensor itself: the pen's bursts are received by
the digitizer controller (here an ELAN9008, `04F3:4631`), which turns them into
ordinary HID input reports over I2C. The operating system never sees the air
protocol — the controller hides it. **[ours: the protocol itself is not publicly
documented; its existence is inferred from the controller's outputs.]**

The pen's HID report (report ID 7 on this device) carries, per field: In Range, Tip
Switch, Barrel Switch, Invert, Eraser, X/Y position, Tip Pressure (0–4096), Battery
Strength, X/Y Tilt, Azimuth, Scan Time, the 32-bit transducer serial and the vendor
ID (`0x330A`). The usage semantics are public **[public: USB HID Usage Tables,
Digitizer Page `0x0D`]**; the exact layout is this device's report descriptor
**[ours]**.

That is why a bare Linux box already gets real pen input: the kernel's
`i2c-hid`/`hid-multitouch` driver reads the same report the Windows in-box driver
does. Hover is just `In Range` set before `Tip Switch` — a stylus event with no tip
bit **[public]**. In a Linux capture with the pen bonded to neither host, 13,284 of
13,303 digitizer frames were report 7: hover, a pressure ramp, tilt, eraser and
barrel presses all arrived with zero setup. *(findings/006 §3; the exact hover
distance above the glass was never measured — don't quote a figure.)*

One field is easy to double-count: **battery strength appears on both links** — as a
byte in report 7 and as a Bluetooth notification (§ below). Whether they always agree
is an open question.

## Link 2: Bluetooth — only the extras, and the pen never advertises

Once connected, HID over GATT **[public: Bluetooth HID over GATT profile]** carries:

- the **top ("shortcut") button**: keyboard input report `0x43`, notified on ATT
  handle `0x0019`. The pen itself classifies single/double/hold — the host sees one
  distinct report per gesture, so nothing should re-classify it **[ours]**;
- **haptics**: one 7-byte output report `0x41`; `HidD_SetOutputReport`-style writes
  reach the pen, and continuous textures are gated to tip contact by the pen itself
  **[ours: findings/006 §4, docs/protocol/haptics.md]**;
- **battery**: notifications on handle `0x0015` (the percentages Windows showed came
  from here) **[ours]**;
- pen **identity and capabilities**: feature reports `0x40`/`0x42` **[ours]**.

The barrel button is *not* one of the extras: it is on the screen link, in report 7,
visible only while the pen is in range; a gesture battery across three presses showed
zero Bluetooth traffic **[ours: findings/006 §5]**.

Critically, **the pen never advertises**. During a 43-second Linux scan it saw 210
advertising reports from various devices — none from the pen, touching or hovering.
An ordinary `bluetoothctl scan` can therefore never find this pen **[ours:
findings/006 §7.5]**.

## Touch-to-pair, as measured

The trick is that the *digitizer* delivers the Bluetooth address to the host, so the
host can connect directly. The pen does not even have to touch: the digitizer announces
it as soon as it is in range, hovering included, and repeats the announcement every few
seconds while the pen has no Bluetooth link. In a Linux recording with no pairing software
running, 7 of 9 announcements came while the pen only hovered, before it first touched the
screen **[ours]**. The full clock chain from one instrumented Windows session, measured from
a touch (all timestamps from a single local clock, findings/006 §2):

| t (s) | what happens |
|---|---|
| 0 | tip touches the screen; digitizer starts reporting the pen over I2C |
| +0.10–0.15 | digitizer sends **input report `0x6E`** on the `0xFFF4` vendor collection (`COL09`, named "ELAN Seamless Pairing Device" in the driver INF): `6E <Bluetooth address, little-endian> <flag byte>` |
| +0.15 | the host's background LE scan switches off — 2 ms after the report, a direct reaction to it |
| +0.25 | LE *Add Device To Accept List* for that address (104 ms after the report), type **random**; connection setup follows immediately |
| +0.37 | LE connection complete; **LE legacy SMP** pairing begins |
| +0.61 | encryption on: bonded. **Tap to bonded ≈ 0.6 s** |

No advertising report from the pen appears anywhere before the connection — the only
way the host could know the address is the digitizer report **[ours]**. Key
distribution is full in both directions; the pen's random static address is its
identity, so it reconnects to the same address forever. Report `0x6E` is preceded
each time by vendor report 34, the controller's pen-presence telemetry carrying the
transducer serial **[ours: findings/006 §3]**.

The same report fires spontaneously on bare Linux, with no vendor driver involved —
flag byte `0x01`, versus `0x03` on Windows. Whether that second bit is an arm/ack for
the Windows consumer or just controller state is unknown; the handoff itself needs
nothing from Windows **[ours]**.

What remains open: Windows plausibly writes an **output report `0x6F`** (address +
flag + a 16-byte, OOB-key-sized value) back down after pairing. Host→device HID
writes are invisible to a user-mode reader, so it has not been observed either way.
The first Linux pairing — plain Bluetooth Just Works, no `0x6F` — is the decisive
test: if it bonds, `0x6F` is optional **[ours: findings/006 §2 "Still open"]**.

## Why pairing needs one extra step on Linux

Bluetooth on Linux is discovery-centric: `bluetoothd` only pairs devices it has
found. A device that never advertises has no device object, so connecting to a
known address is exactly what `org.bluez.Adapter1.ConnectDevice` exists for — and
that method is still behind BlueZ's `Experimental` flag **[public: BlueZ
adapter-api docs]**.

This project therefore treats the `0x6E` report as the discovery channel
(`oppd watch-pair`, then connect/bond), with the pairing itself going through
`ConnectDevice` when BlueZ runs with experimental features on, and through a small
bus-activated helper speaking the kernel mgmt API when it does not. See
[docs/protocol/seamless-pairing.md](protocol/seamless-pairing.md) and
[docs/packaging.md](packaging.md). Whether the helper-initiated bond completes the
GATT side (HoG discovery, battery, buttons) end-to-end is marked
**[HYPOTHESIS]** there until a live run confirms it.

## Sources

Public: USB HID Usage Tables (Digitizer Page `0x0D`); Bluetooth HID over GATT
profile; BlueZ `adapter-api` (ConnectDevice, experimental flag). Ours (this repo):
findings/006 (pairing clock chain, `0x6E`/report-34 semantics, buttons, battery,
haptics on the wire), findings/005/007, `docs/protocol/digitizer-tlc.md` (report
descriptor layout), `docs/protocol/seamless-pairing.md`, `docs/protocol/haptics.md`.
Session identifiers ("session 08 run 2") refer to the instrumented sessions
inventoried at the top of findings/006.
