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
> First function landed: `oppd watch-pair` bonds the pen the moment its tip touches the
> screen (`docs/protocol/seamless-pairing.md`). Haptics, battery and buttons are in progress.

## Platforms

| | glibc | musl |
|---|---|---|
| **x86_64** | ✅ CI | ✅ CI |
| **aarch64** | ✅ CI | ✅ CI |

All four combinations are equal targets: CI builds, lints and tests each one natively.
See [docs/decisions.md](docs/decisions.md).

## Layout

| Path | What |
|---|---|
| `crates/opp-core` | protocol types: digitizer reports, pairing, haptics |
| `crates/oppd` | the daemon (hidraw + BlueZ, D-Bus API) |
| `crates/oppctl` | command-line client |
| `container/` | podman Containerfiles: `Containerfile.glibc` (Debian), `Containerfile.musl` (Alpine) |
| `docs/` | protocol write-ups, parity matrix, decisions |
| `findings/` | dated research notes |
| `fixtures/` | recorded, scrubbed hardware data used by tests |
| `tools/` | privacy gate, artifact fetcher |

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

nix build                 # release build (glibc) → ./result/bin/{oppd,oppctl}
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
  cargo test --workspace
# musl
podman run --rm -it -v "$PWD":/src:Z -w /src docker.io/library/rust:1.99.1-alpine3.24 \
  sh -c 'apk add --no-cache musl-dev && cargo test --workspace'
```

Other architectures build under emulation once `qemu-user-static` binfmt handlers are
installed: `ARCH=arm64 container/build.sh`. CI builds them natively instead.

### With plain cargo

Rust 1.89 or newer:

```sh
cargo build --release --workspace     # → target/release/{oppd,oppctl}
```

Before every commit run the checks in [CONTRIBUTING.md](CONTRIBUTING.md) §3, and install
the privacy pre-commit hook (§4).

## Running

```sh
oppctl --version
oppd                      # prints the supported digitizers
oppd pairing-state        # reads the digitizer's seamless-pairing flag (read-only)
oppd watch-pair --dry-run # prints pairing notifications as a pen approaches, changes nothing
oppd watch-pair           # bonds the announced pen through BlueZ (needs the system bus)
oppd watch-pen            # live battery + link changes of the paired pen (via BlueZ)
oppd watch-buttons --list # lists the pen's evdev nodes; without --list dumps its raw events
oppd haptic --list        # the pen's effect table (ordinals from its capability report)
oppd haptic click --send  # one click buzz on the paired pen; without --send only the exact
                          # output-report bytes are printed
```

Once the daemon is functional it needs read/write access to the digitizer's
`/dev/hidraw*` node (a udev rule, shipped with the package) and the system D-Bus
(BlueZ). In a container:

```sh
podman run --rm \
  --device /dev/hidraw2 \
  -v /run/dbus/system_bus_socket:/run/dbus/system_bus_socket \
  open-pen-protocol:glibc
```

A NixOS module and distribution packages come later.

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
