# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/).

## Unreleased

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
