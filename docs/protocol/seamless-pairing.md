# Seamless pairing — ELAN digitizer ↔ MPP haptic pen over Bluetooth

Status: **working model, not yet verified**. Everything here is STATIC (from the captured digitizer
HID descriptor and unpacked driver packages — see `findings/002-elan-driver-packages.md` for the
evidence and hashes) or explicitly labelled HYPOTHESIS. Nothing in this flow has been observed on
the wire yet; that is Windows capture session 3.

Confidence labels per CONTRIBUTING.md §5: VERIFIED-HW / STATIC / DOC / HYPOTHESIS.

## 1. What the hardware exposes [STATIC]

The ELAN9008 (`ELAN9008:00 04F3:4631 I2C-HID`) digitizer descriptor contains a vendor top-level
collection **Usage Page 0xFFF4, Usage 0x01 (Application)** with four logical sub-collections, one
per report. Source: `fixtures/px13-hn7306/elan9008-04f3-4631.hid-decode.txt` (file lines
499–600).

| Report | ID | Direction | Fields (usage / size / item type) |
|---|---|---|---|
| 0x16 | 112 (0x70) | Feature | usage 0x10: 1 bit Data,Var,Abs + 7 bits Cnst |
| 0x06 | 86 (0x56) | Feature | usage 0x03: 6 bytes Data,Var,Abs,**BufferedBytes** + usage 0x09: 1 bit Data + 7 bits Cnst |
| 0x13 | 110 (0x6E) | Input | usage 0x02: 6 bytes Data,Var,Abs,**BufferedBytes** + usage 0x08: 1 bit + usage 0x11: 1 bit + 6 bits Cnst |
| 0x15 | 111 (0x6F) | Output | usage 0x02: 6 bytes **BufferedBytes** + usage 0x08: 1 bit + 7 bits Cnst + usage 0x14: 16 bytes **BufferedBytes** |

Observations that drive the model:

- The three 6-byte fields (feature 86, input 110, output 111) are the only address-sized fields in
  the collection; a Bluetooth classic BD_ADDR is exactly 6 bytes. [STATIC fact, HYPOTHESIS meaning]
- The 16-byte output field (usage 0x14) is the only key-sized field; an SSP link key is 16 bytes.
  [STATIC fact, HYPOTHESIS meaning]
- The `BufferedBytes` modifier on those fields means the device treats them as opaque buffers —
  consistent with raw address/key material rather than decoded values. [STATIC]
- Report 110 is the only *Input* report: the digitizer spontaneously notifies the host. Reports 86
  and 112 are host-initiated reads; 111 is the only host→digitizer write in the collection.
- The ELAN "Seamless Pairing" INF matches exactly this collection
  (`HID\VID_04F3&UP:FFF4_U:0001`) and sets the device property
  `{FE683ED0-DE80-4766-B422-4195993F4330},2 = "1"` (INF comment: `DEVPKEY_BluetoothOobCouplingChannel_OptIn`).
  No inbox Windows component reads that property or these reports (82-binary negative,
  findings/002 §2), so a user-mode consumer is expected — ASUS Store app(s), or none at all.

## 2. Working model of the flow [HYPOTHESIS — everything below this line in this section]

Actors: **pen** (SA205H, BLE/classic BT HID device, MPP 2.6), **digitizer** (ELAN9008, on I2C, also
in MPP RF range of the pen), **PC host software** (unknown consumer; candidate: ASUS Store app via
`BluetoothDeviceCustomPairing` OOB APIs [DOC — learn.microsoft.com]), **Windows BT stack**
(bthport/bthserv, which do support SSP with OOB data [STATIC — findings/002 §2]).

1. Pen is unpaired (or factory-reset). User taps the pen tip to the screen — the same gesture that
   makes the digitizer detect the pen's MPP signal.
