# The Px13Pen D-Bus interface — version 1

The interface `px13pend` exports is the project's product (docs/decisions.md): frontends —
`px13penctl`, the tray, the settings window, bar modules — are all clients of it and never touch
the hardware. A pen is described by its **capabilities**, never by a model or a protocol name.

Status: **v1, implemented** by `px13pend serve` (`crates/px13pend/src/service.rs`).

## 1. Naming, bus, availability

- Well-known name: **`io.github.gonzalezerik.Px13Pen.Daemon1`**.
- Bus: the **session bus** by default. `px13pend serve --system` exports the same interface on
  the system bus instead, for a system-wide daemon layout; clients find it on the bus they
  use. `px13pend` always talks to BlueZ on the system bus, whichever bus it exports on.
- Namespace: `io.github.gonzalezerik.Px13Pen` is the reverse domain of the project's
  repository (`github.com/gonzalezerik/px13-sa205h-linux`). If that ever needs to change,
  the rename is a major version bump (§2).

## 2. Versioning

- The **major version** is the digit in each interface name (`Daemon1`, `Pen1`, `Haptics1`)
  and in `Daemon1.ApiVersion`.
- Additive changes — a new method, property, signal or capability key — do not bump the
  major; clients must ignore what they don't know.
- Removals, renames or type changes need the next major, exported alongside the old one.
- A capability key whose value a client doesn't understand is treated as absent.

## 3. Object layout

| Path | Interfaces | |
|---|---|---|
| `/io/github/gonzalezerik/Px13Pen/Daemon` | `io.github.gonzalezerik.Px13Pen.Daemon1` | one per daemon |
| `/io/github/gonzalezerik/Px13Pen/pen/dev_00_01_02_03_04_05` | `io.github.gonzalezerik.Px13Pen.Pen1`, plus `io.github.gonzalezerik.Px13Pen.Haptics1` when the pen does haptics | one per known pen |

Pen object paths embed the Bluetooth address in the BlueZ style: `dev_` followed by the
six address bytes in lower-case hex, underscore-separated.

Discovery: `ListPens()` for the current set, `PenAdded`/`PenRemoved` for changes. A pen
object exists while BlueZ knows the device (bonded pens included after a restart) and is
removed when BlueZ forgets it — announcing a not-yet-paired pen does **not** create one.
`org.freedesktop.DBus.ObjectManager` on the root object is planned, not in v1.

## 4. `io.github.gonzalezerik.Px13Pen.Daemon1`

### Properties

| Property | Type | Meaning |
|---|---|---|
| `ApiVersion` | `u` | major version of this interface, `1` |
| `Version` | `s` | `px13pend` package version |
| `PairingActive` | `b` | the touch-to-pair listener is running |
| `SupportedDigitizers` | `a(qqs)` | vendor, product, name — what `px13pend` can listen on |

### Methods

| Method | In | Out | Meaning |
|---|---|---|---|
| `ListPens()` | — | `ao` | object paths of the known pens |
| `StartPairing()` | — | `b` | turn the touch-to-pair listener back on (it starts on its own) and retry every pen; `b` is whether the state changed. Idempotent. |
| `StopPairing()` | — | `b` | stop it; `b` is whether the state changed. Idempotent. |

Pairing is the flow of docs/protocol/seamless-pairing.md: the host registers its address with
the digitizer, the digitizer announces a pen that came into range (hovering or touching), the
host sends a pairing key, and the pen advertises; the host pairs that advertiser with passkey
entry (passkey 0, §3 there) through the daemon's own agent and trusts it. When a supported
digitizer is present the listener runs from the moment `px13pend` starts, so
bringing a new pen near the screen is all it takes. `StopPairing()` turns it off;
`StartPairing()` turns it back on and retries every pen at once.

A pen that already holds a link is left alone. A pen that is paired but not connected gets a
plain reconnect; only a stale key (the other OS of a dual boot re-paired the pen) leads to a
fresh bond. A failed attempt is retried on a later announcement after a few seconds. Each new
bond, and the first failure of a streak, raises a desktop notification
(`org.freedesktop.Notifications`, session bus only); reconnects stay silent.

