// SPDX-License-Identifier: AGPL-3.0-or-later
//! What one pen looks like after it has been read off the bus: the
//! `Pen1` properties, the `Haptics1` policy, the `Buttons1` actions,
//! the `Notifications1` switch and
//! the projected capability dict (docs/dbus.md), in the field names
//! frontends publish (docs/px13penctl.md). Serialisation rules — which
//! keys vanish when absent — are the contract, so they live with the
//! types.

use serde::Serialize;

/// One pen as the daemon sees it.
#[derive(Clone, Debug, Serialize)]
pub struct Pen {
    pub path: String,
    pub name: String,
    pub address: String,
    pub present: bool,
    pub paired: bool,
    pub trusted: bool,
    /// The pen's own firmware string, once the daemon has read it over
    /// GATT; absent before that (docs/dbus.md §5).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub firmware: Option<String>,
    pub battery: Option<i32>,
    /// The pen's own fuel gauge and dock state (findings/010); absent
    /// when the daemon has nothing beyond the battery service.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub battery_detail: Option<BatteryDetail>,
    pub haptics: Option<Haptics>,
    /// The shortcut-button actions (the daemon's `Buttons1`); absent
    /// when the pen offers no such interface.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buttons: Option<ButtonActions>,
    /// Whether the daemon says when this kind of pen goes idle, docks
    /// or wakes (the daemon's `Notifications1`). One policy for every
    /// pen, like haptics, so the key appears only when it is off —
    /// nobody wants it in every line of a bar module.
    #[serde(skip_serializing_if = "is_on")]
    pub status_notifications: bool,
    pub capabilities: Option<Capabilities>,
}

/// Serde only wants to hear about the switch when someone turned it off.
fn is_on(enabled: &bool) -> bool {
    *enabled
}

/// What the pen's own log adds to the battery level.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BatteryDetail {
    /// State of charge to a hundredth of a percent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub millivolts: Option<i32>,
    /// Asleep on its charger since the last link.
    pub docked: bool,
}

impl BatteryDetail {
    /// From the wire values (`-1` / `-1.0` = no reading); `None` when
    /// there is nothing to say, as from a daemon without the pen log.
    pub fn from_wire(percent: f64, millivolts: i32, docked: bool) -> Option<Self> {
        let percent = (percent >= 0.0).then_some(percent);
        let millivolts = (millivolts >= 0).then_some(millivolts);
        if percent.is_none() && millivolts.is_none() && !docked {
            return None;
        }
        Some(BatteryDetail {
            percent,
            millivolts,
            docked,
        })
    }
}

/// The pen's `Haptics1` policy state; `None` when the pen does no haptics.
/// The policy is per user, not per pen — every pen of the daemon reports
/// the same values (docs/dbus.md §7).
#[derive(Clone, Debug, Serialize)]
pub struct Haptics {
    pub enabled: bool,
    pub intensity: u8,
    /// The tip's contact arms the ink texture.
    pub while_inking: bool,
    /// Discrete effects (the UI ticks) get through.
    pub while_interacting: bool,
    /// Which continuous texture the tip arms.
    pub ink_texture: String,
}

/// The pen's `Buttons1` shortcut-button actions: one shell command per
/// gesture, `""` meaning do nothing. Per user like the haptics policy —
/// every pen reports the same values (docs/dbus.md §7).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ButtonActions {
    pub single: String,
    pub double: String,
    pub long: String,
}

/// The capability dict projected to typed fields. A key absent from the
/// dict is absent here too — that is the daemon's "this pen lacks it"
/// statement (docs/dbus.md §6), so unknown keys are dropped rather than
/// guessed at.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Capabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pairing: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub battery: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub battery_detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buttons: Option<Buttons>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub haptics: Option<CapabilityHaptics>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Buttons {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub barrel: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shortcut: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shortcut_gestures: Option<Vec<String>>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct CapabilityHaptics {
    pub waveforms: Vec<Waveform>,
}

/// One playable waveform: the `(ordinal, name, duration_ms, continuous)`
/// tuple of the capability dict, spread into named fields.
#[derive(Clone, Debug, Serialize)]
pub struct Waveform {
    pub ordinal: u16,
    pub name: String,
    pub duration_ms: u16,
    pub continuous: bool,
}

/// `BatteryPercent` is `-1` until the battery service has reported one;
/// frontends see `None` rather than a magic number.
pub fn battery_from_wire(percent: i32) -> Option<i32> {
    if percent < 0 { None } else { Some(percent) }
}

/// The wire `FirmwareRevision` as a field: the daemon reports the empty
/// string until it has read the pen, which is no firmware to show.
pub fn firmware_from_wire(text: String) -> Option<String> {
    (!text.is_empty()).then_some(text)
}

/// The address embedded in a `dev_XX_XX_XX_XX_XX_XX` object path, in the
/// `Pen1.Address` form: upper-case hex, colon-separated. Empty when the
/// path carries no address.
pub fn address_from_object_path(path: &str) -> String {
    let basename = path.rsplit_once('/').map_or(path, |(_, leaf)| leaf);
    let Some(hex) = basename.strip_prefix("dev_") else {
        return String::new();
    };
    let bytes: Vec<&str> = hex.split('_').collect();
    let well_formed = bytes.len() == 6
        && bytes
            .iter()
            .all(|byte| byte.len() == 2 && byte.chars().all(|c| c.is_ascii_hexdigit()));
    if !well_formed {
        return String::new();
    }
    bytes
        .iter()
        .map(|byte| byte.to_uppercase())
        .collect::<Vec<_>>()
        .join(":")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn battery_detail_needs_something_to_say() {
        assert_eq!(BatteryDetail::from_wire(-1.0, -1, false), None);
        let detail = BatteryDetail::from_wire(78.73, 4105, false).unwrap();
        assert_eq!(detail.percent, Some(78.73));
        assert_eq!(detail.millivolts, Some(4105));
        let docked = BatteryDetail::from_wire(-1.0, -1, true).unwrap();
        assert!(docked.docked && docked.percent.is_none());
    }

    #[test]
    fn battery_minus_one_becomes_none() {
        assert_eq!(battery_from_wire(-1), None);
        assert_eq!(battery_from_wire(0), Some(0));
        assert_eq!(battery_from_wire(100), Some(100));
        assert_eq!(firmware_from_wire(String::new()), None);
        assert_eq!(
            firmware_from_wire("2.0.37".to_string()),
            Some("2.0.37".to_string())
        );
    }

    #[test]
    fn an_address_comes_back_from_a_dev_path() {
        // Assembled at runtime: the privacy gate rejects colon-hex literals.
        let expected = String::from("0A:0B:0C:0D:0E") + ":0F";
        assert_eq!(
            address_from_object_path("/io/github/gonzalezerik/Px13Pen/pen/dev_0a_0b_0c_0d_0e_0f"),
            expected
        );
    }

    #[test]
    fn paths_without_an_address_yield_empty() {
        for path in [
            "/io/github/gonzalezerik/Px13Pen/Daemon",
            "/io/github/gonzalezerik/Px13Pen/pen/",
            "/io/github/gonzalezerik/Px13Pen/pen/unknown",
            "/io/github/gonzalezerik/Px13Pen/pen/dev_0a_0b",
            "/io/github/gonzalezerik/Px13Pen/pen/dev_0a_0b_0c_0d_0e_zz",
        ] {
            assert_eq!(address_from_object_path(path), "", "not an address: {path}");
        }
    }
}
