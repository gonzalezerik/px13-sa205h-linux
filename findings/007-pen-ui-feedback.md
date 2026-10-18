# Finding 007 — pen UI feedback: hover cursor, press-and-hold timing and the context menu on screen

Date: 2026-10-18 · Confidence: **[V-VID]** = measured on the session-07 screen recording frame by
frame; frame numbers are decode order from frame 0, `t = n/30`. Wall-clock = 19:31:14 + t (§2).
Everything here refines or replaces the "analysis pending" state of findings/006 §6.

## 1. Method — and a trap

The recording is 2880×1796 h264, 30 fps, 6416 frames (213.87 s). **Input seeking (`-ss`) lands at
wrong positions on this file** (its edit list/timestamps defeat keyframe seek): the first set of
contact sheets built with `-ss` showed content that does not exist at those times, including
"black frames" — the video has no black frames anywhere. All numbers below come from decoding from
frame 0 and selecting by frame number; contact sheets must be built the same way.

Motion was measured with consecutive-frame difference (`tblend=all_mode=difference`) + `signalstats`
over a crop; presence of the pen cursor with a per-frame luma minimum over an 80×80 box.

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
the Linux check is one line: hover the pen, lift out of range, watch `oppd`/hcidump for a write to
the haptics handle. **[V-VID correlation / HYP semantics]**

## 5. Press-and-hold without the barrel (step 3): 1.9 s, and no circle

- The pen cursor (desktop star) **appears at frame 2244** (t 74.800; 80×80 box at (515, 1197),
  per-frame YMIN 104 → 67 — pure wallpaper through frame 2243) and sits at ≈ (555, 1237), drifting
  ≤ 2 px, through frame 2300. Whether the glyph appears on proximity-in or on contact is not
  separable here, so tip-down ≤ 2244 and the hold was **at most 1.90 s** before the right
  promotion at 2301 (57 frames). **[V-VID]**
- **No press-and-hold ring is drawn.** A 400×400 tile of frames 2225–2310 around the cursor and
  native-resolution zooms of 2285/2290/2296/2300 show wallpaper, then the star, then the menu —
  no arc, no fill — despite `GestureVisualization=31` and `HoldTime=2300` being set (006 §4).
  The maintainer's test notes asked whether the circle starts earlier than 2300 ms suggests; on
  this recording (Windows 11 25H2, this pen) **no circle renders at all** and the right-click
  arrives ~1.9 s after the pen comes down. **[V-VID]**
- Consequence for parity: the thing to replicate is the *timing*, not a progress ring; if OPP ever
  draws its own hold indicator it is adding, not matching.

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
  4440–4469, 5430–5504, 6195–6254) to steps 4–8, and check the step-7 barrel-hold window for a
  ring — if any step shows the visualisation, that bounds what `GestureVisualization` actually
  does.
- The unexplained one-frame luma spikes at 1021 (t 34.0) and 2248 over the canvas crop.
- Exact tip-down/tip-up and barrel flags remain the win32k.etl's job (`POINTER_FLAG_SECONDBUTTON`
  for the barrel); the video gives contact-ordered bounds only.
