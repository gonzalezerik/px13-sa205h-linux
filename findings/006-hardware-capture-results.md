# Finding 006 — hardware capture campaign: pairing, haptics, buttons and identity on the wire

Date: 2026-10-17 · Confidence: **VERIFIED-HW** unless noted otherwise · Method: nine instrumented
Windows 11 25H2 sessions plus one Linux session on the test machine (ASUS ProArt PX13, ELAN9008
`04F3:4631` I2C digitizer, ASUS Pen 3.0-class BLE pen, HID VID `0x330A` PID `0x0001`, pen firmware
"2.0.37"). Windows side: `logman` sessions over `Microsoft-Windows-SPB-HIDI2C` +
`Microsoft-Windows-Input-HIDCLASS` (I2C read timing; no payload), `wpr -start BluetoothStack.Verbose`
(HCI in BTHPORT events, ACL payloads truncated to ~11 bytes), Procmon, registry snapshots before/after,
and a small user-mode HID reader for the `0xFFF4` collection. Linux side: `hid-recorder` on the
digitizer `hidraw`, `evtest`, `btmon`. This finding records what was observed and which earlier
hypotheses it confirms or kills; per-device identifiers (Bluetooth address, transducer serial) are
referred to by role only.

## 1. Session inventory

| Session | Content | Outcome |
|---|---|---|
| 00 inventory | Settings state, PnP paths, link-key export | "Side button" = barrel, "Shortcut button" = top end; digitizer collections COL01–COL09, **COL09 = "ELAN Seamless Pairing Device"**; no haptics/pressure UI while the pen is unpaired |
| 01 boot | boot-time bus traces | `wpr InputOnlyProfile` records nothing usable here (metadata only) — a dead end, use `logman` providers instead; runtime I2C read rate under pen hover ≈ 266 reads/s; HIDCLASS rundown yielded the pen's full BLE report descriptor (pen/kbd/mouse collections) |
| 02 inking | hover/pressure/tilt/eraser + browser JSON | ≈256 Hz in contact, 10-bit pressure as seen by the browser, tilt only in contact, no twist, eraser = invert (`buttons=32`); barrel never appears as a pointer button |
| 03 pairing | first full tap-to-pair HCI trace | host initiates the connection ~0.3 s after tip touch; **LE legacy SMP**, full key distribution both directions |
| 04 haptics | settings toggles + direct-send tests | persistence located (registry, §4); capability feature reports read; every waveform felt |
| 05 buttons | barrel vs top button, isolated | barrel is digitizer-in-band only (zero BLE); top button is the BLE keyboard collection |
| 07 pen UI | screen recording + gesture-engine timeline | Wisp timing parameters exported; frame-by-frame analysis in findings/007 |
| 08 pairing full | repeat of 03 with a fixed HID reader | **the decisive 0x6E trace** (§2) |
| Linux step 1 | `hid-recorder` + `evtest` + `btmon` | digitizer capture good (§5); the pen never advertises over BLE on its own |

## 2. Seamless pairing — the flow, verified

The 08 run-1 HID reader crashed silently at start, so "no 0x6E seen" in run 1 means nothing; run 2
(fixed reader, same procedure) captured it. All timestamps below are one local clock across I2C, HCI
and the HID reader.

1. Pen tip touches the screen; the digitizer starts reporting the pen (MPP in-band — the pen's BLE
   address travels over the 2.4 GHz MPP link to the digitizer, not over Bluetooth).
2. **0.10–0.15 s later** the digitizer sends **input report `0x6E`** on `HID\ELAN9008&COL09`
   (usage page `0xFFF4`): `6E <6-byte Bluetooth address, LITTLE-ENDIAN> <flags 0x03>`.
   A second identical report follows when the pen touches/enters range again. **[VERIFIED-HW]**
3. **2 ms later** the host disables its background LE scan — a direct reaction to the report.
4. +104 ms: LE Add Device To Accept List for that address (address type **random**); immediately
   LE Extended Create Connection straight to it. No LE advertising report from the pen appears
   anywhere before the connect — the host could only know the address from the digitizer.
