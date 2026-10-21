# SPDX-License-Identifier: AGPL-3.0-or-later
#
# The full install tree: binaries, the pairing helper under libexec,
# the generated udev rule, D-Bus activation and policy files, the
# polkit action and the systemd units. The D-Bus and systemd files
# carry @PREFIX@, rewritten here to $out so activation references this
# very package. See docs/packaging.md for the layout used by
# `make install` (which follows the same relative tree under $PREFIX).
{
  lib,
  rustPlatform,
  pkg-config,
  gtk4,
  libadwaita,
}:
rustPlatform.buildRustPackage {
  pname = "px13-sa205h-linux";
  version = (builtins.fromTOML (builtins.readFile ../Cargo.toml)).workspace.package.version;
  src = lib.fileset.toSource {
    root = ./..;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../crates
      ../tools
      ../contrib
      ../data
    ];
  };
  cargoLock.lockFile = ../Cargo.lock;
  nativeBuildInputs = [ pkg-config ];
  buildInputs = [ gtk4 libadwaita ];
  postInstall = ''
    # Repository task binary; not part of anything a user runs.
    rm $out/bin/xtask

    # The helper is privileged and not a command; libexec it.
    mkdir -p $out/libexec
    mv $out/bin/px13pen-pair-helper $out/libexec/px13pen-pair-helper

    install -Dm644 contrib/udev/70-px13-sa205h-linux.rules \
      $out/lib/udev/rules.d/70-px13-sa205h-linux.rules

    install -Dm644 data/dbus/system-services/io.github.gonzalezerik.Px13Pen.PairHelper1.service \
      $out/share/dbus-1/system-services/io.github.gonzalezerik.Px13Pen.PairHelper1.service
    substituteInPlace $out/share/dbus-1/system-services/io.github.gonzalezerik.Px13Pen.PairHelper1.service \
      --replace-fail "@PREFIX@" "$out"

    install -Dm644 data/dbus/system.d/io.github.gonzalezerik.Px13Pen.PairHelper1.conf \
      $out/share/dbus-1/system.d/io.github.gonzalezerik.Px13Pen.PairHelper1.conf

    install -Dm644 data/dbus/session-services/io.github.gonzalezerik.Px13Pen.Daemon1.service \
      $out/share/dbus-1/services/io.github.gonzalezerik.Px13Pen.Daemon1.service
    substituteInPlace $out/share/dbus-1/services/io.github.gonzalezerik.Px13Pen.Daemon1.service \
      --replace-fail "@PREFIX@" "$out"

    install -Dm644 data/polkit/io.github.gonzalezerik.Px13Pen.policy \
      $out/share/polkit-1/actions/io.github.gonzalezerik.Px13Pen.policy

    install -Dm644 data/systemd/system/px13pen-pair-helper.service \
      $out/lib/systemd/system/px13pen-pair-helper.service
    substituteInPlace $out/lib/systemd/system/px13pen-pair-helper.service \
      --replace-fail "@PREFIX@" "$out"

    install -Dm644 data/systemd/user/px13pend.service \
      $out/lib/systemd/user/px13pend.service
    substituteInPlace $out/lib/systemd/user/px13pend.service \
      --replace-fail "@PREFIX@" "$out"
    install -Dm644 data/systemd/user/px13pen-tray.service \
      $out/lib/systemd/user/px13pen-tray.service
    substituteInPlace $out/lib/systemd/user/px13pen-tray.service \
      --replace-fail "@PREFIX@" "$out"
    install -Dm644 data/systemd/user-preset/80-px13-sa205h-linux.preset \
      $out/lib/systemd/user-preset/80-px13-sa205h-linux.preset

    install -Dm644 crates/px13pen-settings/data/io.github.gonzalezerik.Px13Pen.Settings.desktop \
      $out/share/applications/io.github.gonzalezerik.Px13Pen.Settings.desktop
    install -Dm644 crates/px13pen-settings/data/icons/hicolor/scalable/apps/io.github.gonzalezerik.Px13Pen.Settings.svg \
      $out/share/icons/hicolor/scalable/apps/io.github.gonzalezerik.Px13Pen.Settings.svg
  '';
  meta = {
    description = "Linux support for MPP 2.6 haptic pens";
    license = lib.licenses.agpl3Plus;
    mainProgram = "px13pend";
    platforms = [ "x86_64-linux" "aarch64-linux" ];
  };
}
