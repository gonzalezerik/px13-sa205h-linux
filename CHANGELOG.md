# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/).

## Unreleased

- **Turn the status notes off from the tray**: the menu has a *Status
  notifications* check item, the same switch as the settings window's.
- **Inking texture from the first millimetre**: the texture is armed as soon
  as the pen hovers (for whichever end is pointing at the screen) instead
  of when the tip touches. Strokes started from out of range, and short
  taps, used to begin, or entirely happen, without texture while the
  write crossed Bluetooth. Writes also reuse the open pen node instead of
  enumerating every HID device each time.
- **An open tray menu follows the pen**: the menu now announces changes with
  both `LayoutUpdated` and `ItemsPropertiesUpdated`, so hosts that refresh
  an open menu on either one show "Docked", battery and the haptics switch
  as they change instead of after reopening it.
- **The pickup notification shows up right away**: it used to wait up to
  three seconds for a fresh battery reading. It now shows the last known
  percentage the moment the pen reconnects, and updates the same
  notification in place if a fresh reading differs. Setting the pen down and
  picking it straight back up (before "Idle" was shown) no longer sends a
  note at all.
- **The tray's Settings… entry opens the settings window again**: it now
  starts the copy installed beside the tray instead of searching PATH,
  which the service may not have.
- **Shortcut-button commands find your apps on NixOS**: the module pulled
  the units in with `wantedBy`, which also gave `px13pend` NixOS's minimal
  PATH, so a command like `rnote` was never found. The units are now wanted
  through the target and keep the session's PATH. A command that fails now
  leaves its error and exit status in the journal instead of vanishing.
- **Only one daemon per session**: D-Bus activation now goes through
  `px13pend.service`, and a second daemon exits instead of taking over the
  bus name. Two daemons used to answer the same touch, fail each other's
  pairing and show two error notifications.
- **Pairing failures say what to do next** ("The pen didn't answer in time.
  Touch it to the screen again to retry.") instead of showing the raw
  Bluetooth error, which stays in the journal.
- **The pen's Bluetooth keyboard and mouse stay off the desktop**: the udev
  rule now hides them from libinput, so the top button does only what its
  configured commands say and nothing fires twice. Because Linux pairs with
  passkey 0, this also limits what a device posing as a bonded pen could
  do to faking a shortcut gesture. Compositor bindings for Win+F18/19/20
  no longer fire; key remappers that grab every keyboard must exclude
  `330a:0001` (docs/packaging.md).
- **The tray speaks StatusNotifierItem and dbusmenu itself, on zbus 6**:
  `ksni` still depends on zbus 5, which brought back the unmaintained
  `zbus_names`/`zvariant` advisories, and its Unlicense was not on the
  allowed list. Same icon, texts and menu; the menu is now plain data, so
  the accessibility tests read exactly what a host is sent. Fixed on the
  way: the icon never hid after the 60 s grace period once it had been
  shown.
- **A double press of the top button opens RNote out of the box**: the
  `Buttons1.Double` default is now `rnote`, so a pen has somewhere to write the
  second it pairs. Anything stored in `px13pen/buttons.conf` wins in both
  directions: clearing the *Double press* entry in the settings window (or
  writing an empty command over D-Bus) puts the gesture back to doing nothing.
- **Release tarballs, so nobody has to compile**: `Containerfile.glibc` and
  `Containerfile.musl` gained `dist` stages that stage the install tree at
  `/usr/local` and bundle it with `install.sh` (tree + udev rule and bus policy
  into `/etc` + preset + udev reload — it writes only places immutable systems
  keep writable); `cargo xtask install --headless` produces the musl tree
  without the settings window; a tagged release publishes glibc and musl
  tarballs for x86_64 and aarch64 with `SHA256SUMS`; `docs/install.md` walks
  Fedora/RHEL, Debian/Ubuntu, Arch (a `contrib/PKGBUILD` came with it),
  openSUSE, Alpine, Nix and the immutable systems through installing it.
- **The wiki grew its content pages** — reference pages for the digitizer and the pen's
  Bluetooth side (every layout confidence-labelled), five flow pages with sequence
  listings, the Windows host-stack map, capture recipes, device identity, conventions,
  glossary, sources and a kept-list of open questions; plus `docs/wiki/layouts.json`,
  the same layouts as machine-readable data, and `tools/wiki/dissect.py` to decode a
  frame against it.
- **The repository stopped implying support it has never tested**: the README's
  "planned" protocol table (MPP 2.0, USI) is gone, replaced by an explicit one-laptop-
  one-pen scope statement and a disclaimer (no warranty; read-only exploration for
  anyone chasing their own hardware).
- **A path for other hardware**: the wiki's new *Your laptop / pen* guide (read-only
  checks, decide, build-or-report), `tools/capture/sanitize.py` (identifier randomiser
  with hard refusals — raw captures, binaries and base64 never pass through, originals
  are never overwritten), and a Hardware report issue template.

