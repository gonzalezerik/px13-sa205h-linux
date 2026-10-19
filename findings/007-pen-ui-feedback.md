# Finding 007 — pen UI feedback: hover cursor, press-and-hold timing and the context menu on screen

Date: 2026-10-18 · Confidence: **[V-VID]** = measured on the session-07 screen recording frame by
frame; frame numbers are decode order from frame 0, `t = n/30`. Wall-clock = 19:31:14 + t (§2).
**[V-DISP]** = measured on a phone video of the physical display (§5a; 28.28 s, decoded
2160×3840 @ ≈29.95 fps; times are video PTS and approximate).
Revision same day: the first version of §5 read the screen recording and concluded **no
press-and-hold ring is drawn** — that was wrong; the screen recorder does not capture the ring
(§5a). Everything else here stands.
Everything here refines or replaces the "analysis pending" state of findings/006 §6.

## 1. Method — and a trap

The recording is 2880×1796 h264, 30 fps, 6416 frames (213.87 s). **Input seeking (`-ss`) lands at
wrong positions on this file** (its edit list/timestamps defeat keyframe seek): the first set of
contact sheets built with `-ss` showed content that does not exist at those times, including
"black frames" — the video has no black frames anywhere. All numbers below come from decoding from
frame 0 and selecting by frame number; contact sheets must be built the same way.

Motion was measured with consecutive-frame difference (`tblend=all_mode=difference`) + `signalstats`
over a crop; presence of the pen cursor with a per-frame luma minimum over an 80×80 box.

There is a trap one level *above* the file, too: the screen recording captures the composited
desktop, not everything the panel shows. The press-and-hold ring (§5a) renders only to the
display — no frame of this recording shows it, and no on-machine screenshot ever will. Pixel
claims about pen visuals from a screen capture need display-camera confirmation before they become
conclusions.

## 2. Clock anchor

The desktop context menu of step 3 is visible from frame 2301 (t 76.700); the Ninput timeline
records the right-button promotion (`promotionFlags=0x20002`) at 19:32:30.500. With video t0 at
19:31:14 (session notes) the two agree within 200 ms, which also matches the menu appearing ≤ 1
frame after the promotion. The notes' t0 stands. **[V-VID + V-HW]**

## 3. Hover: the cursor glyph and its size (step 2)

Frame differencing over the Paint canvas finds sustained low-amplitude motion strokes (the pen
moving while hovering — no ink appears during any of them) at frames
660–682, 722–741, 799–836, 879–898, 911–927, 935–948 (t 22.0–31.6 s) and again at
1869–1963 (t 62.3–65.4). The maintainer's note "the floating cursor changed when going in and out
of the paint work area" is on the video.

The glyph itself differs by surface, measured at native resolution:

| Surface | Glyph | Size | Example |
|---|---|---|---|
| Desktop | hollow 4-point star | ≈ 20 px span | frame 2290, centre ≈ (555, 1237) |
| Paint canvas | dot with four crosshair ticks | ≈ 40 px span | frame 885, centre ≈ (979, 1002); frame 940 ≈ (985, 1014) |

