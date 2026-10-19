# Open Pen Protocol (OPP)

Open Pen Protocol is open Linux support for the active-pen features the kernel doesn't
cover: tap-to-pair over Bluetooth, haptic feedback, pen battery, Bluetooth buttons and
settings stored in the pen.

OPP doesn't replace the protocol a pen speaks to the screen (MPP, USI, AES). That lives in
the pen's and the digitizer's firmware, and inking (position, pressure, tilt, eraser,
barrel button) already works on Linux through `hid-multitouch` and `wacom`. OPP is the
layer above it: one D-Bus interface that describes any pen by its capabilities, with a
backend per pen protocol underneath. Desktops and apps talk to that interface and never
need to know which protocol the pen speaks.

| Pen protocol | Status |
|---|---|
| **MPP 2.6** (Microsoft Pen Protocol, haptic pens) | in progress, tested on the ASUS Pen 3.0 (SA205H) with the ASUS ProArt PX13 (HN7306) and its ELAN `04F3:4631` digitizer |
| MPP 2.0 / 1.x | planned |
| USI 1.0 / 2.0 | planned |

A protocol is listed as supported only once it has been tested on real hardware.

> **Status: research.** The protocol work (`docs/protocol/`, `findings/`) comes first.
> Touch-to-pair is verified on the hardware: Microsoft's seamless-pairing protocol, run by
> a Linux host with stock BlueZ, pairs the pen in about 0.7 s
> ([findings/008](findings/008-linux-seamless-pairing.md)). The daemon's pairing path is
> being moved onto that flow; until then `tools/capture/linux/06-seamless-pairing.py` runs it.
> Haptics, battery and buttons are in progress.

## Platforms

| | glibc | musl |
|---|---|---|
| **x86_64** | ✅ CI | ✅ CI |
| **aarch64** | ✅ CI | ✅ CI |

All four combinations are equal targets: CI builds, lints and tests each one natively.
The one exception is `opp-settings` on musl, which needs a shared C library to link;
its sources are still linted there (see [docs/decisions.md](docs/decisions.md)).

## Layout

| Path | What |
|---|---|
| `crates/opp-core` | protocol types: digitizer reports, pairing, haptics |
| `crates/oppd` | the daemon (hidraw + BlueZ, D-Bus API) |
| `crates/oppctl` | command-line client |
| `crates/opp-settings` | settings window (GTK 4 / libadwaita) |
| `container/` | podman Containerfiles: `Containerfile.glibc` (Debian), `Containerfile.musl` (Alpine) |
| `docs/` | protocol write-ups, parity matrix, decisions |
| `findings/` | dated research notes |
| `fixtures/` | recorded, scrubbed hardware data used by tests |
| `tools/` | privacy gate, accessibility check, artifact fetcher |

## Development

You need either **Nix** (recommended: it gives the whole toolchain plus the
reverse-engineering workbench) or **podman**. Plain `cargo` works too.

### With Nix

```sh
nix develop               # Rust toolchain, cargo-deny, Ghidra (headless), hid-tools, …
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all

nix build                 # release build (glibc) → ./result/bin/{oppd,oppctl,opp-settings}
nix flake check           # what CI's nix job runs
```

The flake supports `x86_64-linux` and `aarch64-linux`.

### With podman (both C libraries)

```sh
container/build.sh              # glibc and musl: fmt, clippy, tests, then the runtime images
container/build.sh musl         # just one
podman run --rm open-pen-protocol:musl --version
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

Rust 1.89 or newer. `opp-settings` builds against the system GTK 4 and libadwaita
(`libgtk-4-dev libadwaita-1-dev` on Debian/Ubuntu, `gtk4-devel libadwaita-devel` on
Fedora, `gtk4-dev libadwaita-dev` on Alpine); the daemon and CLI need nothing else:

```sh
cargo build --release --workspace     # → target/release/{oppd,oppctl,opp-settings}
```

Before every commit run the checks in [CONTRIBUTING.md](CONTRIBUTING.md) §3, and install
the privacy pre-commit hook (§4).

## Running

```sh
oppctl --version
oppd                      # prints the supported digitizers
oppd pairing-state        # reads the digitizer's seamless-pairing flag (read-only)
oppd watch-pair --dry-run # prints pairing notifications as a pen approaches, changes nothing
python3 tools/capture/linux/06-seamless-pairing.py
                          # touch-to-pair, verified flow (the daemon's own path is being
                          # replaced by it; see docs/protocol/seamless-pairing.md)
oppctl watch              # follow the daemon live: pens, battery, pairing progress
oppd watch-pen            # live battery + link changes of the paired pen (via BlueZ)
oppd watch-buttons --list # lists the pen's evdev nodes; without --list dumps its raw events
oppd haptic --list        # the pen's effect table (ordinals from its capability report)
oppd haptic click --send  # one click buzz on the paired pen; without --send only the exact
                          # output-report bytes are printed
opp-settings              # settings window (GTK 4; needs a running oppd)
```

The daemon needs read/write access to the digitizer's `/dev/hidraw*` node and the system
D-Bus (BlueZ). Installing the package — `make install`, the flake, or the NixOS module — is
the whole setup: a generated uaccess rule hands the signed-in user the pen-related nodes,
`oppd` starts with the graphical session (a systemd user preset the packaging applies), and
the pairing helper is activated by the system bus on demand. A new pen pairs as soon as it
comes near the screen, with a desktop notification.
See [docs/packaging.md](docs/packaging.md). In a container:

```sh
podman run --rm \
  --device /dev/hidraw2 \
  -v /run/dbus/system_bus_socket:/run/dbus/system_bus_socket \
  open-pen-protocol:glibc
```

On NixOS, importing the flake's module is the entire system setup:

```nix
{
  imports = [ inputs.open-pen-protocol.nixosModules.default ];
  services.open-pen-protocol.enable = true;
}
```

That wires the udev rules, the D-Bus activation and policy files, the polkit action and the
units from the package into the system; see [docs/packaging.md](docs/packaging.md).

## Reverse-engineering workbench

`nix develop` includes Ghidra (headless), JDK 21, ilspycmd, rizin, binwalk/unblob,
innoextract/msitools/7zz/wimlib, tshark and hid-tools. The Windows files being analysed
are never committed. Fetch them from their official sources (hashes are verified) with:

```sh
tools/fetch-artifacts.sh          # everything in docs/artifacts.tsv → artifacts/
```

Never flash or update firmware on a pen or a digitizer. See CONTRIBUTING.md §6.

## License

[GNU AGPL-3.0-or-later](LICENSE). See [CONTRIBUTING.md](CONTRIBUTING.md) for the rules on
reusing code and on evidence.
