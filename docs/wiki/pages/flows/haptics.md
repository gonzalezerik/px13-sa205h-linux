# Flow — haptics

Haptics ride the **Bluetooth link only** (the screen link carries no buzz).
The whole model fits in one sentence: the host **writes one 7-byte output
report** to arm an effect, **the pen decides when to play it** (continuous
textures gate on tip contact), and **writing ordinal 1 stops and disarms**.
[VERIFIED-HW — findings/006 §4, findings/009; docs/protocol/haptics.md]

The byte layout and the feature reports are on the
[Bluetooth reference](../reference/bluetooth.md); this page is the behaviour.

## The sequence

```
 HOST                                  PEN
  │── 41 01 FF 09 00 00 00 ─────────────►│  arm ink texture (ordinal 9), once
  │        (pen holds the configuration; │
  │         nothing plays in the air)    │
  │        tip touches, stroke ─────────►│  "writing on paper" — auto-trigger
  │        tip lifts, ~2 s               │     (feature 0x42: Tip Switch)
  │        tip touches again ───────────►│  texture again, no new write,
  │                                      │   past the armed cutoff value
  │── 41 01 FF 01 00 00 00 ─────────────►│  ordinal 1 (None): cuts an active
  │                                      │   texture mid-stroke AND disarms —
  │        next touch = bare glass       │   the next write re-arms
```

One write per effect; one write per **tool activation**, not per stroke —
OneNote's ~100 writes per drawing on Windows is app behaviour, not a device
requirement (findings/006 §4 vs findings/009 §4). [VERIFIED-HW]

## What this pen supports [VERIFIED-HW findings/006 §4]

Feature `0x42` says (waveform list → ordinals 3–15):

| Ordinal | Waveform ID | Effect | Duration (firmware) |
|---|---|---|---|
| 1 | (implicit None) | the real stop + disarm on this firmware | — |
| 2 | (implicit Stop) | **inert** — sent twice against a playing texture, cut nothing | — |
| 3 | 0x1003 | Click | 45 ms |
| 4/5 | 0x1006/0x1007 | Press / Release | 60 / 60 ms |
| 6 | 0x1008 | Hover | 35 ms |
| 7/8 | 0x1009/0x100A | Success / Error | 145 / 185 ms |
| 9–15 | 0x100B–0x1011 | continuous Ink, Pencil, Marker, Chisel, Brush, Eraser, Sparkle | ≈1 980–1 995 ms |

All 13 were felt and are subjectively distinct (pencil scratchiest and
audible, brush smoothest, sparkle "rubbery"). The pen has **no
Collide/Align/Step/Grow**, so Windows' snap-target buzzes exercise its
Click fallback on real hardware [STATIC+V-HW haptics.md §2.4/§2.7].
Continuous durations are ≈2 s, not the spec's 0 [DOC divergence, observed].

## Verified field values [VERIFIED-HW §2.9]

One row fits every felt send: `repeat 01 · intensity FF×user setting ·
effect's own ordinal · cutoff 00 · retrigger 0000`. The intensity byte
scales with the user slider (`round(pct × 255 / 100)`; the 25 % byte
`0x40` is felt, "weak but felt" — no floor needed). Windows' own per-effect
repeat/cutoff/retrigger values are **unrecoverable from the captures** —
the Windows Bluetooth logger truncates ACL payloads to ≈11 bytes, so haptic
writes were logged as a handle only; the wiki labels them unknown rather
than guessing.

## What Windows does (the model to match) [STATIC ISM.dll, haptics.md §2]

- The inbox host (`ISM.dll`) reads Waveform + Duration lists once per pen
  (**feature reports are never written** to the pen — every static
  `HidD_SetFeature` site belongs to a keyboard-lamp device).
- Playback = five HID usages (0x0E: 0x21/0x24/0x25/0x23/0x28) composed into
  one output report, one `WriteFile`.
- Windows generates hover/snap feedback itself (types {1,2}→Hover,
  {0,3,4}→Collide→Click fallback, playCount 1); **tool→waveform selection is
  app-side** — neither the haptics host nor the pen stack contains such a
  map.
- Settings persist per user at `HKCU …\EnhancedPenSupport`:
  `PenFeedbackEnabled` / `PenInkFeedbackEnabled` /
  `PenInteractionFeedbackEnabled` (DWORD 0/1) and `PenFeedbackIntensity`
  (QWORD IEEE-754 double = slider/100). [VERIFIED-HW findings/006 §4]
- Windows stops with ordinal 2 per the spec — which this pen ignores
  (§ above); the spec names it, the device disagrees. [VERIFIED-HW §2.8.1]

## Linux path

The pen's `hidraw` node (it exists only while the link is up) → kernel HID →
UHID → BlueZ HID → GATT write — proven end to end by felt sends
(findings/009 §1). The pen drops BLE when idle, so a wake touch is needed
immediately before a send; no scan trick is — the existing bond reconnects
the trusted device on its own (findings/009 §1).

On Linux `px13pend` implements the §3 checklist: arms the tool's continuous
waveform once when the pen comes in range or the tool changes, writes
ordinal 1 when inking ends or the setting turns off, and exposes
`Haptics1.PlayEffect/Stop` (docs/dbus.md §7). The hover tick policy is
findings/007.

## Open

- Windows' per-effect field values (needs a full-payload Bluetooth
  sniffer — no session has run one).
- Whether the cutoff field ends a texture *within* one continuous contact —
  the Linux session couldn't tell; the daemon never arms a cutoff, so it
  doesn't depend on the answer (findings/009 §4).
