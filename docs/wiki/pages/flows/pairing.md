# Flow — touch-to-pair (seamless pairing)

The pen never advertises on its own (43 s of scanning saw nothing from it —
a measured negative, findings/006 §7.5). Touch-to-pair is the door: the
digitizer hands the host the pen's Bluetooth address while the pen is merely
in range, and touching the screen completes pairing in about 0.6–0.7 s on
both operating systems. [DOC: Microsoft's Seamless Pairing guide;
VERIFIED-HW on Linux: findings/008]

The 0xFFF4 collection's four reports are on the
[digitizer reference](../reference/digitizer.md)'s sibling page and in
`docs/protocol/seamless-pairing.md` §1 with every field's usage and offset.
All addresses on this channel are little-endian [VERIFIED-HW].

## The sequence

Text sequence listing; times from one instrumented Windows session
(findings/006 §2 — single local clock), behaviour identical on Linux
(findings/008):

```
 PEN          DIGITIZER              HOST (adapter + stack)
  |               |                        |
  |               |◄─ feature 0x56 ────────┤  at start: host LE address + type
  |               |◄─ feature 0x70 = 1 ────┤  "host is pairing-capable"
  |               |── 0x70 = 01 ──────────►│  digitizer capable
  |             (pen hovers or touches)    |
  |═══ MPP bursts ═►|                      |
  |               |── input 0x6E ─────────►│  +0.10–0.15 s: pen address +
  |               |                        |   flag byte (Host Pairing Exists)
  |               |                        |  host scan must already be armed;
  |               |                        |  stack moves on the report (+0.15)
  |◄───────────────────────────────────────┤  +0.25: accept-list entry for the
  |               |                        │   announced address, random type
  |◄─ connect ─────────────────────────────┤  +0.37: LE connection + legacy SMP
  |── key? ──(OOB on Windows / passkey 0 on Linux)──►
  |               |◄─ output 0x6F ─────────┤  host writes pen address + key
  |── advertises (identity per host!) ────►│  within ms of the key; name
  |               |                        │   "ASUS Pen", appearance 0x03C7
  |═══ bonded ═════════════════════════════│  +0.61: encryption on — done
```

`0x6E` repeats every few seconds while the pen holds no link — the guide
says one announcement; this digitizer repeats, and Linux uses that
(findings/008).

## Why Linux pairs with passkey 0 {#why-linux-pairs-with-passkey-0}

[VERIFIED-HW findings/008 §5]

The pen's Pairing Request: IO capability KeyboardOnly, OOB flag present,
Bonding + MITM + **legacy only**. That leaves:

- **Just Works** — refused (pen insists on MITM).
- **OOB with the `0x6F` key** — what Windows does; the Linux kernel cannot
  use OOB keys for LE *legacy* SMP (`net/bluetooth/smp.c`; BlueZ's mgmt API
  rejects legacy keys for LE addresses), and the pen does not speak Secure
  Connections.
- **Passkey entry** — the pen can type nothing, so it enters **0**. The host
  enters 0 too and bonds authenticated. Same user experience as Windows;
  the difference is that a passive eavesdropper recording the pairing could
  derive the link key (fixed temporary key). The kernel gap is
  `docs/future/kernel-le-legacy-oob.md`.

A host must pair with **its own** KeyboardOnly BlueZ agent answering 0 —
the desktop's default agent changes the method into something the pen
cannot match (seamless-pairing.md §6).

## Pen behaviour a host must handle [VERIFIED-HW findings/008 §4]

- **Several identities**: announced address, per-host bond identity and
  button-pairing identity can differ and change; connect to whoever
  advertises after the key, never to the announced address.
- **Wake-up advertisement**: a pen holding a pairing for this host
  advertises on wake (link drop or pickup) and keeps doing it ~40 s.
- **Back-off**: after a failed pairing the pen often skips the next touch
  for ~2 minutes — keep the scan armed.
- **Already connected**: announcements keep coming even while the pen is
  connected under another identity — decide by the **transducer serial**
  (reports 7/34 vs the connected pen's `0x40`), not by address.

## Status on Linux

`px13pend` runs the whole flow itself, touch-to-bonded ~0.7 s, including a
pen that forgot nothing and a pen never seen (findings/008 §10, §8).
The reference implementation is `tools/capture/linux/06-seamless-pairing.py`.
Honest limits: the daemon's own runs were all with a pen already holding
this host's pairing; the from-nothing case is proven with the reference
script, and the daemon path for it awaits a second pen or a safe way to
make the pen forget (seamless-pairing.md §6).

## What a host must never do

The guide defines four reports; only they are touched, and the only write
is `0x6F` with exactly its fields. Every other digitizer report set,
including the firmware-shaped `0xFF0B`, stays untouched
([digitizer reference](../reference/digitizer.md#vendor-collections-and-the-safety-buckets)).
