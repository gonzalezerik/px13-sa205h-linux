# Finding 004 — the Windows haptics host: send path, system map, persistence negatives

Date: 2026-10-17 · Confidence: STATIC unless noted · Method: inbox binaries extracted from the
Windows 11 25H2 ESD (`docs/artifacts.tsv`; `wimlib-imagex extract … 9 /Windows/System32/<file>`),
analysed with Ghidra 12.1.2 headless on the laptop, one target at a time. The behaviour model
lives in **`docs/protocol/haptics.md`**; this file records what was analysed, how, and the
negatives that shaped it. No decompiled code is reproduced anywhere in this repo.

## Primary target: ISM.dll

`ISM.dll` (the inbox Input Service Manager component that hosts the pen haptics logic),
SHA-256 `80adf47c0a07455efbd63b40bb772b808342c3bb653ddc42fab0dae3354a1e0d`. Its PDB reference is
`ISM.pdb`; internal source paths visible in strings place the haptics logic under
`…inputprocessors/devices/haptic/processor/lib/hapticprocessor.cpp` and the generic playback
interface in `complianthapticinterface.cpp`.

Solved (all STATIC; addresses = Ghidra entry points in the image above):

- **Send path**: one output report composed with five `HidP_SetUsageValue` calls (page 0x0E:
  Manual Trigger ← ordinal, Repeat, Retrigger, Intensity, Cutoff) + one `WriteFile`
  (`ISM.dll:0x180194760`), wrapped as ForPlayCount (`0x18018d740`), ForDuration (`0x18018d648`)
  and Stop = ordinal 2 @ intensity 1.0 (`0x18018d83c`); app-facing entry `0x1801a00a0`.
- **Capability discovery**: `HidD_GetFeature` of Waveform List + Duration List
  (`ISM.dll:0x180193e70`), kept as `{ordinal:u16, waveformId:u16}` entries per device.
- **Feature-write negative**: every `HidD_SetFeature` call site in ISM.dll belongs to a lamp-array
  device (`hidlamparraydevice.cpp`). The pen only ever receives output reports. This closes the
  question of whether the host tunes the pen via feature reports: it does not.
- **System-originated feedback map**: a CoreMessaging channel `HapticNotification` (registered in
  the service constructor `ISM.dll:0x1800f4408`) → handler `0x1800f7330` (trace tag
  `MITEP_HAPTIC_NOTIFICATION`) → dispatcher `0x1800c9644`: notification type 0–4 (else fail-fast
  at `hapticprocessor.cpp:0xeb`), **{1,2} → waveform 0x1008 (Hover), {0,3,4} → 0x1012 (Collide)**;
  per-device controller + fallback resolver `0x1800c98ec` — **Pen (device type 8) resolves its
  controller from an 8-byte PenID blob inside the message and falls back to 0x1003 (Click)**;
  Mouse/Touchpad resolve by handle and fall back to 0x1008. Resolve-with-fallback against the
  device's waveform list at `0x1800c9454` (trace strings `WaveformFallback`,
  `FailedToFindWaveformAndFallback` in `.rdata` 0x180217142/0x180217209); playback dispatch
  `0x1800c8fbc` sends the pen through ForPlayCount with playCount 1, taking **intensity as a
  double from the message itself** (pen at settings-blob +0x10). Device-type names formatter
  `0x1800c8f4c` (0=Unavailable, 1=Keyboard, 2=Mouse, 4=Touch, 8=Pen, 0x10=Touchpad,
  0x12=Touchpad|Mouse).
- The waveform-ID meanings (0x1008 = Hover, 0x1012 = Collide, 0x1003 = Click) come from the
  Microsoft Haptic Pen Implementation Guide [DOC] — the binary only carries the numeric IDs.

## Negatives that redirected the search

- **wisp.dll** (the Windows Ink pen stack): zero `haptic`/`waveform`/`vibration` strings in ASCII
  or UTF-16 → not the tool→waveform mapper. Combined with ISM.dll having no tool-name strings,
  the pencil/marker/brush → *Continuous waveform choice is **app-side** (the inking app picks the
  tool waveform and calls the generic interface). The inbox system map is only the discrete
  hover/collide set above.
- **InputHost.dll** (process hosting the input pipeline): its import table holds only seven
  registry APIs (`RegCreateKeyExW`, `RegOpenKeyExW`, `RegQueryValueExW`, `RegGetValueW`,
  `RegSetValueExW`, `RegCloseKey`, `RegNotifyChangeKeyValue`) plus the WNF trio
  (`NtQueryWnfStateData`, `RtlPublishWnfStateData`, `RtlSubscribeWnfStateChangeNotification`) and
  security/ACL calls; no file-I/O DLLs; delay-imports are COM/WinRT/UIA/CoreMessaging/
  CoreUIComponents/PROPSYS/devices-query/ntuser only — **no settings-backend (CDP) module**. So
  within the inbox input stack, persistence is registry-or-WNF, and since ISM.dll receives
  intensity per message, the persisted value is read by the notification *producer* elsewhere.
  Where Settings → Pen stores it stays open → Windows capture session 4 (registry + settings-store
  diff around a haptics toggle) is the decisive test.
- **Windows.UI.Input.Inking.dll** (SHA-256 `ab1cd66e25daac9790ae6e107948ac1bc0ac3c62e56437e3ba752deb5ae64658`)
  holds the settings **client**: `CInkInputConfiguration` exposes `IsPenHapticsEnabled`,
  `IsEraserInputEnabled` and `IsPrimaryBarrelButtonInputEnabled` (trace string carries a typo,
  "Feedbck"). The haptics setter (`0x180099610`) stores the value in memory and forwards all
  three booleans together through a COM interface (vtable slot +0xD0 via `0x1800acf28`) to the
  input-pipeline server — no registry write on that path. The DLL's only `RegSetValueExW` call
  site writes `HKCU\Software\Microsoft\TabletTip\EmbeddedInkControl\HandwritingViewLaunchedBefore`
  (a handwriting-panel flag). So the settings API is a thin forwarder; persistence is server-side.
- Earlier chain-wide string sweeps (seamless-pairing package set, 82 BT/HID/DAF binaries) found no haptics
  settings keys either — consistent with the producer living in a shell/Settings component not
  yet in the analysed set.

## Method notes (reproducibility)

- Ghidra xrefs miss several `lea`-referenced strings in ISM.dll; string→code links were recovered
  by scanning `.text` for rip-relative `lea` (`48/4c 8d`, modrm `0x05` form) targeting the string
  blob 0x180217000–0x180217300. Trace records reference the string at +14 from the `lea` target.
- PE parsing helpers (imports, delay imports, lea scan) were throwaway Python run with `-I` against
  the scratchpad copies; delay-import descriptors are 32 bytes with the name RVA at +4, DIT at
  +12, INT at +16.
- Binary string sweeps need `grep -a` (and `strings -el` for UTF-16).

## Consequences for the project

- `docs/protocol/haptics.md` now carries the full normative model for mppd's haptics module
  (output-report-only playback, fallback rules, timing semantics, system-originated map).
- The oppd haptics output can be specified byte-for-byte from the captured SA205H descriptor +
  this model; still gated on byte-level approval before any output report is sent.
- Remaining threads are observation-only (Windows session 4): notification-type enum
  semantics and the persistence store.
