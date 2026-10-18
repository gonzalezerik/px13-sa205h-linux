#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Build (and test) the container images with podman.
#   container/build.sh                  glibc + musl for this machine's architecture
#   container/build.sh musl             one libc
#   ARCH=arm64 container/build.sh       another architecture (needs qemu-user-static binfmt)
set -euo pipefail
cd "$(dirname "$0")/.."
arch="${ARCH:-}"
for libc in ${@:-glibc musl}; do
  platform=()
  [ -n "$arch" ] && platform=(--platform "linux/$arch")
  podman build "${platform[@]}" -f "container/Containerfile.$libc" --target test -t "open-pen-protocol:$libc-test" .
  podman build "${platform[@]}" -f "container/Containerfile.$libc" -t "open-pen-protocol:$libc${arch:+-$arch}" .
  podman run --rm "open-pen-protocol:$libc${arch:+-$arch}" --version
done
