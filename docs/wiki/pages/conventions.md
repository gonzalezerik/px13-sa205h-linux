# Conventions — how to read every page here

Three small languages run through this wiki: confidence labels, byte
notation, and citation paths. They are defined once, here.

## Confidence labels

Every protocol claim carries one of four labels (the same labels the
repository's `CONTRIBUTING.md` requires on every protocol fact):

| Label | Meaning | Trust |
|---|---|---|
| **VERIFIED-HW** | Seen on real hardware; cites the capture and the frame or offset | strongest |
| **STATIC** | Read out of a shipped binary or descriptor; cites file and offset | strong, but the code may never run |
| **DOC** | Published specification or vendor guide; cites the source | what the vendor says, not what the hardware does |
| **HYPOTHESIS** | Our own guess, stated so it can be killed | none by itself |

**Observed behaviour wins over static analysis, which wins over
documentation.** Pages may attach a label to a whole table ("Complete
report map [STATIC]") and override it per row; the nearest label governs.
Some findings use **OBSERVED** for passively watching a stream that has no
write side at all — treat it as VERIFIED-HW with no causation implied.

Labels also fail honestly: a field marked HYPOTHESIS that a later capture
answered gets the answer inline, with the original guess kept visible. The
pairing page is the model — it lists four early guesses that measurement
replaced.

## Byte notation

- **Offsets are from the start of the captured frame, with the Report ID as
  byte 0.** A digitizer pen report therefore reads
  `07 <payload…>`, and a pen Bluetooth output report `41 <payload…>`.
- **Little-endian unless a table says otherwise.** This covers almost
  everything: the digitizer's serial, the Bluetooth addresses in the pairing
  reports, the haptics retrigger field.
- **Bits are named by HID usage plus mask**, not by numbering scheme —
  `Host Pairing Exists (0x0D:0x11, bit in flag byte)` — because the HID
  spec's bit numbering and capture tools disagree; masks don't.
- **Direction words are the device's point of view:** *Input* = device→host,
  *Output* = host→device, *Feature* = host reads (and, for the few writable
  features, writes).
- Units: pressure is 0–4096 usage units, tilt is signed degrees to 0.01°,
  azimuth is 0–36000 (0.01°), the digitizer battery field is a raw 0–255
  byte whose exact percentage mapping is [HYPOTHESIS]; the Bluetooth
  battery descriptor and the debug-log gauge carry percentages properly.

## Example values are synthetic

No real device identifiers appear anywhere in this wiki. Serials and keys
in examples are invented round numbers (`0x12345678`); Bluetooth addresses
are never written out in any form, real or invented — the project's
privacy gate rejects any address-shaped literal in a tracked file — and
captured values are referred to by role ("the host's adapter address")
exactly as in the findings.

## Citations

- `findings/006 §4` — a section of the repository's numbered findings
  document (the evidence trail: what was captured, when, and what it showed).
- `docs/protocol/haptics.md §2.2` — the working protocol documents the wiki
  distils; where wiki and protocol doc disagree, the protocol doc plus its
  findings are authoritative.
- `fixtures/px13-hn7306/…` — captured descriptors and dumps checked into
  the repository, each with its SHA-256 where it matters.
- Ghidra-recovered behaviour cites `ISM.dll:0x…`-style image addresses
  against the named binary and its SHA-256; decompiled code itself is not
  reproduced here.
