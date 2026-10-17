# SPDX-License-Identifier: AGPL-3.0-or-later
{
  description = "mpp-linux: Linux support for MPP 2.6 haptic pens";

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
        default = pkgs.rustPlatform.buildRustPackage {
          pname = "mpp-linux";
          version = (fromTOML (builtins.readFile ./Cargo.toml)).workspace.package.version;
          src = pkgs.lib.fileset.toSource {
            root = ./.;
            fileset = pkgs.lib.fileset.unions [ ./Cargo.toml ./Cargo.lock ./crates ];
          };
          cargoLock.lockFile = ./Cargo.lock;
          meta = {
            description = "Linux support for MPP 2.6 haptic pens";
            license = pkgs.lib.licenses.agpl3Plus;
            mainProgram = "mppd";
            platforms = systems;
          };
        };
      });

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
