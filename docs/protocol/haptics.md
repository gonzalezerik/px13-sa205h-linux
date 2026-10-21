# Haptics — the Windows haptic-pen host, and what our daemon must reproduce

Status: **static model complete, wire verification pending**. DOC facts come from Microsoft's
Haptic Pen Implementation Guide (captured 2026-10-17, page dated 2026-03-25) and the HID haptics
page (HUTRR63b). STATIC facts come from disassembling the inbox haptics host **ISM.dll**
(Windows 11 25H2, SHA-256 `80adf47c0a07455efbd63b40bb772b808342c3bb653ddc42fab0dae3354a1e0d`,
extracted from `artifacts/windows/win11-25h2-26200.8873-consumer-x64-en-us.esd` index 9 with
`wimlib-imagex extract … /Windows/System32/ISM.dll`). Function addresses cited as
`ISM.dll:0x…` are Ghidra 12.1.2 recovered entry addresses in that image; no decompiled code is
reproduced here, only behaviour. Nothing has been observed on the wire yet — that is the Windows
capture session 4 and the Linux captures.

**Update 2026-10-17:** the wire verification has landed (Windows sessions 04/05 + direct-send feel
tests; `findings/006-hardware-capture-results.md` §4). The static model below is kept as written and
annotated; the new §2.7 records what was observed on this pen.

Confidence labels per CONTRIBUTING.md §5: VERIFIED-HW / STATIC / DOC / HYPOTHESIS.

## 1. Wire level [DOC]

The pen exposes a Simple Haptic Controller collection (Usage Page 0x0E, Usage 0x01) inside its
Stylus top-level collection. Three report kinds matter:

- **Feature — capability discovery** (host reads, never writes for pens):
  - **Waveform List (0x0E/0x10)** — ordinal-indexed (Ordinal page 0x0A, usages ≥ 3) list of the
    waveform IDs the device supports. Ordinals 1 (**None**) and 2 (**Stop**) are implicit.
  - **Duration List (0x0E/0x11)** — duration in ms per ordinal; **0 means continuous** (plays
    until stopped or cut off). [annotated 2026-10-17: this device reports ≈2 s durations for its
    continuous waveforms instead of 0 — §2.7.3]
- **Output — playback** (the only report the host writes):
  - **Manual Trigger (0x0E/0x21)** — an **ordinal** into the device's waveform table (1=None,
    2=Stop, 3+ = Waveform List entries), *not* a waveform ID.
  - **Intensity (0x0E/0x23)** — 0–100 % of maximum strength; 0 = transducer off.
  - **Repeat Count (0x0E/0x24)** — extra plays after the first; 0 = play once; unfinished
    repeats are dropped if cutoff is exceeded.
  - **Retrigger Period (0x0E/0x25)** — ms to wait between repeats; if shorter than the waveform,
    the waveform is restarted at each period.
  - **Waveform Cutoff Time (0x0E/0x28)** — ms ceiling on total playback (repeats + continuous).
- **No Auto Trigger.** Windows deliberately does not implement it (a documented divergence from
  the HID spec): the device itself decides when a *continuous* waveform starts (on tip contact);
  *discrete* waveforms play immediately on the output report. The host only ever configures
  continuous waveforms via the output report and triggers discrete ones manually.

### Waveform ID table [DOC]

Exact table from the guide (resolves the compressed list in `findings/001-identification.md`):

| ID | Name | Class | Mandatory | Notes |
|---|---|---|---|---|
| 0x1001 | None | — | yes | no-op; must not change ongoing playback |
| 0x1002 | Stop | — | yes | stops ongoing playback |
| 0x1003 | Click | discrete | yes | **default fallback for interaction feedback** |
| 0x1006 | Press | discrete | pair with Release | button press pulse |
| 0x1007 | Release | discrete | pair with Press | button release pulse |
| 0x1008 | Hover | discrete | optional | hover over interactive UI |
| 0x1009 | Success | discrete | pair with Error | ascending pattern |
| 0x100A | Error | discrete | pair with Success | descending pattern |
| 0x100B | InkContinuous | continuous | yes | **default fallback for inking** (ballpoint feel) |
| 0x100C | PencilContinuous | continuous | optional | |
| 0x100D | MarkerContinuous | continuous | optional | |
| 0x100E | ChiselMarkerContinuous | continuous | optional | highlighter |
| 0x100F | BrushContinuous | continuous | optional | |
| 0x1010 | EraserContinuous | continuous | optional | |
| 0x1011 | SparkleContinuous | continuous | optional | special/multicolour tools |
| 0x1012 | Collide | discrete | optional | soft pulse at a boundary/limit |
| 0x1013 | Align | discrete | optional | sharp pulse on snap-to-guide |
| 0x1014 | Step | discrete | optional | firm pulse for discrete value changes |
| 0x1015 | Grow | discrete | optional | motion/transitions/system activity |

