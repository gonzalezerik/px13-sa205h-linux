# Finding 003 — the three ASUS Store apps: what they do (and do not) do with the pen

Date: 2026-10-17 · Confidence: STATIC unless noted · Method: packages fetched with
`tools/fetch-store-app.py` (verified against the download service's own digests, SHA-256
pinned in `artifacts/store/manifest.json`), extracted to a scratchpad, keyword-swept with
context verification on every hit. No code was executed from the packages.

## Packages analysed

| App | Version (x64) | SHA-256 (first 16) |
|---|---|---|
| MyASUS (`B9ECED6F.ASUSPCAssistant`) | 4.0.77.0 | `13c53d21a999f55a` |
| ProArt Creator Hub (`B9ECED6F.ProArtCreatorCenter`) | 2.3.13.0 | `935c60466a408b54` |
| ScreenXpert (`B9ECED6F.ScreenPadMaster`) | 4.0.33.0 | `2ef5ef71e3504729` |

Notes on acquisition: MyASUS 4.2.x/4.4.x are arm64-only builds (Snapdragon line); 4.0.77.0
is the newest x64 release, selected by checking bundle contents, not filenames. Creator Hub
2.3.13.0 is a genuine all-arch bundle; ScreenXpert is a plain x64 package.

## Headline: none of the three apps touches the pen

Three questions, three negatives:

1. **Pen settings — not here.** No module references the pen device APIs in its own code
   (`PenDevice`, `PenSettings`, `IsPenHapticFeedbackEnabled` appear only inside bundled
   `Windows.winmd` copies — the Windows SDK contract, present in every WinRT app). No
   `resources.pri` in any of the three apps contains a UTF-16 "Pen" UI string. The pen
   haptics/intensity controls on this machine are therefore the **inbox** Settings →
   Pen & Windows Ink pages, driven by the OS `PenDevice`/`PenSettings` API.
2. **Vendor Bluetooth/GATT commands to the pen — not here.** No reference to the pairing
   APIs (`BluetoothDeviceCustomPairing`, seamless-pairing property GUID, `SA205H`) in any
   ASUS module. GATT clients exist but serve other devices: `AsusMouseAgent.exe` (MyASUS
   mouse module — C++/WinRT GattRead/GattWrite + firmware flow for ASUS *mice*) and
   `AsusScreenXpertReunion.exe` (Reunion screen-share device discovery). No `ELAN9008`
   device path or HID-device-interface GUID anywhere.
3. **Pen/digitizer firmware queries — not here.** Firmware code paths are the generic
   LiveUpdate offer UI (MyASUS) and unrelated device firmware (Realtek audio DLL). No
   pen- or digitizer-specific version query exists in the three apps.

## What the apps actually contain (device-relevant parts)

- **MyASUS** (2815 files; `ModuleDll/` = 2632 DLLs): per-model "Exclusive" feature page
  (WPF), Home, LiveUpdate, CustomerSupport, mouse module (`AsusMouseAgent.exe` = the vendor
  GATT pattern for ASUS mice: service/characteristic read-write + firmware over GATT —
  useful as a *pattern reference* for how ASUS does vendor GATT, not pen-specific).
  `HidD_` users target their own devices (mouse, sensors), not the digitizer.
- **ProArt Creator Hub** (1308 files): the Laptop module's `HapticPad*` strings
  (`HapticSettingIsSupport`, `HapticPadCard`, …) are the **haptic touchpad** setting — not
  pen haptics. Display module carries colour-calibration tooling (`i1d3SDK.dll` = X-Rite
  colorimeter, `dcxmtr.dll`) and `AsusDialDeviceHelper.dll` + `SLABHIDDevice.dll` for the
  ProArt Dial accessory. `AsusDisplayUUID.dll` mentions Wacom only as display-panel
  digitizer UUID data.
- **ScreenXpert** (277 files): screen-pad (ScreenPad) host, digital keyboard, Reunion.
  `LibTouch.dll`/`AsusXCommonLib.dll` use HID for the screen pad itself.

## Consequences for the project

- The 0xFFF4 seamless-pairing consumer is **not** in the Store apps (this closes the lead
  opened by finding 002). Remaining candidates: a firmware-autonomous flow (digitizer
  triggers plain SSP pairing itself) or a component not yet identified. The Windows capture
  session that traces the first tap-to-pair is the decisive evidence.
- Pen haptics settings live in the inbox stack → the Windows haptics-host static analysis
  (inbox binaries from the image) is the primary static target for haptics behaviour.
- The capture kit's ASUS-app settings session stays gated (nothing to capture until an app
  is shown to hold pen settings — currently none does).

## Method detail (for reproducibility)

Keyword sweep per app: `CustomPairing|SeamlessPairing|FE683ED0|SA205H|BluetoothDeviceCustom`,
`Devices.Bluetooth`, `GattDeviceService|GattWriteRequest|GattCharacteristic`, `HidD_`,
`haptic`, `Stylus*|PenSettings*|PenDevice*|PenHaptic*`, `04F3`, `ELAN`, `firmware`,
`Wacom`, plus UTF-16 `Pen` in `resources.pri`. Every hit was context-checked; the noise
classes were: bundled `Windows.winmd` (SDK contract), random 4-byte matches inside binary
blobs (all `04F3`/`ELAN`/`FFF4` string hits fell here), and .NET framework internals in
ScreenXpert's bundled runtime (`StylusAsyncPlugin*` = WPF infrastructure).
