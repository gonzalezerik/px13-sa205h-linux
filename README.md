# PX13 SA205H Linux

Linux support for the ASUS Pen 3.0 (SA205H) on the ASUS ProArt PX13 (HN7306).

This project covers the active-pen features the kernel doesn't: tap-to-pair over
Bluetooth, haptic feedback, pen battery, Bluetooth buttons and settings stored in the
pen.

It doesn't replace the protocol a pen speaks to the screen (MPP, USI, AES). That lives in
the pen's and the digitizer's firmware, and inking (position, pressure, tilt, eraser,
barrel button) already works on Linux through `hid-multitouch` and `wacom`. This project
is the layer above it: one D-Bus interface that describes any pen by its capabilities, with
a backend per pen protocol underneath. Desktops and apps talk to that interface and never
need to know which protocol the pen speaks.

| Pen protocol | Status |
|---|---|
| **MPP 2.6** (Microsoft Pen Protocol, haptic pens) | in progress, tested on the ASUS Pen 3.0 (SA205H) with the ASUS ProArt PX13 (HN7306) and its ELAN `04F3:4631` digitizer |
| MPP 2.0 / 1.x | planned |
| USI 1.0 / 2.0 | planned |

A protocol is listed as supported only once it has been tested on real hardware.

> **Status: touch-to-pair works.** Bring the pen to the screen and `px13pend` pairs it in
> under a second: no pairing button, no code to confirm, nothing to configure. That matches
> Windows step for step and in speed (about 0.7 s from touch to bonded on Linux, 0.6 s on
> Windows). It runs Microsoft's published seamless-pairing protocol on stock BlueZ and a stock
> kernel, for a pen that already knows the laptop and for one that has never seen it
> ([findings/008](findings/008-linux-seamless-pairing.md),
> [how it works](docs/protocol/seamless-pairing.md)).
>
> **How Linux differs from Windows: the pairing key.** Both hand the pen a 128-bit key through
> the digitizer. Windows then pairs *out-of-band* with that random key, so it never crosses the
> radio. Linux can't yet: the pen only does LE *legacy* pairing, and the kernel implements
> out-of-band keys only for LE Secure Connections (BlueZ documents the legacy case as
> unsupported). Linux therefore pairs with passkey entry, and since the pen has no keypad it
> always enters **0**, so the host sends a key of zeros and enters 0 too. The user sees no
> difference, but that one pairing moment is weaker: someone recording the radio right then
> could work out the link key, which Windows' random key prevents. Later connections are
> unaffected. The kernel change that would close this gap is described in
> [docs/future/kernel-le-legacy-oob.md](docs/future/kernel-le-legacy-oob.md); with it the
> daemon would send a random key and pair exactly as Windows does.
>
> Haptics, battery and the Bluetooth buttons are in progress.

## Platforms

| | glibc | musl |
|---|---|---|
| **x86_64** | ✅ CI | ✅ CI |
| **aarch64** | ✅ CI | ✅ CI |

All four combinations are equal targets: CI builds, lints and tests each one natively.
The one exception is `px13pen-settings` on musl, which needs a shared C library to link;
its sources are still linted there (see [docs/decisions.md](docs/decisions.md)).

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

Before every commit run the checks in [CONTRIBUTING.md](CONTRIBUTING.md) §3, and install
the privacy pre-commit hook (§4).

## Running

```sh
px13penctl --version
px13pend                      # prints the supported digitizers
px13pend pairing-state        # reads the digitizer's seamless-pairing flag (read-only)
px13pend watch-pair --dry-run # prints pairing notifications as a pen approaches, changes nothing
px13pend watch-pair           # touch-to-pair in the foreground (docs/protocol/seamless-pairing.md)
px13penctl watch              # follow the daemon live: pens, battery, pairing progress
px13pend watch-pen            # live battery + link changes of the paired pen (via BlueZ)
px13pend watch-buttons --list # lists the pen's evdev nodes; without --list dumps its raw events
px13pend haptic --list        # the pen's effect table (ordinals from its capability report)
px13pend haptic click --send  # one click buzz on the paired pen; without --send only the exact
                          # output-report bytes are printed
px13pen-settings              # settings window (GTK 4; needs a running px13pend)
px13pen-tray                  # tray icon (starts with the session; docs/tray.md)
```

The daemon needs read/write access to the digitizer's `/dev/hidraw*` node and the system
D-Bus (BlueZ). Installing the package — `make install`, the flake, or the NixOS module — is
the whole setup: a generated uaccess rule hands the signed-in user the pen-related nodes,
and `px13pend` starts with the graphical session (a systemd user preset the packaging applies).
Pairing needs no privilege and no prompt: a new pen pairs as soon as it comes near the screen,
with a desktop notification the moment it is bonded.
See [docs/packaging.md](docs/packaging.md). In a container:

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
