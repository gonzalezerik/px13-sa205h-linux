// SPDX-License-Identifier: AGPL-3.0-or-later
//! The D-Bus service: `oppd serve` exports the interface specified in
//! docs/dbus.md — one `Daemon` object, one object per known pen, and
//! the capability dict that keeps pen models out of every frontend.
//!
//! One `block_on` drives everything. A 100 ms tick drains the pairing
//! listener (the digitizer's hidraw node opened non-blocking, so
//! `StopPairing` never waits on a read), and every fifth tick re-reads
//! BlueZ with `GetManagedObjects` to create and remove pen objects and
//! to refresh the `Pen1` properties. BlueZ is polled rather than
//! subscribed to: one call shape presence.rs already proved out, half
//! a second of lag no tray can notice, and no match rules to maintain.
//!
//! Pen state lives behind one `Arc<Mutex<PenValues>>` per pen, shared
//! by the `Pen1` and `Haptics1` interface objects, so the haptics
//! methods see exactly the `Present` flag the sync task maintains.
//! Getters lock, clone and release; change signals are emitted only
//! with the lock dropped, because emitting reads the property back
//! through the same getter.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use hidapi::{HidApi, HidDevice};
use opp_core::haptics::{
    Effect, ORDINAL_NONE, ORDINAL_STOP, SA205H_WAVES, intensity_byte, resolve,
};
use opp_core::seamless;
use zbus::fdo::{self, ManagedObjects};
use zbus::object_server::{InterfaceRef, SignalEmitter};
use zbus::wire::{ObjectPath, OwnedObjectPath, OwnedVariant, Variant};
use zbus::{Connection, Proxy};

const BUS_NAME: &str = "org.openpenprotocol.Daemon1";
const DAEMON_PATH: &str = "/org/openpenprotocol/Daemon";
const PEN_ROOT: &str = "/org/openpenprotocol/pen";

const BLUEZ: &str = "org.bluez";
const BLUEZ_ROOT: &str = "/org/bluez";
/// BlueZ exports its ObjectManager on the root object, not on `/org/bluez`.
const BLUEZ_OBJECT_MANAGER_PATH: &str = "/";
const DEVICE1: &str = "org.bluez.Device1";
const BATTERY1: &str = "org.bluez.Battery1";
const OBJECT_MANAGER: &str = "org.freedesktop.DBus.ObjectManager";

/// The service loop period: how fast the pairing listener reacts.
const TICK: Duration = Duration::from_millis(100);
/// Pen objects and properties follow BlueZ every fifth tick (500 ms).
const SYNC_EVERY_TICKS: u32 = 5;
/// Upper bound on hidraw frames handled per tick, so the digitizer's
/// ~256 Hz report stream cannot starve the loop.
const DRAIN_LIMIT: usize = 512;

/// How long one address is left alone after an attempt. The digitizer
/// repeats its announcement every few seconds while a pen hovers
/// unconnected, so a failed pairing is retried on the next announcement
/// after this pause instead of never, and a working one is not hammered.
const RETRY_AFTER: Duration = Duration::from_secs(5);
/// Failures within this long of the previous one belong to the same
/// streak and stay out of the notifications; a later one is news again.
const FAILURE_STREAK: Duration = Duration::from_secs(60);

/// Battery sentinel while BlueZ has not reported a level yet (docs/dbus.md §5).
const BATTERY_UNKNOWN: i32 = -1;

/// Start the daemon. `--system` exports on the system bus instead of
/// the session bus; BlueZ is always reached on the system bus (§1).
pub fn serve(args: &[String]) -> Result<(), String> {
    let adapter = crate::adapter_name(args)?;
    let system = serve_flags(args);
    zbus::block_on(serve_async(system, adapter))
}

/// Whether `serve` was asked for the system bus.
fn serve_flags(args: &[String]) -> bool {
    args.iter().any(|arg| arg == "--system")
}

