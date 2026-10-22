# `px13pen-settings` — the pen settings window

A GTK 4 / libadwaita window over the same D-Bus interface everything
else uses (docs/dbus.md): one `px13pend` connection, live pen cards, no
Bluetooth and no HID from this side. It replaces the pen panel of the
Windows driver for the controls that have a Linux meaning (the matrix
lives in docs/parity.md).

```console
$ px13pen-settings            # session-bus px13pend
$ px13pen-settings --system   # px13pend serve --system
```

## What it shows

One card per connected pen: name, address, link state, battery, and —
once the daemon has read it from the connected pen — the pen's own
firmware (`Pen1.FirmwareRevision`). A
card only grows haptics controls when the daemon exported a `Haptics1`
interface for that pen — a pen that cannot do haptics never shows a
switch that would do nothing. The haptics rows are the master switch
(`Haptics1.Enabled`), an intensity slider (`Haptics1.Intensity`,
0–100 %), a **While inking** switch (`Haptics1.WhileInking` — the tip's
contact arms a texture while it touches the screen), a **While
interacting** switch (`Haptics1.WhileInteracting` — short ticks for
clicks, presses and hovers) and an **Ink texture** picker
(`Haptics1.InkTexture` — the continuous waveforms the pen reports,
minus the eraser, which the daemon arms by itself when the pen is
inverted). A **Hover-out tick** switch (`Haptics1.HoverTick`) follows
*While interacting*: one `hover` tick as the pen lifts out of the
screen's hover range. It is **off by default** — Windows is only
*seen* to buzz at hover-stroke ends (findings/007 §4), and the
captured packets stopped at the handle, so what Windows actually
sends there is unknown and this daemon will not invent it silently.
It also needs *While interacting* on: it is an interaction tick.
Controls the current policy cannot use are greyed out:
everything but the master switch while haptics are off, and the
texture picker while *While inking* is off. Every card ends with the
**Status notifications** switch (`Notifications1.Enabled`): whether
the daemon says out loud when the pen goes idle, docks or wakes —
on for every card, haptics or not.

A pen with the top shortcut button (`Buttons1` on its object) also
gets a **Shortcut button** card: one entry per gesture — *Single
press*, *Double press*, *Long press* — holding the shell command that
gesture runs, empty meaning do nothing. *Double press* starts out as
`rnote` — a pen wants a note app the moment it pairs — and clearing
the entry is how you take that back. Typing reveals a check button
in the entry; pressing it or pressing Enter sends the command. The card
says what the entries share: these commands are all the button does,
because the pen's own Win+F18/19/20 key press is kept away from the
desktop (docs/decisions.md, 2026-10-22).

Pairing needs no button: `px13pend` listens from the start and pairs a new
pen as soon as it comes near the screen. `Search for pens` runs
`StartPairing()`, which also retries pens whose pairing failed; pairing
progress arrives as `PairingEvent` toasts (announced → bonded, or failed
with the daemon's reason). Pressing it again runs `StopPairing()`.

When `px13pend` goes away the window stays open and says so: it is easier
to restart a daemon than to recover a dead settings window.

## Where the settings live

The haptics policy belongs to the daemon, not to this window: the
switches and the slider write `Haptics1` properties, and `px13pend`
persists them to `px13pen/haptics.conf` under `$XDG_CONFIG_HOME`
(`~/.config` when unset). One file per user, shared by every pen —
the same shape as the Windows registry values it mirrors.

```ini
# px13pen haptics policy — written by px13pend; see docs/settings.md.
enabled=true
intensity=100
while_inking=true
while_interacting=true
hover_tick=false
ink_texture=ink
```

The daemon rewrites the whole file on every change and re-reads it at
startup; unknown keys and out-of-range values fall back to the
defaults, so hand-editing is safe but pointless while a daemon is
running — the next change overwrites the edit. Deleting the file
resets everything to the defaults (all on at full strength, except
the opt-in `hover_tick`, which starts off) after a daemon restart.

The shortcut button actions live beside it, in
`px13pen/buttons.conf`: one shell command per top-button gesture,
empty meaning do nothing.

```ini
# px13pen shortcut button actions — written by px13pend; see docs/settings.md.
# One shell command per gesture; empty means do nothing.
# The pen's own Win+F18/19/20 keys never reach the desktop.
single=notify-send "pen button"
double=
long=
```

Same rules: `Buttons1` writes it, the daemon re-reads it at startup,
and unknown keys are ignored. A command runs as `sh -c <command>`
from the daemon's session, detached; its exit status is not reported
back.

The status notification switch lives beside them, in
`px13pen/status.conf`:

```ini
# px13pen status notifications — written by px13pend; see docs/settings.md.
enabled=true
```

Same rules again: `Notifications1.Enabled` writes it and the daemon
re-reads it at startup. `enabled=false` silences the pen-status notes
— `Idle · 84 %` when the pen falls asleep off the charger,
`Docked · Charging 84 %` when it falls asleep on it, and the plain
battery percentage when it wakes (docs/dbus.md §9). The notes are
low-urgency, replace each other and expire on their own; silencing
them quiets the pen's comings and goings only — pairing still
notifies.

## Menu and desktop entry

The corner menu holds **About PX13 Pen** — version, licence, upstream
and issue links, the version read from the package metadata so it
cannot drift. The package installs the desktop entry
(`io.github.gonzalezerik.Px13Pen.Settings`) and its icon, so the window shows up
in normal launcher searches.

## Threading and write coalescing

The widget thread owns only widgets. One background thread owns the
bus: it follows the daemon and applies commands the window sends. The
two sides talk through two channels, and the widget thread drains the
event queue on a 120 ms timer.

That timer also flushes the slider. A drag changes the value tens of
times per second; only the last value at each tick goes to the bus, and
only when the pen's intensity differs from it. A full drag is one or
two Bluetooth attribute writes, not fifty. Echoes of our own writes are
swallowed by a guard flag while a widget is being set from a daemon
event.

## Accessibility

The target is WCAG 2.2 AA, and the static part is enforced before the
UI can build: `tools/a11y/check.sh` runs as part of `cargo test` (see
`crates/px13pen-settings/tests/a11y.rs`) and rejects sliders without an
accessible name, icon-only buttons without a text alternative, switch
rows without titles, and untitled windows. The runtime part — real
AT-SPI navigation with Orca — is a manual pass against a running
desktop and is tracked there.
