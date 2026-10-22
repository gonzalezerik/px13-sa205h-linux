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
interface is what "PX13 SA205H Linux" means here:

1. **A documented, versioned D-Bus interface** with a capability schema, written as a
   spec, so desktops and apps can use any pen this project supports without knowing its protocol.
2. **A backend per pen protocol** that maps the pen onto that interface. MPP 2.6 first;
   MPP 2.0 (pairing, top button, battery, no haptics) is mostly a subset of it; USI
   (settings stored in the pen, sent as HID feature reports) after that. Passive EMR
   pens and Apple-only pens are out of scope: there is nothing for this project to add to them.
3. **Later, app-triggered haptics.** Letting an app or compositor make the pen buzz
   needs an xdg-desktop-portal interface or a Wayland protocol next to `tablet-v2`.
   This project would be the reference implementation for a proposal.

## 2026-10-18: capabilities, not pen models

`px13pen-core` describes a pen by what it can do: pairing method, haptics (with its waveform
list), buttons, battery source and pen-side settings. The D-Bus API, the CLI and every
frontend work from those capabilities and never check which protocol or model a pen is.
Some pens switch between protocols (MPP and USI), which a per-model design couldn't
represent. A backend is written only once real hardware and captures exist for it,
recorded under `fixtures/<device>/`. Until then a protocol is listed as planned, never
as supported.

## 2026-10-18: one repository, split at D-Bus