async fn serve_async(system: bool, adapter: String) -> Result<(), String> {
    let bus = if system { "system" } else { "session" };
    let connection = if system {
        Connection::system().await
    } else {
        Connection::session().await
    }
    .map_err(|error| format!("cannot reach the {bus} bus: {error}"))?;
    // BlueZ and the pairing helper only exist on the system bus,
    // whichever bus the interface is exported on (§1).
    let bluez = if system {
        connection.clone()
    } else {
        Connection::system()
            .await
            .map_err(|error| format!("cannot reach the system bus (BlueZ): {error}"))?
    };
    let seamless = digitizer_present();
    connection
        .request_name(BUS_NAME)
        .await
        .map_err(|error| format!("cannot own {BUS_NAME} (another oppd serving?): {error}"))?;
    // Touch-to-pair is the point of a seamless-pairing digitizer, so the
    // listener runs from the start; StopPairing() turns it off.
    let listener = if seamless {
        open_listener()
            .inspect_err(|error| eprintln!("pairing listener: {error}"))
            .ok()
    } else {
        None
    };
    let listening = listener.is_some();
    let state = Arc::new(State {
        connection: connection.clone(),
        bluez,
        notify: !system,
        adapter,
        seamless,
        pens: Mutex::new(BTreeMap::new()),
        listener: Mutex::new(listener),
        attempts: Mutex::new(HashMap::new()),
    });
    connection
        .object_server()
        .at(
            DAEMON_PATH,
            Daemon {
                state: Arc::clone(&state),
                pairing_active: listening,
            },
        )
        .await
        .map_err(|error| format!("cannot export {DAEMON_PATH}: {error}"))?;
    let daemon = connection
        .object_server()
        .interface::<_, Daemon>(DAEMON_PATH)
        .await
        .map_err(|error| format!("Daemon interface vanished: {error}"))?;
    // A Bluetooth stack that is not up yet is no reason to exit: the loop
    // syncs again every half second.
    if let Err(error) = state.sync_pens(&daemon, false).await {
        eprintln!("bluez sync: {error}");
    }
    println!(
        "serving {BUS_NAME} on the {bus} bus (adapter {}, seamless pairing: {})",
        state.adapter,
        match (state.seamless, listening) {
            (true, true) => "listening",
            (true, false) => "digitizer present, listener off",
            (false, _) => "no digitizer",
        }
    );
    let mut tick: u32 = 0;
    loop {
        futures_timer::Delay::new(TICK).await;
        if let Err(error) = state.drain_announcements(&daemon).await {
            eprintln!("pairing listener: {error}");
        }
        tick = tick.wrapping_add(1);
        if tick.is_multiple_of(SYNC_EVERY_TICKS)
            && let Err(error) = state.sync_pens(&daemon, true).await
        {
            eprintln!("bluez sync: {error}");
        }
    }
}

/// Whether a supported digitizer is openable — decides the `pairing`
/// capability key. Failing here is not fatal: pens still get `manual`.
fn digitizer_present() -> bool {
    HidApi::new().is_ok_and(|api| crate::find_digitizer(&api).is_ok())
}

/// Open the digitizer's hidraw node non-blocking, ready for the
/// service loop to drain `0x6E` announcements from.
fn open_listener() -> Result<HidDevice, String> {
    let api = HidApi::new().map_err(|error| format!("no-digitizer: {error}"))?;
    let info = crate::find_digitizer(&api)?;
    let device = api
        .open_path(info.path())
        .map_err(|error| format!("no-digitizer: {error}"))?;
    device
        .set_blocking_mode(false)
        .map_err(|error| format!("no-digitizer: {error}"))?;
    Ok(device)
}

/// Everything the interface objects share. The pen map, the pairing
/// device and the announcement dedupe each live behind their own
/// mutex, and no lock is ever held across an await.
struct State {
    /// The bus the interface is exported on (session by default).
    connection: Connection,
    /// The system bus: BlueZ and the pairing helper.
    bluez: Connection,
    /// Desktop notifications, which live on the session bus only.
    notify: bool,
    adapter: String,
    seamless: bool,
    pens: Mutex<BTreeMap<String, PenEntry>>,
    listener: Mutex<Option<HidDevice>>,
    attempts: Mutex<HashMap<String, Attempt>>,
}

/// The listener's memory of one announced address.
struct Attempt {
    at: Instant,
    failed: bool,
}

/// One known pen: the shared values plus what the sync task remembers
/// about how the capability dict was built.
struct PenEntry {
    oppd_path: String,
    data: Arc<Mutex<PenValues>>,
    /// The inputs `build_capabilities` saw; a change rebuilds the dict.
    caps_shape: (bool, bool),
}

/// The mutable per-pen state behind `Pen1` and `Haptics1`.
struct PenValues {
    name: String,
    address: String,
    present: bool,
    paired: bool,
    trusted: bool,
    battery: i32,
    enabled: bool,
    intensity: u8,
    capabilities: BTreeMap<String, OwnedVariant>,
}

impl PenValues {
    /// Fresh values for a first sighting; the haptics policy defaults
    /// are on/100 and host-only, never written to the pen (§7).
    fn new(observed: &Observation, capabilities: BTreeMap<String, OwnedVariant>) -> Self {
        PenValues {
            name: observed.name.clone(),
            address: observed.address.clone(),
            present: observed.present,
            paired: observed.paired,
            trusted: observed.trusted,
            battery: observed.battery.map(i32::from).unwrap_or(BATTERY_UNKNOWN),
            enabled: true,
            intensity: 100,
            capabilities,
        }
    }

    /// Fold one BlueZ scan into the values; an absent battery reading
    /// keeps the last known level (§5: `-1` only while none arrived).
    fn apply(&mut self, observed: &Observation) -> Diff {
        let mut diff = Diff::default();
        if self.name != observed.name {
            self.name.clone_from(&observed.name);
            diff.name = true;
        }
        if self.address != observed.address {
            self.address.clone_from(&observed.address);
            diff.address = true;
        }
        if self.present != observed.present {
            self.present = observed.present;
            diff.present = true;
        }
        if self.paired != observed.paired {
            self.paired = observed.paired;
            diff.paired = true;
        }
        if self.trusted != observed.trusted {
            self.trusted = observed.trusted;
            diff.trusted = true;
        }
        if let Some(percent) = observed.battery {
            let percent = i32::from(percent);
            if self.battery != percent {
                self.battery = percent;
                diff.battery = true;
            }
        }
        diff
    }
}

