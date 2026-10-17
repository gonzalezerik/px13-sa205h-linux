# Digitizer vendor TLCs — full report map of ELAN 04F3:4631 and safety classification

Status: **static classification complete; runtime confirmation pending**. Source: the captured
report descriptor `fixtures/px13-hn7306/elan9008-04f3-4631.rdesc` (1219 bytes, SHA-256
`314146f2c051af275ae17baa95374b3230772470a293fd2e4272c339499aef98`, read from the live device on
this laptop), parsed item-by-item and cross-checked against the bundled `hid-decode` dump. All
structure facts are STATIC (descriptor bytes; TLC byte offsets given); every *purpose* statement
is HYPOTHESIS unless cited. Nothing here was sent to the device — per CONTRIBUTING.md §6 no writes and no
unproven reads were performed.

Device: **ELAN9008:00 04F3:4631**, I2C-HID on AMDI0010:00 (i2c-0), bound to `hid-multitouch`.
On Linux it creates 7 input nodes: the touchscreen node, a "Stylus" node, and five "UNKNOWN"
nodes (event8, event10–13) from the vendor collections below.

Notation note: some sources write the page as `0xFF01FF`; the descriptor encodes `06 ff 01` =
**Usage Page 0x01FF** (little-endian). Same collection, this doc uses 0x01FF.

## 1. Complete report map [STATIC]

| TLC @ byte | Page/Usage | Rpt ID | Direction | Size | Contents (usages) |
|---|---|---|---|---|---|
| 4 | 0x0D/0x04 Touch Screen | 1 | Input | 60 B | 5×finger {tip,id,confidence,w,h,X,Y}, scan time, contact count |
| | | 10 | Feature | 1 B | Contact Count Maximum (0x55, max 10) |
| | | 68 | Feature | 256 B | 0xFF00/0xC5 blob |
| 560 | 0x01FF/0x01 | 2 | Input | 64 B | 0x01FF/0x00 raw |
| 583 | 0xFF00/0x01 | 3 | **Output** | 32 B | 0xFF00/0x01 raw |
| 600 | 0x0D/0x02 Pen | 7 | Input | 24 B | see §2.2 |
| 813 | 0xFF00/0x81 | 23 | Input | 64 B | 0xFF00/0x05 raw |
| 838 | 0xFF0B/0x0B | 72 | Feature | 2 B | 0xFF0B/0x48 |
| | | 41–45 | Feature | 15/31/62/254/510 B | usages 0x29–0x2D |
| | | 46–54 | Input | 15/31/62/254/510/1022/2046/4094/7487 B | usages 0x2E–0x36 |
| 972 | 0xFF0F/0x60 | 25 | Output **and** Input | 4 B + 4 B | out 0xFF0F/0x00, in 0xFF0F/0xC6 |
| 1010 | 0xFF0F/0x50 | 34 | Input | 16 B | four signed 32-bit, usages 0x72–0x75 |
| 1050 | 0xFFF4/0x01 | 112 | Feature | 1 B | 0x10 enable bit + 7 const |
| | | 86 | Feature | 7 B | 0x03 6-byte **buffer** + 0x09 bit |
| | | 110 | Input | 7 B | 0x02 6-byte buffer + 0x08/0x11 bits |
| | | 111 | Output | 23 B | 0x02 6-byte buffer + 0x08 bit + 0x14 16-byte buffer |

The 0xFFF4 collection's 6- and 16-byte fields carry the HID **Buffer** item flag (raw bytes, not
discrete fields) — consistent with the BD_ADDR/link-key reading in
`docs/protocol/seamless-pairing.md`; see that file for the pairing model and its write ban.

## 2. Standard collections (context)

### 2.1 Touch Screen (R1/R10)
Five finger logical collections + scan time + contact count; `hid-multitouch` handles it
(VERIFIED-HW: event7 + mouse2 exist). R68 is a 256-byte Feature on 0xFF00/0xC5 inside this TLC —
the "Windows 8 certification" parameter blob [HYPOTHESIS: community lore, not verified].

### 2.2 Pen (R7) — the MPP in-range report [STATIC]
Fields: In Range (0x32), Tip Switch (0x42), Barrel Switch (0x44), Invert (0x3C), Eraser (0x45),
Secondary Barrel Switch (0x5A) — 6 bits + 2 constant; X/Y (0x01/0x30,0x31) 16-bit, logical max
17664×10496; Tip Pressure (0x30/0x0D) 0–4096; Battery Strength (0x3B) 0–255; X/Y Tilt (0x3D/0x3E)
signed, ±90.00°; Azimuth (0x3F) 0–36000 (0.01°); Scan Time (0x56); Transducer Serial Number
(0x5B, 32-bit); Transducer Vendor ID (0x91, 16-bit, min 1); + 2 vendor bytes (0xFF00/0x01).
**No haptics page (0x0E) anywhere on this device** — pen haptics live on the pen's Bluetooth HID
side (see `docs/protocol/haptics.md`). Kernel: "Stylus" node event9 (VERIFIED-HW).