- **Enabling the NixOS module now reaches the devices that are already there.** A rule
  takes effect only when an event runs it, and a system switch sends none, so the
  built-in digitizer's node waited out every switch without its uaccess ACL until a
  replug or a hand-typed `udevadm trigger`. The module installs a oneshot unit that
  re-runs the trigger over present `hidraw` devices whenever the assembled rules change
  (docs/packaging.md).
- **A pen is identified by its transducer serial, not its Bluetooth address.** The
  digitizer names the serial of the pen in its range in input reports 7 and 34, and the
  pen answers its identity report with the same number (findings/006 §2–3), so that is
  what `px13pend` matches on: an announcement carrying the serial of a pen this host
  already paired pairs nothing, asleep or awake. A dual-boot pen waving the identity
  another system bonded no longer drives the daemon into pairing the pen it already has
  (docs/decisions.md). The identity comes from a plain GATT read of the pen's Report
  characteristic: the pen exports no Report Reference descriptors, so a HID feature get
  has nothing to map onto.
- **An opt-in hover-out tick.** `Haptics1.HoverTick` (a settings row, default off) plays
  one `hover` tick as the pen lifts out of the digitizer's range after hovering. Windows
  buzzes a few frames after hover strokes end, but only the frames were seen — the
  causing event is inferred (findings/007 §4) — so the behaviour earns a switch, not a
  default.
- **Status notes.** The pen says what it is doing: `Idle · 84 %` when the link drops off
  the desk, `Docked · Charging 84 %` when it falls asleep on its charger and a plain
  `84 %` when it is picked up again — one note per change, decided before it is sent,
  each arriving in place of the last (crates/px13pend/src/status.rs).
  `Notifications1.Enabled` silences them (`px13pen/status.conf`, a settings row).
- **Tray icon (`px13pen-tray`).** A StatusNotifierItem with a dbusmenu — KDE, Xfce,
  Cinnamon, MATE, LXQt and the trays of Waybar, swaybar, ironbar and friends: battery
  and link state, haptics on/off with Low/Medium/High presets, Pair a pen…, Settings….
  The icon means "a pen is live right now" and goes away a minute after the last link
  drops, so the pen's idle drop of 6–15 s never blinks it. Labels in words, battery by
  icon shape — the accessibility gate landed before the tray (docs/tray.md).
- **The pen's firmware revision, read.** `Pen1.FirmwareRevision` takes the version
  string from the standard Device Information service (0x180A, characteristic 0x2A26)
  with one plain GATT read while the pen is connected; the earlier guess of a Bluetooth
  HID GET is superseded. Shown in the settings card and `px13penctl status`.
- **The top button's gestures are events, and each can run a command.** The pen
  classifies its own single, double and long presses; `px13pend` reads the gesture off a
  copy of the pen's reports — hidraw delivers to every reader, so the compositor's
  exclusive grab stays untouched — emits `Pen1.ButtonEvent`, and runs one user-stored
  shell command per gesture (`Buttons1`, `px13pen/buttons.conf`); the settings window
  edits them. The plan of re-emitting gestures as keys through uinput is gone: the pen's
  own Win+F18/19/20 already reach the compositor, and only the gesture behind a press
  was ever invisible (docs/decisions.md, findings/006 §5).