/// Which `Pen1` properties a scan changed; drives the change signals.
#[derive(Default)]
struct Diff {
    name: bool,
    address: bool,
    present: bool,
    paired: bool,
    trusted: bool,
    battery: bool,
    capabilities: bool,
}

impl Diff {
    fn any(&self) -> bool {
        self.name
            || self.address
            || self.present
            || self.paired
            || self.trusted
            || self.battery
            || self.capabilities
    }
}

/// One pen as a scan saw it, straight off `GetManagedObjects`.
#[derive(Clone)]
struct Observation {
    bluez_path: String,
    name: String,
    address: String,
    present: bool,
    paired: bool,
    trusted: bool,
    battery: Option<u8>,
}

impl State {
    /// Drain pairing announcements from the digitizer. Every read is
    /// non-blocking; an empty read ends the drain.
    async fn drain_announcements(&self, daemon: &InterfaceRef<Daemon>) -> Result<(), String> {
        for _ in 0..DRAIN_LIMIT {
            let note = {
                let guard = locked(&self.listener);
                let Some(device) = guard.as_ref() else {
                    return Ok(());
                };
                let mut frame = [0u8; crate::READ_BUF_BYTES];
                match device.read(&mut frame) {
                    Ok(0) | Err(_) => return Ok(()),
                    Ok(len) => seamless::parse_pairing_report(&frame[..len]),
                }
            };
            let Some(note) = note else { continue };
            let address = note.address_string();
            // A pen already on a live link needs nothing; the digitizer
            // may still mention it now and then.
            if self.pen_connected(&address) {
                continue;
            }
            let failed_before = {
                let mut attempts = locked(&self.attempts);
                if attempts
                    .get(&address)
                    .is_some_and(|attempt| attempt.at.elapsed() < RETRY_AFTER)
                {
                    continue;
                }
                let failed_before = attempts
                    .get(&address)
                    .is_some_and(|attempt| attempt.failed && attempt.at.elapsed() < FAILURE_STREAK);
                attempts.insert(
                    address.clone(),
                    Attempt {
                        at: Instant::now(),
                        failed: failed_before,
                    },
                );
                failed_before
            };
            eprintln!("pen announced: {address}");
            emit_pairing(daemon, "announced", &address).await?;
            let outcome = crate::bond(&self.bluez, &self.adapter, &address).await;
            let failed = outcome.is_err();
            locked(&self.attempts).insert(
                address.clone(),
                Attempt {
                    at: Instant::now(),
                    failed,
                },
            );
            match outcome {
                Ok(bonded) => {
                    eprintln!("{}", bonded.describe(&address));
                    emit_pairing(daemon, "bonded", &address).await?;
                    if bonded.is_new_bond() {
                        let name = self.pen_name(&address).await;
                        self.notify(&format!("{name} paired"), "The pen is ready to use.")
                            .await;
                    }
                }
                Err(message) => {
                    eprintln!("pairing failed: {message}");
                    emit_pairing(daemon, "failed", &format!("{address}: {message}")).await?;
                    // One notification per failing streak; the retries that
                    // follow every few seconds stay in the log.
                    if !failed_before {
                        self.notify("Pen pairing failed", &message).await;
                    }
                }
            }
        }
        Ok(())
    }

    /// Whether BlueZ already has this pen on a live link, from the last
    /// sync (at most half a second old).
    fn pen_connected(&self, address: &str) -> bool {
        locked(&self.pens).values().any(|entry| {
            let values = locked(&entry.data);
            values.present && values.address.eq_ignore_ascii_case(address)
        })
    }

    /// The pen's Bluetooth alias for messages; its model name until BlueZ
    /// has one.
    async fn pen_name(&self, address: &str) -> String {
        let path = format!(
            "{BLUEZ_ROOT}/{}/dev_{}",
            self.adapter,
            address.replace(':', "_")
        );
        let alias = match Proxy::new(&self.bluez, BLUEZ, path.as_str(), DEVICE1).await {
            Ok(device) => device.get_property::<String>("Alias").await.ok(),
            Err(_) => None,
        };
        alias.unwrap_or_else(|| "Pen".to_string())
    }

    /// A desktop notification through org.freedesktop.Notifications.
    /// Best effort: no notification server is not an error, only a log line.
    async fn notify(&self, summary: &str, body: &str) {
        if !self.notify {
            return;
        }
        let mut hints: HashMap<&str, Variant<'_>> = HashMap::new();
        hints.insert(
            "desktop-entry",
            Variant::from("org.openpenprotocol.Settings"),
        );
        hints.insert("category", Variant::from("device"));
        let actions: Vec<&str> = Vec::new();
        let sent: zbus::Result<u32> = match Proxy::new(
            &self.connection,
            "org.freedesktop.Notifications",
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
        )
        .await
        {
            Ok(server) => {
                server
                    .call(
                        "Notify",
                        &(
                            "Open Pen Protocol",
                            0u32,
                            "input-tablet",
                            summary,
                            body,
                            actions,
                            hints,
                            -1i32,
                        ),
                    )
                    .await
            }
            Err(error) => Err(error),
        };
        if let Err(error) = sent {
            eprintln!("notification not shown: {error}");
        }
    }