So over a window's ink surface the hover cursor is roughly **twice as large and a different
shape**. Which layer selects it (the hover indicator adapting to the window's cursor, or the app)
is not distinguishable from pixels; what matters for parity is that Windows gives hover a
surface-dependent visual, and lifts the glyph entirely when the pen leaves range (frames 683, 690,
740, 748: canvas clean). **[V-VID]**

## 4. The six haptic ticks fire at hover-stroke end, not on hold or right-click

The six host→pen haptic writes (findings/006 §4) sit at frames 684, 744, 840, 954, 1965, 4992.
Four of them land **2–6 frames after the end of a canvas hover stroke** (682→684, 741→744,
836→840, 948→954), the fifth ends the later stroke 1869–1963 (→1965). The two back-to-back strokes
879–898 and 911–927 have **no** tick — the pen stayed in range between them. And **none** of the
12 right-button promotions has a tick near it, including the step-3 hold completion at 2301.

This replaces the 006 §6 hypothesis ("tick tied to the hold threshold completing"): the ticks
correlate with the **end of a hover interaction** — the pen pulled out of range after moving —
which fits the ordinal-6 `hover` waveform the pen reports (006 §4). The sixth tick (4992, t 166.4)
falls between later steps and is unmapped. Treat "Windows buzzes the pen on hover-out" as a
**[HYP]** for event semantics (the frame correlation is solid, the causal event is inferred);
the Linux check is one line: hover the pen, lift out of range, watch `px13pend`/hcidump for a write to
the haptics handle. **[V-VID correlation / HYP semantics]**

## 5. Press-and-hold without the barrel (step 3): ≤1.9 s to the right-click

- The pen cursor (desktop star) **appears at frame 2244** (t 74.800; 80×80 box at (515, 1197),
  per-frame YMIN 104 → 67 — pure wallpaper through frame 2243) and sits at ≈ (555, 1237), drifting
  ≤ 2 px, through frame 2300. Whether the glyph appears on proximity-in or on contact is not
  separable here, so tip-down ≤ 2244 and the hold was **at most 1.90 s** before the right
  promotion at 2301 (57 frames). **[V-VID]**
- No arc appears around the cursor anywhere in this recording (400×400 tile of frames 2225–2310,
  native zooms of 2285/2290/2296/2300: wallpaper, then the star, then the menu). That is a fact
  about the capture, not about the display — see §5a. The first revision of this section read the
  missing pixels as a missing ring; corrected.

## 5a. The press-and-hold ring exists — filmed off the physical display

Pressing and holding the pen tip on the Windows 11 desktop builds a white ring around the contact
point, frame by frame, until it closes; the context menu then opens and the ring fades. On-machine
screen recordings and screenshots never show any of it (§5, §1) — only a camera pointed at the
panel does. Evidence: a 28.28 s phone video of the display covering six press-and-hold cycles
(handheld, decoded 2160×3840 @ ≈29.95 fps), frame-stepped with per-frame crops at each contact
point. The `GestureVisualization=31` visualisation (006 §6) is presumably what this is. **[V-DISP]**

| cycle | first ring mark | ring closed | menu opens | mark→menu |
|---|---|---|---|---|
| 1 | 4.11 s (dot above tip) | ≤ 4.80 s | 5.21 s | ≈ 1.10 s |
| 2 | dot at contact ≈ 7.7 s, arc sweeping by 8.3 s | ≈ 9.3 s | ≈ 9.4 s | ≈ 1.7 s |
| 3 | dot at contact ≈ 11.1 s, arc sweeping by 12.0 s | ≈ 13.0 s | ≈ 13.1 s | ≈ 2.0 s |

Times are video PTS in seconds: ±1 frame (≈33 ms) plus a hand-held, moving camera — treat them as
approximate, good to roughly ±0.1–0.2 s. Cycle 4's ring start is hidden by a hand/keyboard
close-up; cycle 6 sweeps and closes (≈27.4–28.0 s) but the video ends before its menu.

The animation: a small dot sits at/above the tip for a beat (up to ≈0.7 s in cycles 2–3 — the
sweep does not start the instant of contact), then a white arc sweeps around the contact point over
≈0.6–1.0 s of visible growth, the closed circle holds a few hundred ms, the menu slides in, and
the ring fades within ≈0.3 s of the menu.

Consequences:

- The right-click arrives ≈1.1–2.0 s after the first mark across the measurable cycles —
  consistent with the ≤1.90 s bound from the screen recording (§5) and under `HoldTime=2300` in
  every case: whatever gates pen press-and-hold, 2300 ms is visibly not it.
- For parity: a Linux stack that draws no press-and-hold ring is showing a **gap**, not parity —
  Windows does render one. The hover glyph (§3) and this ring are display-layer visuals; whether
  this project draws the ring (e.g. a layer-shell overlay following the cursor) or a compositor/toolkit
  should is an open question for the maintainer, recorded here, not decided.

## 6. The context menu

Visible frames 2301–2415 (**3.83 s**), bounding box ≈ (12..655, 410..1238). Its **bottom edge lands
on the pen glyph's y-coordinate within 1 px** — the menu opens *upward* from the contact point
(Win11 flipping a menu that would run off-screen). Its horizontal placement (left edge at x ≈ 12,
543 px left of the contact) follows a screen-edge rule not pinned down here. Dismissed by frame
2416 (t 80.53, wall 19:32:34.5 — the procedure says Esc; Ninput interaction taps 33.639/34.209 and
IC destroy 35.180 bracket the dismissal). The menu covering the glyph is why the star "disappears"
at 2301. **[V-VID]**

## 7. Still open on this video

- Map the later popup windows (frames 3270–3299, 3480–3539, 3585–3607, 3660–3734, 3780–3824,
  4440–4469, 5430–5504, 6195–6254) to steps 4–8. A ring check on this recording is now moot —
  screen capture cannot record the ring at all (§5a); a barrel-hold ring, if one exists, needs
  another display-camera video.
- The unexplained one-frame luma spikes at 1021 (t 34.0) and 2248 over the canvas crop.
- Exact tip-down/tip-up and barrel flags remain the win32k.etl's job (`POINTER_FLAG_SECONDBUTTON`
  for the barrel); the video gives contact-ordered bounds only.
