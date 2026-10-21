# Glossary

Words this wiki uses constantly, in the sense it uses them. General HID and
Bluetooth background lives in the specifications cited on the
[sources](sources.md) page.

**Active pen / MPP** — the pen is not a passive stylus: it talks to the
touch sensor over 2.4 GHz using Microsoft's Pen Protocol (MPP 2.6 on this
pen). The OS never sees that protocol; the digitizer controller translates.

**Announcement** — input report `0x6E` from the digitizer naming a pen in
range and its Bluetooth address; the start of touch-to-pair.
[VERIFIED-HW findings/006, findings/008]

**ATT / GATT** — the Bluetooth layers under which the pen's services are
read and written; ATT handles are numbered slots (e.g. the battery
notification arrives on one fixed handle), services group characteristics.

**Barrel switch** — the pen's side button, reported on the *screen link*
(digitizer report 7), not over Bluetooth. [VERIFIED-HW findings/006 §5]

**Bond / pairing** — the stored keys that let a pen reconnect without user
action. This pen requires authenticated (MITM-protected) pairing and does
LE legacy SMP only. [VERIFIED-HW findings/008]

**Collection / TLC** — HID top-level collection: one report-descriptor
subtree = one reported device-ish thing. The digitizer exposes the Touch
Screen, Pen, and several vendor collections; the pen's Bluetooth side
exposes a keyboard-style collection, the HID service, and a Simple Haptic
Controller.

**Debug log** — the pen firmware's own serial console, streamed over the
Nordic UART service's TX characteristic: fuel gauge, current, wake reasons,
charger sequence. See [battery](flows/battery.md). [VERIFIED-HW findings/010]

**Digitizer controller** — the laptop-side chip receiving the pen's radio
bursts and reporting over I2C-HID: an ELAN9008, USB identity `04F3:4631`.

**HoGAT** — HID over GATT: the Bluetooth profile making a LE device act as
a keyboard/stylus. The pen's buttons, haptics and feature reports ride it.

**Hover / In Range** — the pen is close enough for the digitizer to report
it (`In Range` bit, report 7) without the tip touching. Hover is an
informational state; the kernel's stylus node just reports proximity.
[VERIFIED-HW findings/006 §3]

**HAPTICS ordinals** — waveforms are played by *ordinal index* into the
pen's waveform table (1 = None, 2 = Stop, 3+ from the Waveform List), not
by waveform ID. On this firmware ordinal 2 is inert; 1 is the real stop.
[VERIFIED-HW findings/009]

**Identity / transducer serial** — the 32-bit number every pen reports on
both links (digitizer reports 7 and 34, pen Bluetooth feature `0x40`, and —
on Linux — the plain readable characteristic standing in for it). It is how
one physical pen is recognised across its several Bluetooth addresses.
[VERIFIED-HW findings/006 §2–3]

**I2C-HID / hid-multitouch** — the kernel's path from the digitizer's
report descriptor to input events; `hid-multitouch` binds this device.

**hidraw** — the kernel's raw-HID character device; the only interface
`px13pend` uses on the digitizer, and the write path for Bluetooth output
reports (kernel → uhid → BlueZ → GATT).

**MITM protection** — pairing that a man in the middle cannot slip through;
this pen refuses Just Works. On Linux the method available is passkey entry
with passkey 0, which authenticates the exchange but leaves the pairing
moment exposed to a passive eavesdropper.
[VERIFIED-HW findings/008; docs/protocol/seamless-pairing.md §3]

**OOB (out-of-band)** — passing pairing keys over a channel other than the
radio. Windows pairs this pen OOB through the digitizer; the Linux kernel
cannot do LE-legacy OOB yet, hence passkey 0.

**Report ID** — the first byte of a HID frame selecting which report
follows; offsets throughout this wiki count it as byte 0.

**Seamless pairing / touch-to-pair** — Microsoft's published flow in which
the digitizer hands the host the pen's Bluetooth address while the pen is
merely in range, so touching the screen is the whole pairing gesture.
[DOC: Microsoft's implementation guide; VERIFIED-HW on Linux findings/008]

**Touch-to-pair flag** — the byte in `0x6E` whose `Host Pairing Exists`
bit says whether the pen already knows the registered host
(`0x03` = yes, `0x01` = no/none registered). [VERIFIED-HW findings/008]

**Transducer** — the pen-side resonant circuit the digitizer powers and
reads; "transducer serial" is the pen's serial number.

**uaccess** — the udev mechanism giving the logged-in user access to a
device node; the project ships rules making the digitizer and pen
readable/writable without root.

**UHID** — the kernel interface that lets userspace (here BlueZ's input
plugin) present a Bluetooth HID device as a local HID device; the reason
writing a report to the pen's hidraw node reaches the pen.
[STATIC findings/001 §4; VERIFIED-HW findings/009]

**Vendor TLC names** — vendor collections are named by usage page:
`0x01FF` (maintenance response), `0xFF00` (maintenance command, R2/R3),
`0xFF0B` (block/flash pattern — untouchable), `0xFF0F` (small command pair +
telemetry R34), `0xFFF4` (the seamless-pairing collection).

**WoT** — Wake-on-Touch: the ELAN service (`ElanTouchXiSvc.exe`) that arms
the digitizer to wake the machine; on this machine it is also the living
example of the `0xFF00` command channel's use. [STATIC findings/005]

**Waveform ID** — the HID haptics page's name for an effect (`0x1003`
Click, `0x100B` InkContinuous…); what a device's Waveform List is made of;
distinct from the ordinal that plays it.
