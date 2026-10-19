# 008 — Touch-to-pair on Linux: from "the pen never answers" to bonded in 0.7 s

Date: 2026-10-18. Hardware: ASUS ProArt PX13 (HN7306), ELAN9008 digitizer `04F3:4631`, ASUS Pen
3.0 SA205H, MediaTek Bluetooth controller, BlueZ 5.87, Linux 7.2. All runs on the target laptop
with the package installed (uaccess rule, user daemon, pairing helper). Pen addresses are written
by role, never by value:

- **announced address**: what input report `0x6E` carries;
- **bond identity**: the address the pen advertises and bonds under for a given host;
- **pairing-mode identity**: the address the pen uses after its button pairing mode.

Confidence labels per CONTRIBUTING.md §5. Everything below is VERIFIED-HW unless marked.

## Summary

Seamless (touch-to-pair) pairing is Microsoft's published *Seamless Pairing for Windows Pens*
protocol. A Linux host can run it with stock BlueZ and a stock kernel: register the host with the
digitizer, hand over a pairing key when the pen is announced, catch the pen's advertisement, and
pair. Both cases work — a pen that already knows this host, and a pen meeting a host it has never
paired with (tested with a second host identity) — in about 0.7 s from touch to bonded, the same as
the Windows measurement (findings/006 §2).

The one gap to Windows: the pairing step itself. Windows pairs out-of-band with the 128-bit key it
sent through the digitizer. The Linux kernel has no LE legacy out-of-band support, so the host pairs
with passkey entry instead, and the pen, having nothing to type with, uses passkey 0. That pairing is
authenticated in the protocol's terms (the pen accepts it as MITM-protected) but its temporary key is
public, so the pairing moment is not protected against an eavesdropper. See §6.

## 1. The protocol has a public specification

The digitizer's INF names the collection's device property `DEVPKEY_BluetoothOobCouplingChannel_OptIn`.
Searching that name leads to Microsoft's *Seamless Pairing for Windows Pens Implementation Guide*
(learn.microsoft.com, Windows hardware design guidelines). It defines every report of the `0xFFF4`
collection that `docs/protocol/digitizer-tlc.md` mapped from the descriptor, and the sequence:

| Report | Name in the guide | Direction | Content |
|---|---|---|---|
| `0x70` | Device Capability Report | feature, both ways | Seamless Pair Capable (1 bit) |
| `0x56` | Host Bluetooth Report | feature, host → digitizer | Host Bluetooth Address (6 bytes) + type (0 public) |
| `0x6E` | Device Bluetooth Report | input | Device Bluetooth Address (6) + type bit + **Host Pairing Exists** bit |
| `0x6F` | Pairing Key Report | output | Device address (6) + type + 128-bit temporary key |

Sequence in the guide: on enumeration the host sets `0x56` and exchanges `0x70`; when a capable pen
comes in range the digitizer sends `0x6E` once; if no pairing with this host exists, the host sends a
fresh key in `0x6F` and waits up to 30 s; the pen sends a connectable directed advertisement to the
host, and the host pairs out-of-band with that key. The guide is the source for the field names used
in the project from now on. This replaces the earlier hypotheses (the 16-byte field is a temporary
key, not a link key; the second flag bit is Host Pairing Exists, not a consumer acknowledgement).

## 2. What a bare Linux host saw before (why nothing ever connected)

- `0x6E` arrives without any host action, on hover as well as touch (findings/006 §3), with flag
  `0x01`: random address, **no pairing with this host**. "This host" is whatever address was last set
  in `0x56`, and nobody had set it.
- Asking the kernel to pair the announced address (management *Pair Device*) only makes it scan for
  that address. The pen never advertised: no host had registered and no key had arrived. Every attempt
  timed out.
- Feature `0x56` cannot be read back (0 bytes), on Linux as on Windows: it is set-only in practice.

## 3. Registering the host changes the digitizer's answer

After `SET FEATURE 0x56` with the adapter's public address (little-endian, as in `0x6E`) and type 0,
and `SET FEATURE 0x70 = 01`, the next `0x6E` for the same pen carried flag **`0x03`**: Host Pairing
Exists. The pen still held a bond for this controller address from an earlier Windows pairing, which
the guide's state machine reports exactly that way. Little-endian was right on the first try.

## 4. The key report is what makes the pen advertise

| Run | `0x56` / `0x70` set | `0x6F` written | Pen advertised |
|---|---|---|---|
| 1 | yes | no | no (in 20 s) |
| 2 | yes | yes | **yes**, within 1 ms of the key |
| 3 | yes | no | no |
| later runs | yes | yes | yes, 2–200 ms after the key, unless in back-off (§7) |

The advertisement is a connectable, scannable `ADV_IND` (not directed, unlike the guide's
description), LE General Discoverable, local name "ASUS Pen", appearance `0x03C7` (pen), HID service
`0x1812`. It comes from the pen's **bond identity**, not the announced address, so a host that only
waits for the announced address never sees it. The burst is short: one run caught six reports within
72 ms, then nothing. **A scan has to be running before the pen is touched**; starting one after the
announcement (process start-up plus discovery start, 100–200 ms) misses most bursts.

