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

Confidence labels per CONTRIBUTING.md §5: VERIFIED-HW / STATIC / DOC / HYPOTHESIS.

## 1. Wire level [DOC]

The pen exposes a Simple Haptic Controller collection (Usage Page 0x0E, Usage 0x01) inside its
Stylus top-level collection. Three report kinds matter:

- **Feature — capability discovery** (host reads, never writes for pens):
  - **Waveform List (0x0E/0x10)** — ordinal-indexed (Ordinal page 0x0A, usages ≥ 3) list of the
    waveform IDs the device supports. Ordinals 1 (**None**) and 2 (**Stop**) are implicit.
  - **Duration List (0x0E/0x11)** — duration in ms per ordinal; **0 means continuous** (plays
    until stopped or cut off).
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

## 3. What oppd must reproduce (normative checklist)

1. Read Waveform List + Duration List once per pen connection via **GET_FEATURE** (allowed:
   reads are non-destructive); build the ordinal↔waveform table. [DOC §1, STATIC §2.1]
2. Play effects with a **single output report** setting all five usages (0x21/0x23/0x24/0x25/
   0x28) and one write; never write feature reports to the stylus collection. [STATIC §2.2/2.3]
3. Stop = ordinal 2, intensity 1.0. [STATIC §2.2]
4. Fallback rules: interaction feedback → **Click** when the requested discrete waveform is
   absent; inking → **InkContinuous** when the tool waveform is absent. [DOC, STATIC §2.4]
5. System-originated: hover-family → 0x1008, boundary/snap-family → 0x1012, pen fallback Click,
   playCount 1. [STATIC §2.4]
6. Intensity is a percentage supplied by the settings layer at play time, not stored in the
   daemon. [STATIC §2.4.5]
7. Continuous waveforms: duration 0 in the device's list; start is device-side (tip contact);
   the host only sets a cutoff. [DOC §1]

## 4. Open items

- Notification-type enum semantics (which of 0–4 is hover-enter vs. press etc.) — needs the
  message producer or Windows capture session 4 traces. [HYPOTHESIS]
- Persistence location (§2.6) — session 4.
- Exact report IDs/sizes for the SA205H come from the captured descriptor
  (`fixtures/px13-hn7306/`), not from this doc.
