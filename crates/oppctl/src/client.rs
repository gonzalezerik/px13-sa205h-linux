// SPDX-License-Identifier: AGPL-3.0-or-later
//! The D-Bus side of `oppctl`: read the daemon's view of the pens
//! (docs/dbus.md) once for `status`, or follow its signals for `watch`.
//! Everything serialisable lives in `json`; this module only moves
//! values across the bus.

use std::collections::BTreeMap;
use std::num::NonZeroUsize;

use futures_util::TryStreamExt;
use zbus::message::Type;
use zbus::wire::{OwnedObjectPath, OwnedVariant};
use zbus::{Connection, MatchRule, Message, MessageStream, Proxy};

use crate::json::{self, Buttons, Capabilities, CapabilityHaptics, Event, Haptics, Pen, Waveform};

const BUS_NAME: &str = "org.openpenprotocol.Daemon1";
const DAEMON_PATH: &str = "/org/openpenprotocol/Daemon";
const DAEMON_IFACE: &str = "org.openpenprotocol.Daemon1";
const PEN_IFACE: &str = "org.openpenprotocol.Pen1";
const HAPTICS_IFACE: &str = "org.openpenprotocol.Haptics1";
const PEN_ROOT: &str = "/org/openpenprotocol/pen/";
const PROPERTIES_IFACE: &str = "org.freedesktop.DBus.Properties";

/// One look at the pens, filtered and sorted, printed and done.
pub async fn status(all: bool, system: bool, as_json: bool) -> Result<(), String> {
    let connection = connect(system).await?;
    let mut pens = list_pens(&connection).await?;
    pens.retain(|pen| all || pen.present);
    pens.sort_by(|a, b| a.name.cmp(&b.name).then(a.path.cmp(&b.path)));
    if as_json {
        if let Some(line) = json::status_json(&pens) {
            println!("{line}");
        }
    } else {
        for pen in &pens {
            println!("{}", json::status_line(pen));
        }
    }
    Ok(())
}

/// Stream events as JSON Lines until the daemon goes away. One match
/// rule for every signal of `oppd`, registered before the snapshot is
/// read: a signal that lands in between then shows up twice, which a
/// consumer survives by re-reading full pen objects; one that landed in
/// the other order would be lost forever.
pub async fn watch(all: bool, system: bool) -> Result<(), String> {
    let connection = connect(system).await?;
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .sender(BUS_NAME)
        .build()
        .map_err(|error| format!("cannot build the signal match rule: {error}"))?;
    let queue = NonZeroUsize::new(256).expect("256 is not zero");
    let mut stream = MessageStream::for_match_rule(rule, &connection, Some(queue))
        .await
        .map_err(|error| format!("cannot subscribe to {BUS_NAME} signals: {error}"))?;
    let pens = list_pens(&connection).await?;
    emit(&Event::Snapshot {
        pens: visible(pens, all),
    });
    while let Some(message) = stream
        .try_next()
        .await
        .map_err(|error| format!("lost the connection: {error}"))?
    {
        if let Some(event) = translate(&connection, &message, all).await {
            emit(&event);
        }
    }
    Err("oppd closed the connection".to_string())
}

/// The bus `oppd serve` exports on: the session bus, or the system bus
/// when it was started with `--system`.
async fn connect(system: bool) -> Result<Connection, String> {
    let (which, result) = if system {
        ("system", Connection::system().await)
    } else {
        ("session", Connection::session().await)
    };
    result
        .map_err(|error| format!("cannot reach the {which} bus (is oppd serve running?): {error}"))
}

/// `ListPens()` plus one property read per pen. A pen that vanishes
/// between the list and the read is skipped, not fatal.
async fn list_pens(connection: &Connection) -> Result<Vec<Pen>, String> {
    let daemon = Proxy::new(connection, BUS_NAME, DAEMON_PATH, DAEMON_IFACE)
        .await
        .map_err(|error| format!("{BUS_NAME} is not answering on {DAEMON_PATH}: {error}"))?;
    let paths: Vec<OwnedObjectPath> = daemon
        .call_method("ListPens", &())
        .await
        .map_err(|error| format!("ListPens: {error}"))?
        .body()
        .deserialize()
        .map_err(|error| format!("ListPens reply: {error}"))?;
    let mut pens = Vec::new();
    for path in &paths {
        if let Ok(pen) = read_pen(connection, path.as_str()).await {
            pens.push(pen);
        }
    }
    Ok(pens)
}

