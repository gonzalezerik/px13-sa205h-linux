# Parity matrix — Windows haptic-pen stack vs Linux vs Open Pen Protocol

v1 (identification pass). Rows seeded from the project goals + findings/001-identification.md. Every "how Windows does
it" claim carries a confidence label (CONTRIBUTING.md §5) and a citation; `→001 §n` points at the evidence
detail in findings/001. **HW-verified? is "no" for everything that needs the pen until the hardware captures.**

**v2 pass (2026-10-17):** the hardware captures are in. Rows are upgraded in place with `→006 §n`
citations (findings/006-hardware-capture-results.md); original v1 claims are kept and marked, not
deleted.

Legend: ✅ supported today · ⚠️ partial/gap · ❌ absent · 🔍 measure first · V-HW / STATIC / DOC / HYP

| # | Feature | How Windows does it (evidence) | Linux today | Open Pen Protocol plan | HW? |
|---|---------|--------------------------------|-------------|----------------|-----|
| 1 | Inking: pressure 0..4096, tilt ±90° | HID digitizer usages via inbox `hidclass`/ELAN I2C driver → Windows Ink pressure curves/smoothing/prediction [DOC →001 §2; V-HW: ≈256 Hz in contact, 10-bit as seen by the browser (device max 4096), tilt only in contact, no twist →006 §1] | ✅ hid-multitouch → evdev/libinput tablet tool; libinput applies tip-threshold + auto pressure-offset, custom-range API; no curve [DOC →001 §5] | measure raw vs libinput vs app; pressure-curve support in oppd/settings if gap [HYP] | yes |
| 1a | Azimuth 0..360° | full range to app [DOC; V-HW: browser saw azimuth 0–359.5° →006 §1] | ⚠️ kernel compresses to `ABS_MT_ORIENTATION` ±(max/4) [STATIC →001 §3]; libinput exposes no azimuth axis [DOC →001 §5] | quantify loss on hardware; upstream kernel mapping fix + libinput axis | yes |
| 1b | Eraser (invert) + eraser pressure | firmware invert model, MS-mandated [DOC →001 §5; V-HW: invert → correct eraser identity on the wire (buttons bit 32 at the browser), eraser strokes drawn right →006 §1] | ⚠️ libinput handles invert; forum claim of capped eraser pressure [HYP →001 §5] | hid-recorder + evtest raw capture to confirm/deny; fix where found | yes (identity; pressure cap still untested) |
| 1c | Barrel buttons | **in-band digitizer only** — R7 barrel switch while in range, zero BLE, host uses it as right-click modifier on tip contact [DOC; V-HW →006 §5] | the raw bit verified arriving on hidraw on bare Linux (R7 `0x05`/`0x07` frames) [V-HW →006 §3,5]; kernel BTN_STYLUS surfacing = pending one evtest check | expose button + modifier via oppd, Windows-style right-click default | yes |
| 1d | Digitizer config (SET/GET_FEATURE at probe: report rate, hover, palm rejection) | ELAN WoT/precision driver may program the device [HYP → digitizer TLC / WoT analysis] | hid-multitouch sends its own probe feature set [STATIC] | enumerate Windows-side SETs via static RE (3d/3e), compare, replicate if beneficial | no |
| 1e | Report rate / jitter / latency | ELAN firmware sensing; host-neutral in theory [DOC; V-HW: median 3.9 ms in contact ≈256 Hz, hover ≈175 Hz, ~266 I2C reads/s →006 §1] | 🔍 descriptor Scan Time present [VERIFIED-HW →001 §1] | measure via hid-recorder timestamps + Scan Time; any delta = bug to explain | yes |
| 2 | Pen identity: serial+vendor, multi-pen | Transducer Serial 0x0D/0x5B(+0x6E) + Vendor 0x0D/0x91 in **both** digitizer and BT input reports; host correlates [DOC →001 §2; V-HW: BLE feature 0x40 = transducer serial + vendor 0x330A/0x0C0F/0x0003 →006 §2; digitizer report 34 carries the serial on pen presence →006 §3; no serial→address map in the registry — the address handoff is the 0x6E report alone] | ⚠️ digitizer reports serial+vendor [VERIFIED-HW →001 §1]; kernel does not expose per-tool identity to userspace in a stable way; BT side absent until paired | oppd reads digitizer serial events, matches against BT pen's identity → per-pen config [HYP] | yes |
| 3 | Battery in desktop (UPower) | Settings pen battery page [DOC; V-HW: BLE Battery Level notifications on handle 0x0015 (80→77 % over an evening) →006 §5] | ✅ kernel hid-input power_supply from digitizer Battery Strength [STATIC →001 §3]; desktop rendering untested | verify UPower/waybar [hardware check 3]; add BT-side battery read if richer | partial (BLE notify V-HW; UPower rendering untested) |
| 4 | Seamless pairing (touch-to-pair over BT) | ELAN "Seamless Pairing" DCH driver writes/reads vendor TLC 0xFFF4 reports 86/110/111/112, hands BD_ADDR+key to BT stack [HYP →001 §7.1; driver URL in fixtures json] — **superseded by observation**: the ELAN INF is a marker with no service; verified flow = digitizer input report 0x6E (BD_ADDR LE + flags) → Windows Bluetooth stack connects straight to the pen's random-static address, LE legacy SMP, bonded ≈0.6 s after touch [V-HW →006 §2] | ❌ nothing [STATIC] → 🟡 every input is present on bare Linux: report 0x6E fires spontaneously, no vendor driver needed [V-HW →006 §3]; only a listener is missing | seamless-pairing analysis → oppd pairing flow [design done — findings/006 §7.1: hidraw 0x6E → ConnectDevice(random) → Pair] | yes |
| 5a | Haptics: continuous texture per tool | host CONFIGURES continuous waveform (InkContinuous fallback + tool-specific); device plays at its own discretion while tip contacts; never Auto Trigger [DOC →001 §2; V-HW: 7-byte output 0x41 over BLE GATT (ATT write → handle 0x0021); device auto-triggers on the tip switch — continuous felt only in contact, all seven textures felt distinct; OneNote/Paint drive it per stroke →006 §4] | ❌ no pen-haptics driver; hid-haptic core exists but touchpad-only user [STATIC →001 §3] | oppd: GET feature Waveform/Duration List, program continuous config via output report [HYP — exact byte layout after the haptics host analysis + the pen descriptor] [layout now known — haptics.md §2.7] | yes |
| 5b | Haptics: discrete (click/success/error/hover/press/release/collide/align/step/grow) | Manual Trigger 0x0E/0x21 = ordinal into waveform list, in one output report [DOC →001 §2; V-HW: feature 0x42 ordinals 3–8 = click/press/release/hover/success/error, click/success/error felt in the air; pen lacks Collide/Align/Step/Grow → Windows snap buzzes observed as the Click fallback →006 §4] | ❌ [STATIC →001 §3] | oppd plays on events (tail click, hover-in/out, success/error via D-Bus from apps) [HYP] | yes |
| 5c | Haptics intensity | Intensity 0x23 0–100 % in output report [DOC →001 §2; V-HW: 0xFF = full; byte ≈ slider/100 × 255 consistent with every send →006 §4] | ❌ | settings slider → oppd Intensity field | yes |
| 5d | Haptics on/off | host-side policy [DOC; V-HW: per-user registry key `EnhancedPenSupport`, three DWORDs + intensity QWORD double →006 §4; toggles send no preview buzz] | ❌ | settings toggle → oppd suppresses writes | yes |
| 5e | Repeat / retrigger / cutoff | Repeat Count 0x24, Retrigger Period 0x25 ms, Waveform Cutoff 0x28 ms [DOC →001 §2; V-HW: all carried in output 0x41 (cutoff in 10 ms units, retrigger 16-bit LE); capability durations 45–1995 ms, current state feature 0x41 →006 §4] | ❌ | expose in oppd API; defaults from 3b RE | yes |
| 6 | Tail button over BT: single/double/hold | BT HID keyboard TLC: WIN+F20 / WIN+F19 / WIN+F18 [DOC →001 §2; V-HW: BLE keyboard report 0x43 notifications on handle 0x0019, one press+release pair per gesture; **the pen itself classifies** single/double/long (a double press is still one pair); key codes truncated in the trace →006 §5] | ⚠️ BlueZ exposes as keyboard; kernel hid-input maps F18-F20 keys but no double/hold semantics [HYP] | oppd decodes key combos → configurable actions via D-Bus [→006 §5: no combo decode needed — map the distinct reports; hold buzz is pen-local, no host write] | yes |
| 6b | Stowage dock/undock events | WIN+CTRL+F20 dock / F19 undock over BT [DOC →001 §2] | ❌ (keys only) | oppd surfaces dock/undock signals | no |
| 7 | Settings: button remap, pressure curve, haptic strength, handedness, ASUS extras | "Pen & Windows Ink" + MyASUS/ProArt Creator Hub [DOC; keys → findings/003] | ❌ no equivalent | oppd config + GTK4 settings app (WCAG 2.2 AA) | no |
| 8 | Pen info: firmware version, model (read-only) | ASUS apps read it [DOC; V-HW: firmware "2.0.37", HID VID 0x330A/PID 0x0001, identity via BLE feature 0x40 →006 §1–2] | ❌ | oppd reads via BT HID GET (safe reads only) [HYP source after 3c] | yes |
| 9a | Wake-on-Touch (digitizer) | ELAN WoT DCH driver [DOC, URL fixtures] | 🔍 hid-multitouch may already wake device; unverified [HYP] | WoT analysis + observe on hardware | no |
| 9b | Palm rejection hints / "pen present" modes | possibly driver feature reports [HYP → 1d] | hid-multitouch defaults | compare after 3d | no |
| 9c | Charging events | pen charges via USB-C or magnetic contacts [DOC/unknown — hardware check 1] | unknown | document; surface if detectable | no |

## Summary of gaps (what Open Pen Protocol must build)

1. **Seamless pairing** (row 4) — nothing on Linux; RE-first.
2. **Pen haptics host** (5a–5e) — kernel core exists but no pen binding; userspace oppd owns it.
3. **Identity correlation** (2) — digitizer↔BT-pen matching + per-pen settings.
4. **BT button semantics** (6, 6b) — combo decoding + remapping.
5. **Azimuth end-to-end** (1a) — kernel + libinput upstream work.
6. **Settings app** (7) — Windows-parity UI, accessible.

## Measurement backlog (feeds hardware rows to "yes")

- Raw evdev/hid-recorder vs libinput vs app: pressure min/max/linearity, eraser pressure cap claim,
  tilt, azimuth range, report rate (hover + touch), jitter.
- Digitizer feature-report state at probe (GET, read-only) vs after Windows session (4w).
- BT: pen advertisement, pairing flow trace, HID descriptor over HIDP, GATT.
