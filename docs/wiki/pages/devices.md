# Devices

The exact hardware everything is pinned to — a claim here means *this*
machine and *this* pen unless labelled otherwise. [VERIFIED-HW —
`fixtures/px13-hn7306/`, findings/001]

## The laptop — ASUS ProArt PX13 (HN7306EAC)

| | |
|---|---|
| Model / BIOS | HN7306EAC, BIOS `HN7306EAC.310` |
| Touch + pen sensor | ELAN9008 on I2C-HID (`AMDI0010:00` / `i2c-0`), USB identity `04F3:4631`, 1 219-byte report descriptor, bound to `hid-multitouch` |
| Wireless | MediaTek combo adapter (the LE side runs the pen link) |

Seven input nodes appear for the digitizer: touchscreen, one "Stylus", five
"UNKNOWN" (vendor collections) — see the [digitizer reference
node-map note](reference/digitizer.md#node-map).

## The pen — ASUS Pen 3.0 (SA205H)

| | |
|---|---|
| Protocol | Microsoft Pen Protocol **2.6** with haptics, 2.4 GHz to the digitizer [DOC: ASUS spec; MPP itself is not public — inferred from controller outputs] |
| Bluetooth identity | HID `0x330A:0001`; PnP ID vendor `0x330A` product `0x0001` version `0x0100`; advertised name "ASUS Pen", appearance `0x03C7` |
| Firmware | revision string `2.0.37` with build date (`0x2A26`) |
| Battery | Li-ion, wirelessly charged; ≈2.3 mA idle draw, IMU wakes it |
| Transducer serial | 32-bit; identical on both links — the project's pen identifier (see the [glossary](glossary.md)) |

Buttons: top "shortcut" (BLE) and side "barrel" (screen link) —
[buttons](flows/buttons.md). What it exposes over GATT:
[Bluetooth reference](reference/bluetooth.md).

## The charger

A small NFC wireless-power transmitter: presents itself to the laptop as a
power-only USB-C sink, **no data channel**. The pen sleeps while charging,
so charging is only visible as state-of-charge steps between dockings
(≈1 %/min) — [battery](flows/battery.md).

## Known driver-level gaps on Linux (measured or statically shown)

- Nothing in-tree binds HID usage page `0x0E` on a pen/Bluetooth device —
  no haptics without `px13pend` [STATIC findings/001 §4].
- `hid-multitouch` **compresses azimuth** (0–36000 vs Windows' full range
  handling) — impact on real pens still to be measured [STATIC
  findings/001; [open question](open-questions.md)].
- No LE-legacy SMP out-of-band support — hence passkey 0
  ([pairing](flows/pairing.md#why-linux-pairs-with-passkey-0)).

For what works on the hardware today, see the repository README's status
section and `docs/parity.md`; this wiki keeps the *why* behind each row.
