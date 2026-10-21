# Windows map — what actually runs the pen there

The parity target is Windows' behaviour, so the wiki records what the
Windows stack turns out to be, layer by layer — including what it is
**not** (the store apps) and what the captures could not see (truncated
payloads). Labels per [conventions](conventions.md).

## Layers, with their evidence

| Layer | Component | What it does with the pen | Evidence |
|---|---|---|---|
| Pen stack | `wisp.dll` (inbox) | report-7 parsing, hover/gesture promotion; **zero** haptics/waveform strings — tool→waveform selection is app-side | [STATIC findings/004; haptics.md §2.5] |
| Gesture engine | `Ninput` | mouse promotion: `0x20001` taps→left, `0x20002` press-and-hold & barrel-tap→right; wisp parameters on the test machine: `HoldMode=1 HoldTime=2300 TapTime=100 DblTime=300 RightMaskEnable=1 …` | [findings/006 §6] |
| Haptics host | **`ISM.dll`**, hosted by `InputHost.dll` | owns the pen HID handle; capability GET_FEATUREs; five usages → one output report → one `WriteFile`; system-originated hover/Collide→Click map; never writes feature reports to the pen | [STATIC findings/004, addresses there; §2.7 wire-verified] |
| Haptics settings | inbox Settings → Pen & Windows Ink (`PenDevice`/`PenSettings` API) | persists per user: `HKCU\…\EnhancedPenSupport` — `PenFeedbackEnabled`, `PenInkFeedbackEnabled`, `PenInteractionFeedbackEnabled` (DWORD 0/1), `PenFeedbackIntensity` (QWORD double = slider/100) | [VERIFIED-HW findings/006 §4; haptics.md §2.6] |
| Bluetooth pairing | Windows BT stack + ELAN INF opt-in | INF property `{FE683ED0-DE80-4766-B422-4195993F4330}, 2 = "1"` — `DEVPKEY_BluetoothOobCouplingChannel_OptIn;ForUse` — declares the `0xFFF4` collection (INF name **"ELAN Seamless Pairing Device"**, COL09) as the OOB coupling channel; stack then runs OOB pairing with the `0x6F` key | [STATIC findings/002 §INF; VERIFIED-HW findings/006 §2] |
| Digitizer service | `ElanTouchXiSvc.exe` (WoT) | the live user of the **dangerous** `0xFF00` R2/R3 channel: six commands — FW-version query, IC sleep, IC wake, IC reset, Wake-on-touch enable, disable — all one 4-byte payload on R3, answer on R2 (exact bytes: findings/005's command table) | [STATIC findings/005] |
| Digitizer driver | ELAN inbox/OEM driver | report parsing + the pairing collection's contract in the INF | [findings/002] |
| Store apps | MyASUS · ProArt Creator Hub · ScreenXpert | **nothing pen-related** — no pen settings, no vendor GATT to the pen, no pen/digitizer firmware query (keyword + import sweep over all packages). Creator Hub's "HapticPad" strings are the touchpad | [STATIC negative findings/003] |

## What the captures could not show [VERIFIED-HW negative]

- **Haptic payload contents**: the `BluetoothStack.Verbose` logger used in
  the Windows sessions truncates ACL payloads to ≈11 bytes — haptic writes
  logged as the handle only (`52 21 00`); Windows' per-effect
  repeat/cutoff/retrigger values are unrecoverable from these traces
  (haptics.md §2.9).
- **Pen advertisements before the connection**: an initiating controller
  doesn't report advertisements to the host — the 43 s "pen doesn't
  advertise" negative needed a Linux scan (findings/006 §7.5).
- The **press-and-hold ring** renders (a display-camera video proves it,
  ~0.6–1.0 s sweep) but is invisible to screen-capture APIs — "capture
  blindness, not absence" (findings/007 §5a).

## Read-across for the Linux build

Each row above has its Linux counterpart: wisp+Ninput → kernel
`hid-multitouch` + compositor; ISM.dll → `px13pend`; OOB pairing → passkey
0 ([pairing](flows/pairing.md#why-linux-pairs-with-passkey-0)); the ELAN
service → nothing (its channel stays untouched here). The one Windows
behaviour **without** a Linux counterpart so far is the press-and-hold
ring — deliberately out of scope — and the digitizer service, deliberately
not copied: it writes the dangerous channel.
