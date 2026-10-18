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
  pname = "open-pen-protocol";
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
    mv $out/bin/opp-pair-helper $out/libexec/opp-pair-helper

    install -Dm644 contrib/udev/70-open-pen-protocol.rules \
      $out/lib/udev/rules.d/70-open-pen-protocol.rules

    install -Dm644 data/dbus/system-services/org.openpenprotocol.PairHelper1.service \
      $out/share/dbus-1/system-services/org.openpenprotocol.PairHelper1.service
    substituteInPlace $out/share/dbus-1/system-services/org.openpenprotocol.PairHelper1.service \
      --replace-fail "@PREFIX@" "$out"

    install -Dm644 data/dbus/system.d/org.openpenprotocol.PairHelper1.conf \
      $out/share/dbus-1/system.d/org.openpenprotocol.PairHelper1.conf

    install -Dm644 data/dbus/session-services/org.openpenprotocol.Daemon1.service \
      $out/share/dbus-1/services/org.openpenprotocol.Daemon1.service
    substituteInPlace $out/share/dbus-1/services/org.openpenprotocol.Daemon1.service \
      --replace-fail "@PREFIX@" "$out"

    install -Dm644 data/polkit/org.openpenprotocol.policy \
      $out/share/polkit-1/actions/org.openpenprotocol.policy

    install -Dm644 data/systemd/system/opp-pair-helper.service \
      $out/lib/systemd/system/opp-pair-helper.service
    substituteInPlace $out/lib/systemd/system/opp-pair-helper.service \
      --replace-fail "@PREFIX@" "$out"

    install -Dm644 data/systemd/user/oppd.service \
      $out/lib/systemd/user/oppd.service
    substituteInPlace $out/lib/systemd/user/oppd.service \
      --replace-fail "@PREFIX@" "$out"
    install -Dm644 data/systemd/user-preset/80-open-pen-protocol.preset \
      $out/lib/systemd/user-preset/80-open-pen-protocol.preset

    install -Dm644 crates/opp-settings/data/org.openpenprotocol.Settings.desktop \
      $out/share/applications/org.openpenprotocol.Settings.desktop
    install -Dm644 crates/opp-settings/data/icons/hicolor/scalable/apps/org.openpenprotocol.Settings.svg \
      $out/share/icons/hicolor/scalable/apps/org.openpenprotocol.Settings.svg
  '';
  meta = {
    description = "Linux support for MPP 2.6 haptic pens";
    license = lib.licenses.agpl3Plus;
    mainProgram = "oppd";
    platforms = [ "x86_64-linux" "aarch64-linux" ];
  };
}
