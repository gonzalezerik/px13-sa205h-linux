# Install on your Linux

Prebuilt tarballs are published with every release: **glibc** and **musl**,
for **x86_64** and **aarch64**, each built the same way CI builds them
(`container/Containerfile.*`, `dist` stage). Pick your row below; the
download is on the repository's Releases page. Verify it before running
anything:

```sh
sha256sum -c SHA256SUMS        # from the same release as the tarball
```

**Choosing a tarball**

| Tarball | Contains | Runs on |
|---|---|---|
| `…-musl.tar.gz` | daemon, `px13penctl`, tray icon — statically linked | any distro with systemd + BlueZ, including old LTS releases, Alpine and immutable systems |
| `…-glibc.tar.gz` | the above **plus the settings window** | current stable distros: glibc ≥ 2.41 and libadwaita ≥ 1.6 (Fedora ≥ 42, Debian ≥ 13, Ubuntu ≥ 25.04, Arch, Tumbleweed) |

Everything past this page assumes a graphical session with systemd and
BlueZ (the daemon talks to BlueZ over D-Bus and to the digitizer over
`/dev/hidraw`). The usual flow everywhere is: extract, `sudo ./install.sh`
from inside the extracted directory, sign out and back in — then bring a
pen near the screen and it pairs itself. The sections below only cover
what differs.

## Fedora, RHEL, CentOS Stream, Alma, Rocky

Fedora gets the full desktop experience; the RHEL family ships older
libraries, so there use the musl tarball (everything but the settings
window):

```sh
tar xzf px13-sa205h-linux-<version>-x86_64-<libc>.tar.gz
cd px13-sa205h-linux-<version>-x86_64-<libc>
sudo ./install.sh
```

The script installs under `/usr/local`, drops the udev rule into
`/etc/udev/rules.d`, applies the user preset and reloads udev. SELinux
needs no attention: nothing runs from unusual contexts.

## Debian and Ubuntu

Debian 13+ and Ubuntu 25.04+: glibc tarball, same three commands as above.
Ubuntu 24.04 LTS and Debian 12: the musl tarball — its settings window
needs libadwaita 1.6, which first shipped after those releases. The tray
icon and `px13penctl` cover the same controls; the window appears in the
application menu once installed from a glibc tarball.

## Arch and friends

Install from [`contrib/PKGBUILD`](../contrib/PKGBUILD):

```sh
makepkg -si
```

Arch's systemd pacman hooks apply the user preset and reload udev for you.
(The PKGBUILD is also how the future AUR package will be built; from a
checkout before that, `sudo cargo xtask install` after
`cargo build --release --locked --workspace` does the same.)

## openSUSE

Tumbleweed takes the glibc tarball; Leap ships an older glibc, so there
use the musl one. Both take the three-command flow from the Fedora
section; nothing openSUSE-specific comes up.

## Alpine

Use the musl tarball — it is built on Alpine in CI. Alpine's default init
is OpenRC with no systemd user sessions: the user units then have nothing
to run in, so start the daemon yourself at session start
(`px13pend serve &` from your compositor/WM config, or
`loginctl enable-linger` plus the user unit if you run systemd) and grant
yourself the hidraw nodes with a group rule, since `TAG+="uaccess"` needs
systemd-logind:

```
# /etc/udev/rules.d/70-px13-sa205h-linux.rules, group variant
SUBSYSTEM=="hidraw", KERNELS=="0018:04F3:4631.*", GROUP="input", MODE="0660"
SUBSYSTEM=="hidraw", KERNELS=="0005:330A:0001.*", GROUP="input", MODE="0660"
```

plus adding your user to `input`, and `udevadm control --reload-rules`.

## NixOS and Nix

NixOS is the zero-configuration path — the flake ships a module (units,
udev, D-Bus activation, polkit, and a oneshot that applies the rule to
devices already plugged in):

```nix
{
  imports = [ inputs.px13-sa205h-linux.nixosModules.default ];
  services.px13-sa205h-linux.enable = true;
}
```

On non-NixOS with Nix installed, `nix build` from a checkout produces
`result/bin/`. `docs/packaging.md` documents what the module registers.

## Immutable systems (Fedora Silverblue/Onyx/Sericea, Bluefin, Bazzite, openSUSE MicroOS)

Use the **musl** tarball and `sudo ./install.sh`. It writes only where
these systems are writable by design: the tree lands under `/usr/local`
(ostree maps it into `/var`), the udev rule and bus policy into `/etc`,
and the preset enables the units for your user's next login. The one
best-effort step it skips when `/usr/share` is read-only is the polkit
action for the privileged pairing helper — which normal touch-to-pair
never uses, so nothing you would notice is missing. KDE-based immutables
(Bazzite) show the tray natively; GNOME-based ones need an
AppIndicator/StatusNotifier extension for it.

SteamOS: same tarball; its image rules about `/etc` and `/usr/local`
match what the script needs, and in Desktop Mode the tray works under KDE.

## Anything else

Build from the checkout: Rust ≥ 1.89, GTK 4 / libadwaita dev packages for
the settings window, then `make install PREFIX=/usr` (or `/usr/local`;
`docs/packaging.md` lists every directory and override, plus the two
post-install commands `install.sh` also runs). Gentoo users will find the
Makefile's layout matches what `make install DESTDIR=… PREFIX=/usr`
expects.

The app cannot be packaged as Flatpak or Snap: the daemon needs session
D-Bus, system BlueZ and `/dev/hidraw` under the conventions above
(`docs/packaging.md` explains why the sandbox models do not fit).

## After installing (any distro)

Sign out and back in — `px13pend` then starts with the session and the
touch-to-pair listener is live before you do anything. Bring the pen to
the screen: it bonds in under a second and a notification confirms it.
`px13penctl status` shows the pen and battery; the tray icon appears while
a pen is live and disappears a minute after the last link drops. If
nothing answers, check `systemctl --user status px13pend` first and the
README's disclaimer second — this is hobby software talking to embedded
controllers, not vendor firmware.