5. +0.37 s LE Enhanced Connection Complete; **LE legacy SMP** pairing (Request/Response/Confirm/
   Random, no public-key exchange — session 03 trace); +0.61 s Encryption Change; BTHUSB logs
   "paired". **Tap to bonded ≈ 0.6 s.**
6. SMP key distribution is full in both directions (the pen distributes LTK, EDIV/Rand, IRK,
   identity address and CSRK; the host distributes IRK, identity address, CSRK). The pen's random
   address is its **identity** (random static) — consistent with a reconnecting HID device.
7. `bthserv` (`Microsoft.Bluetooth.Service.dll`) and `DeviceAssociationService` begin working at the
   connect moment; the ELAN INF only names COL09 and sets an interface marker property, with **no
   driver service of its own**. Combined with the 82-binary negative in findings/002 §2, the
   consumer of report `0x6E` is **Windows' own Bluetooth stack**, not an ASUS component. **[VERIFIED-HW
   for the timing, STATIC+HW for the attribution]**

Collection facts measured on Windows (caps: input 8 B / output 24 B / feature 8 B):

- **feature `0x70` reads back `01`** — the "seamless pairing enabled" flag (§ seamless-pairing.md).
- **feature `0x56` `HidD_GetFeature` fails with error 122** on this build — the 6-byte buffer + bit
  feature is not readable from user mode as-is.
- The pen's own collection (COL04) cannot be opened from user mode (sharing violation; held by the
  system stack).
- The pen's BLE **feature report `0x40`** carries its identity: transducer serial (by role), vendor
  `0x330A`, then `0x0C0F`, `0x0003`. No serial→address mapping exists in the registry — the address
  handoff is purely the `0x6E` report.

**Still open:** whether Windows writes **output report `0x6F`** (address + flag + 16-byte,
LE-key/OOB-sized value) back to the digitizer — a user-mode reader cannot see host→device HID
writes, and the SMP payloads are truncated in the BTHPORT log, so the OOB flag in the pairing
frames is invisible. A plain Just Works pairing from Linux (§7) is the decisive test: if it bonds,
`0x6F`/OOB is optional.

## 3. The digitizer's report traffic, from the Linux capture

Offline analysis of the Linux step-1 `hid-recorder` file (84.2 s, 13,303 frames, report-ID histogram
`{0x07: 13284, 0x22(34): 10, 0x6E(110): 9}`):

- **Report `0x6E` fires spontaneously on bare Linux** — nine frames, no vendor driver involved,
  always the same 6-byte address (byte-for-byte the address Windows acted on in 08 run 2) and flag
  byte **`0x01`**, vs **`0x03`** on Windows. The second bit (usage `0x11`) therefore only appears
  while the Windows consumer is active; whether it is an arm/ack or a state of the digitizer is
  unknown. **[VERIFIED-HW observation, HYPOTHESIS meaning]** The pen→digitizer→host handoff works
  without Windows: an px13pend listener is all that is missing for tap-to-pair.
- **Every `0x6E` is preceded within a few milliseconds by report 34** (`0xFF0F/0x50`, four signed
  32-bit, usages `0x72–0x75`); the first value carries the pen's transducer serial little-endian,
  the other three are zero (one extra report-34 frame had no follow-up `0x6E`). This answers the
  R34 emitter question in digitizer-tlc.md §5: R34 is the digitizer's *pen-presence* telemetry,
  and it is the leading edge of the seamless-pairing notification. **[VERIFIED-HW, one session]**
- **Report 23 emitted nothing** during the session (scoped negative for its async-channel
  hypothesis).
- The barrel bit is on the wire on Linux: during the hover phases, R7 button byte `0x05`
  (In Range + Barrel) and `0x07` (In Range + Tip + Barrel) frames appear — the same in-band barrel
  behaviour Windows showed in session 05 (§4). See §5 for the evdev side.

## 4. Haptics — verified on the wire

