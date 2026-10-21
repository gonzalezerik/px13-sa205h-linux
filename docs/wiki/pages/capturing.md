# Capturing — how the evidence gets made

Every VERIFIED-HW claim here came from an instrumented session with a
numbered script beside it. This page is the method, so a claim can be
re-made or challenged.

## The ground rules

- **Read-only by default.** Captures enable notifications and read;
  nothing is written to a device except the project's deliberate,
  reviewed writes (haptic output reports, the `0x6F` pairing key).
- **Anything that writes asks first.** The first-haptics script prints
  every byte and waits for a per-step `y` — those were the project's
  first writes to a pen.
- **Raw captures stay out of git.** They contain link keys and per-device
  serials; they move machine-to-machine by direct copy, and findings
  reference sessions by role and date ("session 04"), inventoried at the
  top of findings/006. No wiki page quotes a captured identifier — see
  [conventions](conventions.md#example-values-are-synthetic).

## Linux scripts (`tools/capture/linux/`)

| Script | What it records |
|---|---|
| `01-digitizer-hid-recorder.sh` | raw HID from the digitizer (`hid-recorder`, hid-tools) + `evtest`'s decoded view, while the pen hovers/touches/presses/rests |
| `02-btmon-advertise.sh` | `btmon` HCI log while the pen advertises (read-only; nothing paired) |
| `03-manual-pair.sh` | one manual pairing, typed by the operator in `bluetoothctl`, with btmon running; then dumps BlueZ's view, the GATT tree, the BT HID descriptor, the input nodes that appeared |
| `04-tap-to-pair.sh` | one live touch-to-pair against the installed daemon: feature-flag state before/after + full BlueZ dump after |
| `05-first-haptics.sh` | the byte-by-byte, per-step-confirmed first haptic sends (prerequisite: paired + awake — the link drops while idle) |
| `06-seamless-pairing.py` | the standalone reference touch-to-pair run behind findings/008: `0x56` → `0x70` → watch `0x6E` → write `0x6F` → pair the advertiser, passkey 0 |
| `07-buttons-idle-dock.sh` | three read-only observations in one sitting: per-button evdev codes, idle-sleep duration (and that plain sleep carries no `Charge State` line), `Docked` flip on the charger |

`tools/capture/import-windows.sh` pulls Windows-side session files into
the same evidence tree.

## Windows sessions

The instrumented Windows sessions (01–07) pair, trace and toggle while
ETW/`BluetoothStack.Verbose`/Process-Monitor-style logging records the
stack's side; their outputs produced the pairing clock chain, the report
7/34 semantics, the haptics wire model and the settings-store location
(findings/006 §1 inventory). Known logging hole, learned the hard way:
the Bluetooth verbose logger truncates payloads to ≈11 bytes — capture
**full** ATT payloads needs a sniffer (a second adapter), which no
session has run yet.

## Artifacts (the static side)

`tools/fetch-artifacts.sh` downloads everything listed in
`docs/artifacts.tsv` and checks SHA-256: the Windows 11 25H2 install
image the haptics host was extracted from (single files pull via
`wimlib-imagex extract <esd> 9 /Windows/System32/<file> …`), the ELAN and
MediaTek driver packages, the pairing-capable INFs.
`tools/fetch-store-app.py` fetched the store packages verified against
the download service's own digests (findings/003 method section).
Decompiled code stays out of the repository; findings cite behaviour and
addresses, never decompiled bodies.