0x1004 and 0x1005 are unassigned in the guide.

## 2. How the Windows host actually behaves (ISM.dll) [STATIC]

ISM.dll is the inbox "haptics host": it owns the pen's HID handle, reads the capability feature
reports, and writes playback output reports. Behaviour recovered from the image:

### 2.1 Capability discovery

On device arrival ISM.dll issues **HidD_GetFeature** for the Waveform List and Duration List
(`ISM.dll:0x180193e70`) and keeps an in-memory table of 8-byte entries
`{ ordinal:u16, waveformId:u16 }` per device (read through a per-device vtable slot +0x38).

### 2.2 Playback = one output report, five fields, one WriteFile

The composer (`ISM.dll:0x180194760`) fills a single HID output report through five
`HidP_SetUsageValue` calls — page 0x0E: **0x21** ← waveform *ordinal*, **0x24** ← repeat count,
**0x25** ← retrigger period, **0x23** ← intensity, **0x28** ← cutoff — then a single **WriteFile**
to the device handle stored at object offset +0x100. Callers wrap it two ways:

- **ForPlayCount** (`ISM.dll:0x18018d740`): play N times at a given intensity — used for discrete
  effects and by the system-originated path (§2.4).
- **ForDuration** (`ISM.dll:0x18018d648`): play (continuous) with a cutoff ceiling.
- **Stop** (`ISM.dll:0x18018d83c`): ordinal **2** at intensity **1.0** — i.e. stopping is just
  "play Stop".
- The app-facing generic entry point lives in `complianthapticinterface.cpp`
  (`ISM.dll:0x1801a00a0`).

### 2.3 The pen never receives a feature-report write [STATIC negative]

All seven `HidD_SetFeature` call sites in ISM.dll belong to a lamp-array device
(`hidlamparraydevice.cpp` — keyboard light bars). For the pen haptics path, feature reports are
**read-only**; every pen-directed write is the §2.2 output report. (This constrains our daemon:
a 1:1 host needs no SetFeature on the stylus collection.)

### 2.4 System-originated feedback map

Windows generates discrete feedback itself (hover, boundary/snap) through an input-pipeline
message channel. Chain: a CoreMessaging channel named `HapticNotification` (registered in the
ISM.dll service constructor `ISM.dll:0x1800f4408` among its input channels) → handler
`ISM.dll:0x1800f7330` (traced as `MITEP_HAPTIC_NOTIFICATION`) → dispatcher `ISM.dll:0x1800c9644`:

1. The message carries a **notification type 0–4** (anything else is a fatal fail-fast,
   `hapticprocessor.cpp:0xeb`).
2. Requested waveform: types **{1,2} → 0x1008 Hover**, types **{0,3,4} → 0x1012 Collide**.
   (The five type values' UI semantics are not named in the binary — HYPOTHESIS: hover-enter/
   hover-leave vs. press/release/boundary family.)
3. Per-device controller + fallback (`ISM.dll:0x1800c98ec`), keyed by a device-type field
   (0=Unavailable, 1=Keyboard, 2=Mouse, 4=Touch, 8=Pen, 0x10=Touchpad, 0x12=Touchpad|Mouse):
   - **Pen (8)**: controller resolved from an 8-byte **PenID blob** embedded in the message
     (serial + vendor — the 0x0D/0x5B+0x91 identity correlation); fallback **0x1003 Click**.
   - **Mouse (2)** / **Touchpad (0x10)**: controller resolved from a device handle; fallback
     **0x1008 Hover**.
4. Resolve-and-play (`ISM.dll:0x1800c9454`): if the requested waveform ID is in the device's
   Waveform List → play its ordinal; else retry with the fallback (traced `WaveformFallback`);
   else give up (`FailedToFindWaveformAndFallback`). Pen playback goes through the ForPlayCount
   wrapper with **playCount = 1** (`ISM.dll:0x1800c8fbc`).
5. **Intensity travels inside the message**: a per-device double (pen at message settings-blob
   offset +0x10; touchpad +0x20; mouse +0x30). ISM.dll does not read any persisted setting for
   this path — the producer of the notification already carries the current intensity.

### 2.5 Tool → waveform selection is app-side [STATIC negative + DOC]

Neither ISM.dll nor the pen stack's `wisp.dll` (zero haptics/waveform/vibration strings in
ASCII or UTF-16) contains a tool→waveform map. The continuous waveform for the active inking
tool (Pencil/Marker/ChiselMarker/Brush/Eraser/Sparkle) is chosen by the **application** and
handed to the generic interface (§2.2) — consistent with the guide's wording ("when the user
selects pencil as inking tool"). The inbox system-originated map is only §2.4.

