# Packaging

Installing this project should be the entire setup: no BlueZ configuration flags, no
hand-written udev rules, no units for the user to enable. Everything a working system needs ships in the
package and is picked up by the distro machinery that is already running.

## Install tree

`make install` (or `cargo xtask install`) follows the usual packager conventions: `PREFIX`
(default `/usr/local`) and `DESTDIR` for staging are honoured, and every directory can be
overridden individually for distro layouts.

| Destination (under `DESTDIR` + `PREFIX` unless noted) | File |
|---|---|
| `bin/` | `px13pend`, `px13penctl`, `px13pen-settings`, `px13pen-tray` |
| `libexec/` | `px13pen-pair-helper` (privileged, not a command) |
| `lib/udev/rules.d/` | `70-px13-sa205h-linux.rules` (generated, see below) |
| `share/dbus-1/system-services/` | `io.github.gonzalezerik.Px13Pen.PairHelper1.service` (system activation) |
| `share/dbus-1/system.d/` | `io.github.gonzalezerik.Px13Pen.PairHelper1.conf` (bus policy) |
| `share/dbus-1/services/` | `io.github.gonzalezerik.Px13Pen.Daemon1.service` (session activation) |
| `share/polkit-1/actions/` | `io.github.gonzalezerik.Px13Pen.policy` |
| `lib/systemd/system/` | `px13pen-pair-helper.service` (D-Bus activated, no `[Install]`) |
| `lib/systemd/user/` | `px13pend.service`, `px13pen-tray.service` (start with the graphical session) |
| `lib/systemd/user-preset/` | `80-px13-sa205h-linux.preset` (`enable` both) |
| `share/applications/`, `share/icons/` | settings window entry and icon |

Overridable variables: `BINDIR`, `LIBEXECDIR`, `UDEVRULESDIR`, `DBUS_SYSTEM_SERVICES_DIR`,
`DBUS_SYSTEM_POLICY_DIR`, `DBUS_SESSION_SERVICES_DIR`, `POLKIT_ACTION_DIR`,
`SYSTEMD_SYSTEM_UNIT_DIR`, `SYSTEMD_USER_UNIT_DIR`, `SYSTEMD_USER_PRESET_DIR`. Files that reference the install prefix
carry `@PREFIX@`, substituted at install time (the flake substitutes its own store path).

After installing on a running system, tell udev about the new rule and apply the user
preset once:

```sh
udevadm control --reload-rules && udevadm trigger
systemctl --global preset px13pend.service px13pen-tray.service
```

Distribution packaging does both from its usual install hooks, so users run neither. From
then on `px13pend` and the tray icon start with every graphical session: the touch-to-pair
listener has to be
up before a new pen first comes near the screen, which is why it does not wait for a D-Bus
call (session activation still starts it on demand where the preset was not applied). The
pairing helper is activated by the system bus when first called (`Type=dbus`) and needs
nothing.

D-Bus only reads activation files named after the bus name they declare, plus `.service`;
`cargo test -p xtask` checks both files against that rule.

## The udev rule is generated

`contrib/udev/70-px13-sa205h-linux.rules` is produced by

```sh
cargo xtask generate-udev-rules      # rewrite it
cargo xtask generate-udev-rules --check   # what CI runs; fails on drift
```

from the supported-device tables in `px13pen-core`, so a new device lands in the rule at the same
commit that adds support for it. The rule only adds `TAG+="uaccess"` for the pen-related
`hidraw` nodes. The leading digits of each `hidraw` parent name are the **HID bus type**, not a
boot-volatile number: `0018` is HID over I2C (the built-in digitizer), `0005` is HID over
Bluetooth (the pen). Those ids are part of the kernel's HID core and are the same on every
machine.

## Pairing and privilege

Touch-to-pair needs no privileged step. The user's own `px13pend` registers the host with the
digitizer, sends the key report, and pairs the pen's advertisement through ordinary BlueZ calls
(`StartDiscovery`, `Device1.Pair`) with its own KeyboardOnly agent answering passkey 0, then
marks the pen `Trusted` (docs/protocol/seamless-pairing.md). It uses nothing marked
`[experimental]` in BlueZ, so `main.conf` stays untouched.

The package still ships a small privileged pairing helper, no longer used for touch-to-pair. It
is kept for one future job: once the kernel supports LE legacy out-of-band keys, adding the
key as remote out-of-band data over the management socket
(docs/future/kernel-le-legacy-oob.md). It was written for an earlier pairing method (Just Works
to the announced address over `MGMT_OP_PAIR_DEVICE`), which this pen refuses. The helper:

- checks the polkit action `io.github.gonzalezerik.Px13Pen.pair-pen` against the *calling connection*
  before doing anything (active local sessions: yes; anything else: no);
- accepts only Bluetooth random static addresses — the kind the digitizer announces — and
  has no other function;
- runs as root but with `CapabilityBoundingSet=CAP_NET_ADMIN` and the usual unit sandboxing,
  and cannot be reached except through the bus policy in `data/dbus/system.d/`.

Nothing calls it during touch-to-pair, so no process is privileged at any point of pairing.

## NixOS

The flake's module is the zero-config path there:

```nix
{
  imports = [ inputs.px13-sa205h-linux.nixosModules.default ];
  services.px13-sa205h-linux.enable = true;
}
```

It puts the package in the system profile and registers the same files the tarball ships:
`services.udev.packages`, `services.dbus.packages` (activation + policy), `systemd.packages`
(units) with `px13pend` and `px13pen-tray` wanted by `graphical-session.target`, and the
polkit action via the standard data dirs.

A rule takes effect only when an event runs it, and a switch sends none — the built-in
digitizer's node existed long before the rule did. So the module also installs a oneshot
`px13-sa205h-linux-udev-trigger.service`, which runs
`udevadm trigger --subsystem-match=hidraw --action=change` and re-runs on every switch where
the assembled `/etc/udev/rules.d` changes. Enabling the module grants access to a device that
is already there: no replug, and no hand-typed `udevadm trigger`.

The overlay (`overlays.default`) exposes `pkgs.px13-sa205h-linux` for systems that want it in the package
set directly; `nixosModules.px13-sa205h-linux` is the same module for those systems.

## Distribution packages

The layout above is the contract: a `.deb`/`.rpm` post-install scriptlet needs
`udevadm control --reload-rules && udevadm trigger`, a polkit/DBus reload
(`systemctl reload polkit dbus`, or they re-read on next activation) and the user preset
(`%systemd_user_post px13pend.service px13pen-tray.service` on Fedora, `dh_installsystemduser`
on Debian — both apply `80-px13-sa205h-linux.preset`). `px13pend` and the tray then start at
each user's next login; nothing is enabled system-wide.

**Flatpak/Snap**: `px13pend` cannot live in either — it needs the session D-Bus, the system
BlueZ service and `/dev/hidraw` device access under the conventions above, which the
sandboxing models are not built for. `px13pen-settings` is a candidate for a future Flatpak since
it only talks to `io.github.gonzalezerik.Px13Pen.Daemon1` on the session bus. On immutable distros the
NixOS module (Bazzite/NixOS) or an rpm-ostree layer of the package is the way; installing
into `$HOME` alone cannot register the udev rule.
