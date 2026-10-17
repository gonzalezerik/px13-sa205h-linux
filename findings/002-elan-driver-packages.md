# 002 — ELAN/ASUS pointing-device packages: what they actually ship

Date: 2026-10-17 · Method: static unpack (7zz recipe) + text analysis + binary grep of inbox
Windows components extracted from the Win11 25H2 ESD. No package was ever executed.

Confidence labels: VERIFIED-HW / STATIC / DOC / HYPOTHESIS (see CONTRIBUTING.md §5).
All STATIC facts cite the file + SHA-256; wrapper hashes are in `docs/artifacts.tsv`.

## 1. ELAN Seamless Pairing V1.2.2.4 — the package contains no code

Wrapper `asus/SeamlessPairing_DCH_ELAN_Z_V1.2.2.4_46362.exe`
(SHA-256 `3a627fdfe32bf29396b25e7a8bda81226518d21971703973f482e811f97cd70f`) unpacks (7zz `-t#` →
`3.7z` → payload) to exactly three files:

| File | SHA-256 |
|---|---|
| `ElanTouchAutoPairing.inf` | `3d1b7c63b2fa1d5b644885c250beb67c66594a67e2487a2c851c45f877613d81` |
| `elantouchpro.cat` | `843445ea3e375603ffae5ce1d443ca9c451da5abfd2fab9f2c88deb0f601765e` |
| `Install.bat` (`pnputil -i -a *.inf /subdirs`) | `42cc924890d9e1a3bce006f60f8c8905edca8c13136c675b0b060d20c39512d4` |

No `.sys`, no `.dll`, no service binary. [STATIC]

INF facts (`DriverVer 02/10/2025, 1.2.2.4`): [STATIC — ElanTouchAutoPairing.inf]
- Matches `HID\VID_04F3&UP:FFF4_U:0001` — the vendor TLC 0xFFF4/usage 0x01 PDO that the ELAN9008
  descriptor exposes (see `fixtures/px13-hn7306/elan9008-04f3-4631.hid-decode.txt`), described in
  the INF as "Seamless Pairing Device".
- Every `CopyFiles`/`AddService` line is commented out; the live `AddService = ,2` is a no-op.
  An older design is visible in the comments: a kernel filter `ElanTouchProFilter.sys`
  (service section commented out) — removed in this version.
- The INF's only live effect is on the HID interface `{4D1E55B2-F16F-11CF-88CB-001111000030}`:
  `AddProperty {FE683ED0-DE80-4766-B422-4195993F4330}, 2, DEVPROP_TYPE_STRING(0x11) = "1"`,
  with the INF comment `DEVPKEY_BluetoothOobCouplingChannel_OptIn;ForUse`.

The installer `2.exe` inside the wrapper is a 32-bit Delphi program whose embedded `[0]` payload is
a `zlb` container (magic `zlb\x1a`, 3,288,680 B). The identical blob (3,288,264 B, same 2021-06-03
file date) is embedded in the WoT installer too → it is the shared installer runtime, not package
content. [STATIC — 7zz listings of both wrappers]

**Conclusion: "ELAN Seamless Pairing" V1.2.2.4 is a null INF whose sole purpose is to set one device
property on the 0xFFF4 HID interface.** [STATIC]

## 2. The property is not consumed by the inbox Bluetooth/HID stack (negative result)

To test whether "OOB coupling channel" is an inbox Windows feature, the Win11 25H2 ESD
(`windows/win11-25h2-26200.8873-consumer-x64-en-us.esd`, index 9 = Pro) was searched with
`wimlib-imagex`:

- 82 binaries extracted from `System32` + `System32\drivers`: every `bth*`, `hid*`, `daf*`,
  `DevicePairing*`, `deviceassociation`, `deviceaccess`, `bthprops.cpl`, `BthLEEnum`
  (present in this image as `Microsoft.Bluetooth.Legacy.LEEnumerator.sys`), etc.
- Searched each for the property GUID in every plausible encoding: little-endian byte pattern
  `d03e68fe80de6647b4224195993f4330`, big-endian `fe683ed0de804766b4224195993f4330`, ASCII and
  UTF-16 `"FE683ED0"`. **Zero hits.** [STATIC]
- `strings` (ASCII + UTF-16LE) over the same set: no `Coupling` string anywhere. [STATIC]
- Microsoft Learn search API for `"OobCouplingChannel"`: 0 results. [DOC — learn.microsoft.com
  search API, 2026-10-17]

Related inbox facts found while grepping: [STATIC]
- `bthport.sys` contains the SSP OOB primitives:
  strings `OOBDataFlag` and `Unknown read OOB data HCI command` (HCI Read/Write Remote OOB Data).
- `bthserv.dll` imports `DafCreateAssociationContextFromOobBlob` (Device Association Framework can
  start an association from an OOB blob).
- So the inbox *can* pair with OOB data, but nothing inbox reads this property or the 0xFFF4
  reports. The property name in the INF is ELAN's own label, not a documented Windows DEVPKEY.

## 3. ELAN WoT V1.0.1.16 — a user-mode HID service on the digitizer

