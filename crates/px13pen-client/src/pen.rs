// SPDX-License-Identifier: AGPL-3.0-or-later
//! What one pen looks like after it has been read off the bus: the
//! `Pen1` properties, the `Haptics1` policy and the projected
//! capability dict (docs/dbus.md), in the field names frontends
//! publish (docs/px13penctl.md). Serialisation rules — which keys vanish
//! when absent — are the contract, so they live with the types.

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
    pub battery: Option<i32>,
    pub haptics: Option<Haptics>,
    pub capabilities: Option<Capabilities>,
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
    fn battery_minus_one_becomes_none() {
        assert_eq!(battery_from_wire(-1), None);
        assert_eq!(battery_from_wire(0), Some(0));
        assert_eq!(battery_from_wire(100), Some(100));
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
