// SPDX-License-Identifier: AGPL-3.0-or-later
//! Touch-to-pair, host side of Microsoft's seamless pairing protocol
//! (docs/protocol/seamless-pairing.md, findings/008).
//!
//! The daemon registers the adapter's address with the digitizer, keeps an
//! LE scan running while a pen is in range, answers the digitizer's
//! announcement with a pairing key, and pairs whichever device then
//! advertises as a pen. Pairing goes through BlueZ with the daemon's own
//! agent: the pen insists on MITM protection and reports KeyboardOnly, so
//! the method is passkey entry, and a pen with nothing to type with enters 0.
//! The kernel has no LE legacy out-of-band support yet, so the key sent in
//! the key report does not take part in this pairing; it is what makes the
//! pen advertise. docs/future/kernel-le-legacy-oob.md has the rest.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use futures_timer::Delay;
use hidapi::HidDevice;
use px13pen_core::seamless::{self, PairingNotification};
use zbus::fdo::{self, ManagedObjects};
use zbus::wire::{ObjectPath, OwnedObjectPath, Variant};
use zbus::{Connection, Proxy};

const BLUEZ: &str = "org.bluez";
const ADAPTER1: &str = "org.bluez.Adapter1";
const DEVICE1: &str = "org.bluez.Device1";
const AGENT_MANAGER1: &str = "org.bluez.AgentManager1";
const OBJECT_MANAGER: &str = "org.freedesktop.DBus.ObjectManager";

/// Where the daemon's pairing agent lives on the system bus.
pub(crate) const AGENT_PATH: &str = "/io/github/gonzalezerik/Px13Pen/agent";

/// BlueZ's appearance value for a pen (HID category, digital pen).
const APPEARANCE_PEN: u16 = 0x03C7;

/// How long after the key report the pen gets to advertise. It answers
/// within milliseconds when it answers at all.
const ADVERTISE_WINDOW: Duration = Duration::from_secs(3);

/// How often the advertiser search re-reads BlueZ's device list.
const POLL: Duration = Duration::from_millis(20);

/// How long a paired pen gets to come back on its own before its bond is
/// treated as stale (the other system of a dual boot re-paired it).
const RECONNECT_WINDOW: Duration = Duration::from_secs(3);

/// How a seamless pairing ended.
pub(crate) enum Outcome {
    /// A new bond with the pen.
    Paired { name: String, address: String },
    /// The pen was already paired here and came back by itself.
    Reconnected { address: String },
}

/// The daemon's BlueZ agent. It answers only for pairings the daemon
/// started itself: BlueZ routes a pairing's questions to the agent of the
/// client that called Pair, so other devices keep using the desktop's agent.
#[derive(Clone, Default)]
pub(crate) struct Agent {
    armed: Arc<AtomicBool>,
}

impl Agent {
    fn arm(&self, armed: bool) {
        self.armed.store(armed, Ordering::SeqCst);
    }
}

fn rejected() -> fdo::Error {
    fdo::Error::Failed("rejected: not a pairing this daemon started".to_string())
}

#[zbus::interface(name = "org.bluez.Agent1")]
impl Agent {
    fn release(&self) {}

    /// Passkey entry: the pen has no way to type and enters 0, so the host
    /// enters 0 as well.
    fn request_passkey(&self, _device: OwnedObjectPath) -> fdo::Result<u32> {
        if self.armed.load(Ordering::SeqCst) {
            Ok(0)
        } else {
            Err(rejected())
        }
    }

    fn request_pin_code(&self, _device: OwnedObjectPath) -> fdo::Result<String> {
        Err(rejected())
    }

    fn display_passkey(&self, _device: OwnedObjectPath, _passkey: u32, _entered: u16) {}

    fn display_pin_code(&self, _device: OwnedObjectPath, _pincode: String) -> fdo::Result<()> {
        Err(rejected())
    }

