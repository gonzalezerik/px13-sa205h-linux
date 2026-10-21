# Open questions

Everything the project does **not** know, kept honest so nobody builds on
a guess. Each row: the question, the best current answer, and the one
thing that would settle it.

## The devices

| Question | Best current answer | Settled by |
|---|---|---|
| What are the three silent vendor GATT services for? | Command/response shape, zero traffic in any session; update/test/config is guesswork [findings/010 §1] | The rule keeps them untouched — stays open by design |
| `wake reason` code table | Only `4` ever seen (every wake; also ~1 s before docks) | A pen that wakes some other way — lid, charger-less timer? |
| Does report 7's Battery Strength byte track the Bluetooth percentage? | Both present; mapping of the raw byte unknown (see the [digitizer reference](reference/digitizer.md)) | One session logging both while the gauge moves |
| Report 7's `0x0C0F`/`0x0003` words in `0x40` | Constant so far [HYPOTHESIS] | A second pen / firmware version to diff |
| Hover distance above the glass | Never measured — **don't quote a figure** (the repository's `docs/how-the-pen-connects.md` says so explicitly) | A calibrated spacer rig against report 7 |

## The digitizer

| Question | Best current answer | Settled by |
|---|---|---|
| What is R68 (256 B feature)? | Command-adjacent, unproven (`docs/protocol/digitizer-tlc.md` §3) | Nothing safe yet — GET needs a proven-safe precedent first |
| Who consumes R25 (4+4 B pair)? | Not WoT (disproved), not wake-on-touch [findings/005 §negatives] | Windows traces around the ELAN service, or vendor docs |
| Does 0xFF0B register no input node while consuming an input number? | Plausible node-map gap; 7 487 B input exceeds the I2C-HID buffer [HYPOTHESIS — `docs/protocol/digitizer-tlc.md` §3.1] | Kernel-side experiment — deliberately low priority (never touch that collection) |

## Pairing & reconnect

| Question | Best current answer | Settled by |
|---|---|---|
| The daemon's fresh-pen path (pen that never met this host, no button-pairing assist) | Reference script proves the flow; daemon runs were all with a pen already holding this host's pairing ([pairing]) | A second pen, or a safe way to make the pen forget |
| How many host pairings does a pen carry? | Only ever seen one host's history at a time — untested | Second-pen sessions |
| Notification types 0–4 semantics (hover-enter vs boundary family) | {1,2}→Hover, {0,3,4}→Collide observed; names are HYPOTHESIS (`docs/protocol/haptics.md` §2.4) | Windows traces of the message producer |
| Windows' per-effect repeat/cutoff/retrigger | Traces truncate at ≈11 B — **unrecoverable**, labelled unknown | A full-payload Bluetooth sniffer (second adapter) |
| Does the cutoff field end a texture within one continuous contact? | Session couldn't tell; daemon never arms one [findings/009 §4] | Timed sends with a stopwatch, not a felt call |

## The stack

| Question | Best current answer | Settled by |
|---|---|---|
| Does evdev surface the barrel bit as `BTN_STYLUS` on this device? | Wire bit proven; evdev check queued ([buttons]) | One-minute `evtest` press |
| Azimuth compression impact (`hid-multitouch` vs Windows) | Static-only finding [findings/001 §4] | Side-by-side drawing capture, both OSes |
| Any `Charge State:` text other than `Not Charging`? | Never seen in any context [findings/010 §4] | Only a pen behaviour change would say |

[pairing]: flows/pairing.md#status-on-linux
[buttons]: flows/buttons.md
