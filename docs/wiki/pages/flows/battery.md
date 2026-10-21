# Flow — battery, fuel gauge and the charger

Three independent sources describe the pen's power, at three resolutions.
[VERIFIED-HW — findings/006 §5, findings/010]

| Source | Transport | Resolution |
|---|---|---|
| Battery service notification | BLE, handle `0x0015` | whole percent (observed `0x50`/`0x4F`/`0x4D` = 80/79/77 %) — what Windows' UI showed |
| Report 7 Battery Strength | screen link, digitizer | raw 0–255; percentage mapping unknown [HYPOTHESIS] |
| Firmware debug log | Nordic UART TX, notifies | hundredths of a percent + mV + µA |

Whether report 7's byte and the Bluetooth percentage always agree is an
[open question](../open-questions.md).

## The firmware log [VERIFIED-HW findings/010 §2]

While the pen holds a link, its Nordic UART TX characteristic streams
plain ASCII about once a second, in use or not:

```
<i> : [bat meter] adc 4080 vbat 4070mV ocv 82% soc 74.56% t 1.0s i -2.3 -2.3mA
vbat1 = 4070 v2 = 4080, soc = 74, i_bat = -2310
wake reason = 4
imu int cnt = 85800
```

- **Framing is not line-based**: one `printf` arrives as several
  notifications (`"<i> : "`, `"[bat meter] "`, the values, `"\r\n"`);
  `vbat1`/`wake reason`/`PEN ENTER SLEEP`/`Charge State` each arrive as one
  notification **without** a line ending. A parser must buffer.
- `[bat meter]`: `vbat` in mV, `soc` to a hundredth of a percent, `i` =
  battery current in mA (negative discharging). `vbat1` repeats it: whole
  percent, µA. The pen draws −2.3 mA idle, −5.7 mA bursts while inking.
- `imu int cnt` — the pen has a motion sensor (IMU); it is what wakes the
  pen. `wake reason = 4` is the only value seen, at every wake; table unknown.
- Subscribing (`StartNotify`) is the only thing ever done on that
  characteristic; the project never writes the paired RX characteristic.

## The dock signal — the interesting one [VERIFIED-HW findings/010 §3]

Six dockings over two days, identical sequence every time:

```
 pen set on charger
   │
   ├─ "Charge State: Not Charging"        ← logged at the instant of set-down,
   │                                         before charger power arrives;
   │                                         this text alone MEANS NOTHING
   ├─ [bat meter] … i +0.00 … / i_bat = 0 ← the current has stopped: THE signal
   │                                         (off-charger sleep still draws −2.3 mA)
   ├─ "PEN ENTER SLEEP"                   ← within 0.3 s of the line (rule allows 3 s)
   └─ BLE link drops ~0.1 s later
```

Rules this produces, with the negatives that force them (findings/010 §4):

- **The text is not the signal** — a pen put to sleep off its charger logs
  the same line (2 of 3 times) while still discharging. The current
  stopping *is* the signal.
- **`wake reason = 4` before a dock is not it either** — it appears at
  every wake and before off-charger sleeps too.
- The pen **sleeps while it charges** — no link, so charging can't be
  watched live; it charges anyway (≈1 %/min measured: 74.3 % → 78 % across
  a 3.5-minute stay; `soc` rose across dockings all while logging "Not
  Charging"). Lifting reconnects in under a second.
- No `Charge State` text other than `Not Charging` has ever been seen, in
  any context.
- The charger is a separate radio device presenting only as a power-only
  USB-C sink — no data channel to the laptop.

## On Linux

`px13pend serve` subscribes whenever a pen connects (BlueZ ends the
subscription at link drop) and exports additively on `Pen1`
(docs/dbus.md §5): `BatteryPercentPrecise`, `BatteryMillivolts`, `Docked`
(set by the sequence above, cleared on next connect), and capability
`battery-detail="pen-log"`. Parser: `px13pen-core::telemetry`.

**BlueZ quirk for any second reader** [VERIFIED-HW 2026-10-21]: BlueZ
broadcasts a separate `PropertiesChanged` *per registered StartNotify
watcher* — with the daemon and a test client both subscribed, each heard
the log twice. Property updates are idempotent; anything *counting* log
events must deduplicate or filter by sender.
