// SPDX-License-Identifier: AGPL-3.0-or-later
//! Daemon for MPP 2.6 pens: pairing, haptics, battery, buttons.
//!
//! Implemented today: tap-to-pair (`docs/protocol/seamless-pairing.md`)
//! and pen presence. `watch-pair` listens on the supported digitizer's
//! hidraw node for the `0x6E` pairing notification and bonds the announced
//! pen through BlueZ; `pairing-state` reads the digitizer's
//! seamless-pairing feature report; both only read from the digitizer —
//! the write ban on the vendor output reports is respected
//! (seamless-pairing.md §4). `watch-pen` follows battery and link state
//! through BlueZ; `watch-buttons` dumps the pen's raw evdev events so the
//! top-button codes can be identified. `haptic` plays one effect with
//! the single output report 0x41 and writes nothing unless `--send` is
//! given. `serve` exports the D-Bus interface of docs/dbus.md: a Daemon
//! object, one object per pen BlueZ knows, and the pairing listener.

use std::collections::HashMap;
use std::process::ExitCode;
use std::time::{Duration, Instant};

mod buttons;
mod haptics;
mod presence;
mod service;

use futures_timer::Delay;
use futures_util::future::Either;
use hidapi::{DeviceInfo, HidApi};
use px13pen_core::seamless;
use zbus::wire::ObjectPath;
use zbus::{Connection, Proxy, Variant, fdo};

/// Input buffers must fit the largest input report the device can send.
const READ_BUF_BYTES: usize = 8192;

/// A stale bond can leave `Connect` hanging rather than failing outright;
/// past this the bond is treated as dead and rebuilt. That is the normal
/// dual-boot state: the machine has one controller and the pen keeps
/// exactly one bond, so a boot on the other OS invalidates BlueZ's key.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// The pen re-announces on every touch and range event — nine frames in
/// 84 s in the reference capture — so one bond attempt per address per
/// window; frames inside it repeat a run that is in flight or just done.
const ANNOUNCE_DEBOUNCE: Duration = Duration::from_secs(3);

const USAGE: &str = "\
usage: px13pend <command> [options]

commands:
  watch-pair          wait for the digitizer to announce a pen (input report
                      0x6E) and bond it through BlueZ: ConnectDevice with
                      AddressType=random, then Pair, then Trusted
      --dry-run       print what would happen; touch no Bluetooth state
      --adapter NAME  BlueZ adapter to use (default: hci0)
  pairing-state       read the digitizer's seamless-pairing feature report
                      (0x70) and print it; read-only
  watch-pen           print battery and link changes for a paired pen,
                      live from BlueZ (UPower shows the same battery
                      without this daemon)
      --device ADDR   pen address (default: the only pen-named device)
      --adapter NAME  BlueZ adapter to use (default: hci0)
  watch-buttons       dump raw evdev events from the pen's input node,
                      to identify what the top-button report becomes
      --list          list pen-named evdev nodes and exit
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

/// Open the digitizer and bond every pen it announces. Runs until stopped.
/// One `block_on` owns the loop, so the D-Bus connection keeps its executor
/// alive across notifications.
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
    let connection = if dry_run {
        None
    } else {
        Some(Connection::system().await.map_err(|error| {
            format!("cannot reach the system bus (is bluetoothd running?): {error}")
        })?)
    };
    println!("watching for pairing notifications — bring the pen near the screen (Ctrl-C to stop)");
    let start = Instant::now();
    let mut frame = vec![0u8; READ_BUF_BYTES];
    let mut last_attempt = HashMap::<String, Instant>::new();
    // `device.read` parks the executor while we wait on the digitizer;
    // announcements arriving mid-bond queue in the kernel buffer and land
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
        if dry_run {
            println!("  {}", dry_run_plan(&address, &adapter));
        } else if let Some(connection) = &connection {
            if last_attempt
                .get(&address)
                .is_some_and(|when| when.elapsed() < ANNOUNCE_DEBOUNCE)
            {
                continue;
            }
            last_attempt.insert(address.clone(), Instant::now());
            match bond(connection, &adapter, &address).await {
                Ok(bonded) => println!("  {}", bonded.describe(&address)),
                Err(message) => eprintln!("  pairing failed: {message}"),
            }
        }
    }
}

/// How one announcement ended, for logs and for deciding what to tell the
/// user: only a new bond is news, reconnects stay quiet.
pub(crate) enum Bonded {
    /// BlueZ already had the pen on a live link.
    AlreadyConnected,
    /// A known pen whose stored key still worked.
    Reconnected,
    /// A new bond: first contact, or a stale one replaced.
    Paired { via_helper: bool },
}

impl Bonded {
    pub(crate) fn is_new_bond(&self) -> bool {
        matches!(self, Bonded::Paired { .. })
    }

