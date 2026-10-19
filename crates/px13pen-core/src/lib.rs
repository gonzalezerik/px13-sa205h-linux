// SPDX-License-Identifier: AGPL-3.0-or-later
//! Protocol types shared by `px13pend` and `px13penctl`.
//!
//! Everything here is written from `docs/protocol/`. Nothing is derived from
//! decompiled code (see CONTRIBUTING.md §6).

pub mod haptics;
pub mod seamless;
pub mod telemetry;

/// A USB-IF vendor/product pair identifying a HID device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HidId {
    pub vendor: u16,
    pub product: u16,
}

/// Digitizers this project has been verified against.
pub const SUPPORTED_DIGITIZERS: &[(HidId, &str)] = &[(
    HidId {
        vendor: 0x04f3,
        product: 0x4631,
    },
    "ELAN9008 (ASUS ProArt PX13 HN7306)",
)];

/// Pens this project has been verified against, as they appear on the
/// pen's own HID node (bus id 0005, Bluetooth). The udev rule is
/// generated from these tables, never written by hand.
pub const SUPPORTED_PENS: &[(HidId, &str)] = &[(
    HidId {
        vendor: 0x330a,
        product: 0x0001,
    },
    "MPP 2.6 haptic pen (ASUS SA205H class)",
)];

/// Looks up a digitizer by its HID id.
pub fn supported_digitizer(id: HidId) -> Option<&'static str> {
    SUPPORTED_DIGITIZERS
        .iter()
        .find(|(known, _)| *known == id)
        .map(|(_, name)| *name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_supported_pen_is_known() {
        let id = HidId {
            vendor: 0x330a,
            product: 0x0001,
        };
        assert!(SUPPORTED_PENS.iter().any(|(known, _)| *known == id));
    }

    #[test]
    fn px13_digitizer_is_known() {
        let id = HidId {
            vendor: 0x04f3,
            product: 0x4631,
        };
        assert!(supported_digitizer(id).is_some());
    }

    #[test]
    fn unknown_digitizer_is_rejected() {
        let id = HidId {
            vendor: 0x04f3,
            product: 0x0000,
        };
        assert!(supported_digitizer(id).is_none());
    }
}
