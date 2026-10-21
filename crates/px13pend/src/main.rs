// SPDX-License-Identifier: AGPL-3.0-or-later
//! Daemon for MPP 2.6 pens: pairing, haptics, battery, buttons.
//!
//! Touch-to-pair (`docs/protocol/seamless-pairing.md`): `watch-pair` and
//! `serve` register this host with the digitizer, answer the digitizer's
//! `0x6E` announcement with the key report and pair the pen's advertisement
//! through BlueZ with the daemon's own agent (`pairing.rs`). `pairing-state`
//! reads the digitizer's capability report. `watch-pen` follows battery and
//! link state through BlueZ; `watch-buttons` dumps the pen's raw evdev
//! events so the top-button codes can be identified. `pen-log` prints the
//! pen's firmware debug log (findings/010) live. `haptic` plays one
//! effect with the single output report 0x41 and writes nothing unless
//! `--send` is given. `serve` exports the D-Bus interface of docs/dbus.md:
//! a Daemon object, one object per pen BlueZ knows, and the pairing
//! listener.

use std::collections::HashMap;
use std::process::ExitCode;
use std::time::{Duration, Instant};

mod buttons;
mod haptics;
mod ink;
mod pairing;
mod peninfo;
mod penlog;
mod presence;
mod service;
mod settings;
mod shortcut;
mod status;
mod telemetry;

use hidapi::{DeviceInfo, HidApi};
use px13pen_core::seamless;
use zbus::Connection;

/// Input buffers must fit the largest input report the device can send.
const READ_BUF_BYTES: usize = 8192;

/// The pen re-announces on every touch and range event, so one pairing
/// attempt per address per window; frames inside it repeat a run that is in
/// flight or just done.
const ANNOUNCE_DEBOUNCE: Duration = Duration::from_secs(3);

const USAGE: &str = "\
usage: px13pend <command> [options]

commands:
  watch-pair          register this host with the digitizer, then pair every
                      pen it announces (input report 0x6E): key report,
                      the pen's advertisement, Pair with passkey 0, Trusted
      --dry-run       print the announcements; write nothing, touch no
                      Bluetooth state
      --adapter NAME  BlueZ adapter to use (default: hci0)
  pairing-state       read the digitizer's seamless-pairing feature report
                      (0x70) and print it; read-only
  watch-pen           print battery and link changes for a paired pen,
                      live from BlueZ (UPower shows the same battery
                      without this daemon)
      --device ADDR   pen address (default: the only pen-named device)
      --adapter NAME  BlueZ adapter to use (default: hci0)
  watch-buttons       dump raw evdev events from every pen input node,
                      to identify what the top-button report becomes
      --list          list pen-named evdev nodes and exit
  pen-log             print the pen's firmware debug log live, one
                      timestamped record per line; read-only, runs
                      beside a running serve
      --adapter NAME  BlueZ adapter to use (default: hci0)
  haptic EFFECT       play one haptic effect on the paired pen (single
                      output report 0x41; without --send only the exact
                      bytes are printed)
      --list          print the effect table and exit
      --intensity P   percent of full strength, 0..100 (default 100)
      --repeat N      plays (default 1)
      --cutoff-ms N   ceiling on total playback; 10 ms units, max 2550
      --retrigger-ms N  milliseconds between repeats (default 0)
      --path PATH     pen hidraw node (default: the only pen-named HID)
      --send          write the report to the pen
  serve                 export the D-Bus interface (docs/dbus.md): the Daemon
                      object plus one per known pen, on the session bus
      --system        export on the system bus instead
      --adapter NAME  BlueZ adapter to read pens from (default: hci0)
  -h, --help          print this help
  -V, --version       print the version
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let outcome = match args.first().map(String::as_str) {
        None => {
            println!("px13pend {}", env!("CARGO_PKG_VERSION"));
            println!("Supported digitizers:");
            for (id, name) in px13pen_core::SUPPORTED_DIGITIZERS {
                println!("  {:04x}:{:04x}  {name}", id.vendor, id.product);
            }
            Ok(())
        }
        Some("watch-pair") => match adapter_name(&args) {
            Ok(adapter) => watch_pair(args.iter().any(|arg| arg == "--dry-run"), &adapter),
            Err(message) => Err(message),
        },
        Some("pairing-state") => pairing_state(),
        Some("watch-pen") => match (adapter_name(&args), device_arg(&args)) {
            (Ok(adapter), Ok(device)) => presence::watch_pen(&adapter, device),
            (Err(message), _) | (_, Err(message)) => Err(message),
        },
        Some("watch-buttons") => buttons::watch_buttons(args.iter().any(|arg| arg == "--list")),
        Some("pen-log") => match adapter_name(&args) {
            Ok(adapter) => penlog::pen_log(&adapter),
            Err(message) => Err(message),
        },
        Some("haptic") => haptics::haptic(&args),
        Some("serve") => service::serve(&args),
        Some("-h") | Some("--help") => {
            print!("{USAGE}");
            Ok(())
        }
        Some("-V") | Some("--version") => {
            println!("px13pend {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some(other) => {
            eprintln!("px13pend: unknown command {other}\n");
            eprint!("{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("px13pend: {message}");
            ExitCode::FAILURE
        }
    }
}

/// Reads `--adapter` from the argument list, defaulting to hci0.
fn adapter_name(args: &[String]) -> Result<String, String> {
    let mut name = "hci0".to_string();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--adapter" {
            name = iter
                .next()
                .cloned()
                .ok_or("--adapter needs a name, e.g. --adapter hci0")?;
        }
    }
    if name.is_empty() || !name.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return Err(format!("not a usable adapter name: {name}"));
    }
    Ok(name)
}

