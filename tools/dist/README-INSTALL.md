# PX13 SA205H Linux — release tarball

Linux support for the ASUS Pen 3.0 (SA205H) on the ASUS ProArt PX13:
touch-to-pair, pen haptics, battery, top-button gestures, tray icon.
Everything here was measured on that one laptop and pen; the repository's
wiki covers what is known and how it was found out.

**Install:** extract, then run `sudo ./install.sh`, then sign out and back
in. The daemon starts with your session, the tray icon appears when a pen
is live, and touching the pen to the screen pairs it — no configuration.
`px13penctl status` shows what the daemon sees.

**Which tarball:** `musl` is statically linked and runs on any distro with
systemd and BlueZ — it holds the daemon, the CLI and the tray icon. The
`glibc` tarball adds the settings window and needs the current stable
libraries (glibc ≥ 2.41, libadwaita ≥ 1.6). Per-distro steps, including the
immutable systems: `docs/install.md` in the repository.

Verify the download against the release's `SHA256SUMS`:
`sha256sum -c SHA256SUMS`.

Installed under `/usr/local` (plus the udev rule and bus policy under
`/etc`): binaries `px13pend`, `px13penctl`, `px13pen-tray` (and
`px13pen-settings` on glibc builds), systemd user units with a preset that
enables them, the session tray icon, D-Bus activation, and the pairing
helper under `libexec` (unused by touch-to-pair; see docs/packaging.md).
To uninstall: remove those files, run `systemctl --global disable
px13pend.service px13pen-tray.service`, delete the udev rule from
`/etc/udev/rules.d`, and `sudo udevadm control --reload-rules`.
