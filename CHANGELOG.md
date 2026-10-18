# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/).

## Unreleased

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