- **The charger, the fuel gauge and the dock, read from the pen's debug log.** The pen
  streams its firmware log over a vendor Bluetooth service that `px13pend` only ever
  reads. From it come `Pen1.BatteryPercentPrecise` and `BatteryMillivolts`, and
  `Pen1.Docked` — true after a dock sleep (a charge-state line, a gauge sample showing
  the battery no longer discharging, sleep right behind it; the line alone also precedes
  an off-charger sleep), false on the next connect. `px13pend pen-log` shows the decoded
  log on its own (findings/010).
- **Haptics policy: kept, gated, and armed by the pen itself.** The haptics settings
  (`px13pen/haptics.conf`) survive restarts; playback is gated per waveform class —
  `WhileInking` for continuous textures, `WhileInteracting` for ticks. The daemon arms
  the chosen continuous ink texture from the pen's own tip and eraser contact reports,
  one write per tool activation (the eraser end always arms the eraser texture), and
  stops it with the none ordinal the pen actually honours (findings/009,
  docs/protocol/haptics.md).
- **Seamless pairing works end to end.** Touching the pen to the screen pairs it in under a
  second through `px13pend`, with no button, code or configuration: the same single step and
  about the same time as on Windows (findings/008 §10). A pen that wakes up advertising for a
  pairing it still holds is now paired directly instead of being dropped as a leftover, which
  made cold pickups fail. Unlike Windows, which pairs out-of-band with the random key it sends
  through the digitizer, Linux pairs with passkey entry and passkey 0, because the kernel has
  no LE legacy out-of-band support; the pairing moment is therefore not protected against an
  eavesdropper (docs/future/kernel-le-legacy-oob.md).
- `px13pend` pairs by touch the way the hardware runs proved: it registers the host with the
  digitizer, scans while a pen is in range, answers each announcement with the key report and
  pairs the pen's advertisement through its own KeyboardOnly agent (passkey 0), so no desktop
  pairing prompt appears; the "paired" notification fires the moment the bond completes. The
  previous Just Works route to the announced address is gone from the daemon.
- Touch-to-pair verified on the hardware. The digitizer's `0xFFF4` collection is Microsoft's
  published *Seamless Pairing for Windows Pens* protocol; a Linux host that registers its
  address (`0x56`), exchanges capability (`0x70`), sends a pairing key (`0x6F`) on the pen's
  announcement (`0x6E`) and pairs the pen's advertisement with passkey entry (passkey 0)
  bonds in about 0.7 s, for a pen that knows the host and for a never-seen host
  (findings/008). `tools/capture/linux/06-seamless-pairing.py` runs the flow. The kernel
  lacks LE legacy out-of-band pairing, so Linux cannot yet use the key the way Windows does;
  noted in `docs/future/kernel-le-legacy-oob.md`. The protocol and connection docs are
  rewritten around the guide; the daemon's pairing path is marked as being replaced.
- `px13pend serve` reaches BlueZ and the pairing helper on the system bus even when it
  exports on the session bus; before, the per-user daemon looked for BlueZ on the
  session bus and exited at startup.
- The touch-to-pair listener runs from the moment `px13pend` starts, and `px13pend` starts
  with the graphical session (a systemd user preset), so a new pen pairs as soon as
  it comes near the screen. A failed pairing is retried on a later announcement
  instead of never; new bonds and failures raise a desktop notification.
- The pairing helper's system activation file is now named
  `io.github.gonzalezerik.Px13Pen.PairHelper1.service`; D-Bus ignored it under its old `.conf`
  name, so the helper never started.
- `px13penctl watch` prints readable lines by default; `--json` keeps the JSON Lines
  contract.
- Migrated the daemon's BlueZ access from zbus 5 to zbus 6.0: the blocking API
  is gone upstream, so the D-Bus calls now run the async API under
  `zbus::block_on`, and `Value`/`OwnedValue` became `Variant`/`OwnedVariant`.
  This also clears the RustSec advisories on the `zbus_names` and `zvariant`
  crates, which zbus 6 folds into itself. MSRV is now Rust 1.89.
- Renamed the project to PX13 SA205H Linux (formerly Open Pen Protocol) to say what it
  supports.

- Project scaffold: Rust workspace (`px13pen-core`, `px13pend`, `px13penctl`), Nix flake,
  podman Containerfiles for glibc (Debian) and musl (Alpine), CI on x86_64 and
  aarch64, privacy gate.
- PX13 digitizer fixtures and identification findings.
