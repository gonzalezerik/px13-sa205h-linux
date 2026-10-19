# 001 — Identification: pen, Windows requirements, Linux support state

Date: 2026-10-17 · Machine: analysis host without the pen attached (hardware facts below are from the PX13
fixtures captured earlier or are pending hardware checks)

Confidence labels: VERIFIED-HW / STATIC / DOC / HYPOTHESIS (see CONTRIBUTING.md §5).

## 1. Hardware identification

- Laptop: ASUS ProArt PX13 HN7306EAC, BIOS HN7306EAC.310. [VERIFIED-HW — fixtures/px13-hn7306/host.txt]
- Digitizer: ELAN9008 `04F3:4631` on I2C-HID (AMDI0010:00/i2c-0), 1219-byte "Win8 certified"
  descriptor, bound to `hid-multitouch`. [VERIFIED-HW — fixtures/px13-hn7306/elan9008-04f3-4631.rdesc,
  elan9008-04f3-4631.hid-decode.txt, proc-bus-input-devices.txt]
- Pen: ASUS Pen 3.0 **SA205H**, **MPP 2.6** with haptics (ASUS spec/press). [DOC]
- Report 7 (Pen collection) carries X 0..17664, Y 0..10496, Tip Pressure 0..4096, Battery
  Strength 0..255, X/Y tilt ±90°, Azimuth 0..360°, Scan Time, **Transducer Serial Number (32-bit)**,
  **Transducer Vendor ID (16-bit)**. So the digitizer already gives the host everything needed to
  correlate inking with a BT haptic pen. [VERIFIED-HW — hid-decode fixture]
- No haptics page (0x0E) on the digitizer — matches MS guidance (haptics ride Bluetooth). [VERIFIED-HW]
- Button count and charging method of the physical pen: **hardware check 1.** BT advertisement name:
  **hardware check 2.**

## 2. What Windows requires/provides (DOC, fetched 2026-10-17)

Sources:
- MS **Haptic Pen Implementation Guide** —
  https://learn.microsoft.com/en-us/windows-hardware/design/component-guidelines/haptic-pen-implementation-guide
  (ms.date 2026-03-25, updated 2026-04-17)
- MS **Pen Protocol Implementation** —
  https://learn.microsoft.com/en-us/windows-hardware/design/component-guidelines/pen-protocol-implementation (2017/2021)
- HID **Haptics page** HUTRR63b — https://usb.org/sites/default/files/hutrr63b_-_haptics_page_redline_0.pdf
- HUT digitizer page 0x0D — https://usb.org/sites/default/files/hut1_4.pdf (HID Usage Tables 1.4+)

Key facts (own words; details → docs/protocol/haptics-bt.md later):

- Haptic pens connect **HID over Bluetooth** using Windows inbox drivers (HID class + HID BTH
  miniport); no third-party driver needed. [DOC]
- The BT device exposes a **Stylus TLC (page 0x0D, usage 0x20)** containing a **Simple Haptic
  Controller logical collection (0x0E/0x01)**. [DOC]
- Pen identity: **Transducer Serial Number 0x0D/0x5B** (32-bit, optional part 2 at 0x0D/0x6E) and
  **Transducer Vendor ID 0x0D/0x91** must be reported in the input report of both the digitizer and
  the BT stylus collection so Windows correlates them; mandatory for haptics and multi-pen. [DOC]
- Capability discovery: host GETs a **feature report** with **Waveform List (0x0E/0x10)** and
  **Duration List (0x0E/0x11)**, ordinal-indexed (ordinal page 0x0A, usages ≥3; 1=None, 2=Stop implicit).
  Waveform IDs (exact table in docs/protocol/haptics.md §1): 0x1001 None, 0x1002 Stop,
  0x1003 Click, 0x1006 Press, 0x1007 Release, 0x1008 Hover, 0x1009 Success, 0x100A Error,
  0x100B InkContinuous (mandatory inking fallback), 0x100C–0x1011 Pencil/Marker/ChiselMarker/
  BrushContinuous…, 0x1012 Collide, 0x1013 Align, 0x1014 Step, 0x1015 Grow; 0x1004/0x1005
  unassigned. Continuous waveforms have duration 0 (play until stopped/cutoff). [DOC]
