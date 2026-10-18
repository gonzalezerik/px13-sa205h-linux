# Seamless pairing — ELAN digitizer ↔ MPP haptic pen over Bluetooth

Status: **working model, not yet verified**. Everything here is STATIC (from the captured digitizer
HID descriptor and unpacked driver packages — see `findings/002-elan-driver-packages.md` for the
evidence and hashes) or explicitly labelled HYPOTHESIS. Nothing in this flow has been observed on
the wire yet; that is Windows capture session 3.

**Update 2026-10-17:** the flow has since been captured (Windows sessions 03 + 08, Linux step 1) and
is VERIFIED-HW as annotated below — the original hypothesis text is kept and marked, per the note at
the end of this file. Full evidence: `findings/006-hardware-capture-results.md` §2–3 and §7.

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

## 2. Working model of the flow [originally HYPOTHESIS; annotated with capture results 2026-10-17]

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
   **[VERIFIED-HW — session 08 run 2 + Linux capture]** the report fires spontaneously ~0.10–0.15 s
   after the tip touches: 6-byte BD_ADDR **little-endian** + one flag byte. It does not need a
   touch: in the Linux capture (no consumer running) it arrived 9 times, 7 of them while the pen
   only hovered, before its first touch — once in range, an unconnected pen is announced again
   every few seconds. Flag is `0x03` while the
   Windows consumer is active and `0x01` on bare Linux with no consumer — the second bit only
   appears with the Windows stack listening; its meaning stays HYPOTHESIS.
4. The host consumer reads **feature 86** (6-byte buffer + 1 bit) — possibly the digitizer's own
   address or an OOB pairing token/TK. [HYPOTHESIS — **unconfirmed**: `HidD_GetFeature` on 86 fails
   with error 122 from user mode on 25H2, and kernel-side reads are not visible in our traces]
5. The host starts Bluetooth pairing for the address from 110, supplying OOB data if available
   (Windows `BluetoothDeviceCustomPairing` + DAF OOB path), so no PIN/confirmation UI appears.
   **[VERIFIED-HW for the connect — sessions 03 + 08]** the host disables its background scan
   **2 ms** after report 110, adds the address (type **random**) to the accept list and sends LE
   Extended Create Connection straight to it; no advertising report from the pen is ever used.
   Pairing is **LE legacy SMP** with full key distribution both ways. The OOB part remains
   HYPOTHESIS — SMP payload bytes are truncated in the trace.
6. On success the host writes **output report 111**: 6-byte BD_ADDR + 1 bit + **16-byte link key**
   (usage 0x14) — relaying the key to the pen through the digitizer→pen MPP channel so the pen can
   complete/confirm the encrypted link. [HYPOTHESIS — this is the "coupling channel" the INF name
   suggests: the digitizer couples the PC and the pen by carrying key material both ways.]
   **[STILL OPEN]** user-mode tracing cannot see host→device HID writes; whether Windows ever
   writes report 111 is unobserved. The first Linux tap-to-pair run (no 111 write at all) is the
   decisive test: if plain Just Works bonds, step 6 is optional.
7. Feature 112 (1 bit) is a simple enable/ready flag for the whole mechanism. [HYPOTHESIS — the
   read-back value `01` on Windows is **VERIFIED-HW**; the meaning of the bit is not]

Alternative model (equally consistent with all static evidence so far): the digitizer firmware does
steps 2–4 and 6 entirely on its own over the MPP link, Windows pairs with plain SSP just-works
(no OOB), and the 0xFFF4 reports exist only for the ASUS app's UX (toast, progress). The Windows capture
session-3 trace distinguishes these: OOB model ⇒ HCI `Remote OOB Data Request/Reply` + SSP with
OOB flag in the btsnoop/ETL; firmware model ⇒ plain SSP with no user interaction and no host HID
traffic on 110/111 beyond maybe a read.
**[ANNOTATED 2026-10-17]** the *who* is resolved: the consumer is **Windows' own Bluetooth stack** —
`bthserv` + DeviceAssociationService start working at the connect moment, ELAN's INF ships no
service, and tap-to-pair completed on a Windows install with **no ASUS apps present at all**. The
OOB-vs-Just-Works half of the distinction is still open; the Linux pairing run answers it.

## 3. Field-level open questions

- Exact semantics of usages 0x02/0x03/0x08/0x09/0x10/0x11/0x14 — vendor-defined, no public ELAN doc
  found. [unknown — the 0x08/0x11 bit pair is now *observed* as one flag byte: `0x03` with the
  Windows consumer active, `0x01` on bare Linux; meanings still unknown]
