# contrib

Examples for feeding `px13penctl` output (the contract is docs/px13penctl.md)
into bars and shells. There is nothing to install and no per-bar plugin:
every bar can run a command, and px13penctl's empty-when-idle rule means
"no pen connected" is the absence of output, so a module hides itself.

## Waybar

`waybar/pen.sh` renders the current pens as one Waybar JSON module; it
needs `jq`. Copy it somewhere, make it executable, and reference it from
the bar config (`waybar/pen.json` has the snippet):

```json
"custom/pen": {
  "exec": "~/.config/waybar/scripts/pen.sh",
  "return-type": "json",
  "interval": 10,
  "exec-on-event": false
}
```

`interval: 10` re-runs `px13penctl status --json` every ten seconds — plenty
for a battery, slightly stale for a hot plug.

## Event-driven shells

`status` is for pollers; `watch --json` is for anything that wants the
moment things change — a quickshell/eww handler, an `exec` line in a
systemd unit, a fifo next to a script:

```console
$ px13penctl watch --json | jq --unbuffered -r '
    select(.type == "pen.changed") | "\(.pen.name) \(.pen.battery // "?")"
  '
```

## The tray and settings

The tray agent and `px13pen-settings` talk to the D-Bus interface itself
(docs/dbus.md); `px13penctl` exists for everything that just wants lines.
