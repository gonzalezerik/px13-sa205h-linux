# Flow — the two buttons, on two different links

The pen has two user buttons and they do not share a transport. Getting
this wrong is the classic trap: **the barrel button never touches
Bluetooth**, and the top button's *gestures* are already classified by the
pen. [VERIFIED-HW — findings/006 §5]

```
 BARREL (side button)          TOP (shortcut button)
   │                             │
   │ 2.4 GHz air protocol        │  Bluetooth only — zero BLE traffic
   ▼                             ▼   across a single/double/hold battery
 ELAN9008 digitizer            HID keyboard report 0x43,
 report 7, Barrel Switch bit    notified on handle 0x0019
 (visible only while in range)  (works even with the pen out of range)
```

## Barrel switch [VERIFIED-HW]

- Arrives as the Barrel Switch bit of
  [digitizer report 7](../reference/digitizer.md#report-7-the-pen-report) —
  in-band with position, only while the pen is in range.
- Zero Bluetooth traffic for barrel gestures (three gesture blocks,
  single/double/long: nothing on the link).
- Windows consumes it as a right-click modifier (barrel held + tap =
  right-click; `RightMaskEnable=1`). A browser that never showed
  `buttons=2` misled an early test: the pointer layer promotes barrel+tap
  to a right button; the raw digitizer bit was always there.
- On Linux the bit demonstrably reaches a bare host (capture: R7 barrel
  frames); whether evdev surfaces it as `BTN_STYLUS` on this device is a
  one-minute check still queued.

## Top ("shortcut") button [VERIFIED-HW]

Notifications on handle `0x0019`; each frame is a boot-keyboard payload
with the report id as byte 0 (Linux hidraw reads keep the id):

| Gesture | Press frame | Release frame | Documented Windows mapping |
|---|---|---|---|
| single | Left-GUI (`0x08`) + `F20` (`0x6F`) | all zeroes | Win+F20 [DOC findings/001 §2] |
| double | Left-GUI + `F19` (`0x6E`) | all zeroes | Win+F19 |
| long (hold) | Left-GUI + `F18` (`0x6D`) | all zeroes | Win+F18 |

Key facts a host must respect:

- **One press+release pair per gesture — the pen classifies single, double
  and hold itself** (a double press is still one pair). Windows only ever
  sees distinct key reports; a host must not re-time gestures.
- The press-and-hold buzz is **pen-local**: no host→pen write anywhere in
  the trace. That is how host-commanded buzzes and pen-local buzzes are
  told apart.
- Reading the button on Linux: the compositor may hold the matching evdev
  node under an exclusive grab, but the pen's `hidraw` node hands every
  reader its own copy — `px13pend` reads gestures there (findings/010 §4)
  and emits `Pen1.ButtonEvent` (`single`/`double`/`long`) plus one shell
  command per gesture via `Buttons1` (docs/dbus.md §5, §8). The native
  Win+F18/19/20 still reach the compositor, so bind one side or the other.

## Keys that look documented but never arrive [VERIFIED-HW negative]

Microsoft's pen-button documentation also defines **stowage keys** —
Win+Ctrl+F20 on dock, Win+Ctrl+F19 on undock. A reader followed the pen
across three dockings and two lifts: **zero report bytes in those
windows**. Dock detection cannot wait for these keys; the firmware-log
dock signal in [battery](battery.md) is what there is. (The evdev node
also exists only seconds after a wake, so an undock-instant key would be
invisible there regardless.)
