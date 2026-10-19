// SPDX-License-Identifier: AGPL-3.0-or-later
//! Print a pen's firmware debug log (findings/010) live, one record per line.
//!
//! `serve` already parses the log for its `Pen1` properties; this is the
//! standalone reader the capture scripts use. It subscribes to the Nordic
//! UART TX characteristic of every connected pen and prints each reassembled
//! record with a timestamp until stopped. Enabling notifications is the only
//! GATT action taken; nothing is ever written to the pen. BlueZ keeps one
//! notify watch per connection, so this runs fine next to a `serve`.

use std::collections::{HashMap, HashSet};
use std::num::NonZeroUsize;
use std::time::{Duration, Instant};

use futures_util::FutureExt;
use futures_util::stream::StreamExt;
use px13pen_core::telemetry::{LogReader, NUS_TX_UUID};
use zbus::fdo::ManagedObjects;
use zbus::message::Type;
use zbus::wire::OwnedVariant;
use zbus::{Connection, MatchRule, Message, MessageStream, Proxy};

const BLUEZ: &str = "org.bluez";
const DEVICE1: &str = "org.bluez.Device1";
const GATT_CHARACTERISTIC1: &str = "org.bluez.GattCharacteristic1";
const OBJECT_MANAGER: &str = "org.freedesktop.DBus.ObjectManager";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";

/// Drain cadence; the log runs at about one record per second.
const TICK: Duration = Duration::from_millis(250);
/// Every eighth tick re-reads BlueZ, so a pen that reconnected gets its
/// log subscribed once its services are resolved again.
const SYNC_EVERY_TICKS: u32 = 8;
/// Records queued between drains.
const QUEUE: usize = 256;

/// Follow every connected pen's log until stopped.
pub fn pen_log(adapter: &str) -> Result<(), String> {
    zbus::block_on(pen_log_async(adapter))
}

async fn pen_log_async(adapter: &str) -> Result<(), String> {
    let connection = Connection::system().await.map_err(|error| {
        format!("cannot reach the system bus (is bluetoothd running?): {error}")
    })?;
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .interface(PROPERTIES)
        .member("PropertiesChanged")
        .path_namespace(format!("/org/bluez/{adapter}"))
        .build()
        .map_err(|error| format!("match rule: {error}"))?;
    let mut stream = MessageStream::for_match_rule(rule, &connection, NonZeroUsize::new(QUEUE))
        .await
        .map_err(|error| format!("cannot watch pen logs: {error}"))?;
    println!("watching pen logs under /org/bluez/{adapter} — Ctrl-C to stop");
    let start = Instant::now();
    let mut reader = Reader::default();
    let mut tick = 0u32;
    loop {
        futures_timer::Delay::new(TICK).await;
        while let Some(Some(Ok(message))) = stream.next().now_or_never() {
            reader.handle(&message, start);
        }
        if tick.is_multiple_of(SYNC_EVERY_TICKS) {
            reader.sync(&connection, adapter).await;
        }
        tick = tick.wrapping_add(1);
    }
}

/// What is subscribed: each pen's log reassembler, and which devices are
/// pens (link up/down lines print only for those).
#[derive(Default)]
struct Reader {
    /// Log characteristic path → its reassembler.
    feeds: HashMap<String, LogReader>,
    /// Device paths seen owning a log characteristic.
    pens: HashSet<String>,
}

impl Reader {
    /// Print what arrived since the last call.
    fn handle(&mut self, message: &Message, start: Instant) {
        let header = message.header();
        let Some(path) = header.path() else {
            return;
        };
        let path = path.as_str();
        let Ok((interface, changed, _invalidated)) =
            message
                .body()
                .deserialize::<(String, HashMap<String, OwnedVariant>, Vec<String>)>()
        else {
            return;
        };
        let stamp = start.elapsed().as_secs_f64();
        if interface == GATT_CHARACTERISTIC1 {
            let Some(log) = self.feeds.get_mut(path) else {
                return;
            };
            let Some(value) = changed.get("Value") else {
                return;
            };
            let Ok(chunk) = Vec::<u8>::try_from(value.clone()) else {
                return;
            };
            for line in log.push_lines(&chunk) {
                println!("[{:>7.2} s] {line}", stamp);
            }
        } else if interface == DEVICE1
            && self.pens.contains(path)
            && let Some(connected) = changed
                .get("Connected")
                .and_then(|value| value.downcast_ref::<bool>().ok())
        {
            println!(
                "[{:>7.2} s] link {}",
                stamp,
                if connected { "up" } else { "down" }
            );
        }
    }

    /// Subscribe pens that came up since the last sync and drop the feeds
    /// of pens that left (BlueZ already ended their subscription).
    async fn sync(&mut self, connection: &Connection, adapter: &str) {
        let objects = match managed_objects(connection).await {
            Ok(found) => found,
            Err(error) => {
                eprintln!("bluez: {error}");
                return;
            }
        };
        let prefix = format!("/org/bluez/{adapter}/dev_");
        let mut up = HashSet::new();
        let mut logs = Vec::new();
        for (path, interfaces) in &objects {
            let path = path.as_str();
            let Some(rest) = path.strip_prefix(&prefix) else {
                continue;
            };
            let device = format!("{}{}", prefix, rest.split('/').next().unwrap_or(""));
            if !rest.contains('/') {
                if interfaces.iter().any(|(name, properties)| {
                    name.as_str() == DEVICE1
                        && properties
                            .get("Connected")
                            .and_then(|value| value.downcast_ref::<bool>().ok())
                            .unwrap_or(false)
                }) {
                    up.insert(device);
                }
                continue;
            }
            if interfaces.iter().any(|(name, properties)| {
                name.as_str() == GATT_CHARACTERISTIC1
                    && properties
                        .get("UUID")
                        .and_then(|uuid| uuid.downcast_ref::<&str>().ok())
                        .is_some_and(|uuid| uuid.eq_ignore_ascii_case(NUS_TX_UUID))
            }) {
                self.pens.insert(device.clone());
                logs.push((device, path.to_string()));
            }
        }
        self.feeds
            .retain(|characteristic, _| logs.iter().any(|(_, known)| known == characteristic));
        for (device, characteristic) in logs {
            if self.feeds.contains_key(&characteristic) || !up.contains(&device) {
                continue;
            }
            match start_notify(connection, &characteristic).await {
                Ok(()) => {
                    self.feeds.insert(characteristic, LogReader::new());
                    println!("subscribed to the pen log on {device}");
                }
                Err(error) => eprintln!("pen log on {device}: {error}"),
            }
        }
    }
}

async fn managed_objects(connection: &Connection) -> Result<ManagedObjects, String> {
    // BlueZ exports its ObjectManager on the root object.
    let root = Proxy::new(connection, BLUEZ, "/", OBJECT_MANAGER)
        .await
        .map_err(|error| format!("BlueZ root object: {error}"))?;
    root.call("GetManagedObjects", &())
        .await
        .map_err(|error| format!("cannot list Bluetooth objects: {error}"))
}

async fn start_notify(connection: &Connection, path: &str) -> Result<(), String> {
    let characteristic = Proxy::new(connection, BLUEZ, path, GATT_CHARACTERISTIC1)
        .await
        .map_err(|error| error.to_string())?;
    characteristic
        .call::<_, _, ()>("StartNotify", &())
        .await
        .map_err(|error| error.to_string())
}
