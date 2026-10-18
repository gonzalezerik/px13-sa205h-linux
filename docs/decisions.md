# Decisions

Short records of choices that shape the project. Newest last.

## 2026-10-17: AGPL-3.0-or-later

Same license as the maintainer's other hardware projects. Kernel patches drafted for
upstream are GPL-2.0, as the kernel requires.

## 2026-10-17: glibc and musl are equal targets, on x86_64 and aarch64

Linux pens ship on laptops running every kind of distribution, including musl ones
(Alpine, postmarketOS, Void musl, Chimera). We support both C libraries on both
architectures from the first commit rather than retrofitting musl later. CI builds,
lints and tests all four combinations natively (GitHub's x86_64 and aarch64 runners,
no emulation), using the same container stages for each.

- **glibc** builds in Debian trixie (`container/Containerfile.glibc`) and through the
  Nix flake.
- **musl** builds in Alpine (`container/Containerfile.musl`). The flake doesn't
  offer a musl package because nixpkgs' binary cache has no musl Rust toolchain;
  a Nix musl build would compile rustc from source first.

## 2026-10-17: podman for containers

Containers are built and run with podman (rootless, daemonless). The Containerfiles
are plain OCI and also build with Docker or Buildah.

## 2026-10-17: userspace first

The daemon talks to the digitizer through hidraw and to the pen through BlueZ, so it
works on stock kernels. Anything that belongs in the kernel is drafted as an
upstream patch series; the daemon must keep working without it.

## 2026-10-18: scope is the layer above the pen protocol

A pen protocol (MPP, USI, Wacom AES) is how a pen and a digitizer signal each other
through the screen. It is fixed in the pen's and the digitizer's firmware, and the
digitizer already turns ink into standard HID reports that the kernel handles. We don't
define a new pen protocol. That would need new pens and new digitizers, and USI already
exists for exactly that purpose.

What Linux lacks is a common interface for everything outside that channel: pairing,
the Bluetooth link, haptics, buttons, battery and settings stored in the pen. That
interface is what "Open Pen Protocol" means here:

1. **A documented, versioned D-Bus interface** with a capability schema, written as a
   spec, so desktops and apps can use any pen OPP supports without knowing its protocol.
2. **A backend per pen protocol** that maps the pen onto that interface. MPP 2.6 first;
   MPP 2.0 (pairing, top button, battery, no haptics) is mostly a subset of it; USI
   (settings stored in the pen, sent as HID feature reports) after that. Passive EMR
   pens and Apple-only pens are out of scope: there is nothing for OPP to add to them.
3. **Later, app-triggered haptics.** Letting an app or compositor make the pen buzz
   needs an xdg-desktop-portal interface or a Wayland protocol next to `tablet-v2`.
   OPP would be the reference implementation for a proposal.

## 2026-10-18: capabilities, not pen models

`opp-core` describes a pen by what it can do: pairing method, haptics (with its waveform
list), buttons, battery source and pen-side settings. The D-Bus API, the CLI and every
frontend work from those capabilities and never check which protocol or model a pen is.
Some pens switch between protocols (MPP and USI), which a per-model design couldn't
represent. A backend is written only once real hardware and captures exist for it,
recorded under `fixtures/<device>/`. Until then a protocol is listed as planned, never
as supported.

## 2026-10-18: one repository, split at D-Bus

`oppd` owns the hardware and exports the D-Bus interface. Everything else (`oppctl`,
the tray icon, the settings app, bar examples, desktop widgets) is a thin client of
that interface, kept in this repository while the interface is still changing.
Distributions can still package each crate separately. A frontend moves to its own
repository only if someone else takes over maintaining it.

## 2026-10-18: settings that Linux already owns stay there

Many Windows pen settings belong to libinput or the compositor on Linux, and we don't
duplicate them:

| Windows setting | On Linux |
|---|---|
| Handedness, pen as mouse, barrel button as right-click, top end erases | libinput tablet configuration, exposed by GNOME Settings, KDE's Drawing Tablet page and WM config files |
| Ignore touch while using the pen | libinput touch arbitration, already automatic |
| Show cursor, visual effects, press-and-hold, double-tap timing | compositor and toolkit behaviour, marked not applicable |
| Battery | BlueZ → UPower when the pen uses the standard BLE battery service |

GNOME's tablet panel only lists devices that libwacom knows about. libwacom 2.20 doesn't
know the ELAN `04F3:4631` digitizer, so we'll propose a data file upstream to libwacom.

OPP owns pairing, haptics (on/off and intensity, shown only when the pen reports haptic
support), the shortcut button and battery when no standard source exists. The shortcut
button is emitted as a configurable key through uinput, so users bind it in their own
desktop or WM shortcut settings.

## 2026-10-18: frontends

- **CLI:** `oppctl status --json` and a streaming `oppctl watch --json`. Its output is
  empty while no pen is connected, so a Waybar `custom` module hides itself, and other
  bars (Noctalia, quickshell, eww, ironbar) can use it from scripts. Example configs go
  in `contrib/`. No per-bar plugins.
- **Tray icon:** one StatusNotifierItem with dbusmenu (`ksni`). It covers KDE, XFCE,
  Cinnamon, MATE, LXQt, Budgie, GNOME with the AppIndicator extension, and the trays of
  Waybar, swaybar, Noctalia, ironbar, HyprPanel, eww and quickshell. SNI gives a menu,
  not a custom popup: battery, link state, haptics toggle, intensity
  (Low/Medium/High), Pair a pen…, Settings….
- **Settings window:** one GTK4 + libadwaita app (`opp-settings`), chosen for its
  native AT-SPI accessibility and a card layout that matches the Windows page. Its
  accessibility test lands before the window does. The control inventory to match is
  the Windows Pen & Windows Ink page; anything Windows-only gets an explicit "not
  applicable" decision.
- **Desktop widgets (optional, later):** a GNOME Shell Quick Settings tile and a KDE
  Plasma widget, the only way to get a popup with sliders under the panel. Both are
  thin clients of the same D-Bus interface.

## 2026-10-18: the pen appears when it's there

A small session agent, started on login, stays invisible until a pen connects.

- **First pairing:** tapping the pen on the screen pairs it, and a desktop notification
  (`org.freedesktop.Notifications`) says so, with a "Pen settings" action. A failed
  pairing also notifies, so a tap never silently does nothing.
- **Reconnects** are silent: the tray icon and the bar module just come back.
- **Disconnect:** the icon goes away after a grace period, not immediately, because BLE
  pens drop the link when idle. The period is set once idle-disconnect timing has been
  measured on hardware.
- `opp-settings` always has an application-menu entry, so it's reachable before the
  first pairing, while troubleshooting, and on desktops without a tray.
- Automatic pairing can be switched off (shared machines, kiosks).
- The tray icon has a title and tooltip with the pen name and battery level, and
  battery states differ by icon shape, not only colour.

Open: whether `oppd` runs per user or system-wide. A per-user service can reach BlueZ,
send notifications and keep settings per user, with hidraw access from a udev `uaccess`
rule. A system service only pays off if pairing or haptics must work before anyone
logs in.

## 2026-10-18: musl is the headless half

The musl target links C libraries statically, and Alpine ships no static GTK
stack — so `opp-settings` cannot link there, and static linking is exactly what
the musl binaries are for. It is a desktop window, not one of the headless
binaries the musl image distributes, so the Alpine stages build, lint and test
everything except it; `cargo fmt` and `cargo clippy` (which never links) still
cover its sources. The settings window ships through the Nix package and runs
on any glibc desktop. Every other crate stays equal across all four
combinations.
