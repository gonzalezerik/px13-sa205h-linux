// SPDX-License-Identifier: AGPL-3.0-or-later
//! Daemon for MPP 2.6 pens: pairing, haptics, battery, buttons.
//!
//! Implemented today: tap-to-pair (`docs/protocol/seamless-pairing.md`).
//! `watch-pair` listens on the supported digitizer's hidraw node for the
//! `0x6E` pairing notification and bonds the announced pen through BlueZ;
//! `pairing-state` reads the digitizer's seamless-pairing feature report.
//! Both only read from the digitizer — the write ban on the vendor output
//! reports is respected (seamless-pairing.md §4). The rest of the daemon
//! (haptics, battery, buttons) is still to come.

use std::collections::HashMap;
use std::process::ExitCode;
use std::time::Instant;

use hidapi::{DeviceInfo, HidApi};
use opp_core::seamless;
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::Value;

/// Input buffers must fit the largest input report the device can send.
const READ_BUF_BYTES: usize = 8192;

const USAGE: &str = "\
usage: oppd <command> [options]

commands:
  watch-pair          wait for the digitizer to announce a pen (input report
                      0x6E) and bond it through BlueZ: ConnectDevice with
                      AddressType=le, then Pair, then Trust
      --dry-run       print what would happen; touch no Bluetooth state
      --adapter NAME  BlueZ adapter to use (default: hci0)
  pairing-state       read the digitizer's seamless-pairing feature report
                      (0x70) and print it; read-only
  -h, --help          print this help
  -V, --version       print the version
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let outcome = match args.first().map(String::as_str) {
        None => {
            println!("oppd {}", env!("CARGO_PKG_VERSION"));
            println!("Supported digitizers:");
            for (id, name) in opp_core::SUPPORTED_DIGITIZERS {
                println!("  {:04x}:{:04x}  {name}", id.vendor, id.product);
            }
            Ok(())
        }
        Some("watch-pair") => match adapter_name(&args) {
            Ok(adapter) => watch_pair(args.iter().any(|arg| arg == "--dry-run"), &adapter),
            Err(message) => Err(message),
        },
        Some("pairing-state") => pairing_state(),
        Some("-h") | Some("--help") => {
            print!("{USAGE}");
            Ok(())
        }
        Some("-V") | Some("--version") => {
            println!("oppd {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some(other) => {
            eprintln!("oppd: unknown command {other}\n");
            eprint!("{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("oppd: {message}");
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

/// The single supported digitizer present on this machine, by `hidraw`.
fn find_digitizer(api: &HidApi) -> Result<&DeviceInfo, String> {
    let found: Vec<&DeviceInfo> = api
        .device_list()
        .filter(|info| {
            let id = opp_core::HidId {
                vendor: info.vendor_id(),
                product: info.product_id(),
            };
            opp_core::supported_digitizer(id).is_some()
        })
        .collect();
    match found.first() {
        None => Err(
            "no supported digitizer found — is this a PX13, and does the current user have permission to read hidraw (input group)?".to_string(),
        ),
        Some(only) if found.len() == 1 => Ok(*only),
        Some(_) => Err(format!(
            "{} supported digitizers present; using more than one at a time is not supported",
            found.len()
        )),
    }
}

/// Open the digitizer and bond every pen it announces. Runs until stopped.
fn watch_pair(dry_run: bool, adapter: &str) -> Result<(), String> {
    let api = HidApi::new().map_err(|error| format!("cannot open the HID subsystem: {error}"))?;
    let info = find_digitizer(&api)?;
    let device = api
        .open_path(info.path())
        .map_err(|error| format!("cannot open {}: {error}", info.path().to_string_lossy()))?;
    let connection = if dry_run {
        None
    } else {
        Some(Connection::system().map_err(|error| {
            format!("cannot reach the system bus (is bluetoothd running?): {error}")
        })?)
    };
    println!("watching for pairing notifications — tap the pen tip to the screen (Ctrl-C to stop)");
    let start = Instant::now();
    let mut frame = vec![0u8; READ_BUF_BYTES];
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
        if dry_run {
            println!(
                "  dry run: would ConnectDevice({address}, AddressType=le), Pair, Trust on {adapter}"
            );
        } else if let Some(connection) = &connection {
            match bond(connection, adapter, &address) {
                Ok(summary) => println!("  {summary}"),
                Err(message) => eprintln!("  pairing failed: {message}"),
            }
        }
    }
}

/// Bond one announced pen through BlueZ: direct connect to the announced
/// random-static address, pair (LE Just Works — the digitizer already
/// authenticated the pen by announcing it), then trust for reconnects.
fn bond(connection: &Connection, adapter: &str, address: &str) -> Result<String, String> {
    let adapter_path = format!("/org/bluez/{adapter}");
    let pen_path = format!("{adapter_path}/dev_{}", address.replace(':', "_"));
    let adapter_proxy = Proxy::new(
        connection,
        "org.bluez",
        adapter_path.as_str(),
        "org.bluez.Adapter1",
    )
    .map_err(|error| format!("adapter {adapter}: {error}"))?;
    let pen_proxy = Proxy::new(
        connection,
        "org.bluez",
        pen_path.as_str(),
        "org.bluez.Device1",
    )
    .map_err(|error| format!("pen object {pen_path}: {error}"))?;
    if pen_proxy.get_property::<bool>("Paired").unwrap_or(false) {
        return Ok("already paired — BlueZ reconnects it on use".to_string());
    }
    let mut options = HashMap::new();
    options.insert("AddressType", Value::new("le"));
    let connected: zbus::Result<()> = adapter_proxy.call("ConnectDevice", &(address, options));
    if let Err(error) = connected {
        if !is_bluez_error(
            &error,
            &[
                "org.bluez.Error.AlreadyConnected",
                "org.bluez.Error.InProgress",
            ],
        ) {
            return Err(format!("ConnectDevice({address}): {error}"));
        }
    }
    let paired: zbus::Result<()> = pen_proxy.call("Pair", &());
    if let Err(error) = paired {
        if !is_bluez_error(&error, &["org.bluez.Error.AlreadyExists"]) {
            return Err(format!("Pair: {error}"));
        }
    }
    pen_proxy
        .set_property("Trust", true)
        .map_err(|error| format!("Trust: {error}"))?;
    Ok(format!("paired and trusted {address}"))
}

/// True for the named BlueZ D-Bus errors, which mean "already done" rather
/// than failure.
fn is_bluez_error(error: &zbus::Error, names: &[&str]) -> bool {
    matches!(error, zbus::Error::MethodError(name, _, _) if names.contains(&name.as_str()))
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