    /// Re-read BlueZ's tree, then create, refresh and remove pen
    /// objects to match. `announce` gates PenAdded/PenRemoved; the
    /// initial sync stays quiet because late clients call `ListPens`.
    async fn sync_pens(&self, daemon: &InterfaceRef<Daemon>, announce: bool) -> Result<(), String> {
        let observed = self.scan_pens().await?;
        let (installs, removals, updates) = {
            let mut installs: Vec<(String, String, Arc<Mutex<PenValues>>)> = Vec::new();
            let mut updates: Vec<(String, Diff)> = Vec::new();
            let mut current = locked(&self.pens);
            let removals: Vec<String> = current
                .iter()
                .filter(|(bluez_path, _)| !observed.iter().any(|o| o.bluez_path == **bluez_path))
                .map(|(_, entry)| entry.oppd_path.clone())
                .collect();
            for oppd_path in &removals {
                current.retain(|_, entry| &entry.oppd_path != oppd_path);
            }
            for observation in &observed {
                let shape = (self.seamless, observation.battery.is_some());
                if let Some(entry) = current.get_mut(&observation.bluez_path) {
                    let mut diff = locked(&entry.data).apply(observation);
                    if entry.caps_shape != shape {
                        locked(&entry.data).capabilities = build_capabilities(shape.0, shape.1);
                        entry.caps_shape = shape;
                        diff.capabilities = true;
                    }
                    if diff.any() {
                        updates.push((entry.oppd_path.clone(), diff));
                    }
                } else {
                    let oppd_path = pen_object_path(&observation.bluez_path);
                    let data = Arc::new(Mutex::new(PenValues::new(
                        observation,
                        build_capabilities(shape.0, shape.1),
                    )));
                    installs.push((
                        oppd_path.clone(),
                        observation.name.clone(),
                        Arc::clone(&data),
                    ));
                    current.insert(
                        observation.bluez_path.clone(),
                        PenEntry {
                            oppd_path,
                            data,
                            caps_shape: shape,
                        },
                    );
                }
            }
            (installs, removals, updates)
        };

        for oppd_path in removals {
            self.connection
                .object_server()
                .remove::<Pen, _>(path_of(&oppd_path)?)
                .await
                .ok();
            self.connection
                .object_server()
                .remove::<Haptics, _>(path_of(&oppd_path)?)
                .await
                .ok();
            if announce {
                let path = owned_path(&oppd_path)?;
                daemon
                    .pen_removed(path)
                    .await
                    .map_err(|error| format!("cannot emit PenRemoved: {error}"))?;
            }
        }

        for (oppd_path, name, data) in installs {
            self.connection
                .object_server()
                .at(
                    path_of(&oppd_path)?,
                    Pen {
                        data: Arc::clone(&data),
                    },
                )
                .await
                .map_err(|error| format!("cannot export {oppd_path}: {error}"))?;
            // Every pen of the first supported family plays haptics, so
            // Haptics1 is always present today; the capability dict
            // tells clients the same thing.
            self.connection
                .object_server()
                .at(path_of(&oppd_path)?, Haptics { data })
                .await
                .map_err(|error| format!("cannot export haptics on {oppd_path}: {error}"))?;
            if announce {
                let path = owned_path(&oppd_path)?;
                daemon
                    .pen_added(path, &name)
                    .await
                    .map_err(|error| format!("cannot emit PenAdded: {error}"))?;
            }
        }

        for (oppd_path, diff) in updates {
            let path = path_of(&oppd_path)?;
            let Ok(iface) = self
                .connection
                .object_server()
                .interface::<_, Pen>(path)
                .await
            else {
                continue;
            };
            let guard = iface.get_mut().await;
            if diff.name {
                guard
                    .name_changed(iface.signal_emitter())
                    .await
                    .map_err(|error| format!("cannot emit Name change: {error}"))?;
            }
            if diff.address {
                guard
                    .address_changed(iface.signal_emitter())
                    .await
                    .map_err(|error| format!("cannot emit Address change: {error}"))?;
            }
            if diff.present {
                guard
                    .present_changed(iface.signal_emitter())
                    .await
                    .map_err(|error| format!("cannot emit Present change: {error}"))?;
            }
            if diff.paired {
                guard
                    .paired_changed(iface.signal_emitter())
                    .await
                    .map_err(|error| format!("cannot emit Paired change: {error}"))?;
            }
            if diff.trusted {
                guard
                    .trusted_changed(iface.signal_emitter())
                    .await
                    .map_err(|error| format!("cannot emit Trusted change: {error}"))?;
            }
            if diff.battery {
                guard
                    .battery_percent_changed(iface.signal_emitter())
                    .await
                    .map_err(|error| format!("cannot emit BatteryPercent change: {error}"))?;
            }
            if diff.capabilities {
                guard
                    .capabilities_changed(iface.signal_emitter())
                    .await
                    .map_err(|error| format!("cannot emit Capabilities change: {error}"))?;
            }
        }
        Ok(())
    }

