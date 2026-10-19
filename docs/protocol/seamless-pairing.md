# Seamless pairing — ELAN digitizer ↔ MPP haptic pen over Bluetooth

Status: **VERIFIED-HW on Linux** (2026-10-18/19): touch-to-pair works on the ASUS ProArt PX13 with
the ASUS Pen 3.0 SA205H, stock BlueZ and a stock kernel, for a pen that already knows the host and
for a host the pen has never paired with. The daemon (`px13pend`) does it on its own, from a cold
pen, in about 0.7 s from the announcement to bonded, as fast as Windows and with the same single
step for the user (findings/008 §10). Evidence and the full test record: `findings/008`. Earlier
Windows captures: `findings/006` §2–3. Confidence labels per CONTRIBUTING.md §5.

The mechanism is Microsoft's published **Seamless Pairing for Windows Pens Implementation Guide**
(learn.microsoft.com, Windows hardware component guidelines). It is pen-protocol agnostic: the pen
gives its Bluetooth address to the digitizer over the pen protocol (MPP 2.6 here), and the digitizer
exposes a small HID collection the host uses to pair. Field names below are the guide's.

## 1. The HID collection [STATIC + DOC]

Vendor top-level collection **Usage Page 0xFFF4, Usage 0x01**, a sibling of the pen collection
(`fixtures/px13-hn7306/elan9008-04f3-4631.hid-decode.txt`, lines 499–600; matches the guide's
sample descriptor).

| Report | Guide name | Direction | Fields |
|---|---|---|---|
| `0x70` | Device Capability Report | feature, both ways | Seamless Pair Capable (usage 0x10, 1 bit) |
| `0x56` | Host Bluetooth Report | feature, set by the host | Host Bluetooth Address (0x03, 6 bytes) + Host Address Type (0x09, 1 bit: 0 public) |
| `0x6E` | Device Bluetooth Report | input | Device Bluetooth Address (0x02, 6 bytes) + Device Address Type (0x08: 1 = random) + **Host Pairing Exists** (0x11) |
| `0x6F` | Pairing Key Report | output | Device Bluetooth Address (6) + type (1 bit) + **Host Pairing Key** (0x14, 16 bytes) |

All three address fields are **little-endian** on the wire (VERIFIED-HW for `0x6E` and `0x56`).
`0x56` is set-only in practice: a GET returns no data, on Windows and on Linux.

The digitizer's INF opts the collection in by setting the device property
`DEVPKEY_BluetoothOobCouplingChannel_OptInForUse` `{FE683ED0-DE80-4766-B422-4195993F4330},2 = 1`
on the HID interface, exactly as the guide prescribes. On Windows an inbox service opens the
collection; no ASUS software is involved (findings/002, findings/006).

## 2. The flow [DOC, VERIFIED-HW where marked]

1. **Enumeration.** The host sets `0x56` to its own LE address and type, sets `0x70 = 1` (host
   capable) and reads `0x70` back (digitizer capable: `01`). [VERIFIED-HW]
2. **Pen in range.** Once a capable pen is in range (hover is enough) and the digitizer knows whether
   the pen already has a pairing with the registered host, it sends `0x6E`. [VERIFIED-HW: flag
   `0x01` before any host registration; `0x03` after registering an address the pen was bonded to;
   `0x01` for a fresh host identity.] The guide says "one time"; this digitizer repeats it every few
   seconds while the pen has no link.
3. **Key.** The host writes `0x6F` with the announced address and a temporary key. The guide expects
   this only when no pairing exists; on this pen the key report is also what makes a pen *with* an
   existing pairing advertise, so a host should always send it. [VERIFIED-HW]
4. **Advertisement.** The pen advertises within milliseconds of the key: connectable `ADV_IND`, name
   "ASUS Pen", appearance `0x03C7`, HID service `0x1812`, from the **identity it bonds under for that
   host**, which is not necessarily the announced address. The burst is short (tens of
   milliseconds): the host's scan must already be running. [VERIFIED-HW] The guide describes a
   directed advertisement; this pen sent an undirected one in every run.
5. **Pairing.** Windows pairs out-of-band with the key from step 3. **Linux pairs with passkey
   entry and passkey 0** (§3), the closest method the stock kernel offers. [VERIFIED-HW for Linux;
   Windows: LE legacy SMP observed, OOB inferred from the guide]
6. **Afterwards** the pen reports Host Pairing Exists for this host and reconnects on its own when
   used; BlueZ treats it as a normal bonded, trusted HID device. [VERIFIED-HW]

## 3. Pairing on Linux: passkey 0, and why

The pen answers every Pairing Request with IO capability **KeyboardOnly**, OOB data **present** and
**Bonding + MITM + Legacy**. Its choices leave the host three methods (findings/008 §5):