### 2.6 Where intensity / on-off persist — still open

Chain-wide statics so far: InputHost.dll (the input pipeline process hosting ISM.dll) imports
only seven registry APIs plus the WNF notification trio — no file I/O, no settings-backend DLL in
its import or delay-import tables; the pen-side haptics path performs no feature writes (§2.3);
and ISM.dll receives intensity per message (§2.4.5). The settings **client** side is thin too:
`CInkInputConfiguration` in Windows.UI.Input.Inking.dll (`IsPenHapticsEnabled`,
`IsEraserInputEnabled`, `IsPrimaryBarrelButtonInputEnabled`) stores values in memory and forwards
all three via one COM call to the input-pipeline server — its only registry write is an unrelated
handwriting-panel flag. So the persisted value is read by the
**producer** of the HapticNotification message (outside ISM.dll), most plausibly the Settings
`PenDevice`/`PenSettings` API surface. HYPOTHESIS: per-pen haptics on/off + intensity live in
the per-user settings store (registry or CDP), read by the shell/input producer. Decisive test:
Windows capture session 4 (registry + settings-store diff around a haptics toggle).
**[RESOLVED 2026-10-17 — capture session 04, VERIFIED-HW]** the hypothesis was right, concretely:
a per-user registry key, `HKCU\Software\Microsoft\Windows NT\CurrentVersion\Windows\EnhancedPenSupport`
— `PenFeedbackEnabled`, `PenInkFeedbackEnabled`, `PenInteractionFeedbackEnabled` (DWORD 0/1 =
master / while-inking / while-interacting) plus `PenFeedbackIntensity` as a **QWORD IEEE-754 double
= slider/100** (observed 0.49 → 1.0 across a slider move). A synced copy lives in the CloudStore
`pensettings` blob; touchpad and mouse keep sibling keys in the same key. ISM.dll itself still
touches no settings storage (§2.4.5 unchanged) — the message producer reads these.

### 2.7 Wire verification on this pen (2026-10-17 captures) [VERIFIED-HW]

Full evidence, including the felt-impressions panel and per-test timelines:
`findings/006-hardware-capture-results.md` §4. Headlines:

1. **Transport**: the haptics collection lives on the pen's **BLE HID-over-GATT** side. Windows'
   playback goes out as an **ATT Write Command (0x52) to handle `0x0021`**; `HidD_SetOutputReport`
   (ATT Write Request) is accepted by the pen as well.
2. **Output report `0x41`, 7 bytes**: `41 <repeat> <intensity> <manual-trigger ordinal>
   <cutoff ×10 ms> <retrigger lo> <hi>` — one write per effect, matching §2.2's five-field model.
   Confirmed by direct sends the maintainer felt (`41 01 FF 03 00 00 00` = one click at full
   intensity).
3. **Feature `0x42` (capabilities)**: auto-trigger control `0x000D0042` = **Tip Switch**; waveform
   list `0x1003, 0x1006–0x1011` → **ordinals 3–15** = click, press, release, hover, success, error,
   then continuous ink/pencil/marker/chisel/brush/eraser/sparkle; durations (ms)
   `[45, 60, 60, 35, 145, 185, 1995, 1995, 1995, 1995, 1980, 1995, 1980]` — continuous entries are
   ≈2 s, **not 0** (§1 annotation). **Feature `0x41`** (current state): repeat 0, intensity `FF`,
   auto-trigger 1, retrigger 0. **Feature `0x40`**: pen identity (transducer serial + vendor
   `0x330A`, `0x0C0F`, `0x0003`).
4. **The device auto-triggers on the tip switch**: the same continuous report is felt as nothing in
   the air and as "writing on paper" while drawing — the host arms the configuration and the pen
   decides when to play, exactly the §1 "host never uses Auto Trigger" model with the device-side
   half now observed.
5. All 13 effects were felt and are subjectively distinct (pencil scratchiest and audible, brush
   smoothest, sparkle "rubbery"). `0xFF` intensity = full; `0x40` (≈ the 25 % slider byte) is still
   felt, perceived strength unclear.
6. The pen lacks Collide/Align/Step/Grow, so Windows' snap-target buzzes exercise the **Click
   fallback** of §2.4.4 on real hardware.
7. Ordinary apps drive ink haptics through the same path: OneNote and Paint produced ~100/~25
   stroke-related writes each, with the paper texture confirmed present. Settings toggles send no
   preview write.
8. Not all buzz is host-commanded: the top-button press-and-hold buzz has **no host write** behind
   it (pen-local feedback — findings/006 §5).

### 2.8 First live sends from Linux (2026-10-19) [VERIFIED-HW]

Full evidence: `findings/009-first-linux-haptics.md`. Output reports written to the pen's
`hidraw` node (kernel HID → uhid → BlueZ HID → GATT) were felt: the Linux transport works
end to end. Two model corrections came out of it:

