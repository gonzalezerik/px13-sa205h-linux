// SPDX-License-Identifier: AGPL-3.0-or-later
//! Follow each connected pen's debug log (findings/010) through BlueZ.
//!
//! BlueZ ends a notification subscription when the link drops, so a pen
//! is subscribed again on every reconnect: `sync` runs after each BlueZ
//! poll with the set of connected pens and calls `StartNotify` on the
//! log characteristic of any pen that is not subscribed yet. Values
//! arrive as `PropertiesChanged` on the characteristic object; one match
//! rule covers every pen, and `drain` empties what has queued without
//! waiting, so the service loop's tick stays the only clock.
//!
//! Only notifications are enabled. Nothing is ever written to the pen.

use std::collections::{HashMap, HashSet};
use std::num::NonZeroUsize;
use std::time::{Duration, Instant};

use futures_util::FutureExt;
use futures_util::stream::StreamExt;
use px13pen_core::telemetry::{Event, Gauge, LogReader, NUS_TX_UUID};
use zbus::fdo::ManagedObjects;
use zbus::message::Type;
use zbus::wire::OwnedVariant;
use zbus::{Connection, MatchRule, MessageStream, Proxy};

const BLUEZ: &str = "org.bluez";
const GATT_CHARACTERISTIC1: &str = "org.bluez.GattCharacteristic1";
const OBJECT_MANAGER: &str = "org.freedesktop.DBus.ObjectManager";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";

/// A sleep this soon after a `Charge State:` line is the pen being set on
/// its charger (findings/010 §3: 0.2 s apart in every capture).
const DOCK_WINDOW: Duration = Duration::from_secs(3);
/// Notifications queued between ticks; the log runs at a few per second.
const QUEUE: usize = 256;

/// What a pen's log means for its `Pen1` properties.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Report {
    Gauge(Gauge),
    /// The pen went to sleep on its charger.
    Docked,
}

/// One subscribed pen's log.
struct Feed {
    /// The BlueZ device path the characteristic belongs to.
    device: String,
    reader: LogReader,
    charge_state_at: Option<Instant>,
}

impl Feed {
    fn new(device: &str) -> Self {
        Feed {
            device: device.to_string(),
            reader: LogReader::new(),
            charge_state_at: None,
        }
    }

    /// Feed one notification at `now`; returns the reports it completed.
    fn push(&mut self, chunk: &[u8], now: Instant) -> Vec<Report> {
        let mut reports = Vec::new();
        for event in self.reader.push(chunk) {
            match event {
                Event::Gauge(gauge) => reports.push(Report::Gauge(gauge)),
                Event::ChargeState(_) => self.charge_state_at = Some(now),
                Event::Sleep => {
                    if self
                        .charge_state_at
                        .take()
                        .is_some_and(|at| now.duration_since(at) <= DOCK_WINDOW)
                    {
                        reports.push(Report::Docked);
                    }
                }
                Event::Wake(_) => {}
            }
        }
        reports
    }
}

pub struct Telemetry {
    bluez: Connection,
    adapter: String,
    stream: MessageStream,
    /// Characteristic path → its pen's feed.
    feeds: HashMap<String, Feed>,
}

impl Telemetry {
    /// One match rule for every characteristic under the adapter.
    pub async fn new(bluez: &Connection, adapter: &str) -> Result<Self, String> {
        let rule = MatchRule::builder()
            .msg_type(Type::Signal)
            .interface(PROPERTIES)
            .member("PropertiesChanged")
            .path_namespace(format!("/org/bluez/{adapter}"))
            .arg(0, GATT_CHARACTERISTIC1)
            .build()
            .map_err(|error| format!("telemetry match rule: {error}"))?;
        let stream = MessageStream::for_match_rule(rule, bluez, NonZeroUsize::new(QUEUE))
            .await
            .map_err(|error| format!("cannot watch pen logs: {error}"))?;
        Ok(Telemetry {
            bluez: bluez.clone(),
            adapter: adapter.to_string(),
            stream,
            feeds: HashMap::new(),
        })
    }