    /// The pen-named devices under the adapter — the same heuristic
    /// presence.rs uses.
    async fn scan_pens(&self) -> Result<Vec<Observation>, String> {
        let root = Proxy::new(
            &self.bluez,
            BLUEZ,
            BLUEZ_OBJECT_MANAGER_PATH,
            OBJECT_MANAGER,
        )
        .await
        .map_err(|error| format!("BlueZ root object: {error}"))?;
        let objects: ManagedObjects = root
            .call("GetManagedObjects", &())
            .await
            .map_err(|error| format!("cannot list Bluetooth objects: {error}"))?;
        let prefix = format!("{BLUEZ_ROOT}/{}/", self.adapter);
        let mut found = Vec::new();
        for (path, interfaces) in &objects {
            if !path.as_str().starts_with(&prefix) {
                continue;
            }
            let Some(device) = interfaces
                .iter()
                .find(|(name, _)| name.as_str() == DEVICE1)
                .map(|(_, properties)| properties)
            else {
                continue;
            };
            let name = as_text(device.get("Name"));
            if !name.to_lowercase().contains("pen") {
                continue;
            }
            found.push(Observation {
                bluez_path: path.as_str().to_string(),
                address: as_text(device.get("Address")),
                present: as_flag(device.get("Connected")),
                paired: as_flag(device.get("Paired")),
                trusted: as_flag(device.get("Trusted")),
                battery: interfaces
                    .iter()
                    .find(|(iface, _)| iface.as_str() == BATTERY1)
                    .and_then(|(_, properties)| properties.get("Percentage"))
                    .and_then(crate::presence::as_percent),
                name,
            });
        }
        Ok(found)
    }
}

fn as_text(value: Option<&OwnedVariant>) -> String {
    value
        .and_then(|value| value.downcast_ref::<&str>().ok())
        .unwrap_or("")
        .to_string()
}

fn as_flag(value: Option<&OwnedVariant>) -> bool {
    value
        .and_then(|value| value.downcast_ref::<bool>().ok())
        .unwrap_or(false)
}

async fn emit_pairing(
    daemon: &InterfaceRef<Daemon>,
    kind: &str,
    detail: &str,
) -> Result<(), String> {
    daemon
        .pairing_event(kind, detail)
        .await
        .map_err(|error| format!("cannot emit PairingEvent: {error}"))
}

/// Parse a stored pen path for the object server.
fn path_of(oppd_path: &str) -> Result<ObjectPath<'_>, String> {
    ObjectPath::try_from(oppd_path).map_err(|error| format!("bad pen path {oppd_path}: {error}"))
}

/// The same path for a signal argument.
fn owned_path(oppd_path: &str) -> Result<OwnedObjectPath, String> {
    OwnedObjectPath::try_from(oppd_path)
        .map_err(|error| format!("bad pen path {oppd_path}: {error}"))
}

/// `/org/bluez/hci0/dev_XX_…` becomes `/org/openpenprotocol/pen/dev_xx_…`:
/// the basename, lower-cased, under the pen root (docs/dbus.md §3).
fn pen_object_path(bluez_path: &str) -> String {
    let basename = bluez_path
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or("unknown");
    format!("{PEN_ROOT}/{}", basename.to_lowercase())
}

/// The capability dict (docs/dbus.md §6). A missing key means the pen
/// lacks that capability; `settings` stays absent because no supported
/// pen stores any yet.
fn build_capabilities(seamless: bool, has_battery: bool) -> BTreeMap<String, OwnedVariant> {
    let waveforms: Vec<(u16, String, u16, bool)> = SA205H_WAVES
        .iter()
        .map(|wave| {
            (
                u16::from(wave.ordinal),
                wave.name.to_string(),
                wave.duration_ms,
                wave.continuous(),
            )
        })
        .collect();
    let mut haptics = BTreeMap::new();
    haptics.insert("waveforms".to_string(), owned(waveforms));
    let mut buttons = BTreeMap::new();
    buttons.insert("barrel".to_string(), owned(true));
    buttons.insert("shortcut".to_string(), owned(true));
    buttons.insert(
        "shortcut-gestures".to_string(),
        owned(vec!["single", "double", "long"]),
    );
    let mut capabilities = BTreeMap::new();
    capabilities.insert(
        "pairing".to_string(),
        owned(if seamless { "seamless-touch" } else { "manual" }),
    );
    capabilities.insert("haptics".to_string(), owned(haptics));
    capabilities.insert("buttons".to_string(), owned(buttons));
    capabilities.insert(
        "battery".to_string(),
        owned(if has_battery {
            "ble-battery-service"
        } else {
            "none"
        }),
    );
    capabilities
}

fn owned<T: Into<Variant<'static>>>(value: T) -> OwnedVariant {
    OwnedVariant::try_from(value.into()).expect("a value that built a Variant converts to owned")
}

/// Map a method's intensity argument to the wire byte: `255` asks for
/// the `Intensity` property, `0..=100` is the percent, anything else
/// has no meaning on the bus (docs/dbus.md §7).
fn map_intensity(argument: u8, policy_percent: u8) -> Option<u8> {
    match argument {
        255 => Some(intensity_byte(policy_percent)),
        0..=100 => Some(intensity_byte(argument)),
        _ => None,
    }
}

