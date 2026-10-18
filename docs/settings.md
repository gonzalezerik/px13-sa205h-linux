# `opp-settings` — the pen settings window

A GTK 4 / libadwaita window over the same D-Bus interface everything
else uses (docs/dbus.md): one `oppd` connection, live pen cards, no
Bluetooth and no HID from this side. It replaces the pen panel of the
Windows driver for the controls that have a Linux meaning (the matrix
lives in docs/parity.md).

```console
$ opp-settings            # session-bus oppd
$ opp-settings --system   # oppd serve --system
```

## What it shows

One card per connected pen: name, address, link state and battery. A
card only grows haptics controls when the daemon exported a `Haptics1`
interface for that pen — a pen that cannot do haptics never shows a
switch that would do nothing. The haptics rows are the switch
(`Haptics1.Enabled`) and an intensity slider (`Haptics1.Intensity`,
0–100 %).

`Search for pens` runs `StartPairing()`; pairing progress arrives as
`PairingEvent` toasts (announced → bonded, or failed with the daemon's
reason). Pressing it again runs `StopPairing()`.

When `oppd` goes away the window stays open and says so: it is easier
to restart a daemon than to recover a dead settings window.

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
`crates/opp-settings/tests/a11y.rs`) and rejects sliders without an
accessible name, icon-only buttons without a text alternative, switch
rows without titles, and untitled windows. The runtime part — real
AT-SPI navigation with Orca — is a manual pass against a running
desktop and is tracked there.