    /// Match subscriptions to the connected pens: drop the feeds of pens
    /// that left (BlueZ already ended their subscription) and subscribe
    /// the newcomers. A pen whose services are not resolved yet has no
    /// characteristic object; the next sync tries again.
    pub async fn sync(&mut self, connected: &[String]) {
        self.feeds
            .retain(|_, feed| connected.contains(&feed.device));
        let subscribed: HashSet<&str> = self
            .feeds
            .values()
            .map(|feed| feed.device.as_str())
            .collect();
        let pending: Vec<&String> = connected
            .iter()
            .filter(|device| !subscribed.contains(device.as_str()))
            .collect();
        if pending.is_empty() {
            return;
        }
        let characteristics = match self.log_characteristics().await {
            Ok(found) => found,
            Err(error) => {
                eprintln!("pen log: {error}");
                return;
            }
        };
        for device in pending {
            let Some(path) = characteristics
                .iter()
                .find(|path| path.starts_with(&format!("{device}/")))
            else {
                continue;
            };
            match self.start_notify(path).await {
                Ok(()) => {
                    self.feeds.insert(path.clone(), Feed::new(device));
                }
                Err(error) => eprintln!("pen log {path}: {error}"),
            }
        }
    }

    /// Everything queued since the last call, without waiting, as
    /// `(BlueZ device path, report)` pairs.
    pub fn drain(&mut self) -> Vec<(String, Report)> {
        let mut reports = Vec::new();
        while let Some(Some(Ok(message))) = self.stream.next().now_or_never() {
            let header = message.header();
            let Some(path) = header.path() else {
                continue;
            };
            let Some(feed) = self.feeds.get_mut(path.as_str()) else {
                continue;
            };
            let Ok((_interface, changed, _invalidated)) =
                message
                    .body()
                    .deserialize::<(String, HashMap<String, OwnedVariant>, Vec<String>)>()
            else {
                continue;
            };
            let Some(value) = changed.get("Value") else {
                continue;
            };
            let Ok(chunk) = Vec::<u8>::try_from(value.clone()) else {
                continue;
            };
            let device = feed.device.clone();
            for report in feed.push(&chunk, Instant::now()) {
                reports.push((device.clone(), report));
            }
        }
        reports
    }

    /// Object paths of every log characteristic BlueZ knows on the adapter.
    async fn log_characteristics(&self) -> Result<Vec<String>, String> {
        let root = Proxy::new(&self.bluez, BLUEZ, "/", OBJECT_MANAGER)
            .await
            .map_err(|error| format!("BlueZ root object: {error}"))?;
        let objects: ManagedObjects = root
            .call("GetManagedObjects", &())
            .await
            .map_err(|error| format!("cannot list Bluetooth objects: {error}"))?;
        let prefix = format!("/org/bluez/{}/", self.adapter);
        Ok(objects
            .iter()
            .filter(|(path, _)| path.as_str().starts_with(&prefix))
            .filter(|(_, interfaces)| {
                interfaces.iter().any(|(name, properties)| {
                    name.as_str() == GATT_CHARACTERISTIC1
                        && properties
                            .get("UUID")
                            .and_then(|uuid| uuid.downcast_ref::<&str>().ok())
                            .is_some_and(|uuid| uuid.eq_ignore_ascii_case(NUS_TX_UUID))
                })
            })
            .map(|(path, _)| path.as_str().to_string())
            .collect())
    }

    async fn start_notify(&self, path: &str) -> Result<(), String> {
        let characteristic = Proxy::new(&self.bluez, BLUEZ, path, GATT_CHARACTERISTIC1)
            .await
            .map_err(|error| error.to_string())?;
        characteristic
            .call::<_, _, ()>("StartNotify", &())
            .await
            .map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_charge_state_then_sleep_is_a_dock() {
        let mut feed = Feed::new("/org/bluez/hci0/dev_01");
        let start = Instant::now();
        assert!(feed.push(b"Charge State: Not Charging", start).is_empty());
        let reports = feed.push(b"PEN ENTER SLEEP", start + Duration::from_millis(200));
        assert_eq!(reports, vec![Report::Docked]);
    }

    #[test]
    fn a_plain_sleep_is_not_a_dock() {
        let mut feed = Feed::new("/org/bluez/hci0/dev_01");
        assert!(feed.push(b"PEN ENTER SLEEP", Instant::now()).is_empty());
    }

    #[test]
    fn a_stale_charge_state_is_not_a_dock() {
        let mut feed = Feed::new("/org/bluez/hci0/dev_01");
        let start = Instant::now();
        feed.push(b"Charge State: Not Charging", start);
        let reports = feed.push(b"PEN ENTER SLEEP", start + Duration::from_secs(10));
        assert!(reports.is_empty());
    }

    #[test]
    fn gauge_readings_pass_through() {
        let mut feed = Feed::new("/org/bluez/hci0/dev_01");
        let reports = feed.push(
            b"vbat1 = 4070 v2 = 4080, soc = 74, i_bat = -2310",
            Instant::now(),
        );
        assert_eq!(
            reports,
            vec![Report::Gauge(Gauge {
                millivolts: 4070,
                percent_centi: None,
                microamps: -2310,
            })]
        );
    }
}