`px13pend` owns the hardware and exports the D-Bus interface. Everything else (`px13penctl`,
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

This project owns pairing, haptics (on/off and intensity, shown only when the pen reports haptic
support), the shortcut button and battery when no standard source exists. *(For the shortcut
button this entry's plan — a uinput key emitter — was superseded on 2026-10-21, below.)*

## 2026-10-18: frontends

- **CLI:** `px13penctl status --json` and a streaming `px13penctl watch --json`. Its output is
  empty while no pen is connected, so a Waybar `custom` module hides itself, and other
  bars (Noctalia, quickshell, eww, ironbar) can use it from scripts. Example configs go
  in `contrib/`. No per-bar plugins.
- **Tray icon:** one StatusNotifierItem with dbusmenu (first built on `ksni`; since
  2026-10-22 served directly on zbus 6, because `ksni` still pins zbus 5). It covers KDE, XFCE,
  Cinnamon, MATE, LXQt, Budgie, GNOME with the AppIndicator extension, and the trays of
  Waybar, swaybar, Noctalia, ironbar, HyprPanel, eww and quickshell. SNI gives a menu,
  not a custom popup: battery, link state, haptics toggle, intensity
  (Low/Medium/High), Pair a pen…, Settings….
- **Settings window:** one GTK4 + libadwaita app (`px13pen-settings`), chosen for its
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
  (Superseded 2026-10-21: see "the pen says when it goes and comes" below.)
- **Disconnect:** the icon goes away after a grace period, not immediately, because BLE
  pens drop the link when idle. The period is set once idle-disconnect timing has been
  measured on hardware.
- `px13pen-settings` always has an application-menu entry, so it's reachable before the
  first pairing, while troubleshooting, and on desktops without a tray.
- Automatic pairing can be switched off (shared machines, kiosks).
- The tray icon has a title and tooltip with the pen name and battery level, and
  battery states differ by icon shape, not only colour.

Open: whether `px13pend` runs per user or system-wide. A per-user service can reach BlueZ,
send notifications and keep settings per user, with hidraw access from a udev `uaccess`
rule. A system service only pays off if pairing or haptics must work before anyone
logs in.

## 2026-10-18: musl is the headless half

The musl target links C libraries statically, and Alpine ships no static GTK
stack — so `px13pen-settings` cannot link there, and static linking is exactly what
the musl binaries are for. It is a desktop window, not one of the headless
binaries the musl image distributes, so the Alpine stages build, lint and test
everything except it; `cargo fmt` and `cargo clippy` (which never links) still
cover its sources. The settings window ships through the Nix package and runs
on any glibc desktop. Every other crate stays equal across all four
combinations.

## 2026-10-21: the shortcut button emits events and commands, not keys

The pen classifies its own top-button gestures — single, double, long — and sends each as a
Win+F20/F19/F18 key press over its Bluetooth keyboard collection, which the compositor reads
as ordinary keys. The earlier plan (the 2026-10-18 entry above) had `px13pend` re-emit the gestures as
configurable keys through uinput. The captures showed that was solving the wrong problem: the
keys already reach the compositor; what no host could see was the *gesture* — only the pen's
report says whether a press was single, double or long.

So `px13pend` reads a copy of the pen's reports from its hidraw node — hidraw delivers to
every reader, so the compositor's exclusive evdev grab stays untouched — and makes the
gesture usable where keys cannot be: a `ButtonEvent` signal on the pen (docs/dbus.md §5) and
one user-stored shell command per gesture (`Buttons1`, §8 there) that the daemon runs. No uinput,
no grab, no race with the compositor at session start — the grab-first design loses to every
compositor that grabs on startup and stays off the table unless a compositor-side interface
appears. An action and a compositor-side key binding for the same gesture fire twice; the
settings window says so, because the pen's key press is the pen's, not ours to hide.
(Superseded on the last point by the 2026-10-22 entry: the key press is now hidden.)

## 2026-10-21: the tray's open numbers get set

`px13pen-tray` lands (docs/tray.md), which settles what 2026-10-18 left
open:

- **Grace period: 60 s.** The measured idle drop is 6–15 s after the pen is
  set down (findings/010 §4), so hiding the moment a link drops would
  blink the icon on every short pause; a minute covers how the pen is
  actually picked up and put back down.
- **A pen asleep on its charger hides exactly like one asleep on the
  desk.** The icon means "a pen is live right now"; a docked pen has no
  link to report, so there is no separate charging state. Its menu and
  tooltip rows still say "On charger" whenever another pen keeps the icon
  up.
- **Intensity presets: Low 25 %, Medium 50 %, High 100 %.** Three named
  values in the menu, each written as-is to `Haptics1.Intensity`; the
  settings window keeps the free 0–100 % slider.
- Hidden means `StatusNotifierItem` `Status=Passive`, and the
  accessibility gate (`crates/px13pen-tray/tests/a11y.rs`) landed before
  the tray: labels in words, battery by icon shape, state as check mark
  or radio dot — never by colour.

## 2026-10-21: the pen says when it goes and comes

Supersedes "reconnects are silent" above: a pen now says its state out
loud, in three notes the daemon sends itself — it already owns
notifications, and it is the one that sees the link, the gauge and the
charger report (the tray sends none of its own).

- **The three notes**, titled with the pen's advertised name:
  `Idle · 84 %` when the link drops off the desk, `Docked · Charging
  84 %` when it falls asleep on its charger, and a plain `84 %` when it
  is picked up again — the battery and nothing else, a reconnect said
  in the number. Percentages are the gauge's own reading rounded, or
  `Battery1`'s when no gauge has spoken.
- **One note per drop, decided before it is sent.** A set-down pen
  drops its link within seconds and a charger report — if there is
  one — lands within three seconds of the sleep log, in either order
  around the drop; so the drop waits out that window and is then named
  once. Never an `Idle` a late report then corrects to `Docked`, and a
  pen back inside the window was only picked up: one flap, at most one
  note. (`crates/px13pend/src/status.rs` is that decision machine, on
  an injected clock.)
- **A wake-up waits briefly for a fresh reading** (superseded
  2026-10-22, below: the note now shows at once and is corrected in
  place) — gauge or
  `Battery1` — rather than repeating the stale number from before the
  sleep, then falls back to the newest value it has. A pen nobody ever
  read says nothing: a note with no number in it is not a sentence.
  First sightings and first pairings also stay quiet — pairing has its
  own note, and a daemon must not announce the pen it woke up next to.
- **One note on screen, not a pile:** each status note passes the
  previous one's id as `replaces_id`, arrives with low urgency, a
  transient hint and a five-second expiry. Pairing and failure notes
  keep their own behaviour, and the switch silences neither of them.
- **Silence is the user's call:** `Notifications1.Enabled`
  (`px13pen/status.conf`), default on, a switch on every settings card
  (docs/dbus.md §9, docs/settings.md).

## 2026-10-21: the hover tick is opt-in, the effect defaults stay honest

The captures show Windows buzzing a few frames after hover strokes end
(findings/007 §4) — the frame correlation is solid, the causing event
is inferred, and the Bluetooth logger truncated every payload to the
handle, so what Windows actually sent is unknown. Two consequences:

- **`Haptics1.HoverTick`, default off** (`px13pen/haptics.conf`, a
  settings row after *While interacting*): the daemon plays one
  `hover` tick as the pen lifts out of the digitizer's range, only
  while the user has asked for it (and *While interacting* is on).
  An inferred behaviour earns a switch, not a default.
- **One honest defaults row** (docs/protocol/haptics.md §2.9): every
  felt direct send was `repeat 1, no cutoff, no retrigger`, and the
  pen's power-on state reads the same — `Effect::of` uses exactly
  that for every effect, ink arming and ticks alike. Windows' own
  per-effect values are *labelled unknown* rather than guessed: an
  unlabeled invention is worse than a row with a gap in it.

## 2026-10-21: a pen's identity is its transducer serial

Both ends repeat a pen's 32-bit transducer serial: the digitizer's
input reports 7 and 34 name the serial of the pen in range over the
screen — byte-for-byte verified from the Linux capture (findings/006
§3) — and the pen answers its Bluetooth feature report `0x40` with the
same number (findings/006 §2). The Bluetooth address is not identity:
a dual-boot pen bonds a fresh one per host and goes on announcing the
identity the other host bonded, so an address-keyed daemon chases a
pen it already has (the failed-retry loop of 2026-10-18). So:

- `px13pend` reads feature `0x40` once per connected pen — a plain GATT
  read of the identity Report characteristic (see below) — and
  remembers the serial as long as the pen is known. An announcement
  whose serial matches a pen already paired here pairs nothing, even
  while that pen's link is down: asleep is not gone.
- Announcements identity cannot place behave exactly as before: quiet
  while any pen holds a live link, seamless pairing otherwise. A
  second, genuinely different pen therefore changes nothing yet;
  multiple pens stay a future item with their own per-pen settings.
- Reading the feature is not a HID feature get. The pen exports no
  Report Reference descriptors, so BlueZ's HID profile has no way to
  map one and `HIDIOCGFEATURE` is rejected; `px13pend` instead reads
  the characteristic directly, as BlueZ exports the identity report as
  a readable HID Report characteristic (0x2A4D) with the feature's
  bytes and nothing to say which it is. It reads them all and keeps the
  one answer whose vendor matches — on the live pen that is a single
  ten-byte read among six Report characteristics, the rest haptics
  state and padding. A pen that answers nothing leaves the daemon in
  the old conservative behaviour; nothing else depends on the read.

## 2026-10-22: the pen's Bluetooth keyboard and mouse stay off the desktop

Linux pairs the pen with passkey 0 (docs/how-the-pen-connects.md), so someone who records
the pairing moment can work out the link key and later pose as the pen. What a fake pen
could do is whatever the pen's Bluetooth input devices can do: its keyboard collection
can send any key, not only Win+F18/19/20, and its mouse collection can move and click.
That is the one serious consequence of the passkey-0 gap, and it is cheap to remove.

The shipped udev rule sets `LIBINPUT_IGNORE_DEVICE` on every input device under the pen's
Bluetooth HID device, so libinput-based compositors never see them. Nothing the desktop
needs rides those devices: ink, pressure, tilt, eraser and the barrel button come over the
digitizer, the battery is a power supply, and `px13pend` reads the top button from hidraw
and acts only on its three gesture codes (single, double, long) — anything else on that
report is ignored. A spoofed pen can therefore fake at most a shortcut gesture, which runs
only the commands the user configured, never typed input.

This replaces the 2026-10-21 position that the pen's key press is not ours to hide: with
it hidden, `Buttons1` actions are the button's only effect and nothing fires twice.
Compositor-side bindings for Win+F18/19/20 stop working. Tools that read evdev nodes
directly instead of through libinput (key remappers such as keyd, or anything with a
catch-all device match) still see the devices and must exclude the pen themselves
(vendor `330a`, product `0001`); docs/packaging.md says how. This limits the damage of
the gap; closing it still needs LE legacy out-of-band pairing in the kernel
(docs/future/kernel-le-legacy-oob.md).

## 2026-10-22: the wake note shows at once

Waiting up to three seconds for a fresh battery reading made the pickup note arrive late
enough to feel broken. It now shows the newest known percent the moment the link is up;
a fresh reading inside the next three seconds that differs updates the same note in place
(it already replaces by id), so the stale-number concern is handled without the wait. A
pick-up before the drop was named (no Idle or Docked shown yet) sends nothing, since there
is nothing to take back. A pen never read still says nothing until its first reading.