- Whether report 110 fires spontaneously (on pen approach) or only after a feature write arms it.
  [**RESOLVED 2026-10-17**: spontaneous and unarmed — nine spontaneous reports in an 84 s Linux
  capture with no vendor driver, one per pen touch/range event]
- Byte order of the BD_ADDR fields (little vs big endian) — resolvable from a capture.
  [**RESOLVED 2026-10-17**: little-endian; the Linux reports carry byte-for-byte the address the
  Windows host connected to]
- Whether the 16-byte field is the link key, a temporary key/TK, or a MPP-encrypted blob.
- Whether re-pairing (pen already paired to this PC, pen reset) reuses the same reports.
  [partially observed: a *paired* pen re-entering range triggers another `0x6E` with the same
  address; a factory-reset re-pair has not been tested]

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
   [**done** — sessions 03 + 08; flow verified, timing correlated; OOB flags not visible in the
   truncated HCI payloads. findings/006 §2]
2. Store-app RE (MyASUS / ProArt Creator Hub / ScreenXpert): find (or rule out) the consumer that
   reads 110/86 and writes 111. [**ruled out for pairing** — tap-to-pair works with no ASUS apps
   installed; the consumer is the Windows Bluetooth stack. Store-app RE stays relevant only for
   optional UX, not for the mechanism]
3. Linux-side passive capture: pen advertisement contents (does the unpaired pen
   advertise a BD_ADDR-based name?), and whether the ELAN driver on Linux (`hid-multitouch`/
   `hid-asus`) touches any 0xFFF4 report at probe time. [**done** — the pen never advertises
   (210 reports captured, none from it); Linux HID drivers leave report 110 unconsumed and it
   keeps firing. findings/006 §3, §7.5]
4. [new] Whether output report 111 must be written for pairing to succeed — answered by the first
   Linux tap-to-pair run, which pairs with no 111 write (write ban intact, CONTRIBUTING.md §6).

When each lands, upgrade the matching HYPOTHESIS lines above to VERIFIED-HW or mark them wrong —
do not delete them, annotate.

## 6. Implementation (landed 2026-10-17, corrected 2026-10-18)

`oppd watch-pair` runs this model end to end: it parses report 110 (`0x6E`) off the digitizer's
hidraw node (`opp_core::seamless`), then drives BlueZ. The announced address is a random static
one (§2 step 5, VERIFIED-HW), so the call is `org.bluez.Adapter1.ConnectDevice` with
`AddressType "random"` — BlueZ accepts only `public`/`random`; `"le"` is not a value its API
takes. Then `org.bluez.Device1.Pair`, then the device's `Trusted` property is set.
`oppd pairing-state` reads feature 112 (`0x70`) and nothing else; §4's write ban holds — the
daemon writes no output report and no unclassified feature. `--dry-run` prints the notifications
and stops there.

Two deployment facts, neither of which was in the first version of this section:

- On BlueZ 5.87 `ConnectDevice` is `[experimental]`: the plain daemon does not export it, and the
  call answers `org.freedesktop.DBus.Error.UnknownMethod`. Shipping a tool that requires
  `--experimental` is not acceptable, so that error is the normal path: oppd falls back to the
  kernel directly through a D-Bus-activated helper (`opp-pair-helper`, bus name
  `org.openpenprotocol.PairHelper1`), which opens an `HCI_CHANNEL_CONTROL` management socket and
  sends `MGMT_OP_PAIR_DEVICE` with LE-random address type and `NoInputNoOutput` I/O capability —
  Just Works, no PIN UI. The helper checks the polkit action `org.openpenprotocol.pair-pen`
  against the calling connection before it opens anything, accepts only random static addresses,
  and does nothing besides pairing. Setting `Trusted` afterwards is done by the user's own oppd.
  A stale stored bond (dual-boot re-pair) is handled by removing the device and re-running the
  handshake.
- The kernel fallback itself is HYPOTHESIS until the hardware session confirms it on this pen:
  it follows BlueZ's `mgmt-api.txt`, but a kernel-initiated bond followed by GATT discovery and
  the HID-over-GATT probe has not been observed on this hardware yet. (Hovering, report `0x6E`
  and the digitizer side need no bond at all — findings/006 §3, §7.5.)

Open item 4 closes on the first live bonding run.
