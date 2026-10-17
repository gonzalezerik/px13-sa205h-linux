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
