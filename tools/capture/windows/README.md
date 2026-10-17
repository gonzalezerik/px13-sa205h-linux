# Windows capture kit — how the shipped pen stack really behaves

These sessions capture what the Windows driver stack does with the ASUS pen and
the ELAN digitizer, so the Linux implementation can be built against evidence
instead of guesses: what is sent to the digitizer at boot, the real report
rate and pressure behaviour, the tap-to-pair flow, the haptics host, and where
the settings live.

**This runs on the laptop's own dual-boot Windows install** (Windows 11 Pro
25H2, `nvme0n1p3`, booted from the left rEFInd entry or the ASUS boot menu).
The machine is already set up: the AMD/MediaTek/ELAN/DialPad **drivers** are
installed, no ASUS apps or services are, and Windows was configured offline
before the first boot (Defender/SmartScreen off, telemetry minimal, no driver
or firmware offers from Windows Update, BitLocker blocked, hibernation off,
clock in UTC). Do not undo any of that during a session.

Everything you capture is personal (device identifiers, Bluetooth link keys):
the import step copies it into `captures/windows/`, which is gitignored — keep
it that way.

## Safety — read before the first boot

- **Decline every firmware or BIOS offer, everywhere** — Windows Update, Device
  Manager, and any app dialog. A BIOS update resets custom firmware settings
  (the memory carve-out included). Never run a firmware updater, ELAN or
  otherwise.
- **Don't sign in with a Microsoft account.** The local account is set up; a
  Microsoft account pulls in BitLocker device encryption and sync.
- **Don't re-enable Defender, SmartScreen, driver updates or hibernation.**
  Hibernation staying off is what lets Linux read the NTFS partition.
- The hardware clock is UTC; Windows may switch it to local time when it
  boots. If the clock looks wrong on the next Linux boot, that's why — NTP
  fixes it. Timestamps inside Windows sessions come from Windows itself and
  stay consistent with the traces.
- The capture folder holds Bluetooth link keys and pen serials. Treat it like
  a password.

## What to copy to Windows (before the first session)

Copy these into `C:\kit\` on the Windows partition — from NixOS with the
partition mounted read-write, or via a FAT/exFAT USB stick:

| Item | Where it goes |
|---|---|
| This folder (`tools/capture/windows/`) | `C:\kit\` |
| `tools/pen-test.html` | `C:\kit\pen-test.html` |
| The three trace files (see [Pinned trace files](#pinned-trace-files)), downloaded and hash-checked | `C:\kit\` |
| Sysinternals Process Monitor (`Procmon64.exe`, from learn.microsoft.com/en-us/sysinternals/downloads/procmon) | `C:\kit\` |
| Microsoft Whiteboard (only needed for session 4; install from the Microsoft Store) | — |

Nothing else needs installing. All commands run in an **elevated** Command
Prompt (Start → `cmd` → Run as administrator).

## Session log

Create the capture tree and start a log in **every** session:

```bat
mkdir C:\mpp-captures
cd /d C:\kit
log "capture kit ready"
```

`log.cmd` appends an ISO-8601 wall-clock line to
`C:\mpp-captures\session-log.txt`. Timestamp **before and after every physical
action** (pen touch, button press, slider move) so the traces can be lined up
with what you did. Each session writes into its own folder
`C:\mpp-captures\<NN-name>\`.

## Session 00 — inventory

Answers: which drivers own the pen/digitizer, and where their settings are.

```bat
mkdir C:\mpp-captures\00-inventory
cd /d C:\mpp-captures\00-inventory
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
- Device Manager: the digitizer (Human Interface Devices), the pen devices,
  and the Bluetooth pen entries if paired — Details tab → “Device instance
  path” for each.

## Session 01 — boot trace: what Windows sends to the digitizer

Answers the “digitizer configuration” parity item: every feature report / bus
transfer Windows performs at startup, to compare with what `hid-multitouch`
sends at probe on Linux.

```bat
mkdir C:\mpp-captures\01-boot && cd /d C:\mpp-captures\01-boot
copy C:\kit\BusesAllProfile.wprp .
copy C:\kit\SpbCxWppTrace.cmd .
log "s01: registering boot traces"
wpr -start BusesAllProfile.wprp!InputOnlyProfile.Verbose.File -start
SpbCxWppTrace.cmd start autologger
shutdown /r /t 0
```

After Windows comes back: log in, **do not touch the pen for 2 minutes**
(boot-time traffic only), then:

```bat
cd /d C:\mpp-captures\01-boot
log "s01: post-boot baseline done; now hover + one slow line"
(timeout 120)
```

Hover the pen over the screen for ~5 s, draw one slow line, then:

```bat
log "s01: stopping boot traces"
wpr -stop C:\mpp-captures\01-boot\boot-input.etl
SpbCxWppTrace.cmd stop autologger
copy C:\PerfLogs\Admin\SpbCxWpp_Session\*.etl C:\mpp-captures\01-boot\
```

If `wpr -start … -start` rejects the profile, fall back to
`usb/tracing/BusesTrace.cmd` from the same pinned repo (same bus providers,
built-in boot-trace mode) and note it in the session log.

## Session 02 — inking quality

Answers: report rate, jitter, pressure range, tilt/azimuth, eraser — measured
end-to-end through the same page we will run on Linux.

