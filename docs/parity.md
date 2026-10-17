# Parity matrix — Windows haptic-pen stack vs Linux vs mpp-linux

v1 (identification pass). Rows seeded from the project goals + findings/001-identification.md. Every "how Windows does
it" claim carries a confidence label (CONTRIBUTING.md §5) and a citation; `→001 §n` points at the evidence
detail in findings/001. **HW-verified? is "no" for everything that needs the pen until the hardware captures.**

Legend: ✅ supported today · ⚠️ partial/gap · ❌ absent · 🔍 measure first · V-HW / STATIC / DOC / HYP

| # | Feature | How Windows does it (evidence) | Linux today | mpp-linux plan | HW? |
|---|---------|--------------------------------|-------------|----------------|-----|
| 1 | Inking: pressure 0..4096, tilt ±90° | HID digitizer usages via inbox `hidclass`/ELAN I2C driver → Windows Ink pressure curves/smoothing/prediction [DOC →001 §2] | ✅ hid-multitouch → evdev/libinput tablet tool; libinput applies tip-threshold + auto pressure-offset, custom-range API; no curve [DOC →001 §5] | measure raw vs libinput vs app; pressure-curve support in mppd/settings if gap [HYP] | no |
| 1a | Azimuth 0..360° | full range to app [DOC] | ⚠️ kernel compresses to `ABS_MT_ORIENTATION` ±(max/4) [STATIC →001 §3]; libinput exposes no azimuth axis [DOC →001 §5] | quantify loss on hardware; upstream kernel mapping fix + libinput axis | no |
| 1b | Eraser (invert) + eraser pressure | firmware invert model, MS-mandated [DOC →001 §5] | ⚠️ libinput handles invert; forum claim of capped eraser pressure [HYP →001 §5] | hid-recorder + evtest raw capture to confirm/deny; fix where found | no |
| 1c | Barrel buttons | HID digitizer buttons/stylus usages [DOC] | 🔍 mapping untested on this device | evtest capture; remap via mppd if needed | no |
| 1d | Digitizer config (SET/GET_FEATURE at probe: report rate, hover, palm rejection) | ELAN WoT/precision driver may program the device [HYP → digitizer TLC / WoT analysis] | hid-multitouch sends its own probe feature set [STATIC] | enumerate Windows-side SETs via static RE (3d/3e), compare, replicate if beneficial | no |
| 1e | Report rate / jitter / latency | ELAN firmware sensing; host-neutral in theory [DOC] | 🔍 descriptor Scan Time present [VERIFIED-HW →001 §1] | measure via hid-recorder timestamps + Scan Time; any delta = bug to explain | no |
| 2 | Pen identity: serial+vendor, multi-pen | Transducer Serial 0x0D/0x5B(+0x6E) + Vendor 0x0D/0x91 in **both** digitizer and BT input reports; host correlates [DOC →001 §2] | ⚠️ digitizer reports serial+vendor [VERIFIED-HW →001 §1]; kernel does not expose per-tool identity to userspace in a stable way; BT side absent until paired | mppd reads digitizer serial events, matches against BT pen's identity → per-pen config [HYP] | no |
| 3 | Battery in desktop (UPower) | Settings pen battery page [DOC] | ✅ kernel hid-input power_supply from digitizer Battery Strength [STATIC →001 §3]; desktop rendering untested | verify UPower/waybar [hardware check 3]; add BT-side battery read if richer | no |
| 4 | Seamless pairing (touch-to-pair over BT) | ELAN "Seamless Pairing" DCH driver writes/reads vendor TLC 0xFFF4 reports 86/110/111/112, hands BD_ADDR+key to BT stack [HYP →001 §7.1; driver URL in fixtures json] | ❌ nothing [STATIC] | seamless-pairing analysis → mppd pairing flow | no |
| 5a | Haptics: continuous texture per tool | host CONFIGURES continuous waveform (InkContinuous fallback + tool-specific); device plays at its own discretion while tip contacts; never Auto Trigger [DOC →001 §2] | ❌ no pen-haptics driver; hid-haptic core exists but touchpad-only user [STATIC →001 §3] | mppd: GET feature Waveform/Duration List, program continuous config via output report [HYP — exact byte layout after the haptics host analysis + the pen descriptor] | no |
| 5b | Haptics: discrete (click/success/error/hover/press/release/collide/align/step/grow) | Manual Trigger 0x0E/0x21 = ordinal into waveform list, in one output report [DOC →001 §2] | ❌ [STATIC →001 §3] | mppd plays on events (tail click, hover-in/out, success/error via D-Bus from apps) [HYP] | no |
| 5c | Haptics intensity | Intensity 0x23 0–100 % in output report [DOC →001 §2] | ❌ | settings slider → mppd Intensity field | no |
| 5d | Haptics on/off | host-side policy [DOC] | ❌ | settings toggle → mppd suppresses writes | no |
| 5e | Repeat / retrigger / cutoff | Repeat Count 0x24, Retrigger Period 0x25 ms, Waveform Cutoff 0x28 ms [DOC →001 §2] | ❌ | expose in mppd API; defaults from 3b RE | no |
| 6 | Tail button over BT: single/double/hold | BT HID keyboard TLC: WIN+F20 / WIN+F19 / WIN+F18 [DOC →001 §2] | ⚠️ BlueZ exposes as keyboard; kernel hid-input maps F18-F20 keys but no double/hold semantics [HYP] | mppd decodes key combos → configurable actions via D-Bus | no |
| 6b | Stowage dock/undock events | WIN+CTRL+F20 dock / F19 undock over BT [DOC →001 §2] | ❌ (keys only) | mppd surfaces dock/undock signals | no |
| 7 | Settings: button remap, pressure curve, haptic strength, handedness, ASUS extras | "Pen & Windows Ink" + MyASUS/ProArt Creator Hub [DOC; keys → findings/003] | ❌ no equivalent | mppd config + GTK4 settings app (WCAG 2.2 AA) | no |
| 8 | Pen info: firmware version, model (read-only) | ASUS apps read it [DOC] | ❌ | mppd reads via BT HID GET (safe reads only) [HYP source after 3c] | no |
| 9a | Wake-on-Touch (digitizer) | ELAN WoT DCH driver [DOC, URL fixtures] | 🔍 hid-multitouch may already wake device; unverified [HYP] | WoT analysis + observe on hardware | no |
| 9b | Palm rejection hints / "pen present" modes | possibly driver feature reports [HYP → 1d] | hid-multitouch defaults | compare after 3d | no |
| 9c | Charging events | pen charges via USB-C or magnetic contacts [DOC/unknown — hardware check 1] | unknown | document; surface if detectable | no |

## Summary of gaps (what mpp-linux must build)

1. **Seamless pairing** (row 4) — nothing on Linux; RE-first.
2. **Pen haptics host** (5a–5e) — kernel core exists but no pen binding; userspace mppd owns it.
3. **Identity correlation** (2) — digitizer↔BT-pen matching + per-pen settings.
4. **BT button semantics** (6, 6b) — combo decoding + remapping.
5. **Azimuth end-to-end** (1a) — kernel + libinput upstream work.
6. **Settings app** (7) — Windows-parity UI, accessible.

## Measurement backlog (feeds hardware rows to "yes")

- Raw evdev/hid-recorder vs libinput vs app: pressure min/max/linearity, eraser pressure cap claim,
  tilt, azimuth range, report rate (hover + touch), jitter.
- Digitizer feature-report state at probe (GET, read-only) vs after Windows session (4w).
- BT: pen advertisement, pairing flow trace, HID descriptor over HIDP, GATT.