Wrapper `asus/WoT_DCH_ELAN_Z_V1.0.1.16_46783.exe`
(SHA-256 `a48215b585cb95816d15e0d22f7f873994c1b1d81e25638fbc41f8651b36a9a0`) payload:

| File | SHA-256 |
|---|---|
| `ElanWoT.inf` | `658bbe953f00c670dee04fbf287c8eab9010ae771dc805c95c6576f5a35f2d20` |
| `elanwot.cat` | `220572ccf790eff91897a38d878745e1893ac02d36ec88bcad82003f75592c1f` |
| `ElanTouchXiSvc.exe` | `d28547661e6d4b7934e408615d9b3cf7f708bd35f910e2deffc543fea3032d2b` |

INF facts (`DriverVer 11/23/2022, 1.0.1.16`): [STATIC — ElanWoT.inf]
- Matches `ACPI\VEN_ELAN&DEV_9004..905C` — the I2C-HID digitizer ACPI nodes, **including DEV_9008,
  this machine's digitizer**.
- Installs on top of the inbox `hidi2c.inf` device (`include hidi2c.inf / Needs hidi2c_Device.NT`)
 and additionally needs `input.inf`'s `WakeScreenOnTouch.HW` section.
- Registers `ElanTouchXiSvc.exe` as a Win32 AUTO_START service run from the Driver Store
  (`%16425%`). "WoT" here pairs with the Wake-on-Touch need; the service description is
  "ELAN WoT Capable Service".

`ElanTouchXiSvc.exe` is a native x86-64 console-subsystem binary importing `HID.DLL`
(`HidD_GetFeature`, `HidD_SetFeature`, `HidD_GetAttributes`, `HidD_GetPreparsedData`), `SETUPAPI`,
`ADVAPI32`, `KERNEL32`, `USER32`, `VERSION`, `WINMM`. It imports **no Bluetooth API**, and contains
no `Coupl`/`OOB`/`FFF4`/`FE683ED0` strings or GUID bytes. So it reads/writes HID feature reports on
some ELAN device but is not itself the Bluetooth pairing agent. Full behaviour = the WoT service analysis (findings/005). [STATIC]

## 4. ASUS DialPad V24.0.0.21 — unrelated to the pen

Payload: `AsusDialPadFilter.inf` (`66e9edd951d572360be18e9b11aef754a52b9bab002816a7fd91d9b6139ad966`),
`AsusDialPad.sys` (`0eac41e44db3802f895c2fd5b7f1670386bde5d3143e3c61e3f74517272fbc09`),
`AsusDialPadService.exe` (`b884b4b2781d6fefaeb1472d19166816b3671aeca1eace7a319429031856f945`).
The INF binds `HID\ASUE1A00&Col02`, `HID\ASUP…`, `HID\ASCP…`, `HID\ASCF…`, `HID\ASCE…` collections —
ASUS touchpad/dial hardware IDs, not the ELAN pen digitizer — and the service has no
Bluetooth/pairing strings. Not the seamless-pairing consumer. (findings/003 covers the Store apps.) [STATIC]

## 5. Consequences for the seamless-pairing analysis

The PC-side consumer of the 0xFFF4 pairing reports is **not** in any package we have and **not** in
the inbox components searched. Remaining candidates, in order of likelihood:

1. **ASUS Store apps** (MyASUS / ProArt Creator Hub / ScreenXpert — still to download). Windows 11
   exposes `BluetoothDeviceCustomPairing` with OOB pairing support [DOC — learn.microsoft.com,
   Windows 11 22H2 API], a plausible user-mode path: read reports 110/111 via HID, pair with OOB data.
2. **No PC-side consumer at all**: the digitizer firmware drives the pen side (tells the pen to
   advertise, relays key material over the MPP link) and Windows pairs through ordinary SSP
   just-works without user interaction. The 0xFFF4 property/reports would then be vestigial or
   UX-only (the "tap to pair" toast).
3. A component in the ESD we did not extract (unlikely given the breadth of the grep).

Decisive evidence: Windows capture session 3 (tap-to-pair with `BluetoothStack.wprp` + input trace — will
show whether HCI OOB commands run or plain SSP) and the Store-app RE once the packages are fetched.

Report semantics for the 0xFFF4 TLC (from the digitizer descriptor, STATIC —
`fixtures/px13-hn7306/elan9008-04f3-4631.hid-decode.txt` file lines 499–600, descriptor byte
offsets 1045–1218):

| Report | Dir | Fields |
|---|---|---|
| 112 (0x70) | Feature | 1 bit (usage 0x10) + 7 const |
| 86 (0x56) | Feature | 6 bytes (usage 0x03, buffer) + 1 bit (usage 0x09) + 7 const |
| 110 (0x6E) | Input | 6 bytes (usage 0x02, buffer) + 1 bit (0x08) + 1 bit (0x11) + 6 const |
| 111 (0x6F) | Output | 6 bytes (usage 0x02, buffer) + 1 bit (0x08) + 7 const + 16 bytes (usage 0x14, buffer) |

The 6-byte fields are the only address-sized fields and the 16-byte output field the only
key-sized one — consistent with BD_ADDR + link-key semantics, but the meaning is unconfirmed until
captured (see `docs/protocol/seamless-pairing.md`).
