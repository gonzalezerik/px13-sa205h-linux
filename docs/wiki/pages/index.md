# The PX13 SA205H Linux wiki

How the ASUS Pen 3.0 (SA205H) and the ASUS ProArt PX13's ELAN digitizer work,
how that was found out, and how this project's `px13pend` daemon puts it to
use on Linux. Everything here is written from this project's
findings and driver analysis; the evidence lives in the repository's
`findings/` and `docs/protocol/` files, cited by number throughout.

## Choose your path

**"Does it work on Linux, and how do I get it?"** — the
[devices page](devices.md) pins down exactly which hardware every claim
here covers; for what runs on it today, the repository's README status
section and `docs/parity.md` are the live lists, `docs/packaging.md`
covers installation (NixOS module, udev, D-Bus activation), and
`docs/install.md` walks owners of other distros through the prebuilt
glibc and musl tarballs.

**"How does the pen actually work?"** — start with the two links in the
[Bluetooth reference](reference/bluetooth.md) and the
[digitizer reference](reference/digitizer.md), then read the flows:
[pairing](flows/pairing.md), [haptics](flows/haptics.md),
[buttons](flows/buttons.md), [battery](flows/battery.md),
[reconnect](flows/reconnect.md).

**"I want to reproduce a measurement or capture my own."** — the
[capture recipes](capturing.md) list the exact tools and the order they go
in; the [conventions page](conventions.md) defines the labels that say how
much to trust any given claim.

**"I'm changing this software and need the Windows behaviour."** — the
[Windows host-stack map](windows-map.md) says which Windows component owns
what, and [open questions](open-questions.md) lists what nobody has proven
yet.

**"I have a different pen-capable laptop."** — everything here was measured
on one machine and one pen; the
[other-hardware guide](other-hardware.md) walks you through read-only
checks that tell you what your pair actually exposes, whether a daemon
like this could ever speak to it, and how to report your hardware (or
take the work on yourself) if it can.

## What this wiki is not

It is documentation for one laptop and one pen, reverse-engineered by one
maintainer — not a general MPP or HID reference, and not ASUS- or
Microsoft-endorsed anything. Where a fact is a guess, it is labelled as one;
where an example value appears, it is synthetic.
