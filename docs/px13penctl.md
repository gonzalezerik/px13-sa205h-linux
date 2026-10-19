# `px13penctl` — the contract for bars and scripts

`px13penctl` is a thin client of the `px13pend` D-Bus interface (docs/dbus.md):
it reads and follows pens, and never touches Bluetooth or HID. Bars and
scripts consume this output; the tray and settings app use D-Bus
directly. No per-bar plugins (docs/decisions.md).

```console
$ px13penctl status           # one look, plain text
$ px13penctl status --json    # one look, one JSON object on one line
$ px13penctl watch            # follow live, readable lines (not a contract)
$ px13penctl watch --json     # JSON Lines until px13pend goes away
```

`--system` talks to `px13pend serve --system`. `--all` also shows pens that
are paired but not connected — BLE pens drop the link while idle, so the
default view is "pens on a live link".

## The empty rule

No connected pens: stdout is empty and the exit status is 0. No
`{"pens":[]}`, not even a newline. That is what lets a Waybar custom
module hide itself instead of rendering an empty chip. `px13pend`
unreachable: a message on stderr, exit 1, stdout still empty.

## `status --json`

One compact line:

```json
{"pens":[{<pen>}, …]}
```

### The `<pen>` object

| Field | Type | Source |
|---|---|---|
| `path` | string | the pen's D-Bus object path |
| `name` | string | `Pen1.Name` |
| `address` | string | `Pen1.Address` — six bytes of upper-case hex, colon-separated |
| `present` | bool | `Pen1.Present` — the pen holds a Bluetooth link right now |
| `paired` | bool | `Pen1.Paired` |
| `trusted` | bool | `Pen1.Trusted` |
| `battery` | number or null | `Pen1.BatteryPercent`; `null` until the first reading (the wire's `-1`) |
| `battery_detail` | object, or absent | `{"percent": number, "millivolts": number, "docked": bool}` from `Pen1.BatteryPercentPrecise`, `BatteryMillivolts` and `Docked`; `percent` and `millivolts` are absent until the first reading, and the whole object is absent when the daemon has none of the three |
| `haptics` | object or null | `{"enabled": bool, "intensity": 0–100, "while_inking": bool, "while_interacting": bool, "ink_texture": string}`, or `null` when the pen does no haptics |
| `capabilities` | object or null | `Pen1.Capabilities` projected to JSON; `null` only when the read failed |

Capability keys follow docs/dbus.md §6 — a key that is absent means the
pen lacks that capability: `pairing` (`"seamless-touch"` / `"manual"`),
`battery` (`"ble-battery-service"` / `"none"`), `battery_detail` (`"pen-log"`), `buttons`
(`{"barrel", "shortcut", "shortcut_gestures"}`) and `haptics`
(`{"waveforms": [{"ordinal", "name", "duration_ms", "continuous"}]}`).
Haptics UIs draw from `waveforms`, never from a hardcoded effect list.

### Plain-text status

One line per pen — `SA205H  battery 87 %` — with `battery unknown`
before the first battery reading, the pen's own gauge in brackets when
the daemon has it (`SA205H  battery 78 % (78.73 %, 4105 mV)`), and a
`  link down` suffix under `--all`, or `  on charger` for a pen asleep
on its charger.

## `watch`

Without `--json`, `watch` prints one readable line per event, prefixed
with the seconds since it started — for a terminal left open next to the
pen:

```console
$ px13penctl watch
[   0.0 s] watching px13pend: no pen connected yet
[  12.4 s] pen nearby (XX:XX:XX:XX:XX:XX), pairing…
[  13.0 s] paired: XX:XX:XX:XX:XX:XX
[  13.1 s] pen added: SA205H  battery unknown  connected
[  14.6 s] SA205H  battery 87 %  connected
```

That wording may change; scripts read `--json`.

## `watch --json`

JSON Lines: one object per line, tagged `"type"`, flushed as it
happens. The first line is the whole picture:

```json
{"type":"snapshot","pens":[{<pen>}, …]}
```

An empty `pens` array in a snapshot is a statement — nothing is
connected — unlike `status`, which prints nothing. Then, as things
change:

| Line | Meaning |
|---|---|
| `{"type":"pen.added","pen":{<pen>}}` | a pen object appeared |
| `{"type":"pen.changed","pen":{<pen>}}` | a full re-read of the pen after any property change |
| `{"type":"pen.removed","path":"…","address":"…"}` | the pen object is gone; `address` is rebuilt from `path` and empty when the path carried none |
| `{"type":"pairing","kind":"…","detail":"…"}` | pairing progress, `kind` one of `announced` / `bonded` / `failed` (docs/dbus.md §4) |

`pen.added` and `pen.changed` carry the complete pen every time:
apply, never diff. Without `--all`, `pen.added` and `pen.changed` about
link-down pens are not emitted, so a view stays quiet while pens idle
offline; `pen.removed` is always emitted — forgetting a pen the view
never showed is harmless. `watch` exits 1 when the daemon closes the
connection — restart it to follow a restarted `px13pend`.

A pen object in full (address redacted):

```json
{"type":"pen.changed","pen":{"path":"/io/github/gonzalezerik/Px13Pen/pen/dev_00_01_02_03_04_05","name":"SA205H","address":"XX:XX:XX:XX:XX:XX","present":true,"paired":true,"trusted":true,"battery":87,"haptics":{"enabled":true,"intensity":100,"while_inking":true,"while_interacting":true,"ink_texture":"ink"},"capabilities":{"pairing":"seamless-touch","battery":"ble-battery-service","buttons":{"barrel":true,"shortcut":true,"shortcut_gestures":["single","double","long"]},"haptics":{"waveforms":[{"ordinal":3,"name":"click","duration_ms":45,"continuous":false}]}}}}
```

## Exit codes

`0` — success, including empty output. `1` — `px13pend` unreachable, the
stream ended, or a bad command line; the reason is on stderr.
