# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/).

## Unreleased

- `oppd serve` reaches BlueZ and the pairing helper on the system bus even when it
  exports on the session bus; before, the per-user daemon looked for BlueZ on the
  session bus and exited at startup.
- The touch-to-pair listener runs from the moment `oppd` starts, and `oppd` starts
  with the graphical session (a systemd user preset), so a new pen pairs as soon as
  it comes near the screen. A failed pairing is retried on a later announcement
  instead of never; new bonds and failures raise a desktop notification.
- The pairing helper's system activation file is now named
  `org.openpenprotocol.PairHelper1.service`; D-Bus ignored it under its old `.conf`
  name, so the helper never started.
- `oppctl watch` prints readable lines by default; `--json` keeps the JSON Lines
  contract.
- Migrated the daemon's BlueZ access from zbus 5 to zbus 6.0: the blocking API
  is gone upstream, so the D-Bus calls now run the async API under
  `zbus::block_on`, and `Value`/`OwnedValue` became `Variant`/`OwnedVariant`.
  This also clears the RustSec advisories on the `zbus_names` and `zvariant`
  crates, which zbus 6 folds into itself. MSRV is now Rust 1.89.
- Renamed the project to Open Pen Protocol (OPP): crates `opp-core`, `oppd`, `oppctl`.

- Project scaffold: Rust workspace (`opp-core`, `oppd`, `oppctl`), Nix flake,
  podman Containerfiles for glibc (Debian) and musl (Alpine), CI on x86_64 and
  aarch64, privacy gate.
- PX13 digitizer fixtures and identification findings.
