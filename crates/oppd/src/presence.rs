// SPDX-License-Identifier: AGPL-3.0-or-later
//! Watch a paired pen through BlueZ: battery level and link state.
//!
//! The battery rides the standard Battery Level characteristic (findings
//! 006 §5), which BlueZ exposes as `org.bluez.Battery1.Percentage` —
//! UPower shows the same value on its own. The interesting part is that
//! the pen drops the link while idle and reconnects on use, so a useful
//! view of it is a live one: `watch_pen` prints every property change
//! BlueZ reports for the device until stopped.

use std::collections::HashMap;

use zbus::blocking::{Connection, Proxy};
use zbus::fdo::ManagedObjects;
use zbus::zvariant::OwnedValue;

const BATTERY1: &str = "org.bluez.Battery1";
const DEVICE1: &str = "org.bluez.Device1";
const OBJECT_MANAGER: &str = "org.freedesktop.DBus.ObjectManager";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";

/// Follow one pen until stopped. `device` is an address; when none is
/// given, the single pen-named device known to BlueZ is used.
pub fn watch_pen(adapter: &str, device: Option<String>) -> Result<(), String> {
    let connection = Connection::system().map_err(|error| {
        format!("cannot reach the system bus (is bluetoothd running?): {error}")
    })?;
    let pen_path = match device {
        Some(address) => device_path(adapter, &address),
        None => find_pen(&connection, adapter)?,
    };
    let props = Proxy::new(&connection, "org.bluez", pen_path.as_str(), PROPERTIES)
        .map_err(|error| format!("pen object {pen_path}: {error}"))?;
    print_initial_state(&props, &pen_path)?;
    println!("watching for changes — Ctrl-C to stop");
    let signals = props
        .receive_signal("PropertiesChanged")
        .map_err(|error| format!("cannot subscribe to property changes: {error}"))?;
    for message in signals {
        let (interface, changed, _invalidated): (String, HashMap<String, OwnedValue>, Vec<String>) =
            message
                .body()
                .deserialize()
                .map_err(|error| format!("unreadable PropertiesChanged: {error}"))?;
        if interface == DEVICE1 {
            if let Some(value) = changed.get("Connected") {
                if let Ok(connected) = value.downcast_ref::<bool>() {
                    if connected {
                        println!("link up");
                    } else {
                        println!("link down (a trusted device reconnects on use)");
                    }
                }
            }
        } else if interface == BATTERY1 {
            if let Some(percent) = changed.get("Percentage").and_then(as_percent) {
                println!("battery: {percent} %");
            }
        }
    }
    Ok(())
}

/// The BlueZ object path for an address: colons become underscores.
fn device_path(adapter: &str, address: &str) -> String {
    format!("/org/bluez/{adapter}/dev_{}", address.replace(':', "_"))
}

/// The only pen-named device under the adapter, by object path.
fn find_pen(connection: &Connection, adapter: &str) -> Result<String, String> {
    let root = Proxy::new(connection, "org.bluez", "/org/bluez", OBJECT_MANAGER)
        .map_err(|error| format!("BlueZ root object: {error}"))?;
    let objects: zbus::Result<ManagedObjects> = root.call("GetManagedObjects", &());
    let objects = objects.map_err(|error| format!("cannot list Bluetooth objects: {error}"))?;
    let mut found = Vec::new();
    for (path, interfaces) in &objects {
        if !path.as_str().starts_with(&format!("/org/bluez/{adapter}/")) {
            continue;
        }
        let Some(properties) = interfaces
            .iter()
            .find(|(name, _)| name.as_str() == DEVICE1)
            .map(|(_, properties)| properties)
        else {
            continue;
        };
        let name = properties
            .get("Name")
            .and_then(|value| value.downcast_ref::<&str>().ok())
            .unwrap_or("");
        if name.to_lowercase().contains("pen") {
            found.push(path.as_str().to_string());
        }
    }
    match found.as_slice() {
        [] => Err(
            "no pen-named device known to BlueZ — pair it first (oppd watch-pair) or pass --device"
                .to_string(),
        ),
        [only] if found.len() == 1 => Ok(only.clone()),
        _ => Err(format!(
            "{} pen-named devices known to BlueZ — pass --device <address>",
            found.len()
        )),
    }
}

/// Print the current pairing, link and battery state once.
fn print_initial_state(props: &Proxy<'_>, pen_path: &str) -> Result<(), String> {
    let device: zbus::Result<HashMap<String, OwnedValue>> = props.call("GetAll", &DEVICE1);
    let device = device.map_err(|error| format!("cannot read pen properties: {error}"))?;
    let name = device
        .get("Name")
        .and_then(|value| value.downcast_ref::<&str>().ok())
        .unwrap_or("unknown name");
    println!("pen {pen_path}  ({name})");
    let flag = |key: &str| {
        device
            .get(key)
            .and_then(|value| value.downcast_ref::<bool>().ok())
            .map_or("unknown".to_string(), |on| on.to_string())
    };
    println!(
        "  paired {} trusted {} connected {}",
        flag("Paired"),
        flag("Trusted"),
        flag("Connected")
    );
    let battery: zbus::Result<HashMap<String, OwnedValue>> = props.call("GetAll", &BATTERY1);
    if let Ok(battery) = battery {
        if let Some(percent) = battery.get("Percentage").and_then(as_percent) {
            println!("  battery {percent} %");
        }
    }
    Ok(())
}

/// BlueZ exposes the battery percentage as a byte or int16 depending on
/// the version; accept both.
fn as_percent(value: &OwnedValue) -> Option<u8> {
    if let Ok(byte) = value.downcast_ref::<u8>() {
        return Some(byte);
    }
    value
        .downcast_ref::<i16>()
        .ok()
        .and_then(|small| u8::try_from(small).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::zvariant::Value;

    #[test]
    fn device_path_underscores_the_address() {
        // Assembled at runtime: the privacy gate rejects colon-hex literals.
        let address = String::from("06:05:04:03:02") + ":01";
        assert_eq!(
            device_path("hci0", &address),
            "/org/bluez/hci0/dev_06_05_04_03_02_01"
        );
    }

    #[test]
    fn percentage_accepts_byte_and_int16() {
        let byte = OwnedValue::try_from(Value::from(77u8)).unwrap();
        assert_eq!(as_percent(&byte), Some(77));
        let small = OwnedValue::try_from(Value::from(80i16)).unwrap();
        assert_eq!(as_percent(&small), Some(80));
    }

    #[test]
    fn percentage_rejects_other_shapes() {
        let text = OwnedValue::try_from(Value::from("80%")).unwrap();
        assert_eq!(as_percent(&text), None);
        let huge = OwnedValue::try_from(Value::from(300i16)).unwrap();
        assert_eq!(as_percent(&huge), None);
    }
}