2. The digitizer learns the pen's identity over the MPP link (the pen broadcasts its Transducer
   Serial 0x0D/0x5B and Vendor ID 0x0D/0x91 in-band; the digitizer can also read the pen's BD_ADDR
   if the pen transmits one before pairing).
3. The digitizer notifies the host with **input report 110**: 6-byte candidate BD_ADDR (usage 0x02)
   + two state bits (usage 0x08 "address valid / pairing request?", usage 0x11 "pairing result /
   state?"). [HYPOTHESIS — bit meanings unconfirmed]
4. The host consumer reads **feature 86** (6-byte buffer + 1 bit) — possibly the digitizer's own
   address or an OOB pairing token/TK. [HYPOTHESIS]
5. The host starts Bluetooth pairing for the address from 110, supplying OOB data if available
   (Windows `BluetoothDeviceCustomPairing` + DAF OOB path), so no PIN/confirmation UI appears.
6. On success the host writes **output report 111**: 6-byte BD_ADDR + 1 bit + **16-byte link key**
   (usage 0x14) — relaying the key to the pen through the digitizer→pen MPP channel so the pen can
   complete/confirm the encrypted link. [HYPOTHESIS — this is the "coupling channel" the INF name
   suggests: the digitizer couples the PC and the pen by carrying key material both ways.]
7. Feature 112 (1 bit) is a simple enable/ready flag for the whole mechanism. [HYPOTHESIS]

Alternative model (equally consistent with all static evidence so far): the digitizer firmware does
steps 2–4 and 6 entirely on its own over the MPP link, Windows pairs with plain SSP just-works
(no OOB), and the 0xFFF4 reports exist only for the ASUS app's UX (toast, progress). The Windows capture
session-3 trace distinguishes these: OOB model ⇒ HCI `Remote OOB Data Request/Reply` + SSP with
OOB flag in the btsnoop/ETL; firmware model ⇒ plain SSP with no user interaction and no host HID
traffic on 110/111 beyond maybe a read.

## 3. Field-level open questions

- Exact semantics of usages 0x02/0x03/0x08/0x09/0x10/0x11/0x14 — vendor-defined, no public ELAN doc
  found. [unknown]
- Whether report 110 fires spontaneously (on pen approach) or only after a feature write arms it.
- Byte order of the BD_ADDR fields (little vs big endian) — resolvable from a capture.
- Whether the 16-byte field is the link key, a temporary key/TK, or a MPP-encrypted blob.
- Whether re-pairing (pen already paired to this PC, pen reset) reuses the same reports.

## 4. Safety rules while investigating (CONTRIBUTING.md §6)

- **Never write to the digitizer**: output report 111 is in the write-banned vendor space of this
  device class until proven safe; no `SET_REPORT`/output on 0xFFF4, 0xFF0B, 0xFF01FF, or 0xFF00
  report 3 — ever, without explicit byte-level approval.
- Feature reads (86, 112) are *probably* harmless but are **not yet classified non-destructive**;
  under CONTRIBUTING.md §6 they stay off-limits until classification, which needs either ELAN doc or a
  supervised Windows-side observation of what the real consumer reads.
- All verification therefore happens passively: btmon/btsnoop during a manual pairing on
  Linux, and the Windows ETLs (Windows capture session 3).

## 5. What we need to finish this doc

1. Windows capture session 3 ETLs (BluetoothStack + input): shows the real flow, which side initiates,
   whether HCI OOB commands appear, and correlates report 110 timing with the pairing events.
2. Store-app RE (MyASUS / ProArt Creator Hub / ScreenXpert): find (or rule out) the consumer that
   reads 110/86 and writes 111.
3. Linux-side passive capture: pen advertisement contents (does the unpaired pen
   advertise a BD_ADDR-based name?), and whether the ELAN driver on Linux (`hid-multitouch`/
   `hid-asus`) touches any 0xFFF4 report at probe time.

When each lands, upgrade the matching HYPOTHESIS lines above to VERIFIED-HW or mark them wrong —
do not delete them, annotate.
