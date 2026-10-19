# Finding 010 — the pen streams a firmware debug log; it reports its fuel gauge and its charger

Date: 2026-10-19 · Confidence: **VERIFIED-HW** (§1–§3), **OBSERVED** (§4) · Method: three Linux
sessions on the test machine (ASUS ProArt PX13, ELAN9008 `04F3:4631`, ASUS Pen 3.0-class BLE pen,
HID VID `0x330A` PID `0x0001`, firmware revision `2.0.37`). A small BlueZ D-Bus client called
`StartNotify` on every notifying or indicating characteristic of the pen and logged what arrived;
nothing was written to any characteristic value. The pen was used, set on its wireless charger and
lifted again while the client ran. Per-device identifiers are referred to by role only.

## 1. The pen's GATT database [VERIFIED-HW]

Besides Generic Access, Generic Attribute, Device Information, Battery and HID, the pen exposes
four services that Windows never uses (in the Windows pairing captures of findings/006 Windows
only reads their CCCDs during service discovery):

| Service | Characteristics (properties) | Seen |
|---|---|---|
| Nordic UART Service `6e400001-b5a3-f393-e0a9-e50e24dcca9e` | RX `…0002` (write, write without response), TX `…0003` (notify) | TX streams the log of §2 |
| `a6ed0401-d344-460a-8075-b9e8ec90d71b` | `a6ed0402` (notify), `a6ed0403` (write without response), `a6ed0404` (write without response, indicate) | silent |
| `58480000-5a6a-1122-3300-004095112200` | `58480001` (write, write without response, notify) | silent |
| `a6ed0801-d344-460a-8075-b9e8ec90d71b` | `a6ed0802` (notify), `a6ed0803` (write, indicate) | silent |

The three vendor services sent nothing in any session. Their shape (a write characteristic next
to a notify/indicate one) is that of command/response channels: they answer writes, which this
project has not made. What they do — firmware update, factory test, configuration — is unknown.

Device Information reads: firmware revision `2.0.37` with a build date in the string, PnP ID
vendor source `0x02` (USB-IF), vendor `0x330A`, product `0x0001`, version `0x0100`.

## 2. The log [VERIFIED-HW]

While the pen holds a link, the Nordic UART TX characteristic carries plain ASCII, about once a
second, whether or not the pen is in use:

```
<i> : [bat meter] adc 4080 vbat 4070mV ocv 82% soc 74.56% t 1.0s i -2.3 -2.3mA
vbat1 = 4070 v2 = 4080, soc = 74, i_bat = -2310
wake reason = 4
imu int cnt = 85800
```

- **Framing.** One `printf` arrives as several notifications: `"<i> : "`, `"[bat meter] "`, the
  values, then `"\r\n"`. The `vbat1` line, `wake reason`, `PEN ENTER SLEEP` and `Charge State`
  each arrive as one notification **without** a line ending.
- **`[bat meter]`** — battery voltage (`vbat`, mV), state of charge to a hundredth of a percent
  (`soc`), and battery current (`i`, two readings, mA; negative while discharging). `adc`, `ocv`
  and `t` are logged too; their meaning was not needed.
- **`vbat1 = …`** — the same measurement again: `soc` as a whole percent, `i_bat` in µA.
- **`wake reason = N`** — `4` is the only value seen, on every wake by motion; the code table is
  unknown.
- **`imu int cnt = N`** — an interrupt counter for a motion sensor: the pen has an IMU, which is
  what wakes it.

Over four minutes of use the gauge read 74.56 % → 74.33 % and 4070 → 4050 mV; the current was
−2.3 mA idle and −5.7 mA in bursts while inking.

## 3. The charger [VERIFIED-HW]

Setting the pen on its wireless charger produced the same sequence every time (three times):

```
Charge State: Not Charging
<i> : [bat meter] adc 4115 vbat 4105mV ocv 86% soc 78.30% t 1.0s i +0.00 +0.00mA
vbat1 = 4105 v2 = 4115, soc = 78, i_bat = 0
PEN ENTER SLEEP
```

and the link dropped about 0.1 s later. The pen **sleeps while it charges**: there is no link,
so charging cannot be watched live over Bluetooth. It charges all the same: 74.3 % before one
3.5-minute stay on the charger, 78 % when lifted (about 1 % per minute). Lifting it reconnected
the link in under a second.

`Charge State: Not Charging` is logged at the instant the pen is set down, before the charger's
power can have reached it; the line is read as "I am on a charger", whatever its text says. A
`Charge State` line followed within a few seconds by `PEN ENTER SLEEP` is the dock signal
`px13pend` uses (§5).

The charger is a separate radio device (an NFC wireless power transmitter); it presents itself to
the laptop only as a power-only USB-C sink, with no data channel.

## 4. Not yet seen [OBSERVED]

- A sleep **without** a preceding `Charge State` line (the pen left idle off the charger). It is
  expected to log `PEN ENTER SLEEP` alone, which is why the dock rule needs both lines.
- Any `Charge State` text other than `Not Charging`.
- Any traffic on the three vendor services.

## 5. What `px13pend` does with it

`px13pend serve` subscribes to the log whenever a pen connects (BlueZ ends the subscription when
the link drops) and exports, additively to `Pen1` (docs/dbus.md §5):

- `BatteryPercentPrecise` and `BatteryMillivolts` from the `[bat meter]` line;
- `Docked`, set by the dock signal of §3 and cleared when the pen next connects;
- the capability key `battery-detail` = `"pen-log"` once the log has delivered a reading.

The parser is `px13pen-core::telemetry`; the subscription is `crates/px13pend/src/telemetry.rs`.
Enabling notifications is the only thing the daemon does on these characteristics.
