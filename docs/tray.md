# `px13pen-tray` — the tray icon

One StatusNotifierItem with a dbusmenu (`ksni`), a thin client of
`px13pend` like `px13penctl` (docs/dbus.md): no Bluetooth and no HID
from this side. It covers KDE, XFCE, Cinnamon, MATE, LXQt, Budgie,
GNOME with the AppIndicator extension, and the SNI trays of Waybar,
swaybar, Noctalia, ironbar, HyprPanel, eww and quickshell
(docs/decisions.md).

```console
$ px13pen-tray                # session-bus px13pend
```

The packaging starts it with every graphical session (a systemd user
preset, docs/packaging.md). A desktop without a status-notifier host is
not an error: the process waits for one to appear instead of failing.

## When the icon appears

While some pen holds a link. These pens fall off Bluetooth seconds
after being set down and are back on the next touch (findings/010 §4),
so the icon does not vanish the moment a link drops: it waits a 60 s
grace period, and a pen coming back inside that window cancels the
hide. A pen asleep on its charger is hidden exactly like one asleep on
the desk — the icon means "a pen is live right now", not "a pen
exists". While hidden the item reports `Status=Passive`, so hosts move
it to their hidden drawer instead of painting a dead icon.

## The menu

- One read-only block per pen: name, battery, link state
  (`Connected`, `On charger`, `Link down`). Where the daemon has read
  the pen's fuel gauge (`battery-detail`) the battery is the gauge's
  own reading to two decimals, voltage included.
- **Haptics** — a check item for `Haptics1.Enabled` — and an
  **Intensity** submenu of radio items, Low (25 %), Medium (50 %) and
  High (100 %), marked at the preset closest to the pen's current
  value. The block appears only for a pen whose daemon exports
  `Haptics1`: a pen that cannot buzz grows no switch that would do
  nothing.
- **Pair a pen…** — asks `px13pend` to start touch-to-pair. The
  daemon's own notifications report how pairing went (docs/dbus.md);
  the tray adds none of its own, so a pairing never double-notifies.
- **Settings…** — launches `px13pen-settings`.

A click never blocks the menu on Bluetooth: the write goes out on its
own thread, and the daemon echoes the new value back as a pen change,
which is where the menu re-reads it from.

## Accessibility

The rule (docs/decisions.md): every fact in words as well as in shape,
state never carried by colour alone. The title is `name · battery`, so
a host that shows only the title still says both; the tooltip adds the
link state and the gauge's millivolts. The icon differs by *shape*
across charge levels — `battery-full-symbolic` through
`battery-caution-symbolic` — and is `input-tablet` before there is any
reading to show, never a full battery lying about it. Menu state is a
check mark or a radio dot; the read-only facts are disabled rows, so
nothing invites a click on information. The gate for all of this,
`crates/px13pen-tray/tests/a11y.rs`, asserts against the menu
structures themselves and landed before the tray did.