/// One full pen read: `Pen1` for the facts, `Haptics1` for the policy,
/// the capability dict projected by `decode_capabilities`.
async fn read_pen(connection: &Connection, path: &str) -> Result<Pen, String> {
    let pen = Proxy::new(connection, BUS_NAME, path, PEN_IFACE)
        .await
        .map_err(|error| format!("{path}: {error}"))?;
    let name: String = pen
        .get_property("Name")
        .await
        .map_err(|error| format!("Name: {error}"))?;
    let address: String = pen
        .get_property("Address")
        .await
        .map_err(|error| format!("Address: {error}"))?;
    let present: bool = read_flag(&pen, "Present").await?;
    let paired: bool = read_flag(&pen, "Paired").await?;
    let trusted: bool = read_flag(&pen, "Trusted").await?;
    let percent: i32 = pen.get_property("BatteryPercent").await.unwrap_or(-1);
    Ok(Pen {
        path: path.to_string(),
        name,
        address,
        present,
        paired,
        trusted,
        battery: json::battery_from_wire(percent),
        haptics: read_haptics(connection, path).await,
        capabilities: pen
            .get_property::<BTreeMap<String, OwnedVariant>>("Capabilities")
            .await
            .ok()
            .map(decode_capabilities),
    })
}

async fn read_flag(pen: &Proxy<'_>, name: &str) -> Result<bool, String> {
    pen.get_property::<bool>(name)
        .await
        .map_err(|error| format!("{name}: {error}"))
}

/// `Haptics1` exists only on pens that do haptics, and reading it must
/// never fail a pen read: any error means "no haptics to report".
async fn read_haptics(connection: &Connection, path: &str) -> Option<Haptics> {
    let haptics = Proxy::new(connection, BUS_NAME, path, HAPTICS_IFACE)
        .await
        .ok()?;
    let enabled = haptics.get_property::<bool>("Enabled").await.ok()?;
    let intensity = haptics.get_property::<u8>("Intensity").await.ok()?;
    Some(Haptics { enabled, intensity })
}

/// Turn one daemon signal into at most one event. `PenAdded` and
/// `PropertiesChanged` re-read the whole pen rather than trusting the
/// signal payload; `PenRemoved` and `PairingEvent` carry their own data.
async fn translate(connection: &Connection, message: &Message, all: bool) -> Option<Event> {
    let header = message.header();
    let member = header.member()?.as_str().to_string();
    match member.as_str() {
        "PenAdded" => {
            let (path, _name): (OwnedObjectPath, String) = message.body().deserialize().ok()?;
            let pen = read_pen(connection, path.as_str()).await.ok()?;
            (all || pen.present).then_some(Event::PenAdded { pen })
        }
        "PenRemoved" => {
            let path: OwnedObjectPath = message.body().deserialize().ok()?;
            Some(Event::PenRemoved {
                path: path.as_str().to_string(),
                address: json::address_from_object_path(path.as_str()),
            })
        }
        "PairingEvent" => {
            let (kind, detail): (String, String) = message.body().deserialize().ok()?;
            Some(Event::Pairing { kind, detail })
        }
        "PropertiesChanged" => {
            if header.interface()?.as_str() != PROPERTIES_IFACE {
                return None;
            }
            let path = header.path()?.as_str();
            if !path.starts_with(PEN_ROOT) {
                return None;
            }
            let pen = read_pen(connection, path).await.ok()?;
            (all || pen.present).then_some(Event::PenChanged { pen })
        }
        _ => None,
    }
}

fn visible(pens: Vec<Pen>, all: bool) -> Vec<Pen> {
    pens.into_iter().filter(|pen| all || pen.present).collect()
}

fn emit(event: &Event) {
    print!("{}", json::event_line(event));
}

fn decode_capabilities(caps: BTreeMap<String, OwnedVariant>) -> Capabilities {
    let buttons = dict_of(&caps, "buttons").map(|buttons| Buttons {
        barrel: flag_of(&buttons, "barrel"),
        shortcut: flag_of(&buttons, "shortcut"),
        shortcut_gestures: gestures_of(&buttons, "shortcut-gestures"),
    });
    let haptics = dict_of(&caps, "haptics").map(|haptics| CapabilityHaptics {
        waveforms: waveforms_of(&haptics),
    });
    Capabilities {
        pairing: text_of(&caps, "pairing"),
        battery: text_of(&caps, "battery"),
        buttons,
        haptics,
    }
}