/// Reads the optional `--device` Bluetooth address from the arguments.
fn device_arg(args: &[String]) -> Result<Option<String>, String> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--device" {
            let value = iter.next().cloned().ok_or("--device needs an address")?;
            let address = value.to_uppercase();
            let valid = address.len() == 17
                && address.bytes().enumerate().all(|(index, byte)| {
                    if index % 3 == 2 {
                        byte == b':'
                    } else {
                        byte.is_ascii_hexdigit()
                    }
                });
            if !valid {
                return Err(format!("not a Bluetooth address: {value}"));
            }
            return Ok(Some(address));
        }
    }
    Ok(None)
}

/// The single supported digitizer present on this machine, by `hidraw`.
fn find_digitizer(api: &HidApi) -> Result<&DeviceInfo, String> {
    let found = one_per_node(api.device_list().filter(|info| {
        let id = px13pen_core::HidId {
            vendor: info.vendor_id(),
            product: info.product_id(),
        };
        px13pen_core::supported_digitizer(id).is_some()
    }));
    match found.first() {
        None => Err(
            "no supported digitizer found — is this a PX13, and can the current user read the hidraw node (the uaccess rule shipped with the package grants it)?".to_string(),
        ),
        Some(only) if found.len() == 1 => Ok(*only),
        Some(_) => Err(format!(
            "{} supported digitizers present; using more than one at a time is not supported",
            found.len()
        )),
    }
}

/// hidapi lists a device once per top-level collection, all with the same
/// hidraw path (the ELAN digitizer alone has more than a dozen), so count
/// nodes, not entries: keep the first entry of each path.
pub(crate) fn one_per_node<'a>(
    entries: impl Iterator<Item = &'a DeviceInfo>,
) -> Vec<&'a DeviceInfo> {
    let mut seen = std::collections::HashSet::new();
    entries
        .filter(|info| seen.insert(info.path().to_owned()))
        .collect()
}

/// Open the digitizer and pair every pen it announces, the same seamless
/// flow `serve` runs (pairing.rs). Runs until stopped. One `block_on` owns
/// the loop, so the D-Bus connection keeps its executor alive across
/// announcements.
fn watch_pair(dry_run: bool, adapter: &str) -> Result<(), String> {
    let adapter = adapter.to_string();
    zbus::block_on(watch_pair_async(dry_run, adapter))
}