    fn request_confirmation(&self, _device: OwnedObjectPath, _passkey: u32) -> fdo::Result<()> {
        Err(rejected())
    }

    fn request_authorization(&self, _device: OwnedObjectPath) -> fdo::Result<()> {
        Err(rejected())
    }

    fn authorize_service(&self, _device: OwnedObjectPath, _uuid: String) -> fdo::Result<()> {
        if self.armed.load(Ordering::SeqCst) {
            Ok(())
        } else {
            Err(rejected())
        }
    }

    fn cancel(&self) {}
}

/// Export the agent and register it with BlueZ as KeyboardOnly. It is not
/// made the default agent.
pub(crate) async fn register_agent(bluez: &Connection) -> Result<Agent, String> {
    let agent = Agent::default();
    bluez
        .object_server()
        .at(AGENT_PATH, agent.clone())
        .await
        .map_err(|error| format!("cannot export the pairing agent: {error}"))?;
    let manager = Proxy::new(bluez, BLUEZ, "/org/bluez", AGENT_MANAGER1)
        .await
        .map_err(|error| format!("BlueZ agent manager: {error}"))?;
    let path = ObjectPath::try_from(AGENT_PATH).map_err(|error| error.to_string())?;
    let registered: zbus::Result<()> = manager.call("RegisterAgent", &(path, "KeyboardOnly")).await;
    registered.map_err(|error| format!("RegisterAgent: {error}"))?;
    Ok(agent)
}

/// Tell the digitizer which host it serves: the adapter's address in the
/// Host Bluetooth Report, and this host's capability. Done when the
/// digitizer is opened, not per announcement.
pub(crate) async fn register_host(
    bluez: &Connection,
    adapter: &str,
    device: &HidDevice,
) -> Result<(), String> {
    let adapter_proxy = adapter_proxy(bluez, adapter).await?;
    let text: String = adapter_proxy
        .get_property("Address")
        .await
        .map_err(|error| format!("adapter address: {error}"))?;
    let kind: String = adapter_proxy
        .get_property("AddressType")
        .await
        .unwrap_or_else(|_| "public".to_string());
    let address = seamless::parse_address(&text)
        .ok_or_else(|| format!("adapter address {text} is not a Bluetooth address"))?;
    device
        .send_feature_report(&seamless::host_report(&address, kind == "random"))
        .map_err(|error| format!("host report: {error}"))?;
    device
        .send_feature_report(&seamless::capability_report())
        .map_err(|error| format!("capability report: {error}"))?;
    Ok(())
}

/// Start an LE scan for this client. BlueZ keeps one discovery session per
/// client, so a second start while one runs is harmless.
pub(crate) async fn start_scan(bluez: &Connection, adapter: &str) -> Result<(), String> {
    let adapter_proxy = adapter_proxy(bluez, adapter).await?;
    let mut filter: HashMap<&str, Variant<'_>> = HashMap::new();
    filter.insert("Transport", Variant::from("le"));
    filter.insert("DuplicateData", Variant::from(true));
    let filtered: zbus::Result<()> = adapter_proxy.call("SetDiscoveryFilter", &(filter,)).await;
    filtered.map_err(|error| format!("SetDiscoveryFilter: {error}"))?;
    let started: zbus::Result<()> = adapter_proxy.call("StartDiscovery", &()).await;
    match started {
        Ok(()) => Ok(()),
        Err(error) if is_bluez_error(&error, "org.bluez.Error.InProgress") => Ok(()),
        Err(error) => Err(format!("StartDiscovery: {error}")),
    }
}

/// Stop this client's LE scan; not scanning is not an error.
pub(crate) async fn stop_scan(bluez: &Connection, adapter: &str) {
    if let Ok(adapter_proxy) = adapter_proxy(bluez, adapter).await {
        let _: zbus::Result<()> = adapter_proxy.call("StopDiscovery", &()).await;
    }
}

/// One pen-looking device BlueZ knows.
struct Candidate {
    path: OwnedObjectPath,
    address: String,
    name: String,
    paired: bool,
    connected: bool,
    seen: bool,
}

