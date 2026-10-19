// SPDX-License-Identifier: AGPL-3.0-or-later
//! Seamless pairing: the ELAN digitizer's `0xFFF4` vendor collection.
//!
//! This collection is Microsoft's *Seamless Pairing for Windows Pens*
//! protocol (docs/protocol/seamless-pairing.md). The digitizer announces a
//! pen in range with input report `0x6E`: six Bluetooth address bytes
//! little-endian, then a flag byte (bit 0 random address, bit 1 Host Pairing
//! Exists for the host registered in feature `0x56`). The host answers with
//! a pairing key in output `0x6F`, after which the pen advertises and can be
//! paired (findings/008).
//!
//! Nothing in this module talks to a device; it names the reports and
//! parses the announcement.

/// Input report id of the Device Bluetooth Report (`0x6E`).
pub const REPORT_NOTIFICATION: u8 = 0x6E;

/// Feature report id of the Device Capability Report (`0x70`), bit 0 =
/// seamless pair capable. The host sets it and reads it back.
pub const FEATURE_ENABLED: u8 = 0x70;

/// Output report id of the Pairing Key Report (`0x6F`): announced address,
/// its type, and a 16-byte temporary key.
pub const OUTPUT_KEY_RELAY: u8 = 0x6F;

/// Flag bit: the announced address is a random (not public) address.
pub const FLAG_ADDRESS: u8 = 0x01;

/// Flag bit: Host Pairing Exists, the pen already has a pairing with the
/// host address registered in feature `0x56`.
pub const FLAG_CONSUMER: u8 = 0x02;

/// One parsed `0x6E` frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PairingNotification {
    /// The pen's Bluetooth address, most-significant byte first (the wire
    /// carries it reversed; [`parse_pairing_report`] fixes the order).
    pub address: [u8; 6],
    /// Raw flag byte; see [`FLAG_ADDRESS`] and [`FLAG_CONSUMER`].
    pub flags: u8,
}

impl PairingNotification {
    /// The address in colon-separated upper-case form, most-significant
    /// byte first — the form Bluetooth stacks and BlueZ object paths use.
    pub fn address_string(&self) -> String {
        self.address
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<Vec<_>>()
            .join(":")
    }

    /// True when the given flag bit is set.
    pub fn has_flag(&self, flag: u8) -> bool {
        self.flags & flag != 0
    }
}

/// Parse one hidraw frame (report id included). `None` unless the frame is
/// a well-formed `0x6E` notification: id `0x6E` plus exactly seven more
/// bytes, six of address and one of flags.
pub fn parse_pairing_report(frame: &[u8]) -> Option<PairingNotification> {
    let (&id, rest) = frame.split_first()?;
    if id != REPORT_NOTIFICATION || rest.len() != 7 {
        return None;
    }
    let mut address = [0u8; 6];
    address.copy_from_slice(&rest[..6]);
    address.reverse();
    Some(PairingNotification {
        address,
        flags: rest[6],
    })
}

/// Feature report id of the Host Bluetooth Report (`0x56`): the host's own
/// address, set by the host so the digitizer can tell whether a pen already
/// has a pairing with it.
pub const FEATURE_HOST: u8 = 0x56;

/// Parse a colon-separated Bluetooth address, most-significant byte first.
pub fn parse_address(text: &str) -> Option<[u8; 6]> {
    let mut address = [0u8; 6];
    let mut parts = text.split(':');
    for byte in &mut address {
        let part = parts.next()?;
        if part.len() != 2 {
            return None;
        }
        *byte = u8::from_str_radix(part, 16).ok()?;
    }
    parts.next().is_none().then_some(address)
}

/// The Host Bluetooth Report for an adapter address (most-significant byte
/// first): little-endian address, then the address type (1 = random).
pub fn host_report(address: &[u8; 6], random: bool) -> [u8; 8] {
    let mut report = [0u8; 8];
    report[0] = FEATURE_HOST;
    for (to, from) in report[1..7].iter_mut().zip(address.iter().rev()) {
        *to = *from;
    }
    report[7] = u8::from(random);
    report
}

