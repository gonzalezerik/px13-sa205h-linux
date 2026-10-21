# SPDX-License-Identifier: AGPL-3.0-or-later
{
  description = "Linux support for the ASUS Pen 3.0 (SA205H) on the ASUS ProArt PX13 (HN7306)";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/44a91898084f46797b5fac650c7e8c9ac38c43d4";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAll = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
      # The wiki sources and the accessibility checker, as minimal store
      # trees (mkdocs resolves pages/ relative to its config file).
      wikiSrc = pkgs: pkgs.lib.fileset.toSource { root = ./.; fileset = ./docs/wiki; };
      a11ySrc = pkgs: pkgs.lib.fileset.toSource { root = ./.; fileset = ./tools/a11y/wiki; };
    in
    {
      # glibc build. musl is built natively in Alpine (container/Containerfile.musl):
      # nixpkgs' binary cache has no musl Rust toolchain, so a Nix musl build would
      # compile rustc first. CI treats both the same (see docs/decisions.md).
      packages = forAll (pkgs: {
        default = pkgs.callPackage ./nix/package.nix { };

        # The wiki (docs/wiki): a static site from Markdown, served by any
        # file host. Every page must pass the accessibility check below
        # before it ships.
        wiki = pkgs.runCommand "px13-sa205h-wiki" {
          nativeBuildInputs = [ (pkgs.callPackage ./nix/wiki.nix { }) ];
        } ''
          export HOME="$TMPDIR"
          mkdocs build --strict -f ${wikiSrc pkgs}/docs/wiki/mkdocs.yml --site-dir "$out"
        '';
      });

      # For NixOS and any other nixpkgs consumer that prefers the package
      # in the package set.
      overlays.default = final: prev: {
        px13-sa205h-linux = final.callPackage ./nix/package.nix { };
      };

      # Importing this module (plus the overlay, included here) is the
      # whole system setup: rules, activation files, polkit action and
      # units all ship in the package, and nothing needs enabling.
      nixosModules.default = { ... }: {
        imports = [ self.nixosModules.px13-sa205h-linux ];
        nixpkgs.overlays = [ self.overlays.default ];
      };

      # The same thing without the overlay, for systems that already
      # carry px13-sa205h-linux in their package set.
      nixosModules.px13-sa205h-linux = { config, lib, pkgs, ... }: {
        options.services.px13-sa205h-linux.enable =
          lib.mkEnableOption "PX13 SA205H Linux (px13pend daemon, pen settings and tray icon)";

        config = lib.mkIf config.services.px13-sa205h-linux.enable {
          environment.systemPackages = [ pkgs.px13-sa205h-linux ];

          # uaccess rules for the digitizer and the pen, from the
          # generated rule in lib/udev/rules.d.
          services.udev.packages = [ pkgs.px13-sa205h-linux ];

          # A rule only takes effect when an event runs it, and a
          # switch sends none: the digitizer's hidraw node is there
          # long before this rule is installed, so the first install
          # would leave it without its uaccess ACL until a replug or
          # a hand-typed `udevadm trigger`. Re-run the rule over the
          # devices already present whenever the assembled rules
          # change; RemainAfterExit keeps the finished run "active",
          # which is what makes the restart trigger fire.
          systemd.services."px13-sa205h-linux-udev-trigger" = {
            description = "Apply the pen uaccess rules to present HID devices";
            after = [ "systemd-udev-trigger.service" ];
            wantedBy = [ "multi-user.target" ];
            restartTriggers = [ config.environment.etc."udev/rules.d".source ];
            serviceConfig = {
              Type = "oneshot";
              RemainAfterExit = true;
              ExecStart =
                "${config.systemd.package}/bin/udevadm trigger --subsystem-match=hidraw --action=change";
            };
          };

          # D-Bus activation: the pairing helper on the system bus and
          # px13pend on each session bus. Both start on demand.
          services.dbus.packages = [ pkgs.px13-sa205h-linux ];

          # px13pen-pair-helper.service (D-Bus activated, no [Install]) and
          # the session pair: px13pend.service, which starts with every
          # graphical session so the touch-to-pair listener is up before the
          # pen comes near, and px13pen-tray.service showing its icon.
          systemd.packages = [ pkgs.px13-sa205h-linux ];
          systemd.user.services.px13pend.wantedBy = [ "graphical-session.target" ];
          systemd.user.services.px13pen-tray.wantedBy = [ "graphical-session.target" ];
        };
      };

      checks = forAll (pkgs: {
        build = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      # nixpkgs ships Chromium for x86 only, so the headless render test
      # can only exist on x86_64.
      } // pkgs.lib.optionalAttrs pkgs.stdenv.hostPlatform.isx86_64 {
          # axe-core AA over every built wiki page, rendered in light and
          # dark. This is what keeps the wiki honest about WCAG 2.2 AA; see
          # tools/a11y/wiki/check.py.
          wiki-a11y = pkgs.runCommand "px13-sa205h-wiki-a11y" {
            nativeBuildInputs = [ (pkgs.python3.withPackages (ps: [ ps.playwright ])) ];
            PLAYWRIGHT_BROWSERS_PATH = pkgs.playwright-driver.browsers;
            # axe-core as a hash-pinned download rather than a blob in git:
            # the privacy scan stays readable, and the hash is the review.
            AXE_JS = pkgs.fetchurl {
              url = "https://unpkg.com/axe-core@4.11.0/axe.min.js";
              hash = "sha256-6eWGPDOodPCbwBrNkjS348hxR59e74gC+lglREZebQE=";
            };
          } ''
            export HOME="$TMPDIR"
            python -I ${a11ySrc pkgs}/tools/a11y/wiki/check.py \
              ${self.packages.${pkgs.stdenv.hostPlatform.system}.wiki}
            touch "$out"
          '';
        });

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
              # px13pen-settings builds against these
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