### Signals

| Signal | Args | Meaning |
|---|---|---|
| `PenAdded` | `o path, s name` | a pen object appeared. Clients that start after the pens did use `ListPens()`, so there is no replay at connect. |
| `PenRemoved` | `o path` | a pen object was removed; `name` is gone with it |
| `PairingEvent` | `s kind, s detail` | pairing progress: `kind` is `announced` (digitizer reported a candidate), `bonded` (BlueZ pairing and trust done) or `failed` (BlueZ refused). `detail` is the address, plus a short reason for `failed`. |

## 5. `io.github.gonzalezerik.Px13Pen.Pen1`

### Properties

| Property | Type | Meaning |
|---|---|---|
| `Name` | `s` | Bluetooth name, e.g. the pen's model name |
| `Address` | `s` | Bluetooth address, six bytes in upper-case hex, colon-separated |
| `Present` | `b` | the pen holds a Bluetooth link **right now**. BLE pens drop the link while idle and reconnect on first use (findings/006 §5), so `false` is normal between uses. |
| `Paired` | `b` | BlueZ reports the device as paired |
| `Trusted` | `b` | BlueZ trust flag (needed for reconnects without asking) |
| `BatteryPercent` | `i` | 0–100, or `-1` while no reading has arrived |
| `Capabilities` | `a{sv}` | the capability dict, §6 |

Every property change emits standard `org.freedesktop.DBus.Properties.PropertiesChanged`;
there are no custom changed-signals.

## 6. The capability schema

`Capabilities` is the whole reason frontends don't know pen models. Keys below are the
v1 set; **a missing key means the pen does not have that capability**, an unknown key is
ignored.

| Key | Type | Meaning |
|---|---|---|
| `pairing` | `s` | `"seamless-touch"` — tip-to-screen pairing via the digitizer announcement; `"manual"` — pair like any Bluetooth device |
| `haptics` | `a{sv}` | present when the pen plays haptics. `waveforms`: `a(qsqb)` — `(ordinal, name, duration_ms, continuous)` for each playable waveform. Continuous waveforms are felt only while the tip is in contact (the pen auto-triggers them from the tip switch, findings/006 §4). |
| `buttons` | `a{sv}` | `barrel`: `b` — the barrel switch, reported in-band by the digitizer, not over Bluetooth; `shortcut`: `b` — the top-end button over Bluetooth; `shortcut-gestures`: `as` — the gestures the pen itself distinguishes, from `"single"`, `"double"`, `"long"` (findings/006 §5: the pen classifies them, the host must not re-time) |
| `battery` | `s` | `"ble-battery-service"` — the standard Bluetooth battery service, also visible in UPower without this daemon; `"none"` |
| `settings` | `a{sv}` | settings stored **in the pen** (as opposed to host policy). None today on supported pens, so the key is absent; keys will be added when a pen with stored settings is supported. |

For the first supported pen (SA205H-class), `waveforms` carries:

```
( 3, "click",    45, false)  (10, "pencil",  1995, true)
( 4, "press",    60, false)  (11, "marker",  1995, true)
( 5, "release",  60, false)  (12, "chisel",  1995, true)
( 6, "hover",    35, false)  (13, "brush",   1980, true)
( 7, "success", 145, false)  (14, "eraser",  1995, true)
( 8, "error",   185, false)  (15, "sparkle", 1980, true)
( 9, "ink",    1995, true)
```

Ordinals 1 (`none`) and 2 (`stop`) are not waveforms — they are implicit commands, see
`Haptics1`. The waveform list is what the pen's capability report returns; a frontend
must draw its haptics UI from this array, not from a hardcoded effect list.

## 7. `io.github.gonzalezerik.Px13Pen.Haptics1`

Exported on a pen path **only when** `Capabilities` has `haptics`. Every method returns
whether a report was actually written to the pen; suppression is not an error.

### Methods

