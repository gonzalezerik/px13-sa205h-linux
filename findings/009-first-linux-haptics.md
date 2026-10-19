# Finding 009 — first haptics sent from Linux: the route works, and the pen's stop is ordinal 1

Date: 2026-10-19 · Confidence: **VERIFIED-HW** · Method: one instrumented Linux session on the test
machine (ASUS ProArt PX13, ELAN9008 `04F3:4631` I2C digitizer, ASUS Pen 3.0-class BLE pen, HID VID
`0x330A` PID `0x0001`). Output reports were written to the pen's `hidraw` node with
`px13pend haptic … --send` (kernel HID → uhid → BlueZ HID → GATT write), and the maintainer
reported what was felt, per send. This is the Linux counterpart of the Windows direct-send panel in
findings/006 §4; per-device identifiers are referred to by role only.

## 1. The output route works [VERIFIED-HW]

`41 01 FF 03 00 00 00` (one click at full intensity), written to the pen's `hidraw` node while the
pen held a Bluetooth link, was **felt as a click**. The uhid → BlueZ → GATT output path carries
haptic output reports end to end — the question left open in findings/006 §4 (the layout was
verified on Windows, the Linux transport was hypothesis) is closed.

Operational notes from the session:

- The pen must hold a live link: it drops BLE when idle and reconnects on use, so a wake touch
  (tip to screen, in range) immediately before a send is required. The `hidraw` node for the pen
  exists only while the link is up.
- The `hidraw` write permission question from the run plan never came up: the installed uaccess
  rule covers the pen's `0005:330A:0001` node and the write was accepted on the first try.
- An active LE scan was **not** needed to bring the link back: the existing bond plus a wake touch
  was enough (BlueZ reconnected the trusted bonded device on its own).

## 2. Intensity byte, confirmed on Linux [VERIFIED-HW]

`41 01 40 07 00 00 00` (success, intensity byte `0x40` = 64/255, the 25 % slider byte) was felt
and described as **"weak but felt"** — perceptibly weaker than the full-intensity click. The
`byte ≈ slider/100 × 255` mapping from findings/006 §4 holds; low bytes are usable, not silent.

## 3. Stop is ordinal 1 (None); ordinal 2 does nothing [VERIFIED-HW]

The implicit **Stop ordinal 2 is inert on this firmware**. It was sent twice against an actively
playing continuous texture — once with a 2 s cutoff armed, once with no cutoff — while the tip was
in contact, and the texture did not cut either time ("no" both times).

The implicit **None ordinal 1 is the real stop**: `41 01 FF 01 00 00 00`, sent 3 s into a stroke
with an ink texture playing (no cutoff), **cut the texture mid-stroke**, and a fresh touch after it
was silent (normal glass feel, no texture). So ordinal 1 both stops the current playback and
disarms the pen until the next write.

Consequence for the daemon: the `Haptics1.Stop` method must write **ordinal 1**, not ordinal 2
(docs/dbus.md §7 said ordinal 2, following the HID spec's naming; the device disagrees).

## 4. One write arms a continuous texture; the pen gates on tip contact [VERIFIED-HW]

A single write of a continuous waveform (ink, `41 01 FF 09 …`) stays armed: the maintainer drew a
stroke, lifted for ~2 s, and touched again **without any new write** — the texture came back on the
second touch, past the 2 s cutoff value that had been armed. The pen keeps the waveform and gates
playback on the tip switch across separate contacts, exactly the auto-trigger model of
docs/protocol/haptics.md §1/§2.7.4, now observed from Linux.

So a host **arms once per tool activation** (and re-arms when the tool/waveform changes) and does
**not** need to re-send per stroke. Windows apps re-sending per stroke (OneNote's ~100 writes per
drawing, findings/006 §4) is app behaviour, not a device requirement. Disarm is the ordinal-1 write
of §3.

Not cleanly separated: whether the cutoff field ends a texture *within* one continuous contact (the
maintainer could not say when it ended mid-stroke). The daemon design does not depend on it — we
arm without a cutoff and disarm explicitly.

## 5. Consequences for the build

1. `px13pend`'s while-inking arming: write the tool's continuous waveform once when the pen comes
   into range (or the tool changes), nothing per stroke; write ordinal 1 when inking ends or the
   setting turns off.
2. `Haptics1.Stop` → ordinal 1. The interface contract ("the stop command") is unchanged; only the
   byte behind it and the doc sentence change.
3. Intensity: the settings slider maps `round(percent × 255 / 100)` with no floor needed — 25 % is
   felt.
4. Parity rows 5a–5e (docs/parity.md) can move forward: the wire path is proven on Linux; what
   remains is the policy layer (settings, arming, off = no writes).