## 3. Vendor collections — classification

Buckets: **safe-to-read** (passive input only, no side effects possible), **command**
(host writes change device state — writes forbidden until proven and approved), **dangerous**
(CONTRIBUTING.md §6 blanket ban: 0xFF0B, 0xFF00 report 3, the 0x01FF collection).

| TLC | Bucket | Rationale |
|---|---|---|
| 0xFF00/0xC5 R68 (Feature 256 B) | command-adjacent — **do not read yet** | Feature read *could* be harmless, but unproven; rule: GET_FEATURE only once proven non-destructive. Session 01 will show whether Windows reads it at boot. |
| 0x01FF/0x01 R2 (Input 64 B) | dangerous (rule) | Response side of the maintenance channel paired with R3; input-only, so it only ever answers an R3 command. Passive observation is harmless, but nothing should be initiated. |
| 0xFF00/0x01 R3 (Output 32 B) | **dangerous (rule)** | Host→device command channel; paired with R2 responses. HYPOTHESIS: ELAN maintenance/firmware trigger. Never write. |
| 0xFF00/0x81 R23 (Input 64 B) | safe-to-read (passive) | Unsolicited 64-byte input; no known trigger. HYPOTHESIS: async status/event channel. Observing costs nothing (hid-recorder capture will show whether it ever emits). |
| 0xFF0B/0x0B (Features 41–45,72; Inputs 46–54) | **dangerous (rule)** | Feature=command / Input=response pairs with block sizes 15→510 B (features) and 15→7487 B (inputs) — a flash-page/block pattern. HYPOTHESIS: ELAN ICP (in-circuit programming) = firmware update + config/diagnostics channel. ASUS ships ELAN firmware packages; the updater would use exactly this. Never read or write. |
| 0xFF0F/0x60 R25 (Output 4 B + Input 4 B) | command — writes forbidden | Smallest command/response pair. HYPOTHESIS: quick config/status poke (candidates: Wake-on-Touch enable, MPP uplink control). Which service uses it → the WoT service analysis (findings/005) (ElanTouchXiSvc.exe imports HidD_Get/SetFeature). |
| 0xFF0F/0x50 R34 (Input 16 B) | safe-to-read (passive) | Four signed 32-bit values (0x72–0x75). HYPOTHESIS: telemetry/statistics (e.g. touch counts, errors). |
| 0xFFF4/0x01 (R86/110/111/112) | command (pairing) — see seamless-pairing.md | Already modelled in `docs/protocol/seamless-pairing.md`; R111 is the only write and stays banned until the pairing flow is approved. |

### 3.1 Node mapping gap [HYPOTHESIS]
Six vendor TLCs carry input reports (R2, R23, R46–54, R25, R34, R110) but only **five** UNKNOWN
nodes exist. The input numbering in the fixture skips `input21` between Stylus (input20) and the
next UNKNOWN (input22) — consistent with the 0xFF0B collection's node registration *failing*
(its 7487-byte input report exceeds the I2C-HID buffer limits) after consuming the number.
Confirm from Linux capture 01 (`hid-recorder` shows per-collection report sizes; the missing
collection will simply have no node). Either way `hidraw` exposes the whole device, so our daemon
can reach every report regardless.

## 4. What this means for mppd

- Nothing on this device needs to be written for pen parity: touch (R1), pen (R7) and pairing
  reads are inputs/features; the only writes Windows plausibly performs are R3/R25/R111 and the
  0xFF0B features — all in the dangerous/command buckets and all outside our current scope.
- Passive observation (hid-recorder capture on all event nodes) is free and will show whether
  R2/R23/R34 ever emit unsolicited data — that alone upgrades their purpose hypotheses.
- What Windows *reads* at boot (R68? R72? R86?) is answered by Windows session 01 (boot trace).
- Which reports ElanTouchXiSvc.exe touches is the WoT service analysis (findings/005).

## 5. Open items

1. R68 blob content + whether Windows reads it at boot (session 01).
2. R25 command semantics (the WoT service analysis (findings/005) static + session 01).
3. R23/R34 emitters (hid-recorder capture).
4. Node-mapping hypothesis §3.1 (Linux capture 01).
5. 0xFF0B purpose (firmware updater not in our artifact set; classification stands on the rule).