1. **The implicit Stop ordinal 2 is inert on this firmware** — sent twice against an actively
   playing continuous texture (with and without cutoff, tip in contact), it never cut the texture.
   **The None ordinal 1 is the real stop**: it cuts an active texture mid-stroke and leaves later
   contacts silent (disarms until the next write). §3 item 3 is amended accordingly.
2. **One write of a continuous waveform stays armed across separate tip contacts** (texture
   returned on a second touch with no new write, past the armed cutoff value) — the host arms once
   per tool activation, not per stroke. The 25 % intensity byte was felt and clearly weaker
   (§2.7 item 5's open perception question resolved at the low end).

### 2.9 Per-effect five-field defaults — and where Windows' own values are lost [VERIFIED-HW, one negative]

The captures show Windows writing; they cannot show **what** it wrote. The
`BluetoothStack.Verbose` logger used for the session-07 and session-04 Bluetooth traces truncates
ACL payloads to ≈11 bytes, so the haptic ATT writes are logged as handle only (`52 21 00`) — the
six hover-tick payloads of §07 §4 and the ~125 OneNote/Paint ink payloads are **unrecoverable**.
Windows' exact repeat/cutoff/retrigger choices per effect are therefore *labelled unknown* here,
not guessed at.

What is verified, from the maintainer's own direct-send panel (every write there was felt) and the
pen's power-on output state:

| Field | Value | Evidence |
|---|---|---|
| repeat | `01` — play once | every felt direct send (`41 01 …`); Windows' system-originated map already says playCount 1 (§2.4) |
| intensity | `FF` = full, scaled down by the user's setting | §2.7 items 1–2, 5; §2.8.2 (25 % byte felt, clearly weaker) |
| ordinal | the effect's own | §2.7 item 3 |
| cutoff | `00` — none; the firmware's duration-list entry plays in full | every felt direct send; feature `0x41` readback has no cutoff armed |
| retrigger | `0000` | every felt direct send; feature `0x41` readback: retrigger 0 |

One row fits all thirteen effects — the panel felt click, success, error, the continuous set and
the stop shape with exactly this shape of write, and the pen's own durations (§2.7 item 3) carry
the per-effect character. `px13pend` uses it everywhere (`Effect::of`): the hover tick, the ink
arming, and `PlayEffect` arguments default to it. Anything beyond this row is host invention.

## 3. What px13pend must reproduce (normative checklist)

1. Read Waveform List + Duration List once per pen connection via **GET_FEATURE** (allowed:
   reads are non-destructive); build the ordinal↔waveform table. [DOC §1, STATIC §2.1]
2. Play effects with a **single output report** setting all five usages (0x21/0x23/0x24/0x25/
   0x28) and one write; never write feature reports to the stylus collection. [STATIC §2.2/2.3]
3. Stop = ordinal 2 per the spec [STATIC §2.2] — **but this pen ignores ordinal 2; write
   ordinal 1 (None) to stop/disarm [VERIFIED-HW §2.8.1]**.
4. Fallback rules: interaction feedback → **Click** when the requested discrete waveform is
   absent; inking → **InkContinuous** when the tool waveform is absent. [DOC, STATIC §2.4]
5. System-originated: hover-family → 0x1008, boundary/snap-family → 0x1012, pen fallback Click,
   playCount 1. [STATIC §2.4]
6. Intensity is a percentage supplied by the settings layer at play time, not stored in the
   daemon. [STATIC §2.4.5]
7. Continuous waveforms: duration 0 in the device's list; start is device-side (tip contact);
   the host only sets a cutoff. [DOC §1 — annotated 2026-10-17: this pen's list carries ≈2 s, not
   0; start-side confirmed (§2.7.4)]

## 4. Open items

- Notification-type enum semantics (which of 0–4 is hover-enter vs. press etc.) — needs the
  message producer or Windows capture session 4 traces. [HYPOTHESIS]
- Persistence location (§2.6) — session 4. [**resolved 2026-10-17** — §2.6 annotation,
  findings/006 §4]
- Ordinal 2 (Stop) behaviour on this pen has not been exercised — the direct-send panel covered
  3/7/8/9/11 and the continuous set. [**resolved 2026-10-19** — ordinal 2 is inert, ordinal 1 is
  the stop; §2.8.1, findings/009 §3]
- Windows' per-effect repeat/cutoff/retrigger values (§2.9) — the Bluetooth traces log the
  handle only; recovering them needs a sniffer that captures full ATT payloads (a second adapter
  or an embedded middleman), which no capture session has run.
- Exact report IDs/sizes for the SA205H come from the captured descriptor
  (`fixtures/px13-hn7306/`), not from this doc.