fn text_of(caps: &BTreeMap<String, OwnedVariant>, key: &str) -> Option<String> {
    caps.get(key)
        .and_then(|value| String::try_from(value.clone()).ok())
}

fn flag_of(caps: &BTreeMap<String, OwnedVariant>, key: &str) -> Option<bool> {
    caps.get(key)
        .and_then(|value| bool::try_from(value.clone()).ok())
}

fn gestures_of(caps: &BTreeMap<String, OwnedVariant>, key: &str) -> Option<Vec<String>> {
    caps.get(key)
        .and_then(|value| Vec::<String>::try_from(value.clone()).ok())
}

fn dict_of(
    caps: &BTreeMap<String, OwnedVariant>,
    key: &str,
) -> Option<BTreeMap<String, OwnedVariant>> {
    caps.get(key)
        .and_then(|value| BTreeMap::<String, OwnedVariant>::try_from(value.clone()).ok())
}

/// The `a(qsqb)` waveform array of the `haptics` sub-dict
/// (docs/dbus.md §6); a missing or malformed entry yields no list.
fn waveforms_of(haptics: &BTreeMap<String, OwnedVariant>) -> Vec<Waveform> {
    haptics
        .get("waveforms")
        .and_then(|value| Vec::<(u16, String, u16, bool)>::try_from(value.clone()).ok())
        .map(|tuples| {
            tuples
                .into_iter()
                .map(|(ordinal, name, duration_ms, continuous)| Waveform {
                    ordinal,
                    name,
                    duration_ms,
                    continuous,
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::wire::Variant;

    fn owned<T>(value: T) -> OwnedVariant
    where
        T: Into<Variant<'static>>,
    {
        OwnedVariant::try_from(value.into())
            .expect("a value that built a Variant converts to owned")
    }

    #[test]
    fn capabilities_decode_the_documented_shape() {
        let mut buttons = BTreeMap::new();
        buttons.insert("barrel".to_string(), owned(true));
        buttons.insert(
            "shortcut-gestures".to_string(),
            owned(vec!["single", "double"]),
        );
        let mut haptics = BTreeMap::new();
        haptics.insert(
            "waveforms".to_string(),
            owned(vec![(3u16, "click".to_string(), 45u16, false)]),
        );
        let mut caps = BTreeMap::new();
        caps.insert("pairing".to_string(), owned("seamless-touch"));
        caps.insert("battery".to_string(), owned("ble-battery-service"));
        caps.insert("buttons".to_string(), owned(buttons));
        caps.insert("haptics".to_string(), owned(haptics));

        let decoded = decode_capabilities(caps);
        assert_eq!(decoded.pairing.as_deref(), Some("seamless-touch"));
        assert_eq!(decoded.battery.as_deref(), Some("ble-battery-service"));
        let buttons = decoded.buttons.expect("buttons decoded");
        assert_eq!(buttons.barrel, Some(true));
        assert_eq!(buttons.shortcut, None);
        assert_eq!(
            buttons.shortcut_gestures,
            Some(vec!["single".to_string(), "double".to_string()])
        );
        let waveforms = decoded.haptics.expect("haptics decoded").waveforms;
        assert_eq!(waveforms.len(), 1);
        assert_eq!(waveforms[0].ordinal, 3);
        assert_eq!(waveforms[0].duration_ms, 45);
        assert!(!waveforms[0].continuous);
    }

    #[test]
    fn an_empty_capability_dict_decodes_to_no_keys() {
        let decoded = decode_capabilities(BTreeMap::new());
        assert!(decoded.pairing.is_none());
        assert!(decoded.battery.is_none());
        assert!(decoded.buttons.is_none());
        assert!(decoded.haptics.is_none());
    }

    #[test]
    fn visibility_keeps_connected_pens_unless_asked_for_all() {
        let sample = |present: bool| Pen {
            path: "/org/openpenprotocol/pen/dev_0a_0b_0c_0d_0e_0f".to_string(),
            name: "SA205H".to_string(),
            address: String::new(),
            present,
            paired: true,
            trusted: true,
            battery: None,
            haptics: None,
            capabilities: None,
        };
        let pens = vec![sample(true), sample(false)];
        assert_eq!(visible(pens.clone(), false).len(), 1);
        assert_eq!(visible(pens, true).len(), 2);
    }
}