```bat
mkdir C:\mpp-captures\02-inking && cd /d C:\mpp-captures\02-inking
wpr -start BusesAllProfile.wprp!InputOnlyProfile.Verbose.File
log "s02: input trace started"
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
`pen-test-s02.json` in the session folder, then:

```bat
wpr -stop C:\mpp-captures\02-inking\input.etl
log "s02: done"
```

Also open Settings → Bluetooth & devices → Pen & Windows Ink and run any
built-in pen test/preview it offers while the trace is still running, if the
page was not already open.

## Session 03 — first tap-to-pair (the priority)

Answers: the seamless-pairing flow end to end (which side initiates, what the
digitizer and the pen exchange over Bluetooth, what Windows does with it).
This is the decisive evidence for the pairing protocol write-up.

The pen has **never been paired with this Windows install**, so the first
touch is the real first pairing. (If a later session already paired it —
Settings → Bluetooth & devices shows the pen — remove it first and say so in
the log.)

```bat
mkdir C:\mpp-captures\03-pairing && cd /d C:\mpp-captures\03-pairing
wpr -start BluetoothStack.wprp!BluetoothStack.Verbose.File BusesAllProfile.wprp!InputOnlyProfile.Verbose.File
log "s03: traces started, pen not paired"
```

Touch the pen tip to the screen (the ASUS seamless-pairing gesture), wait for
Windows to finish pairing (toast / device appears in Settings). If nothing
pairs within 60 s, hover the pen near the top edge and try again; log every
attempt:

```bat
log "s03: tap-to-pair attempt 1"
...
log "s03: paired (or: failed after N attempts)"
```

Screenshot the pen's entry in Settings → Bluetooth & devices (its advertised
name), then:

```bat
wpr -stop C:\mpp-captures\03-pairing\pair.etl
pnputil /enum-devices /connected /class HID > hid-after.txt
```

## Session 04 — haptics

Answers: when the haptics host triggers, what intensity/on-off map to, which
apps get haptic ink.

```bat
mkdir C:\mpp-captures\04-haptics && cd /d C:\mpp-captures\04-haptics
wpr -start BusesAllProfile.wprp!InputOnlyProfile.Verbose.File
log "s04: trace started"
```

With the pen paired over Bluetooth:

1. Settings → Pen & Windows Ink: screenshot the haptics section; toggle
   haptics off, on; move the intensity slider to each stop, `log` each change.
2. Open an inking app that offers pen tools (Whiteboard, Paint, Sticky Notes):
   draw with each tool (pen/pencil/marker/eraser) for ~5 s each, `log` each
   tool switch. Note audibly/physically which tools actually vibrate.
3. Press/release the barrel button and the top button while drawing.

```bat
wpr -stop C:\mpp-captures\04-haptics\haptics.etl
log "s04: done"
```

## Session 05 — top and barrel buttons

Answers: what the buttons send over Bluetooth HID (single/double/long press)
and what Windows turns them into.

```bat
mkdir C:\mpp-captures\05-buttons && cd /d C:\mpp-captures\05-buttons
wpr -start BusesAllProfile.wprp!InputOnlyProfile.Verbose.File
log "s05: trace started"
```

On the desktop, one action at a time, `log` before each, 3 s gap between:

- top button: single press ×3, double press ×3, long press (≥ 1 s) ×3
- barrel button: same nine presses
- note on screen what each one did (screenshot the result of each kind once)

```bat
wpr -stop C:\mpp-captures\05-buttons\buttons.etl
log "s05: done"
```

## Session 06 — ASUS app pen settings (only when needed)

Answers: which registry/file keys each ASUS pen setting writes. **Skip this
session until the app that actually holds the pen settings is known** — the
inbox Settings pages may be all there is. If it runs, install that one app
first and decline every firmware offer it makes.

Start `Procmon64.exe` (accept the EULA), Filter → filter to
`Process Name` `contains` `ASUS` **or** the app's own process names (add the
exact names you see), `Operation` `is` `RegSetValue`, plus a second include
rule `RegCreateKey`. File system writes from the same processes too.

```bat
mkdir C:\mpp-captures\06-appsettings && cd /d C:\mpp-captures\06-appsettings
reg export HKCU\SOFTWARE reg-hkcu-before.reg /y
log "s06: procmon running, baseline exported"
```

In the app, change **one pen setting at a time** (button remap, pressure,
haptics, handedness, anything pen-related you find), `log` each change, wait
2 s, then set it back. Export Procmon as PML **and** CSV into the session
folder. Finish with:

```bat
reg export HKCU\SOFTWARE reg-hkcu-after.reg /y
log "s06: done"
```

## Importing back to Linux

Reboot into NixOS and run, from the repo root:

```
sudo tools/capture/import-windows.sh
```

It mounts the Windows partition read-only, copies `C:\mpp-captures\*` into
`captures/windows/`, writes a SHA-256 manifest, and unmounts.

## Decoding the captures

- `.etl` from `wpr`: decode **in the same Windows session** — on the laptop,
  `tracerpt file.etl -of csv -o <name>-csv\ -y` gives CSVs that read fine on
  Linux. WPP providers (the SPB/I2C trace) need symbol lookup: set
  `_NT_SYMBOL_PATH=srv*C:\symbols*https://msdl.microsoft.com/download/symbols`
  before `tracerpt`. Windows Performance Analyzer (WPA) opens the ETLs
  directly and is the better tool for the HID input events.
- On Linux there is no reliable decoder for these ETLs (WPP needs Microsoft
  TMFs), so anything not decoded on Windows stays raw evidence.
- Procmon `.pml` files open in Procmon on Windows; export CSV for Linux-side
  diffing.
- `pen-test-s02.json` is already Linux-readable — diff it against the same page
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