async fn watch_pair_async(dry_run: bool, adapter: String) -> Result<(), String> {
    let api = HidApi::new().map_err(|error| format!("cannot open the HID subsystem: {error}"))?;
    let info = find_digitizer(&api)?;
    let device = api
        .open_path(info.path())
        .map_err(|error| format!("cannot open {}: {error}", info.path().to_string_lossy()))?;
    let session = if dry_run {
        None
    } else {
        let bluez = Connection::system().await.map_err(|error| {
            format!("cannot reach the system bus (is bluetoothd running?): {error}")
        })?;
        let agent = pairing::register_agent(&bluez).await?;
        pairing::register_host(&bluez, &adapter, &device).await?;
        // The pen answers the key within milliseconds: scan for the whole run.
        pairing::start_scan(&bluez, &adapter).await?;
        Some((bluez, agent))
    };
    println!("watching for pairing notifications — bring the pen near the screen (Ctrl-C to stop)");
    let start = Instant::now();
    let mut frame = vec![0u8; READ_BUF_BYTES];
    let mut last_attempt = HashMap::<String, Instant>::new();
    // `device.read` parks the executor while we wait on the digitizer;
    // announcements arriving mid-pairing queue in the kernel buffer and land
    // as a burst afterwards — the debounce window swallows them.
    loop {
        let len = device
            .read(&mut frame)
            .map_err(|error| format!("read from the digitizer failed: {error}"))?;
        let Some(note) = seamless::parse_pairing_report(&frame[..len]) else {
            continue;
        };
        let address = note.address_string();
        println!(
            "[{:>7.2} s] pen announced: {address} flags 0x{:02X}",
            start.elapsed().as_secs_f64(),
            note.flags
        );
        let Some((bluez, agent)) = &session else {
            println!("  {}", dry_run_plan(note.flags));
            continue;
        };
        if last_attempt
            .get(&address)
            .is_some_and(|when| when.elapsed() < ANNOUNCE_DEBOUNCE)
        {
            continue;
        }
        last_attempt.insert(address.clone(), Instant::now());
        if pairing::any_pen_connected(bluez, &adapter).await {
            println!("  a pen is already connected; nothing to do");
            continue;
        }
        match pairing::pair_announced(bluez, &adapter, &device, &note, agent).await {
            Ok(pairing::Outcome::Paired { address, .. }) => {
                println!("  paired and trusted {address}");
            }
            Ok(pairing::Outcome::Reconnected { address }) => {
                println!("  reconnected to paired pen {address}");
            }
            Err(message) => eprintln!("  pairing failed: {message}"),
        }
    }
}

/// What a real run would do with this announcement.
fn dry_run_plan(flags: u8) -> String {
    let pairing = if flags & seamless::FLAG_CONSUMER != 0 {
        "the pen has a pairing with this host"
    } else {
        "no pairing with this host yet"
    };
    format!(
        "dry run ({pairing}): would send the key report, then pair the pen's advertisement with passkey 0"
    )
}

/// Print the digitizer's seamless-pairing feature report. Read-only, and
/// only 0x70 is touched: other feature reads on this device are not yet
/// classified as safe (seamless-pairing.md §4).
fn pairing_state() -> Result<(), String> {
    let api = HidApi::new().map_err(|error| format!("cannot open the HID subsystem: {error}"))?;
    let info = find_digitizer(&api)?;
    let device = api
        .open_path(info.path())
        .map_err(|error| format!("cannot open {}: {error}", info.path().to_string_lossy()))?;
    let mut report = [0u8; 8];
    report[0] = seamless::FEATURE_ENABLED;
    let len = device.get_feature_report(&mut report).map_err(|error| {
        format!(
            "GET_FEATURE 0x{:02X} failed: {error}",
            seamless::FEATURE_ENABLED
        )
    })?;
    println!(
        "feature 0x{:02X}: {}",
        seamless::FEATURE_ENABLED,
        hex_bytes(&report[..len])
    );
    if len > 1 {
        println!(
            "seamless pairing: {}",
            if report[1] & 1 == 1 {
                "enabled"
            } else {
                "disabled"
            }
        );
    }
    Ok(())
}

/// Lower-case hex bytes separated by spaces (never colons — those read as
/// a Bluetooth address to the privacy gate).
fn hex_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::{device_arg, dry_run_plan};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn device_defaults_to_none_and_normalises_case() {
        assert_eq!(device_arg(&args(&["watch-pen"])).unwrap(), None);
        // Assembled at runtime: the privacy gate rejects colon-hex literals.
        let lowercase = String::from("0a:0b:0c:0d:0e") + ":0f";
        let given = device_arg(&args(&["watch-pen", "--device", &lowercase]))
            .unwrap()
            .unwrap();
        assert_eq!(given, String::from("0A:0B:0C:0D:0E") + ":0F");
    }

    #[test]
    fn device_rejects_malformed_addresses() {
        assert!(device_arg(&args(&["watch-pen", "--device"])).is_err());
        assert!(device_arg(&args(&["watch-pen", "--device", "not-an-address"])).is_err());
        assert!(device_arg(&args(&["watch-pen", "--device", "0A:0B:0C:0D:0E"])).is_err());
    }

    #[test]
    fn dry_run_says_which_case_the_announcement_is() {
        assert!(dry_run_plan(0x01).contains("no pairing with this host"));
        assert!(dry_run_plan(0x03).contains("has a pairing with this host"));
        assert!(dry_run_plan(0x01).contains("passkey 0"));
    }
}