- **Just Works** (host NoInputNoOutput): refused, `Pairing Failed: Authentication requirements`.
  The pen insists on MITM protection.
- **Out-of-band** with the `0x6F` key: what Windows does. It needs the host to set its OOB flag and
  use that key as the temporary key. **The Linux kernel cannot do this for LE legacy pairing** —
  `net/bluetooth/smp.c` uses out-of-band data only for Secure Connections, and the management
  interface rejects a legacy key for LE addresses ("Until legacy SMP OOB pairing is implemented",
  commit `d25b78e2ed`; BlueZ `doc/mgmt-protocol.rst`: "Currently there is no support for providing
  the Security Manager TK Value for LE legacy pairing"). The pen is legacy-only, so the supported
  Secure Connections path does not apply.
- **Passkey entry** (host KeyboardOnly): both sides "type" a six-digit passkey that becomes the
  temporary key. The pen has nothing to type with and enters **0**, whatever `0x6F` carried. A host
  that enters 0 as well bonds; any other value fails with `Confirm value failed`.

So the Linux host pairs with passkey 0 and sends a key of zeros. The pairing counts as authenticated
in protocol terms, and the user sees exactly what Windows users see: touch, and the pen is paired.
**The difference is protection of the pairing moment**: with a fixed temporary key, a passive
eavesdropper recording that pairing could derive the link key; with Windows' random out-of-band key
it could not. The fix belongs in the kernel and is noted in `docs/future/kernel-le-legacy-oob.md`;
once a kernel has it, the host sends a random key and pairs out-of-band with no change for users.

## 4. Pen behaviour a host has to handle [VERIFIED-HW]

- **Several identities.** Announced address, per-host bond identity and button-pairing-mode identity
  can all differ, and change after new bonds. Connect to the advertiser that appears after the key,
  not to the announced address.
- **Wake-up advertisement.** A pen that holds a pairing for this host advertises by itself as soon
  as it wakes (picked up, or its link dropped), before the digitizer announces it; after a dropped
  link it keeps advertising for about 40 s. A host that already sees that advertiser must pair it
  rather than forget it as a leftover: pairing an object the stack knows makes the controller wait
  for the pen's next advertisement itself, which catches the short burst that follows the key.
- **Back-off.** After a failed pairing or an advertisement nobody connected to, the pen often skips
  the next touch within about two minutes. Keep the scan armed and react to the next announcement.
- **Already connected.** The digitizer keeps announcing a pen in range even while that pen is
  connected under another identity, so "already connected" must be decided by something stable (the
  transducer serial, present on both links), not by the announced address.
- **Button pairing mode** (hold the top button until the LED blinks white) makes the pen advertise
  under a fresh identity any host can pair with ordinary BlueZ. Manual fallback only.

## 5. What a host must not do

Only the four reports above are written, and only with the contents the guide defines. The
collection's other vendor reports and the firmware-update collection stay untouched
(`docs/protocol/digitizer-tlc.md`, CONTRIBUTING.md §6).

## 6. Implementation status

- `tools/capture/linux/06-seamless-pairing.py` runs the whole flow against the installed package
  (BlueZ through an interactive `bluetoothctl` with a KeyboardOnly agent). It is the reference.
- The daemon runs the same flow (`crates/px13pend/src/pairing.rs`): `px13pend serve` registers the host
  when it opens the digitizer, starts an LE scan for 20 s whenever report 7 shows the pen in range
  and no pen is connected, writes the key report on each announcement, pairs the first unpaired
  pen-looking device that advertises (appearance `0x03C7` or "Pen" in the name) through its own
  KeyboardOnly agent, marks it trusted and notifies at once. A paired pen that does not reconnect
  within 3 s of an announcement is treated as a stale bond and paired fresh. `px13pend watch-pair`
  does the same in the foreground.
- **Use your own agent.** BlueZ sends pairing questions to the agent of the client that started
  the pairing, and otherwise to the session's default agent — on a desktop that is usually the
  shell's (KeyboardDisplay, with passkey and approve/deny popups, and requests rejected while
  another is pending). A default agent silently turns the method into "host displays a random
  passkey", which this pen cannot match. The pairing client must register its own KeyboardOnly
  agent that answers the passkey request with 0, so touch-to-pair shows no prompt at all.

## 7. How the model got here

Earlier versions of this document, written from the descriptor and Windows traces alone, guessed
that the 16-byte field was a link key written back after pairing, that the second flag bit meant a
Windows consumer was listening, that the host connects straight to the announced address, and that
plain Just Works might be enough. The public guide and the Linux runs replaced all four: the field
is a temporary key sent *before* pairing, the bit is Host Pairing Exists, the host connects to
whichever identity advertises, and the pen requires MITM protection. findings/006 keeps the Windows
evidence; findings/008 the Linux runs.
