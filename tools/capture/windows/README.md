# Windows capture kit — how the shipped pen stack really behaves

These sessions capture what the Windows driver stack does with the ASUS pen and
the ELAN digitizer, so the Linux implementation can be built against evidence
instead of guesses: what is sent to the digitizer at boot, the real report
rate and pressure behaviour, the tap-to-pair flow, the haptics host, and where
the settings live.

**Run all of this from a Windows To Go USB SSD. Never boot Windows from (or
into) the laptop's internal disk.** Everything you capture is personal (device
identifiers, Bluetooth link keys): keep the stick and the capture folder
private.

## Safety — read before the first boot

- **Local account only.** A Microsoft account pulls in BitLocker device
  encryption and sync; a plain local account avoids both.
- **Stop firmware before anything else.** Windows Update and the ASUS apps can
  push **BIOS, ELAN digitizer and pen firmware**. A BIOS update resets custom
  firmware settings (memory carve-outs included). Before the first network
  connection:
  1. Settings → Windows Update → Advanced options → turn **off** “Receive
     updates for other Microsoft products” and get the optional/driver
     offers off.
  2. Pause updates (5 weeks).
  3. In MyASUS / ProArt Creator Hub / ScreenXpert, **decline every firmware or
     driver offer**. Never accept a firmware update from any of them.
- **Never run firmware updaters** (ELAN or otherwise), even if offered.
- Windows sets the hardware clock to local time; the Linux install fixes it
  back via NTP on first boot. Cosmetic, but expect a wrong clock on the first
  Linux boot after a Windows session.
- The capture stick holds Bluetooth link keys and pen serials. Treat it like a
  password.

## What to copy onto the stick (before first boot)