/// Round a cutoff up to the 10 ms units the wire encodes; beyond the
/// representable range there is no honest answer.
fn round_cutoff_ms(ms: u16) -> Option<u16> {
    if ms > 2550 {
        None
    } else {
        Some(ms.div_ceil(10) * 10)
    }
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

// ---------------------------------------------------------------------------
// org.openpenprotocol.Daemon1
// ---------------------------------------------------------------------------

/// The daemon object: what is known, what is being paired, and the
/// signals that announce both.
pub struct Daemon {
    state: Arc<State>,
    pairing_active: bool,
}

#[zbus::interface(name = "org.openpenprotocol.Daemon1")]
impl Daemon {
    /// Major version of this interface.
    #[zbus(property(emits_changed_signal = "const"))]
    fn api_version(&self) -> u32 {
        1
    }

    /// The oppd package version.
    #[zbus(property(emits_changed_signal = "const"))]
    fn version(&self) -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }

    /// Whether the touch-to-pair listener is running.
    #[zbus(property)]
    fn pairing_active(&self) -> bool {
        self.pairing_active
    }

    /// What oppd can listen on: vendor, product, name.
    #[zbus(property(emits_changed_signal = "const"))]
    fn supported_digitizers(&self) -> Vec<(u16, u16, String)> {
        opp_core::SUPPORTED_DIGITIZERS
            .iter()
            .map(|(id, name)| (id.vendor, id.product, (*name).to_string()))
            .collect()
    }

    /// Object paths of the known pens, sorted.
    async fn list_pens(&self) -> fdo::Result<Vec<OwnedObjectPath>> {
        let paths = locked(&self.state.pens)
            .values()
            .filter_map(|entry| owned_path(&entry.oppd_path).ok())
            .collect();
        Ok(paths)
    }

    /// Turn the touch-to-pair listener back on after `StopPairing()` (it
    /// starts on its own when a supported digitizer is present) and retry
    /// every pen at once. Returns whether the state changed; idempotent.
    async fn start_pairing(
        &mut self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<bool> {
        if locked(&self.state.listener).is_some() {
            return Ok(false);
        }
        let device = open_listener().map_err(fdo::Error::Failed)?;
        *locked(&self.state.listener) = Some(device);
        // A fresh search retries every pen at once, failed ones included.
        locked(&self.state.attempts).clear();
        self.pairing_active = true;
        self.pairing_active_changed(&emitter)
            .await
            .map_err(|error| {
                fdo::Error::Failed(format!("cannot announce PairingActive: {error}"))
            })?;
        Ok(true)
    }

    /// Stop listening; closes the digitizer node. Returns whether the
    /// state changed; idempotent.
    async fn stop_pairing(
        &mut self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<bool> {
        if locked(&self.state.listener).take().is_none() {
            return Ok(false);
        }
        self.pairing_active = false;
        self.pairing_active_changed(&emitter)
            .await
            .map_err(|error| {
                fdo::Error::Failed(format!("cannot announce PairingActive: {error}"))
            })?;
        Ok(true)
    }

    /// A pen object appeared. Clients that start after the pens did
    /// call `ListPens`, so there is no replay at connect.
    #[zbus(signal)]
    async fn pen_added(
        signal_emitter: &SignalEmitter<'_>,
        path: OwnedObjectPath,
        name: &str,
    ) -> zbus::Result<()>;

    /// A pen object was removed; BlueZ no longer knows the device.
    #[zbus(signal)]
    async fn pen_removed(
        signal_emitter: &SignalEmitter<'_>,
        path: OwnedObjectPath,
    ) -> zbus::Result<()>;

    /// Pairing progress: `kind` is `announced`, `bonded` or `failed`;
    /// `detail` is the address, plus a short reason for `failed`.
    #[zbus(signal)]
    async fn pairing_event(
        signal_emitter: &SignalEmitter<'_>,
        kind: &str,
        detail: &str,
    ) -> zbus::Result<()>;
}

// ---------------------------------------------------------------------------
// org.openpenprotocol.Pen1
// ---------------------------------------------------------------------------

/// One known pen. Everything mirrors BlueZ, plus the capability dict
/// frontends draw their UI from; changes ride standard
/// PropertiesChanged only.
pub struct Pen {
    data: Arc<Mutex<PenValues>>,
}

#[zbus::interface(name = "org.openpenprotocol.Pen1")]
impl Pen {
    /// Bluetooth name, e.g. the pen's model name.
    #[zbus(property)]
    fn name(&self) -> String {
        locked(&self.data).name.clone()
    }

    /// Bluetooth address, colon-separated upper-case hex.
    #[zbus(property)]
    fn address(&self) -> String {
        locked(&self.data).address.clone()
    }

    /// Whether the pen holds a link right now; `false` is normal for
    /// an idle BLE pen between uses.
    #[zbus(property)]
    fn present(&self) -> bool {
        locked(&self.data).present
    }

    /// Whether BlueZ reports the device as paired.
    #[zbus(property)]
    fn paired(&self) -> bool {
        locked(&self.data).paired
    }

    /// The BlueZ trust flag.
    #[zbus(property)]
    fn trusted(&self) -> bool {
        locked(&self.data).trusted
    }

    /// Battery level, or `-1` while no reading has arrived.
    #[zbus(property)]
    fn battery_percent(&self) -> i32 {
        locked(&self.data).battery
    }

    /// The capability dict (docs/dbus.md §6): what this pen can do,
    /// and the waveform table for haptics UIs.
    #[zbus(property)]
    fn capabilities(&self) -> BTreeMap<String, OwnedVariant> {
        locked(&self.data).capabilities.clone()
    }
}

// ---------------------------------------------------------------------------
// org.openpenprotocol.Haptics1
// ---------------------------------------------------------------------------

/// The haptics interface, exported on every pen path (all pens of the
/// first supported family play effects). Every method returns whether
/// a report was actually written; suppression is not an error.
pub struct Haptics {
    data: Arc<Mutex<PenValues>>,
}

#[zbus::interface(name = "org.openpenprotocol.Haptics1")]
impl Haptics {
    /// Master switch. `false` makes every method return `false`
    /// without touching the pen — host policy, never a pen write.
    #[zbus(property)]
    fn enabled(&self) -> bool {
        locked(&self.data).enabled
    }

    #[zbus(property)]
    fn set_enabled(&self, value: bool) {
        locked(&self.data).enabled = value;
    }

    /// Default strength for calls passing the `255` sentinel, 0–100.
    #[zbus(property)]
    fn intensity(&self) -> u8 {
        locked(&self.data).intensity
    }

    #[zbus(property)]
    fn set_intensity(&self, value: u8) -> fdo::Result<()> {
        if value > 100 {
            return Err(fdo::Error::InvalidArgs(format!(
                "Intensity is 0..100, got {value}"
            )));
        }
        locked(&self.data).intensity = value;
        Ok(())
    }

    /// Play a waveform by name (case-insensitive, from the capability
    /// list). `intensity` is 0–100 percent; `255` is the `Intensity`
    /// property.
    async fn play_effect(
        &self,
        name: &str,
        intensity: u8,
        repeat: u8,
        cutoff_ms: u16,
        retrigger_ms: u16,
    ) -> fdo::Result<bool> {
        let ordinal = resolve(name).ok_or_else(|| {
            fdo::Error::InvalidArgs(format!(
                "unknown effect {name}; the pen's Capabilities list names the playable ones"
            ))
        })?;
        self.play(ordinal, intensity, repeat, cutoff_ms, retrigger_ms)
            .await
    }

    /// Play a waveform by ordinal (the number in the capability tuple).
    async fn play_ordinal(
        &self,
        ordinal: u16,
        intensity: u8,
        repeat: u8,
        cutoff_ms: u16,
        retrigger_ms: u16,
    ) -> fdo::Result<bool> {
        let ordinal = u8::try_from(ordinal)
            .map_err(|_| fdo::Error::InvalidArgs("ordinal above 255".into()))?;
        if ordinal != ORDINAL_NONE
            && ordinal != ORDINAL_STOP
            && !SA205H_WAVES.iter().any(|wave| wave.ordinal == ordinal)
        {
            return Err(fdo::Error::InvalidArgs(format!(
                "ordinal {ordinal} is not in the pen's waveform table"
            )));
        }
        self.play(ordinal, intensity, repeat, cutoff_ms, retrigger_ms)
            .await
    }

    /// The stop command (implicit ordinal 2). Behaviour on the first
    /// supported pen is unconfirmed (docs/protocol/haptics.md).
    async fn stop(&self) -> fdo::Result<bool> {
        self.write_policy(Effect::of(ORDINAL_STOP)).await
    }
}

impl Haptics {
    /// Validate the arguments against the policy, build the report,
    /// then honour `Enabled` and `Present`.
    async fn play(
        &self,
        ordinal: u8,
        intensity: u8,
        repeat: u8,
        cutoff_ms: u16,
        retrigger_ms: u16,
    ) -> fdo::Result<bool> {
        let (strength, cutoff) = {
            let data = locked(&self.data);
            (
                map_intensity(intensity, data.intensity).ok_or_else(|| {
                    fdo::Error::InvalidArgs(format!(
                        "intensity must be 0..100 or 255 (the Intensity property), got {intensity}"
                    ))
                })?,
                round_cutoff_ms(cutoff_ms).ok_or_else(|| {
                    fdo::Error::InvalidArgs(format!(
                        "cutoff_ms {cutoff_ms} exceeds the representable 2550 ms"
                    ))
                })?,
            )
        };
        let report = Effect {
            ordinal,
            intensity: strength,
            repeat,
            cutoff_ms: cutoff,
            retrigger_ms,
        };
        self.write_policy(report).await
    }

    /// Suppression, presence and the write. The pen's HID node is
    /// found the way the CLI finds it; with several pen nodes the
    /// CLI's pick-one error passes through.
    async fn write_policy(&self, effect: Effect) -> fdo::Result<bool> {
        let (enabled, present, address) = {
            let data = locked(&self.data);
            (data.enabled, data.present, data.address.clone())
        };
        if !enabled {
            return Ok(false);
        }
        if !present {
            return Err(fdo::Error::Failed(format!(
                "not-present: {address} holds no link right now — bring the pen near the screen"
            )));
        }
        crate::haptics::send_to_pen(None, &effect.to_output_report())
            .map(|()| true)
            .map_err(|error| fdo::Error::Failed(format!("no-hid-node: {error}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serve_defaults_to_the_session_bus() {
        assert!(!serve_flags(&["serve".to_string()]));
        assert!(serve_flags(&["serve".to_string(), "--system".to_string()]));
    }

    #[test]
    fn pen_object_paths_follow_the_bluez_basename_lowercased() {
        assert_eq!(
            pen_object_path("/org/bluez/hci0/dev_0A_0B_0C_0D_0E_0F"),
            "/org/openpenprotocol/pen/dev_0a_0b_0c_0d_0e_0f"
        );
    }

    #[test]
    fn pen_object_paths_survive_a_strange_basename() {
        // An empty basename falls back to "unknown": an empty leaf would be
        // an invalid object path.
        assert_eq!(
            pen_object_path("/org/bluez/hci0/"),
            "/org/openpenprotocol/pen/unknown"
        );
        assert_eq!(pen_object_path(""), "/org/openpenprotocol/pen/unknown");
    }

    #[test]
    fn capability_dict_matches_the_spec_shape() {
        let caps = build_capabilities(true, true);
        assert_eq!(
            caps.get("pairing").unwrap().downcast_ref::<&str>().unwrap(),
            "seamless-touch"
        );
        assert_eq!(
            caps.get("battery").unwrap().downcast_ref::<&str>().unwrap(),
            "ble-battery-service"
        );
        let haptics =
            BTreeMap::<String, OwnedVariant>::try_from(caps.get("haptics").unwrap().clone())
                .unwrap();
        let waveforms =
            Vec::<(u16, String, u16, bool)>::try_from(haptics.get("waveforms").unwrap().clone())
                .unwrap();
        assert_eq!(waveforms.len(), SA205H_WAVES.len());
        assert_eq!(waveforms[0], (3, "click".to_string(), 45, false));
        assert_eq!(waveforms[6], (9, "ink".to_string(), 1995, true));
        assert_eq!(waveforms[12], (15, "sparkle".to_string(), 1980, true));
        let buttons =
            BTreeMap::<String, OwnedVariant>::try_from(caps.get("buttons").unwrap().clone())
                .unwrap();
        assert!(
            buttons
                .get("barrel")
                .unwrap()
                .downcast_ref::<bool>()
                .unwrap()
        );
        assert!(
            buttons
                .get("shortcut")
                .unwrap()
                .downcast_ref::<bool>()
                .unwrap()
        );
        let gestures =
            Vec::<String>::try_from(buttons.get("shortcut-gestures").unwrap().clone()).unwrap();
        assert_eq!(gestures, ["single", "double", "long"]);
        assert!(
            !caps.contains_key("settings"),
            "no supported pen stores settings yet (§6)"
        );
    }

    #[test]
    fn capability_dict_without_seamless_or_battery() {
        let caps = build_capabilities(false, false);
        assert_eq!(
            caps.get("pairing").unwrap().downcast_ref::<&str>().unwrap(),
            "manual"
        );
        assert_eq!(
            caps.get("battery").unwrap().downcast_ref::<&str>().unwrap(),
            "none"
        );
    }

    #[test]
    fn intensity_sentinel_uses_the_policy() {
        assert_eq!(map_intensity(255, 40), Some(intensity_byte(40)));
        assert_eq!(map_intensity(100, 40), Some(0xFF));
        assert_eq!(map_intensity(0, 40), Some(0x00));
        assert_eq!(map_intensity(101, 40), None);
        assert_eq!(map_intensity(254, 40), None);
    }

    #[test]
    fn cutoff_rounds_up_and_refuses_the_unrepresentable() {
        assert_eq!(round_cutoff_ms(0), Some(0));
        assert_eq!(round_cutoff_ms(45), Some(50));
        assert_eq!(round_cutoff_ms(2550), Some(2550));
        assert_eq!(round_cutoff_ms(2551), None);
    }

    fn observation() -> Observation {
        Observation {
            bluez_path: "/org/bluez/hci0/dev_01_02_03_04_05_06".to_string(),
            name: "Pen".to_string(),
            address: String::new(),
            present: false,
            paired: true,
            trusted: true,
            battery: Some(77),
        }
    }

    #[test]
    fn apply_flags_only_real_changes_and_keeps_battery() {
        let base = observation();
        let mut values = PenValues::new(&base, build_capabilities(true, true));
        assert_eq!(values.battery, 77);
        assert!(!values.apply(&base).any());
        let mut next = base.clone();
        next.present = true;
        next.battery = None;
        let changed = values.apply(&next);
        assert!(changed.present);
        assert!(!changed.battery);
        assert_eq!(values.battery, 77, "an absent reading keeps the last level");
    }

    #[test]
    fn no_reading_yet_is_the_sentinel() {
        let mut base = observation();
        base.battery = None;
        let values = PenValues::new(&base, BTreeMap::new());
        assert_eq!(values.battery, BATTERY_UNKNOWN);
    }

    #[test]
    fn policy_defaults_are_on_and_full() {
        let values = PenValues::new(&observation(), BTreeMap::new());
        assert!(values.enabled);
        assert_eq!(values.intensity, 100);
    }
}