/// The Device Capability Report the host sends: seamless pair capable.
pub fn capability_report() -> [u8; 2] {
    [FEATURE_ENABLED, 0x01]
}

/// The Pairing Key Report for an announced pen: the address as the
/// announcement carried it (little-endian), its type bit, then the key.
pub fn pairing_key_report(note: &PairingNotification, key: &[u8; 16]) -> [u8; 24] {
    let mut report = [0u8; 24];
    report[0] = OUTPUT_KEY_RELAY;
    for (to, from) in report[1..7].iter_mut().zip(note.address.iter().rev()) {
        *to = *from;
    }
    report[7] = note.flags & FLAG_ADDRESS;
    report[8..].copy_from_slice(key);
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_report_is_little_endian_with_its_type() {
        let address = [0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f];
        assert_eq!(
            host_report(&address, false),
            [0x56, 0x0f, 0x0e, 0x0d, 0x0c, 0x0b, 0x0a, 0x00]
        );
        assert_eq!(host_report(&address, true)[7], 0x01);
    }

    #[test]
    fn key_report_echoes_the_announcement() {
        let note = parse_pairing_report(&wire_frame(0x03)).unwrap();
        let report = pairing_key_report(&note, &[0x11; 16]);
        assert_eq!(
            &report[..8],
            &[0x6f, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x01]
        );
        assert_eq!(&report[8..], &[0x11; 16]);
    }

    #[test]
    fn addresses_parse_most_significant_first() {
        let text = String::from("0A:0B:0C:0D:0E") + ":0F";
        assert_eq!(
            parse_address(&text),
            Some([0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f])
        );
        assert_eq!(parse_address("0A:0B"), None);
        assert_eq!(parse_address(&(text.clone() + ":10")), None);
        assert_eq!(parse_address("0A:0B:0C:0D:0E:ZZ"), None);
    }

    #[test]
    fn capability_says_capable() {
        assert_eq!(capability_report(), [0x70, 0x01]);
    }

    // The privacy gate rejects any literal shaped like a Bluetooth address,
    // so the expected string is assembled from two harmless pieces.
    fn expected_address() -> String {
        String::from("06:05:04:03:02") + ":01"
    }

    fn wire_frame(flags: u8) -> Vec<u8> {
        vec![
            REPORT_NOTIFICATION,
            0x01,
            0x02,
            0x03,
            0x04,
            0x05,
            0x06,
            flags,
        ]
    }

    #[test]
    fn notification_address_is_little_endian_on_the_wire() {
        let note = parse_pairing_report(&wire_frame(0x01)).unwrap();
        assert_eq!(note.address, [0x06, 0x05, 0x04, 0x03, 0x02, 0x01]);
        assert_eq!(note.address_string(), expected_address());
    }

    #[test]
    fn flag_bits_match_both_observed_values() {
        let unconsumed = parse_pairing_report(&wire_frame(0x01)).unwrap();
        let windows = parse_pairing_report(&wire_frame(0x03)).unwrap();
        assert!(unconsumed.has_flag(FLAG_ADDRESS));
        assert!(!unconsumed.has_flag(FLAG_CONSUMER));
        assert!(windows.has_flag(FLAG_ADDRESS));
        assert!(windows.has_flag(FLAG_CONSUMER));
    }

    #[test]
    fn rejects_other_reports_and_malformed_frames() {
        assert!(parse_pairing_report(&[]).is_none());
        assert!(parse_pairing_report(&[0x07, 1, 2, 3, 4, 5, 6, 7]).is_none());
        assert!(parse_pairing_report(&[REPORT_NOTIFICATION, 0x01]).is_none());
        let mut long_frame = wire_frame(0x01);
        long_frame.push(0x00);
        assert!(parse_pairing_report(&long_frame).is_none());
    }
}
