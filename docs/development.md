# Development

Building, testing and running PX13 SA205H Linux from source. Users want the
[README](../README.md) and [install guide](install.md) instead.

## Platforms

| | glibc | musl |
|---|---|---|
| **x86_64** | ✅ CI | ✅ CI |
| **aarch64** | ✅ CI | ✅ CI |

All four combinations are equal targets: CI builds, lints and tests each one natively.
The one exception is `px13pen-settings` on musl, which needs a shared C library to link;
its sources are still linted there (see [docs/decisions.md](decisions.md)).

## Layout

| Path | What |
|---|---|
| `crates/px13pen-core` | protocol types: digitizer reports, pairing, haptics |
| `crates/px13pend` | the daemon (hidraw + BlueZ, D-Bus API) |
| `crates/px13penctl` | command-line client |
| `crates/px13pen-settings` | settings window (GTK 4 / libadwaita) |
| `crates/px13pen-tray` | tray icon (StatusNotifierItem + dbusmenu) |
| `container/` | podman Containerfiles: `Containerfile.glibc` (Debian), `Containerfile.musl` (Alpine) |
| `docs/` | protocol write-ups, parity matrix, decisions |
| `findings/` | dated research notes |
| `fixtures/` | recorded, scrubbed hardware data used by tests |
| `tools/` | privacy gate, accessibility check, artifact fetcher |

## Building

You need either **Nix** (recommended: it gives the whole toolchain plus the
reverse-engineering workbench) or **podman**. Plain `cargo` works too.

### With Nix

```sh
nix develop               # Rust toolchain, cargo-deny, Ghidra (headless), hid-tools, …
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all

nix build                 # release build (glibc) → ./result/bin/{px13pend,px13penctl,px13pen-settings,px13pen-tray}
nix flake check           # what CI's nix job runs
```

The flake supports `x86_64-linux` and `aarch64-linux`.

### With podman (both C libraries)

```sh
container/build.sh              # glibc and musl: fmt, clippy, tests, then the runtime images
container/build.sh musl         # just one
podman run --rm px13-sa205h-linux:musl --version
```

To work inside a toolchain container without installing Rust:

```sh
# glibc
podman run --rm -it -v "$PWD":/src:Z -w /src docker.io/library/rust:1.99.1-trixie \
  sh -c 'apt-get update && apt-get install -y --no-install-recommends \
  pkg-config libgtk-4-dev libadwaita-1-dev && cargo test --workspace'
# musl
podman run --rm -it -v "$PWD":/src:Z -w /src docker.io/library/rust:1.99.1-alpine3.24 \
  sh -c 'apk add --no-cache musl-dev pkgconf gtk4.0-dev libadwaita-dev && cargo test --workspace'
```

Other architectures build under emulation once `qemu-user-static` binfmt handlers are
installed: `ARCH=arm64 container/build.sh`. CI builds them natively instead.

### With plain cargo

Rust 1.89 or newer. `px13pen-settings` builds against the system GTK 4 and libadwaita
(`libgtk-4-dev libadwaita-1-dev` on Debian/Ubuntu, `gtk4-devel libadwaita-devel` on
Fedora, `gtk4-dev libadwaita-dev` on Alpine); the daemon and CLI need nothing else:

```sh
cargo build --release --workspace     # → target/release/{px13pend,px13penctl,px13pen-settings,px13pen-tray}
```

Before every commit run the checks in [CONTRIBUTING.md](../CONTRIBUTING.md) §3, and install
the privacy pre-commit hook (§4).

## Running

```sh
px13penctl --version
px13pend                      # prints the supported digitizers
px13pend serve                # the daemon: D-Bus API, touch-to-pair listener, button actions
px13pend pairing-state        # reads the digitizer's seamless-pairing flag (read-only)
px13pend watch-pair --dry-run # prints pairing notifications as a pen approaches, changes nothing
px13pend watch-pair           # touch-to-pair in the foreground (docs/protocol/seamless-pairing.md)
px13penctl watch              # follow the daemon live: pens, battery, pairing progress
px13pend watch-pen            # live battery + link changes of the paired pen (via BlueZ)
px13pend watch-buttons --list # lists the pen's evdev nodes; without --list dumps its raw events
px13pend pen-log              # the pen's firmware log: fuel-gauge and charger readings (findings/010)
px13pend haptic --list        # the pen's effect table (ordinals from its capability report)
px13pend haptic click --send  # one click buzz on the paired pen; without --send only the exact
                          # output-report bytes are printed
px13pen-settings              # settings window (GTK 4; needs a running px13pend)
px13pen-tray                  # tray icon (starts with the session; docs/tray.md)
```

The daemon needs read/write access to the digitizer's `/dev/hidraw*` node and the system
D-Bus (BlueZ). Installing the package — the prebuilt tarball from the releases page plus its
`install.sh` ([docs/install.md](install.md) walks each distro through it),
`make install`, the flake, or the NixOS module — is the whole setup: a generated uaccess rule hands the signed-in user the pen-related nodes,
and `px13pend` starts with the graphical session (a systemd user preset the packaging applies).
Pairing needs no privilege and no prompt: a new pen pairs as soon as it comes near the screen,
with a desktop notification the moment it is bonded.
See [docs/packaging.md](packaging.md). In a container:

```sh
podman run --rm \
  --device /dev/hidraw2 \
  -v /run/dbus/system_bus_socket:/run/dbus/system_bus_socket \
  px13-sa205h-linux:glibc
```

On NixOS, importing the flake's module is the entire system setup:

```nix
{
  imports = [ inputs.px13-sa205h-linux.nixosModules.default ];
  services.px13-sa205h-linux.enable = true;
}
```

That wires the udev rules, the D-Bus activation and policy files, the polkit action and the
units from the package into the system — and applies the rule to the devices already
connected, so the switch alone is enough; see [docs/packaging.md](packaging.md).

## Reverse-engineering workbench

`nix develop` includes Ghidra (headless), JDK 21, ilspycmd, rizin, binwalk/unblob,
innoextract/msitools/7zz/wimlib, tshark and hid-tools. The Windows files being analysed
are never committed. Fetch them from their official sources (hashes are verified) with:

```sh
tools/fetch-artifacts.sh          # everything in docs/artifacts.tsv → artifacts/
```

Never flash or update firmware on a pen or a digitizer. See CONTRIBUTING.md §6.

