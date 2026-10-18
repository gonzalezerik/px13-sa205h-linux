# Finding 005 — the ELAN WoT service: command channel = R3 write + R2 read

Date: 2026-10-17 · Confidence: STATIC unless noted · Method: `ElanTouchXiSvc.exe` extracted from the
ASUS WoT V1.0.1.16 wrapper with the 7zz recipe (`docs/artifacts.tsv`), analysed with Ghidra
12.1.2 headless on the laptop. The exe was never executed. No decompiled code is reproduced
anywhere in this repo; addresses are Ghidra entry points in the analysed image.

## Target

`ElanTouchXiSvc.exe`, 547,328 B, SHA-256
`d28547661e6d4b7934e408615d9b3cf7f708bd35f910e2deffc543fea3032d2b`, PE x86-64 (image base
0x140000000), PDB reference `ElanTouchXiSvc.pdb` (not available). Imports of interest:
`HidD_GetFeature`/`HidD_SetFeature` (2 call sites each), `HidD_GetHidGuid`, `CreateFileW`,
`ReadFile`/`WriteFile` (overlapped). No Bluetooth API — consistent with findings/002 §3.

## What it is

A Windows service that keeps **Wake-on-Touch** working on ELAN touch ICs and exposes a small
ELANAPI command surface (named-mutex-guarded). Internally it links a generic ELAN HID library
(self-reported "HIDWinGet Version 1.0.7.2") plus a `CTouchIC` wrapper.

## Device binding [STATIC]

Enumeration (`GetAllHidDeviceList` FUN_1400048d0 → `FindSupportDevice` FUN_140004df0 →
`FilterTouchPadCols` FUN_140005440 → binder FUN_140005d50):

1. HID-class `SetupDi` scan; keep only device paths whose (UTF-8) path contains `elan`.
2. Keep only VID `0x04F3` (or wildcard), and reject composite-interface paths containing `&mi_`.
3. Per physical device, parse the HidP capability collections. Touch Screen (0x0D/0x04) and Pen
   (0x0D/0x02) collections get **no handle**; touchpad finger collections (0x0D/0x05) are
   filtered out entirely.
4. Exactly two vendor handles are opened per touch IC (`SetHidColsEntrySimple` FUN_140005d50):
   - **write handle** on the collection page **0xFF00 / usage 0x01** whose *output* report ID
     equals a configured output ID;
   - **read handle** on the collection page **0x01FF / usage 0x01** (or 0xFF00/0x01 carrying an
     input report) whose *input* report ID equals a configured input ID.
5. `CTouchIC::Connect` (FUN_14000a490) sets the expected IDs to **input = 2, output = 3** for
   touch ICs, and clears them (0/0) when the device reports touchpad type 0xb — touchpads get no
   vendor handles.

Against the captured fixture descriptor (`fixtures/px13-hn7306/elan9008-04f3-4631.rdesc`, see
`docs/protocol/digitizer-tlc.md`): output RID 3 = **R3, 0xFF00/0x01, 32 B Output**; input RID 2 =
**R2, 0x01FF/0x01, 64 B Input**. So on the HN7306 touchscreen the service's entire device surface
is **R3 (write) + R2 (read)** — the maintenance channel already classified dangerous in
digitizer-tlc.md §3.

## Framing [STATIC]

- Command (`WriteCommand` FUN_140002310): buffer `[reportID][0x00][payload-length][payload…]`,
  written with overlapped `WriteFile` at the full output report size (33 B on this device: 1 ID +
  32 data). The on-wire R3 data field is therefore `[00][len][cmd…]`.
- Response (`ReadData` FUN_140002670 + `ReadAsync` FUN_1400020d0): overlapped `ReadFile` on the
  R2 handle with a timeout; the 2-byte `[reportID][?]` header is stripped before returning.
- A slot report ID of 0x80 would switch both paths to `HidD_GetFeature`/`HidD_SetFeature` — not
  reachable on this device, where the IDs are 2/3.

## Command set [STATIC]

ELANAPI dispatcher FUN_14000c750 (each command guarded by a successful connect):

| Cmd | Payload (4 B) | Meaning | Evidence |
|---|---|---|---|
| 0 | `53 00 00 01` | firmware version query — response `'R'` + BCD version in bytes 1–3 (`GetDeviceFWVer` FUN_14000b3b0) | log "CTouchIC::GetDeviceFWVer" |
| 1 | `54 05 05 01` | set IC sleep | log "Set IC Sleep" |
| 2 | `54 85 05 01` | set IC wake | log "Set IC wake" |
| 3 | `77 77 77 77` | IC reset | log "ICReset" |
| 4 | `F0 2F 00 01` | **enable Wake-on-touch** | log "Enable Wake-on-touch" |
| 5 | `F0 2F 00 00` | **disable Wake-on-touch** | log "Disable Wake-off-touch" (sic) |

All six ride the same R3→R2 channel. The service's job at runtime: keep WoT enabled (cmd 4),
toggle IC sleep/wake around system power events (cmds 1/2), and answer FW-version queries (cmd 0)
— HYPOTHESIS for the exact trigger timing (service start / power notifications); Windows session
01 (boot trace) will confirm when the frames appear.

## Negatives [STATIC]

- **R25 / page 0xFF0F: never referenced.** A little-endian dword scan of the whole image finds
  zero occurrences of usage pages 0xFF0F or 0xFFF4. The digitizer-tlc.md hypothesis that
  Wake-on-Touch might be the R25 poke is **disproved** — WoT is an R3 command. R25's consumer
  remains unknown.
- **0xFFF4 (seamless pairing reports 86/110/111/112): never referenced** — the WoT service is not
  a pairing participant (consistent with findings/002: the pairing property is consumed by the
  inbox stack, not by ELAN's service).
- **0xFF0B (ICP/firmware block): never referenced** — this service is not the firmware updater.
- **Pen feature path is dead code here**: `SetPenFeature` FUN_140003280 / pen GetFeature
  FUN_140003450 exist in the library vtable (+0x60/+0x68) but have zero callers in this exe, and
  the pen collection handle is never opened. (Caveat: vtable-indirect dispatch is not fully
  resolved by Ghidra; nothing in the service's own code paths references those slots.)

## What this means for Open Pen Protocol

- Wake-on-Touch on this hardware is one 4-byte R3 command (`F0 2F 00 01/00`) — but R3 stays in the
  **dangerous** bucket (CONTRIBUTING.md §6): writing `F0 2F 00 00` would disable wake-on-touch, and
  `77 77 77 77` resets the IC. Any future use needs byte-level approval and a Windows-side
  capture first (session 01 shows the exact frames + when they are sent).
- The firmware-version read parity path (the "status" command) would need the same R3 command
  `53 00 00 01`, so it falls under the same rule (CONTRIBUTING.md §6). There is no read-only alternative on this device.
- Nothing here changes the pen-parity conclusion: no digitizer write is required for pen features.
