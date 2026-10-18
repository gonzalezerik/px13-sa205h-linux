# SPDX-License-Identifier: AGPL-3.0-or-later
{
  description = "Open Pen Protocol: Linux support for MPP 2.6 haptic pens";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/44a91898084f46797b5fac650c7e8c9ac38c43d4";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAll = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      # glibc build. musl is built natively in Alpine (container/Containerfile.musl):
      # nixpkgs' binary cache has no musl Rust toolchain, so a Nix musl build would
      # compile rustc first. CI treats both the same (see docs/decisions.md).
      packages = forAll (pkgs: {
        default = pkgs.callPackage ./nix/package.nix { };
      });

      # For NixOS and any other nixpkgs consumer that prefers the package
      # in the package set.
      overlays.default = final: prev: {
        open-pen-protocol = final.callPackage ./nix/package.nix { };
      };

      # Importing this module (plus the overlay, included here) is the
      # whole system setup: rules, activation files, polkit action and
      # units all ship in the package, and nothing needs enabling.
      nixosModules.default = { ... }: {
        imports = [ self.nixosModules.open-pen-protocol ];
        nixpkgs.overlays = [ self.overlays.default ];
      };

      # The same thing without the overlay, for systems that already
      # carry open-pen-protocol in their package set.
      nixosModules.open-pen-protocol = { config, lib, pkgs, ... }: {
        options.services.open-pen-protocol.enable =
          lib.mkEnableOption "Open Pen Protocol, MPP 2.6 haptic pen support";

        config = lib.mkIf config.services.open-pen-protocol.enable {
          environment.systemPackages = [ pkgs.open-pen-protocol ];

          # uaccess rules for the digitizer and the pen, from the
          # generated rule in lib/udev/rules.d.
          services.udev.packages = [ pkgs.open-pen-protocol ];

          # D-Bus activation: the pairing helper on the system bus and
          # oppd on each session bus. Both start on demand.
          services.dbus.packages = [ pkgs.open-pen-protocol ];

          # opp-pair-helper.service (D-Bus activated) and the optional
          # systemctl --user view of oppd. Neither has [Install].
          systemd.packages = [ pkgs.open-pen-protocol ];
        };
      };

      checks = forAll (pkgs: { build = self.packages.${pkgs.stdenv.hostPlatform.system}.default; });

      devShells = forAll (pkgs:
        let
          py = pkgs.python3.withPackages (ps: with ps; [
            pefile lief capstone dissect-cstruct bleak pwntools pyelftools
          ]);
        in
        {
          default = pkgs.mkShell {
            packages = with pkgs; [
              # Rust (both libcs are built by container/build.sh with podman)
              cargo rustc clippy rustfmt rust-analyzer cargo-deny pkg-config
              # opp-settings builds against these
              gtk4 libadwaita
              # reverse-engineering workbench (Ghidra runs headless only)
              nodejs_24 procps util-linux ghidra jdk21 rizin radare2 gdb ilspycmd
              innoextract msitools _7zz cabextract binwalk unblob upx
              # HID / input / Bluetooth observation
              hid-tools evtest evemu libinput bluez usbutils i2c-tools wireshark-cli
              # general
              py hexyl xxd ripgrep jq yara file curl
            ]
            # wimlib depends on syslinux, which is x86-only
            ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isx86_64 [ pkgs.wimlib ];
            shellHook = ''
              export JAVA_HOME=${pkgs.jdk21}
              export GHIDRA_INSTALL_DIR=${pkgs.ghidra}/lib/ghidra
            '';
          };
        });
    };
}
