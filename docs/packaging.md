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
| `bin/` | `oppd`, `oppctl`, `opp-settings` |
| `libexec/` | `opp-pair-helper` (privileged, not a command) |
| `lib/udev/rules.d/` | `70-open-pen-protocol.rules` (generated, see below) |
| `share/dbus-1/system-services/` | `org.openpenprotocol.PairHelper1.service` (system activation) |
| `share/dbus-1/system.d/` | `org.openpenprotocol.PairHelper1.conf` (bus policy) |
| `share/dbus-1/services/` | `org.openpenprotocol.Daemon1.service` (session activation) |
| `share/polkit-1/actions/` | `org.openpenprotocol.policy` |
| `lib/systemd/system/` | `opp-pair-helper.service` (D-Bus activated, no `[Install]`) |
| `lib/systemd/user/` | `oppd.service` (starts with the graphical session) |
| `lib/systemd/user-preset/` | `80-open-pen-protocol.preset` (`enable oppd.service`) |
| `share/applications/`, `share/icons/` | settings window entry and icon |

Overridable variables: `BINDIR`, `LIBEXECDIR`, `UDEVRULESDIR`, `DBUS_SYSTEM_SERVICES_DIR`,
`DBUS_SYSTEM_POLICY_DIR`, `DBUS_SESSION_SERVICES_DIR`, `POLKIT_ACTION_DIR`,
`SYSTEMD_SYSTEM_UNIT_DIR`, `SYSTEMD_USER_UNIT_DIR`, `SYSTEMD_USER_PRESET_DIR`. Files that reference the install prefix
carry `@PREFIX@`, substituted at install time (the flake substitutes its own store path).

After installing on a running system, tell udev about the new rule and apply the user
preset once:

```sh
udevadm control --reload-rules && udevadm trigger
systemctl --global preset oppd.service
```

Distribution packaging does both from its usual install hooks, so users run neither. From
then on `oppd` starts with every graphical session: the touch-to-pair listener has to be
up before a new pen first comes near the screen, which is why it does not wait for a D-Bus
call (session activation still starts it on demand where the preset was not applied). The
pairing helper is activated by the system bus when first called (`Type=dbus`) and needs
nothing.

D-Bus only reads activation files named after the bus name they declare, plus `.service`;
`cargo test -p xtask` checks both files against that rule.

## The udev rule is generated

`contrib/udev/70-open-pen-protocol.rules` is produced by

```sh
cargo xtask generate-udev-rules      # rewrite it
cargo xtask generate-udev-rules --check   # what CI runs; fails on drift
```

from the supported-device tables in `opp-core`, so a new device lands in the rule at the same
commit that adds support for it. The rule only adds `TAG+="uaccess"` for the pen-related
`hidraw` nodes. The leading digits of each `hidraw` parent name are the **HID bus type**, not a
boot-volatile number: `0018` is HID over I2C (the built-in digitizer), `0005` is HID over
Bluetooth (the pen). Those ids are part of the kernel's HID core and are the same on every
machine.

## Pairing and privilege

> The pairing method described in this section is being replaced. Touch-to-pair on this pen
> works without any privileged step: register the host with the digitizer, send the key,
> pair the pen's advertisement through BlueZ with passkey entry
> (docs/protocol/seamless-pairing.md). Once the kernel supports LE legacy out-of-band keys,
> the helper's remaining job is adding the key as remote out-of-band data
> (docs/future/kernel-le-legacy-oob.md).

Pairing normally goes through BlueZ `Adapter1.ConnectDevice`. That method is
`[experimental]` in current BlueZ releases, and enabling it would mean asking users to edit
`main.conf` — not acceptable. Instead the daemon detects the missing method and calls
`org.openpenprotocol.PairHelper1`, a D-Bus-activated helper that asks the kernel directly
over the Bluetooth management socket (`MGMT_OP_PAIR_DEVICE`, Just Works). The helper:

- checks the polkit action `org.openpenprotocol.pair-pen` against the *calling connection*
  before doing anything (active local sessions: yes; anything else: no);
- accepts only Bluetooth random static addresses — the kind the digitizer announces — and
  has no other function;
- runs as root but with `CapabilityBoundingSet=CAP_NET_ADMIN` and the usual unit sandboxing,
  and cannot be reached except through the bus policy in `data/dbus/system.d/`.

After a kernel-initiated bond the user's own `oppd` marks the device `Trusted`; no process
other than the pairing moment is privileged at all.

## NixOS

The flake's module is the zero-config path there:

```nix
{
  imports = [ inputs.open-pen-protocol.nixosModules.default ];
  services.open-pen-protocol.enable = true;
}
```

It puts the package in the system profile and registers the same files the tarball ships:
`services.udev.packages`, `services.dbus.packages` (activation + policy), `systemd.packages`
(units) with `oppd` wanted by `graphical-session.target`, and the polkit action via the
standard data dirs. The overlay
(`overlays.default`) exposes `pkgs.open-pen-protocol` for systems that want it in the package
set directly; `nixosModules.open-pen-protocol` is the same module for those systems.

## Distribution packages

The layout above is the contract: a `.deb`/`.rpm` post-install scriptlet needs
`udevadm control --reload-rules && udevadm trigger`, a polkit/DBus reload
(`systemctl reload polkit dbus`, or they re-read on next activation) and the user preset
(`%systemd_user_post oppd.service` on Fedora, `dh_installsystemduser` on Debian — both apply
`80-open-pen-protocol.preset`). `oppd` then starts at each user's next login; nothing is
enabled system-wide.

**Flatpak/Snap**: `oppd` cannot live in either — it needs the session D-Bus, the system
BlueZ service and `/dev/hidraw` device access under the conventions above, which the
sandboxing models are not built for. `opp-settings` is a candidate for a future Flatpak since
it only talks to `org.openpenprotocol.Daemon1` on the session bus. On immutable distros the
NixOS module (Bazzite/NixOS) or an rpm-ostree layer of the package is the way; installing
into `$HOME` alone cannot register the udev rule.
