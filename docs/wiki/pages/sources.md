# Sources

Everything the wiki's claims trace back to. If you are auditing a page,
start from its citations and land here.

## Findings (the evidence trail)

Numbered documents in the repository's `findings/`, in order of work:

| # | Subject | What it settles |
|---|---|---|
| 001 | Identification | machine, pen, digitizer identities; what Windows requires; the Linux kernel/BlueZ/libinput gaps that define the project |
| 002 | ELAN driver packages | what ASUS ships for the digitizer, and what the INF says about the pairing collection |
| 003 | Store apps | what MyASUS / ProArt Creator Hub / ScreenXpert store about pen settings |
| 004 | Windows haptics host | static analysis of `ISM.dll`: capability reads, the five-field output report, the system-originated feedback map |
| 005 | WoT service | `ElanTouchXiSvc.exe`: proof of the `0xFF00` R2/R3 command channel's live use, and disproof of the R25 wake-on-touch candidate |
| 006 | Hardware capture results | the reference capture set: pairing clock chain, reports 7/34 semantics, haptics on the wire, buttons, battery, the pen-does-not-advertise negative |
| 007 | Pen UI feedback | the hover tick and the haptics policy surface shipped today |
| 008 | Linux seamless pairing | touch-to-pair on stock BlueZ: the flag byte, passkey 0, fresh-pen proof, daemon runs |
| 009 | First Linux haptics | output reports felt on Linux; ordinal 2 inert, ordinal 1 is the stop; arming semantics |
| 010 | Pen debug log | the Nordic UART stream: fuel gauge, current, charger sequence, the three silent vendor services |

## Published documents

- Microsoft, *Seamless Pairing for Windows Pens Implementation Guide* —
  the pairing collection's field definitions and expected flow. [DOC]
- Microsoft, *Haptic Pen Implementation Guide* and *Pen Protocol
  Implementation* — waveform ID table, host behaviour expectations, bus
  guidance. [DOC]
- USB HID Usage Tables (digitizer page `0x0D`) and HUTRR63b (haptics page
  `0x0E`) — usage semantics, ordinal/waveform model. [DOC]
- Bluetooth Core Specification — GATT, HID service `0x1812`, Battery
  service, Device Information `0x2A26`, Security Manager legacy pairing.
  [DOC]
- BlueZ documentation (`doc/mgmt-protocol.rst` among it) — the OOB
  limitation that forces passkey 0. [DOC]
- Linux kernel sources: `hid-multitouch.c`, `hid-haptic.c`, `smp.c` — the
  gaps this project fills. [STATIC]

## Artifacts and fixtures

- `docs/artifacts.tsv` (repository) — every downloaded Windows artifact
  with its SHA-256: the 25H2 install image the haptics host was extracted
  from, the ELAN and MediaTek driver packages, the pairing-capable INFs.
- `fixtures/px13-hn7306/` — the digitizer's captured report descriptor and
  decode dumps, host identification, the Linux `input` device list.
- Instrumented capture sessions are cited by role and date ("Windows
  session 04"), inventoried at the top of `findings/006`; their raw
  captures stay out of git because they contain link keys and serials.
  Nothing on this wiki quotes a captured identifier — see
  [conventions](conventions.md#example-values-are-synthetic).

## Precedent wikis worth reading

This wiki's structure borrows from projects that documented hostile
hardware honestly: the Asahi Linux wiki (audience routing, feature-support
tables), the linux-surface wiki (device pages, one feature matrix),
Switchbrew (per-command reference pages with gaps marked), envytools
(machine-readable databases beside the prose), and libinput's docs
(user/developer split). None of them supplied a single byte about this
pen — they supplied the shape of the pages.
