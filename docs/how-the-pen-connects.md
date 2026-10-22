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
 │                    └─ the 0xFFF4 "seamless pairing" collection [public: Microsoft guide]
 │                       0x56 host address in · 0x6E pen address out (hover or touch)
 │                       0x6F pairing key in
 │
 └─ link 2 (Bluetooth LE): the pen advertises only after the host's key [ours]
       ► host pairs the advertiser
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

## Link 2: Bluetooth — only the extras, and the pen does not advertise on its own

Once connected, HID over GATT **[public: Bluetooth HID over GATT profile]** carries:

- the **top ("shortcut") button**: keyboard input report `0x43`, notified on ATT
  handle `0x0019`. The pen itself classifies single/double/hold — the host sees one
  distinct report per gesture, so nothing should re-classify it **[ours]**;
- **haptics**: one 7-byte output report `0x41`; `HidD_SetOutputReport`-style writes
  reach the pen, and continuous textures are gated to tip contact by the pen itself
  **[ours: findings/006 §4, docs/protocol/haptics.md]**;
- **battery**: notifications on handle `0x0015` (the percentages Windows showed came
  from here) **[ours]**;
- **battery, in detail**: the pen's firmware log — a vendor service that hosts only ever
  read — streams its fuel gauge (a percentage to a hundredth, millivolts) and its charge
  state, which is also how a host learns the pen fell asleep on its charger
  **[ours: findings/010]**;
- **firmware revision**: the standard Device Information service (0x180A, characteristic
  `0x2A26`) answers the version string with one plain read **[public: Bluetooth SIG
  assigned numbers]**;
- pen **identity and capabilities**: feature reports `0x40`/`0x42` **[ours]**. On Linux
  the identity has a transport quirk: the pen exports no Report Reference descriptors at
  all, so a HID feature get has nothing to map onto and is rejected; the identity report
  answers instead as a plain readable HID Report (0x2A4D) characteristic — ten bytes,
  serial then vendor — and that number is byte-identical to the digitizer's reports 7 and
  34, which makes it the one number both links agree on **[ours: findings/006 §2,
  docs/decisions.md]**.

The barrel button is *not* one of the extras: it is on the screen link, in report 7,
visible only while the pen is in range; a gesture battery across three presses showed
zero Bluetooth traffic **[ours: findings/006 §5]**.

Critically, **the pen does not advertise on its own**. During a 43-second Linux scan it
saw 210 advertising reports from various devices — none from the pen, touching or
hovering **[ours: findings/006 §7.5]**. It advertises in two situations only: right after
a host hands it a pairing key through the digitizer (below), and in its button pairing
mode (hold the top button until the LED blinks white) **[ours: findings/008]**.

## Touch-to-pair, as measured

Touch-to-pair is Microsoft's published *Seamless Pairing for Windows Pens* protocol
**[public: learn.microsoft.com, "Seamless Pairing for Windows Pens Implementation
Guide"]**. The host registers its own Bluetooth address with the digitizer (feature
`0x56`), the digitizer announces a pen in range (input `0x6E`) and says whether the pen
already has a pairing with that host, the host sends a pairing key (output `0x6F`), and
the pen answers over Bluetooth. The pen does not even have to touch: the digitizer announces
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

No advertising report from the pen appears in the Windows log before the connection:
the host's controller was already initiating a connection when the pen advertised,
and an initiating controller does not report advertisements to the host **[ours]**. Key
distribution is full in both directions; the pen's random static address is its
identity, so it reconnects to the same address forever. Report `0x6E` is preceded
each time by vendor report 34, the controller's pen-presence telemetry carrying the
transducer serial **[ours: findings/006 §3]**.

The same report fires on bare Linux with no vendor driver, flag byte `0x01`: random
address, **no pairing with this host** — because no host had registered an address in
`0x56`. After a Linux host registers its adapter's address, the flag reads `0x03` for a
pen bonded to that address, and `0x01` for a host identity the pen has never seen
**[ours: findings/008 §3, §8]**.

## Touch-to-pair on Linux

It works with stock BlueZ and a stock kernel, about 0.7 s from touch to bonded, the same
as Windows **[ours: findings/008]**:

1. at start, write `0x56` (the adapter's LE address, little-endian, type) and `0x70`;
2. keep an LE scan running while a pen is in range — the pen's advertisement follows the
   key within milliseconds and lasts only tens of milliseconds;
3. on `0x6E`, write `0x6F` with the announced address and a key;
4. pair whichever device then advertises as the pen (name "ASUS Pen", appearance
   `0x03C7`) — usually a different address from the announced one, because the pen
   keeps a separate identity per host;
5. pair with passkey entry and **passkey 0**.

The last step is where Linux differs. Windows pairs *out-of-band* with the 128-bit key it
sent through the digitizer, so the key never crosses the radio. The Linux kernel has no
LE legacy out-of-band support (the pen is legacy-only), so Linux uses passkey entry; the
pen has nothing to type with and enters 0. The pen accepts that as authenticated pairing
and the user sees the same thing as on Windows, but the pairing moment is not protected
against a passive eavesdropper. What a recorded pairing could be used for is kept small:
the pen's Bluetooth keyboard and mouse are hidden from the desktop, so a device posing as
the pen can fake at most a shortcut gesture (docs/decisions.md, 2026-10-22). The kernel
feature that would close the gap is noted in
[docs/future/kernel-le-legacy-oob.md](future/kernel-le-legacy-oob.md); details in
[docs/protocol/seamless-pairing.md](protocol/seamless-pairing.md) §3.

## Sources

Public: Microsoft's *Seamless Pairing for Windows Pens Implementation Guide*; USB HID
Usage Tables (Digitizer Page `0x0D`); Bluetooth HID over GATT profile; Bluetooth Core
Specification, Security Manager (legacy pairing methods); BlueZ `doc/mgmt-protocol.rst`
(Add Remote Out Of Band Data). Ours (this repo):
findings/008 (touch-to-pair on Linux), findings/006 (pairing clock chain, `0x6E`/report-34 semantics, buttons, battery,
haptics on the wire), findings/010 (dock, charger and fuel-gauge readings from the pen's debug log), findings/005/007, `docs/protocol/digitizer-tlc.md` (report
descriptor layout), `docs/protocol/seamless-pairing.md`, `docs/protocol/haptics.md`.
Session identifiers ("session 08 run 2") refer to the instrumented sessions
inventoried at the top of findings/006.