/// Devices under the adapter that look like a pen: BlueZ's pen
/// appearance, or "Pen" in the name. `seen` means BlueZ has a signal
/// strength for it, i.e. it was heard during the current scan.
async fn candidates(bluez: &Connection, adapter: &str) -> Result<Vec<Candidate>, String> {
    let root = Proxy::new(bluez, BLUEZ, "/", OBJECT_MANAGER)
        .await
        .map_err(|error| format!("BlueZ root object: {error}"))?;
    let objects: ManagedObjects = root
        .call("GetManagedObjects", &())
        .await
        .map_err(|error| format!("cannot list Bluetooth objects: {error}"))?;
    let prefix = format!("/org/bluez/{adapter}/");
    let mut found = Vec::new();
    for (path, interfaces) in objects {
        if !path.as_str().starts_with(&prefix) {
            continue;
        }
        let Some((_, device)) = interfaces.iter().find(|(name, _)| name.as_str() == DEVICE1) else {
            continue;
        };
        let text = |key: &str| {
            device
                .get(key)
                .and_then(|value| value.downcast_ref::<&str>().ok())
                .unwrap_or("")
                .to_string()
        };
        let flag = |key: &str| {
            device
                .get(key)
                .and_then(|value| value.downcast_ref::<bool>().ok())
                .unwrap_or(false)
        };
        let appearance = device
            .get("Appearance")
            .and_then(|value| value.downcast_ref::<u16>().ok());
        let name = text("Name");
        if appearance != Some(APPEARANCE_PEN) && !name.contains("Pen") {
            continue;
        }
        found.push(Candidate {
            address: text("Address"),
            name,
            paired: flag("Paired"),
            connected: flag("Connected"),
            seen: device.contains_key("RSSI"),
            path,
        });
    }
    Ok(found)
}

/// True when BlueZ has any pen on a live link. The digitizer keeps
/// announcing a pen in range even while it is connected under another
/// identity, so the announced address cannot answer this.
pub(crate) async fn any_pen_connected(bluez: &Connection, adapter: &str) -> bool {
    candidates(bluez, adapter)
        .await
        .is_ok_and(|found| found.iter().any(|pen| pen.paired && pen.connected))
}