- Playback: one **output report** — **Manual Trigger 0x0E/0x21** (an *ordinal*), plus optional
  **Intensity 0x23** (0–100 %), **Repeat Count 0x24**, **Retrigger Period 0x25** (ms),
  **Waveform Cutoff Time 0x28** (ms). **Windows never uses Auto Trigger**: discrete waveforms play
  immediately, continuous waveforms play "at the device's discretion" (i.e. while tip contacts).
  Host configures continuous waveforms; only discrete are manually triggered. [DOC]
- Tail/top button over BT is reported as a **keyboard TLC**: single = WIN+F20, double = WIN+F19,
  hold = WIN+F18. Pen stowage: WIN+CTRL+F20 (dock) / WIN+CTRL+F19 (undock). [DOC]
- Pen digitizer bus guidance: inbox HID (I2C/USB/BTH) drivers; usages from the guide; sample
  descriptors in the docs. [DOC]

## 3. Linux kernel state (STATIC — torvalds/linux master, all 168 drivers/hid .c files fetched 2026-10-17)

- **`drivers/hid/hid-haptic.{c,h}` exists** — a HID haptics *core* (Angela Czubak / Semihalf, 2021):
  parses the haptics page (Auto Trigger feature GET, waveform/duration ordinal lists), models
  device-mode vs host-mode playback, creates an evdev **FF** device with
  `input_set_capability(EV_FF, FF_HAPTIC)` + `input_ff_create`, and plays effects by filling the
  device's **manual-trigger output report** (hid-haptic.c:160-317, 502-506). [STATIC]
- **`FF_HAPTIC` = 0x4f** in `include/uapi/linux/input.h:494` (uapi exists; evdev userspace can use it). [STATIC]
- **Only user today is `hid-multitouch.c` and only for haptic *touchpads***: `#include
  "hid-haptic.h"` (line 53), `is_haptic_touchpad` set from pressure-unit check (gram/newton) at
  HID_DG_TIPPRESSURE (line ~965), pressure increase/reset feeding device-mode force,
  `hid_haptic_input_configured` for MT_CLS_WIN_8 (lines 1958-64). `CONFIG_HID_HAPTIC` help text
  says "Haptic touchpad support … Elan 2703". **No driver binds usage page 0x0E on a pen/stylus or
  Bluetooth device.** → The upstream gap: nothing on Linux turns an MPP 2.6 pen's haptics collection
  into an FF device; px13pend must own that in userspace, and that's also the shape of the eventual
  upstream patch. [STATIC]
- MPP inking path: `hid-multitouch`/`hid-input` map HID_DG_* usages; confirmed our fixture's Stylus
  node (event9) exists. **Azimuth quirk:** `HID_DG_AZIMUTH` (0..360) is mapped to
  `ABS_MT_ORIENTATION` clipped to ±(logical_max/4) — a quarter revolution of range — so the full
  0..360° azimuth is *compressed* vs Windows. [STATIC hid-multitouch.c:979-991; impact on real pens
  → measure on hardware]
- Battery: `hid-input.c` registers a power_supply for HID battery usages (hidinput_battery, line
  344+) → matches the existing `/sys/class/power_supply/hid-0018:04F3:4631.0002-battery-7`. [STATIC + VERIFIED-HW fixture]

## 4. BlueZ state (STATIC — bluez master fa6ba55, cloned 2026-10-17)

- **No haptics-specific code anywhere** (grep: zero matches in src/, profiles/). BlueZ treats the
  pen as a generic HID device. [STATIC]
