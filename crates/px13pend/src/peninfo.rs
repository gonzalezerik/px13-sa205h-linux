// SPDX-License-Identifier: AGPL-3.0-or-later
//! The pen's own identity text, read from its standard GATT service.
//!
//! Device Information (0x180A) is one of the few places the pen accepts
//! a plain GATT read, and its Firmware Revision characteristic is what
//! the Windows pen panel shows (findings/006 §1–2). Reads only: a pen
//! that is half asleep answers with an error, and the caller retries
//! while it is connected — nothing here can change the pen.

use std::collections::HashMap;

use zbus::fdo::ManagedObjects;
use zbus::{Connection, OwnedVariant, Proxy};

const BLUEZ: &str = "org.bluez";
const OBJECT_MANAGER: &str = "org.freedesktop.DBus.ObjectManager";
const GATT_CHARACTERISTIC1: &str = "org.bluez.GattCharacteristic1";
/// GATT: Device Information service, Firmware Revision String.
const FIRMWARE_REVISION_UUID: &str = "00002a26-0000-1000-8000-00805f9b34fb";

/// The pen's firmware revision, read over GATT: one plain `ReadValue`
/// of the Firmware Revision characteristic BlueZ has discovered under
/// `device_path`. A pen whose service discovery has not finished, or
/// that has slept between scan and read, fails — the caller retries
/// while the link is up.
pub async fn firmware_revision(bluez: &Connection, device_path: &str) -> Result<String, String> {
    let path = firmware_characteristic(bluez, device_path)
        .await?
        .ok_or_else(|| format!("{device_path}: no firmware revision characteristic"))?;
    let characteristic = Proxy::new(bluez, BLUEZ, path.as_str(), GATT_CHARACTERISTIC1)
        .await
        .map_err(|error| format!("{path}: {error}"))?;
    let options: HashMap<&str, OwnedVariant> = HashMap::new();
    let value: Vec<u8> = characteristic
        .call("ReadValue", &(options,))
        .await
        .map_err(|error| format!("cannot read {path}: {error}"))?;
    decode(&value).ok_or_else(|| format!("{path}: answered with no revision"))
}

/// Object path of the Firmware Revision characteristic under a device,
/// as BlueZ exported it.
async fn firmware_characteristic(
    bluez: &Connection,
    device_path: &str,
) -> Result<Option<String>, String> {
    let root = Proxy::new(bluez, BLUEZ, "/", OBJECT_MANAGER)
        .await
        .map_err(|error| format!("BlueZ root object: {error}"))?;
    let objects: ManagedObjects = root
        .call("GetManagedObjects", &())
        .await
        .map_err(|error| format!("cannot list Bluetooth objects: {error}"))?;
    let prefix = format!("{device_path}/");
    Ok(objects
        .iter()
        .filter(|(path, _)| path.as_str().starts_with(&prefix))
        .find(|(_, interfaces)| {
            interfaces.iter().any(|(name, properties)| {
                name.as_str() == GATT_CHARACTERISTIC1
                    && properties
                        .get("UUID")
                        .and_then(|uuid| uuid.downcast_ref::<&str>().ok())
                        .is_some_and(|uuid| uuid.eq_ignore_ascii_case(FIRMWARE_REVISION_UUID))
            })
        })
        .map(|(path, _)| path.as_str().to_string()))
}

/// The characteristic's bytes as text: the pen sends a plain ASCII
/// string; anything that is not text, or is only blank, is not a
/// revision.
fn decode(bytes: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(bytes).ok()?.trim();
    (!text.is_empty()).then(|| text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_string_is_the_revision() {
        assert_eq!(
            decode(b"2.0.37").as_deref(),
            Some("2.0.37"),
            "the pen's answer carries no terminator"
        );
    }

    #[test]
    fn surrounding_blanks_are_trimmed() {
        assert_eq!(decode(b" 1.4.7\r\n").as_deref(), Some("1.4.7"));
    }

    #[test]
    fn a_blank_or_unparsable_answer_is_no_revision() {
        assert_eq!(decode(b"   "), None);
        assert_eq!(decode(b""), None);
        assert_eq!(decode(&[0xff, 0xfe]), None);
    }
}