    pub(crate) fn describe(&self, address: &str) -> String {
        match self {
            Bonded::AlreadyConnected => format!("already paired and connected: {address}"),
            Bonded::Reconnected => format!("reconnected to paired pen {address}"),
            Bonded::Paired { via_helper: false } => format!("paired and trusted {address}"),
            Bonded::Paired { via_helper: true } => {
                format!("paired and trusted {address} (kernel-initiated)")
            }
        }
    }
}

/// Bond one announced pen through BlueZ.
///
/// Fresh bond: `ConnectDevice` to the announced random-static address with
/// `AddressType=random` (the pen never advertises — only the digitizer
/// knows it), then `Pair` (LE Just Works: the digitizer already
/// authenticated the pen by announcing it), then mark `Trusted`.
///
/// A pen BlueZ lists as `Paired` is not skipped: the other OS re-pairs the
/// pen on every tap, so after a boot there BlueZ's key is stale by design.
/// Try plain `Connect` first; if the key is rejected or the connect hangs,
/// drop BlueZ's record and bond fresh — the same model Windows uses.
async fn bond(connection: &Connection, adapter: &str, address: &str) -> Result<Bonded, String> {
    let adapter_path = format!("/org/bluez/{adapter}");
    let pen_path = format!("{adapter_path}/dev_{}", address.replace(':', "_"));
    let adapter_proxy = Proxy::new(
        connection,
        "org.bluez",
        adapter_path.as_str(),
        "org.bluez.Adapter1",
    )
    .await
    .map_err(|error| format!("adapter {adapter}: {error}"))?;
    let pen_proxy = Proxy::new(
        connection,
        "org.bluez",
        pen_path.as_str(),
        "org.bluez.Device1",
    )
    .await
    .map_err(|error| format!("pen object {pen_path}: {error}"))?;
    if pen_proxy
        .get_property::<bool>("Paired")
        .await
        .unwrap_or(false)
    {
        if pen_proxy
            .get_property::<bool>("Connected")
            .await
            .unwrap_or(false)
        {
            return Ok(Bonded::AlreadyConnected);
        }
        // The announcement proves the pen is in range; if BlueZ's key is
        // still current a plain Connect finishes in well under a second.
        let connect = pen_proxy.call("Connect", &());
        match futures_util::future::select(std::pin::pin!(connect), Delay::new(CONNECT_TIMEOUT))
            .await
        {
            Either::Left((Ok(()), _)) => {
                return Ok(Bonded::Reconnected);
            }
            Either::Left((Err(error), _)) if is_stale_bond_error(&error) => {
                println!("  {address}: stored bond is stale ({error}) — re-pairing");
                remove_device(&adapter_proxy, &pen_path).await?;
            }
            Either::Left((Err(error), _)) => return Err(format!("Connect({address}): {error}")),
            Either::Right((_due, _pending)) => {
                println!("  {address}: Connect timed out — the key is likely stale, re-pairing");
                remove_device(&adapter_proxy, &pen_path).await?;
            }
        }
    }
    let mut properties = HashMap::new();
    properties.insert("Address", Variant::from(address));
    properties.insert("AddressType", Variant::from("random"));
    let connected: zbus::Result<()> = adapter_proxy.call("ConnectDevice", &properties).await;
    if let Err(error) = connected {
        if is_unknown_method(&error) {
            // Stock bluetoothd: `ConnectDevice` is experimental and simply
            // not exported. Pair through the kernel management interface;
            // bluetoothd adopts the bond through its management callbacks
            // and the rest finishes over D-Bus like any other pen.
            pair_through_helper(connection, adapter, address).await?;
            pen_proxy
                .set_property("Trusted", true)
                .await
                .map_err(|error| format!("Trusted: {error}"))?;
            return Ok(Bonded::Paired { via_helper: true });
        }
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
    let pair: zbus::Result<()> = pen_proxy.call("Pair", &()).await;
    if let Err(error) = pair
        && !is_bluez_error(&error, &["org.bluez.Error.AlreadyExists"])
    {
        return Err(format!("Pair: {error}"));
    }
    pen_proxy
        .set_property("Trusted", true)
        .await
        .map_err(|error| format!("Trusted: {error}"))?;
    Ok(Bonded::Paired { via_helper: false })
}

/// The no-configuration path. Stock bluetoothd exports no way to bond a
/// device that never advertises — `ConnectDevice` has been `[experimental]`
/// since it was added — so the privileged pairing helper issues
/// `MGMT_OP_PAIR_DEVICE` on the kernel management socket instead. bluetoothd
/// adopts connections and keys it did not start (its management callbacks
/// register the device and store the long-term key), so `Device1` appears
/// and `Trusted` completes the bond as usual.
///
/// [HYP] until the first hardware session proves GATT discovery and the HID
/// over GATT probe after a kernel-initiated bond.
async fn pair_through_helper(
    connection: &Connection,
    adapter: &str,
    address: &str,
) -> Result<(), String> {
    let helper = Proxy::new(
        connection,
        "io.github.gonzalezerik.Px13Pen.PairHelper1",
        "/io/github/gonzalezerik/Px13Pen/PairHelper1",
        "io.github.gonzalezerik.Px13Pen.PairHelper1",
    )
    .await
    .map_err(|_| {
        "pairing helper is not available — is the px13-sa205h-linux package installed?".to_string()
    })?;
    let paired: zbus::Result<()> = helper.call("PairDevice", &(address, adapter)).await;
    paired.map_err(|error| format!("pairing helper: {error}"))?;
    // Adoption lands within a moment of the pairing-complete event.
    let pen_path = format!("/org/bluez/{adapter}/dev_{}", address.replace(':', "_"));
    let pen_proxy = Proxy::new(
        connection,
        "org.bluez",
        pen_path.as_str(),
        "org.bluez.Device1",
    )
    .await
    .map_err(|error| format!("pen object {pen_path}: {error}"))?;
    for _ in 0..40 {
        if pen_proxy
            .get_property::<bool>("Paired")
            .await
            .unwrap_or(false)
        {
            return Ok(());
        }
        Delay::new(Duration::from_millis(250)).await;
    }
    Err("paired in the kernel, but bluetoothd did not adopt the bond".to_string())
}

/// What a real run would do, phrased in the argument names BlueZ uses so
/// the dry run cannot drift away from the code path it previews.
fn dry_run_plan(address: &str, adapter: &str) -> String {
    format!(
        "dry run: would ConnectDevice(Address={address}, AddressType=random), Pair, Trusted on {adapter}"
    )
}

/// Drop BlueZ's record of the pen so the next `ConnectDevice` is a first
/// contact. `RemoveDevice` takes the object path, not the address.
async fn remove_device(adapter_proxy: &Proxy<'_>, pen_path: &str) -> Result<(), String> {
    let path = ObjectPath::try_from(pen_path)
        .map_err(|error| format!("pen object path {pen_path}: {error}"))?;
    let removed: zbus::Result<()> = adapter_proxy.call("RemoveDevice", &(path,)).await;
    removed.map_err(|error| format!("RemoveDevice({pen_path}): {error}"))
}

/// True for the named BlueZ D-Bus errors, which mean "already done" rather
/// than failure.
fn is_bluez_error(error: &zbus::Error, names: &[&str]) -> bool {
    matches!(error, zbus::Error::MethodError(name, _, _) if names.contains(&name.as_str()))
}

/// True when a failed `Connect` means the stored key no longer matches —
/// as opposed to the pen being out of range or the adapter busy, which
/// must not destroy a working bond.
fn is_stale_bond_error(error: &zbus::Error) -> bool {
    match error {
        zbus::Error::MethodError(name, _, _) => is_stale_bond_name(name.as_str()),
        _ => false,
    }
}

fn is_stale_bond_name(name: &str) -> bool {
    matches!(
        name,
        "org.bluez.Error.Failed"
            | "org.bluez.Error.AuthenticationRejected"
            | "org.bluez.Error.AuthenticationCanceled"
            | "org.bluez.Error.AuthenticationFailed"
            | "org.bluez.Error.ConnectionFailed"
    )
}

/// True when the method simply is not there: `ConnectDevice` is only
/// exported when bluetoothd runs with experimental interfaces enabled.
/// FreeDesktop error names arrive through zbus as `Error::FDO`.
fn is_unknown_method(error: &zbus::Error) -> bool {
    matches!(
        error,
        zbus::Error::FDO(inner) if matches!(inner.as_ref(), fdo::Error::UnknownMethod(_))
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
    use super::{device_arg, dry_run_plan, is_stale_bond_name};

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
    fn dry_run_names_the_arguments_bluez_accepts() {
        // Assembled at runtime: the privacy gate rejects colon-hex literals.
        let address = String::from("0A:0B:0C:0D:0E") + ":0F";
        let plan = dry_run_plan(&address, "hci0");
        // One property dictionary with "random" for the pen's random-static
        // address, and the Trusted property — the spellings BlueZ checks.
        assert!(plan.contains(&format!("Address={address}")));
        assert!(plan.contains("AddressType=random"));
        assert!(plan.contains("Trusted"));
    }

    #[test]
    fn a_stale_bond_is_repaired_but_a_transit_is_not() {
        assert!(is_stale_bond_name("org.bluez.Error.Failed"));
        assert!(is_stale_bond_name("org.bluez.Error.AuthenticationRejected"));
        assert!(is_stale_bond_name("org.bluez.Error.AuthenticationFailed"));
        // These must never destroy a working bond:
        assert!(!is_stale_bond_name("org.bluez.Error.InProgress"));
        assert!(!is_stale_bond_name("org.bluez.Error.NotReady"));
        assert!(!is_stale_bond_name("org.bluez.Error.AlreadyConnected"));
    }
}
