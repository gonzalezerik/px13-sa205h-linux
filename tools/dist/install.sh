#!/bin/sh
# SPDX-License-Identifier: AGPL-3.0-or-later
# Installs the release tarball this script ships inside. Run it as root
# from the extracted directory:  sudo ./install.sh
#
# The install layout and the post-install steps are documented in the
# project's docs/packaging.md; this script is the distro packager's job,
# done by hand for systems without a package for it yet.

set -eu

cd "$(dirname "$0")"
[ -d usr/local/bin ] || {
    echo "install.sh: run it from inside the extracted release directory" >&2
    exit 1
}
[ "$(id -u)" = 0 ] || {
    echo "install.sh: needs root — run it as: sudo ./install.sh" >&2
    exit 1
}

# The tree lands under /usr/local: writable on every distro in the install
# guide, including the ostree-immutables (they map /usr/local into /var).
cp -a usr/. /usr/

# udev reads /etc unconditionally, /usr/lib only where /usr is writable —
# so the rule goes into /etc as well. uaccess applied twice is the same
# one access, and plain systems can delete the /usr/lib copy if they care.
install -d /etc/udev/rules.d
install -m 644 usr/local/lib/udev/rules.d/70-px13-sa205h-linux.rules /etc/udev/rules.d/

# Same reason for the pairing helper's bus policy: /etc/dbus-1/system.d is
# always read. Its activation file lives under /usr/local/share, which the
# system bus scans (dbus' standard service dirs include /usr/local).
install -d /etc/dbus-1/system.d
install -m 644 \
    usr/local/share/dbus-1/system.d/io.github.gonzalezerik.Px13Pen.PairHelper1.conf \
    /etc/dbus-1/system.d/

# polkit scans only /usr/share/polkit-1/actions, which is read-only on
# immutable systems. The action gates the privileged pairing helper, which
# touch-to-pair never calls — so shipping without it loses nothing today,
# and the symlink is best effort.
if [ -d /usr/share/polkit-1/actions ]; then
    ln -sf /usr/local/share/polkit-1/actions/io.github.gonzalezerik.Px13Pen.policy \
        /usr/share/polkit-1/actions/io.github.gonzalezerik.Px13Pen.policy ||
        echo "install.sh: /usr/share/polkit-1/actions is read-only; the pairing helper stays unauthorised (touch-to-pair does not use it)" >&2
fi

udevadm control --reload-rules
udevadm trigger --subsystem-match=hidraw --action=change

# Apply the shipped preset: px13pend and the tray start with every login.
systemctl --global preset px13pend.service px13pen-tray.service

echo
echo "Installed. Sign out and back in: px13pend starts with the session, the"
echo "tray icon appears when a pen is live, and touching a pen to the screen"
echo "pairs it. Check any time with:  px13penctl status"