/// Run one seamless pairing for an announcement. The scan must already be
/// running: the pen's advertisement follows the key within milliseconds.
pub(crate) async fn pair_announced(
    bluez: &Connection,
    adapter: &str,
    device: &HidDevice,
    note: &PairingNotification,
    agent: &Agent,
) -> Result<Outcome, String> {
    let adapter_proxy = adapter_proxy(bluez, adapter).await?;

    // A pen paired here reconnects by itself when used. If it has not
    // within the window, its bond is stale (re-paired by another system):
    // forget it and pair fresh.
    let mut ready = None;
    for pen in candidates(bluez, adapter).await? {
        if !pen.paired && pen.seen && ready.is_none() {
            // The pen advertised moments ago (it does so by itself when it
            // wakes, before the digitizer announces it). Pairing this object
            // makes the controller wait for its next advertisement in
            // hardware; removing it would lose the pen until its back-off
            // ends.
            eprintln!("  pen already advertising as {}", pen.address);
            ready = Some(pen);
        } else if pen.paired && !pen.connected {
            if wait_connected(bluez, &pen.path).await {
                return Ok(Outcome::Reconnected {
                    address: pen.address,
                });
            }
            eprintln!("  forgetting stale bond {}", pen.address);
            remove_device(&adapter_proxy, &pen.path).await?;
        } else if !pen.paired && !pen.seen {
            eprintln!("  forgetting unpaired leftover {}", pen.address);
            // Unpaired leftovers would be mistaken for the fresh advertiser.
            remove_device(&adapter_proxy, &pen.path).await?;
        }
    }

    // The host was registered when the digitizer was opened. Re-sending it
    // here, or touching the scan filter, right before the key would land in
    // the pen's few-millisecond answer window; only start a scan that is
    // not already running.
    let discovering = adapter_proxy
        .get_property::<bool>("Discovering")
        .await
        .unwrap_or(false);
    if !discovering {
        start_scan(bluez, adapter).await?;
    }
    device
        .write(&seamless::pairing_key_report(note, &[0u8; 16]))
        .map_err(|error| format!("key report: {error}"))?;

    let sent = Instant::now();
    let deadline = sent + ADVERTISE_WINDOW;
    let mut last_seen = String::new();
    let advertiser = if let Some(pen) = ready {
        pen
    } else {
        loop {
            let found = candidates(bluez, adapter).await?;
            let summary = found
                .iter()
                .map(|pen| {
                    format!(
                        "{} paired={} connected={} seen={}",
                        pen.address, pen.paired, pen.connected, pen.seen
                    )
                })
                .collect::<Vec<_>>()
                .join("; ");
            if summary != last_seen {
                eprintln!("  +{} ms: [{summary}]", sent.elapsed().as_millis());
                last_seen = summary;
            }
            if let Some(pen) = found.into_iter().find(|pen| !pen.paired && pen.seen) {
                break pen;
            }
            if Instant::now() >= deadline {
                return Err(
                    "the pen did not answer (it skips a touch after a failed attempt; touch again)"
                        .to_string(),
                );
            }
            Delay::new(POLL).await;
        }
    };
    eprintln!(
        "  pairing {} at +{} ms",
        advertiser.address,
        sent.elapsed().as_millis()
    );

    let pen_proxy = Proxy::new(bluez, BLUEZ, advertiser.path.as_str(), DEVICE1)
        .await
        .map_err(|error| format!("pen object: {error}"))?;
    agent.arm(true);
    let paired: zbus::Result<()> = pen_proxy.call("Pair", &()).await;
    agent.arm(false);
    if let Err(error) = paired {
        eprintln!("  Pair failed at +{} ms", sent.elapsed().as_millis());
        let _ = remove_device(&adapter_proxy, &advertiser.path).await;
        return Err(format!("Pair: {error}"));
    }
    pen_proxy
        .set_property("Trusted", true)
        .await
        .map_err(|error| format!("Trusted: {error}"))?;
    let name = pen_proxy
        .get_property::<String>("Alias")
        .await
        .unwrap_or(advertiser.name);
    Ok(Outcome::Paired {
        name,
        address: advertiser.address,
    })
}

/// Poll a device's Connected property for the reconnect window.
async fn wait_connected(bluez: &Connection, path: &ObjectPath<'_>) -> bool {
    let Ok(proxy) = Proxy::new(bluez, BLUEZ, path.as_str(), DEVICE1).await else {
        return false;
    };
    let deadline = Instant::now() + RECONNECT_WINDOW;
    while Instant::now() < deadline {
        if proxy
            .get_property::<bool>("Connected")
            .await
            .unwrap_or(false)
        {
            return true;
        }
        Delay::new(Duration::from_millis(100)).await;
    }
    false
}

async fn adapter_proxy<'a>(bluez: &'a Connection, adapter: &str) -> Result<Proxy<'a>, String> {
    Proxy::new(bluez, BLUEZ, format!("/org/bluez/{adapter}"), ADAPTER1)
        .await
        .map_err(|error| format!("adapter {adapter}: {error}"))
}

async fn remove_device(adapter_proxy: &Proxy<'_>, path: &ObjectPath<'_>) -> Result<(), String> {
    let removed: zbus::Result<()> = adapter_proxy.call("RemoveDevice", &(path,)).await;
    removed.map_err(|error| format!("RemoveDevice({}): {error}", path.as_str()))
}

fn is_bluez_error(error: &zbus::Error, name: &str) -> bool {
    matches!(error, zbus::Error::MethodError(found, _, _) if found.as_str() == name)
}