Transport: the pen's Simple Haptic Controller lives on the **BLE HID (over GATT)** side, not the
digitizer. Windows writes the haptic **output report `0x41`** as an **ATT Write Command (0x52) to
handle `0x0021`**; `HidD_SetOutputReport` (ATT Write Request) is accepted by the pen as well.
**[VERIFIED-HW]**

- **Output report `0x41` layout** (7 bytes, confirmed by direct sends the maintainer felt):
  `41 <repeat> <intensity> <manual-trigger ordinal> <cutoff ×10 ms> <retrigger lo> <hi>`.
- **Feature `0x42` (capabilities)**: auto-trigger control `0x000D0042` (**Tip Switch**); waveform
  list `0x1003`, `0x1006–0x1011` → **ordinals 3–15** = click, press, release, hover, success,
  error, then continuous ink/pencil/marker/chisel/brush/eraser/sparkle; durations (ms)
  `[45, 60, 60, 35, 145, 185, 1995, 1995, 1995, 1995, 1980, 1995, 1980]`. The continuous entries
  carry ≈2 s durations, **not 0** — the HID guide's "duration 0 = continuous" convention is not
  what this firmware reports. **[VERIFIED-HW]**
- **Feature `0x41` (current output state)**: repeat 0, intensity `FF`, auto-trigger 1, retrigger 0.
- Continuous waveforms **play only while the tip is in contact** (the auto-trigger source is the
  tip switch) and feel like surface texture, not a buzz: ordinal 9 sent in the air = nothing, the
  same report while drawing on the desktop = "like writing on paper". All seven continuous
  waveforms are felt and are subjectively distinct (pencil scratchiest and audible, brush
  smoothest, sparkle "rubbery"). Discrete click/success/error are felt in the air. **[VERIFIED-HW]**
- Intensity byte: `0xFF` = full (matches registry 1.0); `0x40` = 64/255, the byte a 25 % slider
  would produce, was still felt — whether perceptibly weaker, the maintainer was unsure. A mapping of
  `byte ≈ slider/100 × 255` is consistent with every observation; the perceptual scale is unknown.
  **[VERIFIED-HW observations, mapping consistent-not-proven]**
- **OneNote and Paint do drive ink haptics**: ~100 and ~25 ATT-0x52 writes during drawing
  respectively; the maintainer initially did not recognise the paper texture as haptics, then confirmed
  the texture was there. Settings toggles themselves send **no** preview write. **[VERIFIED-HW]**
- **Persistence** (settles haptics.md §2.6): `HKCU\Software\Microsoft\Windows NT\CurrentVersion\
  Windows\EnhancedPenSupport` — `PenFeedbackEnabled`, `PenInkFeedbackEnabled`,
  `PenInteractionFeedbackEnabled` as `REG_DWORD` 0/1 (master / while-inking / while-interacting),
  and `PenFeedbackIntensity` as `REG_QWORD` holding an **IEEE-754 double = slider/100** (observed
  `0.49 → 1.0` across a slider move). A synced copy lives in the CloudStore `pensettings` blob.
  Touchpad and mouse keep sibling keys in the same key. **[VERIFIED-HW]**
- The pen lacks Collide/Align/Step/Grow in its waveform list; Windows' snap-target buzzes are
  therefore fallback playback (Click), matching the ISM.dll fallback logic statically recovered in
  findings/004. **[V-HW + STATIC]**

## 5. Buttons and battery