| Item | Where it goes |
|---|---|
| This folder (`tools/capture/windows/`) | `C:\kit\` |
| `tools/pen-test.html` | `C:\kit\pen-test.html` |
| The three trace files (see [Pinned trace files](#pinned-trace-files)), downloaded and hash-checked | `C:\kit\` |
| Sysinternals Process Monitor (`Procmon64.exe`) | `C:\kit\` |
| Windows Performance Analyzer (optional, or install later on the stick) | — |

Create the capture tree and start a log in **every** session:

```bat
mkdir C:\captures
cd /d C:\kit
log "capture kit ready"
```

`log.cmd` appends an ISO-8601 wall-clock line to `C:\captures\session-log.txt`.
Timestamp **before and after every physical action** (pen touch, button press,
slider move) so the traces can be lined up with what you did. All commands run
in an **elevated** Command Prompt (Start → `cmd` → Run as administrator).

## Session 0 — inventory

Answers: which drivers own the pen/digitizer, and where their settings are.

```bat
mkdir C:\captures\s0-inventory
cd /d C:\captures\s0-inventory
pnputil /enum-devices /connected /drivers > pnputil-connected.txt
pnputil /enum-drivers > pnputil-drivers.txt
mkdir drivers
pnputil /export-driver * drivers
reg export HKLM\SYSTEM\CurrentControlSet\Enum\HID reg-hid.reg /y
reg export HKLM\SYSTEM\CurrentControlSet\Enum\Bluetooth reg-bluetooth.reg /y
reg export HKLM\SYSTEM\CurrentControlSet\Services\BTHPORT\Parameters\Keys reg-bthport-keys.reg /y
for /f "tokens=*" %%k in ('reg query HKLM\SYSTEM\CurrentControlSet\Services ^| findstr /i "elan asus mediatek"') do reg export "%%k" "svc-%%~nk.reg" /y
reg export HKLM\SOFTWARE\ASUS reg-asus-hklm.reg /y
reg export HKCU\SOFTWARE\ASUS reg-asus-hkcu.reg /y
reg export HKLM\SOFTWARE\ELAN reg-elan.reg /y 2>nul
```

`reg-bthport-keys.reg` contains the Bluetooth **link keys** — private.

Then screenshots (Win+Shift+S, save into the session folder) of:

- Settings → Bluetooth & devices → Pen & Windows Ink (every sub-page,
  including anything haptics- or pressure-related),
- the pen pages in MyASUS, ProArt Creator Hub, ScreenXpert (if present),
- Device Manager: the digitizer (Human Interface Devices), the pen devices,
  and the Bluetooth pen entries if paired — Details tab → “Device instance
  path” for each.

## Session 1 — boot trace: what Windows sends to the digitizer

Answers parity item “digitizer configuration”: every feature report / bus
transfer Windows performs at startup, to compare with what `hid-multitouch`
sends at probe on Linux.

```bat
mkdir C:\captures\s1-boot && cd /d C:\captures\s1-boot
copy C:\kit\BusesAllProfile.wprp .
copy C:\kit\SpbCxWppTrace.cmd .
log "s1: registering boot traces"
wpr -start BusesAllProfile.wprp!InputOnlyProfile.Verbose.File -start
SpbCxWppTrace.cmd start autologger
shutdown /r /t 0
```

After Windows comes back: log in, **do not touch the pen for 2 minutes**
(boot-time traffic only), then:

```bat
cd /d C:\captures\s1-boot
log "s1: post-boot baseline done; now hover + one slow line"
(timeout 120)
```

Hover the pen over the screen for ~5 s, draw one slow line, then:

```bat
log "s1: stopping boot traces"
wpr -stop C:\captures\s1-boot\boot-input.etl
SpbCxWppTrace.cmd stop autologger
copy C:\PerfLogs\Admin\SpbCxWpp_Session\*.etl C:\captures\s1-boot\
```

If `wpr -start … -start` rejects the profile, fall back to
`usb/tracing/BusesTrace.cmd` from the same pinned repo (same bus providers,
built-in boot-trace mode) and note it in the session log.

## Session 2 — inking quality

Answers: report rate, jitter, pressure range, tilt/azimuth, eraser — measured
end-to-end through the same page we will run on Linux.

```bat
mkdir C:\captures\s2-inking && cd /d C:\captures\s2-inking
cd /d C:\captures\s2-inking
wpr -start BusesAllProfile.wprp!InputOnlyProfile.Verbose.File
log "s2: input trace started"
```

Open `C:\kit\pen-test.html` in Edge, press **Start** in the page, then perform,
with a `log` line before each:

1. hover without touching, 5 s
2. pressure ramp: lightest touch → maximum, one slow stroke
3. full pressure range: press as hard as comfortable, hold, release
4. tilt sweep: pen upright → near-flat and back, drawing slowly
5. rotate the pen around its axis (azimuth) while drawing
6. eraser end: several strokes (note: does it erase? does pressure respond?)
7. barrel button held while drawing (note what happens)
8. fast zig-zag across the screen, 5 s
9. slow straight line, top to bottom, 5 s

Press **Stop** in the page, **Export JSON** → save as
`pen-test-s2.json` in the session folder, then:

```bat
wpr -stop C:\captures\s2-inking\input.etl
log "s2: done"
```

Also open Settings → Bluetooth & devices → Pen & Windows Ink and run any
built-in pen test/preview it offers while the trace is still running, if the
page was not already open.

## Session 3 — first tap-to-pair

Answers: the seamless-pairing flow end to end (which side initiates, what the
digitizer and the pen exchange over Bluetooth, what Windows does with it).

First **remove the pen from Windows' paired devices**: Settings → Bluetooth &
devices → the pen → Remove device. Then:

```bat
mkdir C:\captures\s3-pairing && cd /d C:\captures\s3-pairing
wpr -start BluetoothStack.wprp!BluetoothStack.Verbose.File BusesAllProfile.wprp!InputOnlyProfile.Verbose.File
log "s3: traces started, pen removed from BT list"
```

Touch the pen tip to the screen (the ASUS seamless-pairing gesture), wait for
Windows to finish pairing (toast / device appears in Settings). If nothing
pairs within 60 s, hover the pen near the top edge and try again; log every
attempt:

```bat
log "s3: tap-to-pair attempt 1"
...
log "s3: paired (or: failed after N attempts)"
wpr -stop C:\captures\s3-pairing\pair.etl
pnputil /enum-devices /connected /class HID > hid-after.txt
```

Screenshot the pen's entry in Settings → Bluetooth & devices (its advertised
name) before stopping the trace.

## Session 4 — haptics

Answers: when the haptics host triggers, what intensity/on-off map to, which
apps get haptic ink.

```bat
mkdir C:\captures\s4-haptics && cd /d C:\captures\s4-haptics
wpr -start BusesAllProfile.wprp!InputOnlyProfile.Verbose.File
log "s4: trace started"
```

With the pen paired over Bluetooth:

1. Settings → Pen & Windows Ink: screenshot the haptics section; toggle
   haptics off, on; move the intensity slider to each stop, `log` each change.
2. Open an inking app that offers pen tools (Whiteboard, Paint, Sticky Notes):
   draw with each tool (pen/pencil/marker/eraser) for ~5 s each, `log` each
   tool switch. Note audibly/physically which tools actually vibrate.
3. Press/release the barrel button and the top button while drawing.

```bat
wpr -stop C:\captures\s4-haptics\haptics.etl
log "s4: done"
```

## Session 5 — top and barrel buttons

Answers: what the buttons send over Bluetooth HID (single/double/long press)
and what Windows turns them into.

```bat
mkdir C:\captures\s5-buttons && cd /d C:\captures\s5-buttons
wpr -start BusesAllProfile.wprp!InputOnlyProfile.Verbose.File
log "s5: trace started"
```

On the desktop, one action at a time, `log` before each, 3 s gap between:

- top button: single press ×3, double press ×3, long press (≥1 s) ×3
- barrel button: same nine presses
- note on screen what each one did (screenshot the result of each kind once)

```bat
wpr -stop C:\captures\s5-buttons\buttons.etl
log "s5: done"
```

## Session 6 — ASUS app pen settings

Answers: which registry/file keys each ASUS pen setting writes.

Start `Procmon64.exe` (accept the EULA), Filter → filter to
`Process Name` `contains` `ASUS` **or** `CreatorHub` **or** `ScreenXpert`
(add the exact process names you see), `Operation` `is` `RegSetValue`, plus a
second include rule `RegCreateKey`. File system writes from the same processes
too (`RegIO` and `filesystem`).

```bat
mkdir C:\captures\s6-appsettings && cd /d C:\captures\s6-appsettings
reg export HKCU\SOFTWARE reg-hkcu-before.reg /y
log "s6: procmon running, baseline exported"
```

In each app, change **one pen setting at a time** (button remap, pressure,
haptics, handedness, anything pen-related you find), `log` each change, wait
2 s, then set it back. Export Procmon as PML **and** CSV into the session
folder per app. Finish with:

```bat
reg export HKCU\SOFTWARE reg-hkcu-after.reg /y
log "s6: done"
```

## Decoding the captures

- `.etl` from `wpr`: on Windows, `tracerpt file.etl -of csv -o csvdir\ -y`
  gives CSVs that read fine on Linux. WPP providers (the SPB/I2C trace) need
  symbol/TMF lookup: set `_NT_SYMBOL_PATH=srv*C:\symbols*https://msdl.microsoft.com/download/symbols`
  before `tracerpt`. Windows Performance Analyzer (WPA) opens the ETLs
  directly and is the better tool for the HID input events.
