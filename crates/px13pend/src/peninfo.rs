// SPDX-License-Identifier: AGPL-3.0-or-later
//! The pen's own facts, read over GATT — firmware text and transducer
//! serial.
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
/// GATT: a HID Report characteristic (HID service 0x1812). Every HID
/// report of a Bluetooth-le HID device — input, output, feature — is
/// one characteristic that answers a plain read.
const REPORT_UUID: &str = "00002a4d-0000-1000-8000-00805f9b34fb";

/// The pen's firmware revision, read over GATT: one plain `ReadValue`
/// of the Firmware Revision characteristic BlueZ has discovered under
/// `device_path`. A pen whose service discovery has not finished, or
/// that has slept between scan and read, fails — the caller retries
/// while the link is up.
pub async fn firmware_revision(bluez: &Connection, device_path: &str) -> Result<String, String> {
    let path = characteristic_paths(bluez, device_path, FIRMWARE_REVISION_UUID)
        .await?
        .into_iter()
        .next()
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

/// Object paths of the characteristics with this UUID under a device,
/// as BlueZ exported them.
async fn characteristic_paths(
    bluez: &Connection,
    device_path: &str,
    uuid: &str,
) -> Result<Vec<String>, String> {
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
        .filter(|(_, interfaces)| {
            interfaces.iter().any(|(name, properties)| {
                name.as_str() == GATT_CHARACTERISTIC1
                    && properties
                        .get("UUID")
                        .and_then(|value| value.downcast_ref::<&str>().ok())
                        .is_some_and(|found| found.eq_ignore_ascii_case(uuid))
            })
        })
        .map(|(path, _)| path.as_str().to_string())
        .collect())
}

/// The characteristic's bytes as text: the pen sends a plain ASCII
/// string; anything that is not text, or is only blank, is not a
/// revision.
fn decode(bytes: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(bytes).ok()?.trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// The pen's transducer serial, read over GATT (findings/006 §2: serial
/// and vendor, byte-for-byte what the digitizer's report 7 streams).
///
/// The pen exports no Report Reference descriptors, so nothing in the
/// attribute tree names which characteristic answers feature report
/// `0x40`. Instead the daemon reads every Report characteristic and
/// keeps the one whose bytes match the identity layout — serial then
/// the known vendor. The live check found exactly one such answer among
/// the pen's reports, a ten-byte read; the others hold haptics state
/// and keyboard/mouse padding that the vendor check rejects. A read
/// that fails or answers with no identity simply moves on, so a pen
/// that says nothing leaves the daemon in the identity-blind behaviour
/// (identity.rs) and the caller retries while the link is up.
pub async fn transducer_serial(bluez: &Connection, device_path: &str) -> Result<u32, String> {
    let paths = characteristic_paths(bluez, device_path, REPORT_UUID).await?;
    let options: HashMap<&str, OwnedVariant> = HashMap::new();
    for path in paths {
        let Ok(characteristic) =
            Proxy::new(bluez, BLUEZ, path.as_str(), GATT_CHARACTERISTIC1).await
        else {
            continue;
        };
        let answer: Result<Vec<u8>, _> =
            characteristic.call("ReadValue", &(options.clone(),)).await;
        let Ok(value) = answer else { continue };
        if let Some(serial) = crate::identity::parse_identity_report(&value) {
            return Ok(serial);
        }
    }
    Err(format!(
        "{device_path}: no characteristic answered with an identity"
    ))
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