- **Barrel ("side") button**: in-band digitizer **only** — R7 barrel switch, visible only while the
  pen is in range; zero BLE traffic across three gesture blocks (single/double/long), and no host
  haptic feedback until tip contact. Windows uses it as a right-click modifier ("pen button as a
  right-click equivalent" + `RightMaskEnable=1`): barrel held + tap = right-click; tip held near a
  snap target buzzes. **[VERIFIED-HW]** The earlier browser result (no `buttons=2`) is explained:
  the pointer layer promotes barrel+tap to a right button, the raw digitizer bit was always there.
- **Top ("shortcut") button**: BLE **keyboard input report `0x43`, notifications on handle
  `0x0019`**, one press+release pair per gesture — a double press is still a single pair — so the
  **pen itself classifies single/double/long** and Windows only sees distinct key reports (payload
  truncated in the HCI log, key codes unknown). **[VERIFIED-HW]**
- The press-and-hold buzz on the top button is **pen-local**: no host→pen write anywhere in that
  trace. Host-commanded buzzes (snap ticks) and pen-local buzzes are distinguishable this way. **[VERIFIED-HW]**
- **Battery**: notifications on handle `0x0015` (observed `0x50`, `0x4F`, `0x4D` = 80/79/77 %).
  The pen drops the BLE link when idle and reconnects on use; on reconnect the host re-reads
  feature reports `0x42`/`0x40`. **[VERIFIED-HW]**
- Linux wire check: the digitizer's R7 barrel frames (§3) prove the bit reaches a bare Linux host.
  Whether the kernel's evdev mapping surfaces it as `BTN_STYLUS` on this device was not directly
  observed (the concurrent `evtest` run has no barrel press in its timeline) — a one-minute evtest
  confirmation is queued as a hardware check. **[VERIFIED-HW wire / pending evdev]**

## 6. Pen UI timing (session 07, captured; analysis pending)

Wisp parameters on the test machine: `HoldMode=1`, `HoldTime=2300`, `TapTime=100`, `DblTime=300`,
`DblDist=20`, `WaitTime=300`, `Splash=50`, `RightMaskEnable=1`, `EraseEnable=1`, `FlickMode=1`,
`FlickTolerance=50`; cursors: `ContactVisualization=1`, `GestureVisualization=31`, `NWPen=aero_pen.cur`.
The gesture engine (`Ninput`) mouse-promotion flags line up with the steps: `0x20001` left-button
promotion (taps), `0x20002` right-button promotion (press-and-hold and barrel paths). Only 6 haptic
ticks fired against 12 right promotions. Press-and-hold **without** the barrel also right-clicks
(`HoldMode=1`).
*(Update from findings/007, frame-stepped 2026-10-18: the hold→right-click takes ≤1.9 s; the tick
hypothesis is dead — the ticks land at the end of hover strokes, not at hold completion, and never
at a right promotion. The clock anchor held: video t0 = 19:31:14. The recording shows no circle,
but that is capture blindness, not absence — a display-camera video (007 §5a) proves the
press-and-hold ring does render, sweeping ≈0.6–1.0 s and closing just before the menu.)*
**[superseded by findings/007]**

## 7. Consequences for the build (feeds docs/parity.md)

1. **Tap-to-pair is implementable on Linux today**: watch the `0xFFF4` collection's report `0x6E`
   on `hidraw` (it fires unarmed, on bare Linux), take the 6-byte LE address, call
   `org.bluez.Adapter1.ConnectDevice` with `AddressType "random"`, then `Pair`. The first Linux
   bonding run doubles as the test of the open `0x6F`/OOB question: plain Just Works either bonds
   (Windows' extra mechanism optional) or fails (must reverse `0x6F` — write ban applies, needs
   explicit approval first).
2. **Haptics sender**: one 7-byte output report `0x41` per effect over the paired BLE HID;
   capability table from feature `0x42`; stop = ordinal 2 per haptics.md §3 (not yet exercised on
   this pen — the ordinal-2 behaviour on this device is unconfirmed). **[HYPOTHESIS for stop]**
3. **Top-button handling**: subscribe to HID report `0x43` notifications over GATT; do not try to
   classify gestures — the pen already did.
4. **Barrel handling**: read R7 from the digitizer stream (in-range only); expose as modifier +
   button, mirroring Windows' right-click semantics as a default, not a hardcode.
5. `btmon` during the whole Linux session saw **210 advertising reports, none from the pen** — the
   pen never advertises until connected. The digitizer report is the only discovery channel; a
   normal `bluetoothctl scan` will never find it.