- On Linux there is no reliable decoder for these ETLs (WPP needs Microsoft
  TMFs). Decode on the stick, keep the raw ETLs as evidence.
- Procmon `.pml` files open in Procmon on Windows; export CSV for Linux-side
  diffing.
- `pen-test-s2.json` is already Linux-readable — diff it against the same page
  run on Linux (same file, `tools/pen-test.html`).

## Pinned trace files

From <https://github.com/microsoft/busiotools>, branch `master`, commit
`f9c6f70775276c1b82c3541317820b489e59a9f9` (fetched 2026-10-17). Verify each
SHA-256 after download (`certutil -hashfile <file> SHA256`):

| File (repo path) | SHA-256 |
|---|---|
| `usb/tracing/BusesAllProfile.wprp` | `3ef04c549d327204eeabb6be31fd063507629ad97516e7221c5af13d45b9bfaf` |
| `spb/tracing/SpbCxWppTrace.cmd` | `7b4cb84b2bdc9456b916484c7d21f7588cd641acb6c67f707b78f36ac4c812c9` |
| `bluetooth/tracing/BluetoothStack.wprp` | `d69c983384b970e8c0380c48ebebba547f94c4a84c9f4912a10798b6b08cf704` |

Profile names used above (verified inside the files):
`InputOnlyProfile.Verbose.File`, `BluetoothStack.Verbose.File`.
`SpbCxWppTrace.cmd` traces the `SpbCxWpp` provider
(`{E6086B4D-AEFF-472B-BDA7-EEC662AFBF11}`, level 5, all keywords) and writes
its autologger ETL under `C:\PerfLogs\Admin\SpbCxWpp_Session\`.