## 5. Pairing: which method the pen accepts

The pen's SMP Pairing Response, in every run: IO capability **KeyboardOnly**, OOB data **present**
(it holds the key from `0x6F`), authentication requirements **Bonding, MITM, Legacy**.

| Host side | Method that results | Outcome |
|---|---|---|
| KeyboardDisplay (desktop agent), no OOB | passkey entry, host displays a random passkey | `Confirm value failed` |
| NoInputNoOutput, no OOB | Just Works | pen sends `Pairing Failed: Authentication requirements` — it insists on MITM |
| KeyboardOnly, no OOB, passkey **0** entered | passkey entry, both sides "type" | **bonded**, encryption on, full key distribution |
| KeyboardOnly, no OOB, random passkey (key in `0x6F` set to the same value, either byte order) | passkey entry | `Confirm value failed` |

So in passkey entry the pen always enters 0, whatever key `0x6F` carried: the key is only used by
the out-of-band method, which needs the host to set its OOB flag too. Linux cannot (§6).

The successful run, measured from the announcement: key written +2 ms, advertisement seen +6 ms, passkey
requested +230 ms, encryption on +700 ms, bonded +1.2 s including BlueZ's own steps. The pen's
connection drops after pairing as it does when idle; BlueZ reconnects it on use.

## 6. Why Linux cannot pair out-of-band (and what would fix it)

- The pen is legacy-only (no Secure Connections bit), so LE Secure Connections OOB, which the
  kernel supports, is not an option.
- The kernel's SMP code (`net/bluetooth/smp.c`, `build_pairing_cmd()`) sets the OOB flag only on the
  Secure Connections path, and `tk_request()` never takes a temporary key from outside for legacy
  pairing.
- BlueZ's management protocol documents it directly (`doc/mgmt-protocol.rst`, Add Remote Out Of Band
  Data): "Currently there is no support for providing the Security Manager TK Value for LE legacy
  pairing." The kernel rejects non-zero `Hash_192`/`Randomizer_192` for LE addresses since commit
  `d25b78e2ed` (2015), whose message reads "Until legacy SMP OOB pairing is implemented user space
  should be given a clear error". The slot is reserved; the feature was never finished.

A host-side workaround that took the pairing out of the kernel would need exclusive control of the
controller (HCI user channel), disconnecting every other Bluetooth device and leaving no supported
way to hand the keys to BlueZ. Not acceptable for a package that must just work. The proper fix is
the kernel feature itself; it is noted as future work in `docs/future/kernel-le-legacy-oob.md`.

## 7. Pen behaviour worth knowing

- **Identities per host.** The pen kept separate addresses: one announced by the digitizer, one it
  advertised and bonded under for a host it already knew, another after button pairing mode, and a
  new one after bonding with the second test identity. The announced address changed over the day
  as well. Never assume the announced address is the one to connect to; match the advertiser.
- **Wake-up advertisement.** With Host Pairing Exists set, the pen advertised from its bond identity
  the moment it woke, before the digitizer's announcement: 4.4 s ahead in one run (picked up away
  from the screen), 15 ms after first hover in another (picked up after 18 min untouched). After the
  host dropped its link it advertised continuously (about 12 reports per second) for 40 s. In a run
  where the host removed that fresh device object before sending the key, the pen did not answer
  the key. Pairing the existing object instead (the controller then waits for the next
  advertisement) bonded in 0.7 s on the first touch, three runs out of three.
- **Back-off.** After a failed pairing or an advertisement nobody connected to, the next touch within
  about two minutes often produced no advertisement; a later touch did. A daemon should keep its scan
  armed and simply react to the next announcement.
- **Button pairing mode.** Holding the top button until the LED blinks white makes the pen advertise
  "ASUS Pen" under a fresh identity that any host can pair with ordinary BlueZ (scan, pair, trust).
  It worked on the first try and is the manual fallback; seamless pairing needs no button.
- **The announcement repeats** every few seconds while the pen is in range without a link, even while
  the same pen is connected under another identity. Deciding "this pen is already connected" has to
  use something other than the announced address (the transducer serial is on both links).

## 8. A brand-new host (the case of a new pen)

To test a host the pen has never paired with, the controller was given a random static LE identity
(management *Set Static Address* plus the kernel's `force_static_address` debug switch, while powered
off) and that address was registered in `0x56` with type 1. The next `0x6E` carried flag **`0x01`**:
Host Pairing Exists false. Key written, advertisement 1 ms later, passkey 0, bonded in 0.7 s,
connection made from the random identity. The controller was restored to its public identity
afterwards and the test bond removed. This is the path a new pen takes on a fresh install.

## 9. Reproducing it

`tools/capture/linux/06-seamless-pairing.py` runs §3–§5 end to end against the installed package and
records btmon, the bluetoothctl session and a timestamped log. `SEAMLESS_HOST`/`SEAMLESS_HOST_TYPE`
repeat §8.
