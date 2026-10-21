# Flow — waking, reconnecting, and who the pen is today

The pen is frugal: the BLE link drops when idle and returns on use. A host
that treats Bluetooth addresses as identity will get this wrong — the pen
has **several identities** and one serial. [VERIFIED-HW — findings/006 §5,
findings/008 §4, §7]

## Lifecycle

```
 IN USE ──(idle)──► link drops ──► SLEEP (IMU-woken)
                                     │
              picked up / touched / wake reason = 4
                                     │
                                     ▼
                     wake-up ADVERTISEMENT (if it holds a pairing
                     for a host: advertises ~12 reports/s, ~40 s,
                     BEFORE the digitizer announces it)
                                     │
             ┌───────────────────────┴──────────────────────┐
             ▼                                               ▼
   host already paired with that                    pen holds no pairing
   identity → reconnect on the bond                 for this host → the
   (trusted device, no scan needed;                 touch-to-pair path in
   a wake touch alone brings it back)               [pairing](pairing.md)
```

- Reconnect cost on Linux: a wake touch alone brought the trusted bonded
  pen back — no active scan trick needed (findings/009 §1).
- On reconnect the host re-reads the feature reports (`0x42`/`0x40`) —
  the Windows host does exactly this (findings/006 §5).
- **Back-off**: after a failed pairing attempt the pen can skip the next
  touch entirely for ~2 minutes — measured as a run where the touch came
  18 s after a link drop and produced *no* advertisement. Keep the scan
  armed; don't conclude "broken" from one missed touch.

## Identity: which advertiser is my pen? [VERIFIED-HW findings/008 §4]

Three addresses can exist per pen, and they change: the address the
digitizer announced (`0x6E`), the per-host bonded identity, and the button
pairing mode identity (hold the top button until the LED blinks white —
the fallback for a host the pen has never met). Meanwhile announcements
keep arriving **even while the pen is connected** under another identity.

The only stable handle is the **transducer serial**: reports 7/34 on the
screen link equal the connected pen's `0x40` / (Linux) `0x2A4D` value.
`px13pend` matches pens by it (docs/decisions.md).

## What `px13pend` does [VERIFIED-HW findings/008 §7–8]

The daemon's failure history is the reason for its shape: two early runs
failed because the pen advertised *before* the announcement (9 ms and
4.4 s ahead), and the daemon removed that advertiser as a leftover before
sending the key. So:

- watch report 7; when the pen is in range, keep an LE scan armed for 20 s;
- an advertiser already visible when `0x6E` arrives is paired/reconnected
  directly — it is not a leftover;
- an advertiser appearing within 3 s of an announcement while the daemon
  believed the pen bonded is treated as a **stale bond** and paired fresh;
- announcements whose serial matches an already-connected pen are ignored
  (the pen is talking to another host's history, not asking for a new link).

## Open

The daemon's from-nothing path (a pen that has never met this host, with
no button-pairing assist) awaits a second pen or a safe way to make the
pen forget — see [pairing](pairing.md#status-on-linux).