| Method | In | Out | Meaning |
|---|---|---|---|
| `PlayEffect` | `s name, y intensity, y repeat, q cutoff_ms, q retrigger_ms` | `b played` | play a waveform by name (case-insensitive, one of the capability list). Unknown name → `org.freedesktop.DBus.Error.InvalidArgs`. |
| `PlayOrdinal` | `q ordinal, y intensity, y repeat, q cutoff_ms, q retrigger_ms` | `b played` | same, by ordinal (the number in the capability tuple). |
| `Stop` | — | `b played` | the stop command. The spec's ordinal 2 is inert on the first supported pen, so this writes ordinal 1 (`none`), which cuts active playback and disarms the pen until the next write (findings/009 §3). |

Argument semantics (the wire form is the 7-byte output report of
docs/protocol/haptics.md §2):

- `intensity`: percent of full strength, `0..100`, mapped to the wire byte as
  `round(percent × 255 / 100)`. The value `255` means "use the `Intensity` property".
- `repeat`: number of plays, `1` = once. `1` and `3` are verified on hardware; other
  values are passed through per the HID spec.
- `cutoff_ms`: ceiling on total playback, in 10 ms units after rounding up (max 2550 ms);
  `0` = none.
- `retrigger_ms`: pause between repeats; `0` = none.

### Properties

| Property | Type | Meaning |
|---|---|---|
| `Enabled` | `b` r/w | master haptics switch. `false` makes every method return `false` without touching the pen — exactly what the Windows toggles do (findings/006 §4). |
| `Intensity` | `y` r/w | default strength for `PlayEffect`/`PlayOrdinal` calls that pass the `255` sentinel; 0–100, default 100. |

Policy only: `Enabled` and `Intensity` are what `px13pend` applies at play time, never
written to the pen, and **not yet persisted across daemon restarts** — a settings file
lands with the settings app.

## 8. Errors

v1 uses the standard `org.freedesktop.DBus.Error.*` names, with a stable prefix in the
message text so clients can branch:

- `InvalidArgs` — bad effect name, `intensity` outside `0..100` and not `255`.
- `Failed("not-present: …")` — the pen has no live link.
- `Failed("no-hid-node: …")` — paired, but the pen's HID node isn't there (not connected,
  or no permission).
- `Failed("no-digitizer: …")` — `StartPairing` with no supported digitizer present.
- `Failed("pairing-inactive: …")` — `StopPairing` while inactive is *not* an error
  (idempotent); reserved for real pairing-listener failures.

`io.github.gonzalezerik.Px13Pen.Error.*` is reserved for future majors.

## 9. Not in v1 (deliberately)

- `ObjectManager` / `GetManagedObjects` on the root object.
- Button **events**: the shortcut button's key codes are still being identified
  (`px13pend watch-buttons`), so v1 declares the buttons capability but emits no signals;
  the plan is a `ButtonEvent` signal on `Pen1` and a `uinput` key emitter owned by
  `px13pend` (docs/decisions.md), which needs no interface change for the emitter itself.
- Persisted settings, D-Bus service activation.
- The app-facing haptics portal (xdg-desktop-portal) — a separate interface on top of
  this one, out of scope here (docs/decisions.md §scope, item 3).

## 10. Trying it

```console
$ px13pend serve &
$ busctl --user call io.github.gonzalezerik.Px13Pen.Daemon1 \
    /io/github/gonzalezerik/Px13Pen/Daemon io.github.gonzalezerik.Px13Pen.Daemon1 ListPens
ao 1 "/io/github/gonzalezerik/Px13Pen/pen/dev_00_01_02_03_04_05"
$ busctl --user call io.github.gonzalezerik.Px13Pen.Daemon1 \
    /io/github/gonzalezerik/Px13Pen/pen/dev_00_01_02_03_04_05 \
    io.github.gonzalezerik.Px13Pen.Haptics1 PlayEffect s y y q q "click" 255 1 0 0
b true
```

`255` as intensity means "use the daemon's `Intensity` property".