- The input plugin registers the device with the kernel via **UHID** and, when the kernel requests
  `SET_REPORT`, BlueZ forwards it as an HIDP control message (`hidp_send_set_report`,
  profiles/input/device.c:699-737). **Consequence:** an output report written by userspace to the
  pen's hidraw/uhid device can reach the pen — the transport path for haptics exists without
  touching BlueZ. [STATIC — end-to-end write still to be proven on hardware, required before px13pend writes]

## 5. libinput state (DOC — libinput 1.31.0 docs, fetched 2026-10-17)

- https://wayland.freedesktop.org/libinput/doc/latest/tablet-support.html
- Pressure: device-specific tip-down threshold; **automatic pressure-offset detection** for worn
  tools (only at proximity-in, ≤50 % of range, rescales rest); custom pressure range API
  (`libinput_tablet_tool_config_pressure_range_set`, disables offset detection). No multi-point
  pressure curve. [DOC]
- Special axes: distance, pressure (normalized 0..1) and **tilt (x/y degrees)**. **No azimuth/rotation
  axis is documented** → azimuth parity gap at the libinput layer on top of the kernel compression. [DOC]
- Eraser: MS-style firmware invert ("pen out of proximity, eraser in immediately after") is the
  mandated model; libinput has a config to turn eraser-button events into normal button events
  (only for button-eraser pens, not invert pens). [DOC]
- The CachyOS-forum "eraser pressure capped" claim: not reproduced by us yet. [HYPOTHESIS —
  measure raw (evtest/hid-recorder) vs libinput vs app]

## 6. Prior art

- **iptsd / linux-surface**: iptsd = IPTS touch/stylus processing daemon (hidraw→uinput), GPL-2.0.
  Its README and the linux-surface wiki contain **no pen haptics** (wiki grep: only a haptic
  *touchpad* libinput quirk note). Architecture (hidraw→uinput daemon, per-device config) is a good
  pattern reference, no protocol reuse. [STATIC 2026-10-17]
- **linux-input lore (via marc.info)**: no "haptic pen" threads; closest is Tero Kristo's "HID: add
  support for USI style pens" RFC v1-v3 (Nov-Dec 2021, not merged) and the 2021 hid-haptic work that
  became the touchpad core. HID-BPF (Benjamin Tissoires, 2022) is merged and relevant if a kernel-side
  fix for the azimuth compression is ever prototyped. [DOC]
- **GarO / even-g2-companion** (same maintainer): userspace daemon + D-Bus + CLI + GTK patterns to follow.

## 7. Gaps this project must fill (scope confirmation)

1. **Seamless pairing** (ELAN 0xFFF4 vendor TLC reports 86/110/111/112): Windows-only, no Linux
   equivalent anywhere. → seamless-pairing analysis, then px13pend. [HYPOTHESIS that it's BD_ADDR+key handoff]
2. **Haptics host**: no Linux support (kernel/BlueZ/libinput). px13pend writes the pen's
   manual-trigger output report via hidraw-over-BlueZ; expose FF via uhid? (design decision,
   Daemon work — either evdev FF_HAPTIC through a uhid shim, or app-facing D-Bus only). [HYPOTHESIS]
3. **Azimuth end-to-end**: kernel compresses to ABS_MT_ORIENTATION ±90°; libinput exposes no azimuth.
   Upstream candidates: hid-multitouch mapping fix/new axis; libinput support. Measure first on hardware. [STATIC + HYPOTHESIS on impact]
4. **Battery**: kernel path exists; verify UPower/desktop rendering with pen in range. [hardware check 3]
5. **Digitizer config / report rate / eraser pressure**: measure on Linux; compare vs
   Windows via the Windows captures.
6. **Settings parity**: Windows "Pen & Windows Ink" + ASUS apps — what they store → findings/003.

## Next

- Downloads: ELAN Seamless Pairing / WoT / DialPad / MediaTek BT + MyASUS / ProArt Creator
  Hub / ScreenXpert msixbundle + Win11 ISO components; MANIFEST.
- docs/protocol/haptics-bt.md skeleton from §2 (already drafted into parity.md; full write-up after 3b).
