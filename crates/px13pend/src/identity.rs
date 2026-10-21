// SPDX-License-Identifier: AGPL-3.0-or-later
//! Pen identity by transducer serial — the one number every link agrees on.
//!
//! A pen bonds with each host under its own Bluetooth address, and the
//! digitizer keeps announcing a pen over the address it last paired
//! with somebody else. Chasing that old address is chasing the same
//! pen twice: the pen is already here, on a live link, under its new
//! identity (docs/protocol/seamless-pairing.md §6). The serial is the
//! stable half of the pairing story — the digitizer's report 7 names
//! the serial of the pen currently in its range, report 34 repeats it,
//! and the pen's own BLE feature report `0x40` answers with the very
//! same number (findings/006 §2). Matching pens by that serial, not by
//! an address that changes with every bond, lets the daemon tell "the
//! pen I already have, waving an old card" from "a different pen".
//!
//! The decision is pure: frames and pen facts in, a choice out — the
//! whole rule is testable without a pen or a Bluetooth stack.

/// Digitizer report id of the pen (in-range) report.
const REPORT_PEN: u8 = 0x07;
/// Digitizer report id of the pen-presence telemetry report.
const REPORT_PRESENCE: u8 = 0x22;
/// The transducer vendor that report 7 and feature `0x40` agree on.
const VENDOR: u16 = 0x330A;
/// End of the serial inside a report 7 frame (report id included):
/// it follows the 16-bit scan time, and the vendor's two bytes follow
/// it. Byte-exact against the Linux digitizer capture (findings/006 §3)
/// and the Windows feature read (findings/006 §2).
const REPORT7_SERIAL_END: usize = 21;

/// The serial of the pen the digitizer currently senses in range.
/// Report 7 streams a few hundred times a second while a pen hovers,
/// so by the time an announcement is processed the number is fresh.
#[derive(Default)]
pub struct HoverSerial {
    serial: Option<u32>,
}

impl HoverSerial {
    /// Feed one digitizer frame. Only the two pen reports speak for the
    /// pen in range: report 7 while its in-range bit is set, and report
    /// 34, which the panel emits as a pen walks into its field.
    pub fn on_frame(&mut self, frame: &[u8]) {
        match frame.first().copied() {
            Some(REPORT_PEN) if frame.len() >= REPORT7_SERIAL_END => {
                self.serial = if frame[1] & crate::ink::IN_RANGE == 0 {
                    None
                } else {
                    le32(&frame[REPORT7_SERIAL_END - 4..REPORT7_SERIAL_END])
                };
            }
            Some(REPORT_PRESENCE) if frame.len() >= 5 => {
                if let Some(serial) = le32(&frame[1..5]) {
                    self.serial = Some(serial);
                }
            }
            _ => {}
        }
    }

    /// The serial of the pen in range right now, if the digitizer says
    /// there is one.
    pub fn hovering(&self) -> Option<u32> {
        self.serial
    }
}

/// What to do when the digitizer announces a pen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// Run a seamless pairing for this address.
    Pair,
    /// Say nothing and pair nothing: the pen in range is one this host
    /// already paired, announcing an identity somebody else bonded.
    SamePen,
    /// Not now: another pen holds a live link and the announcement
    /// cannot be placed with the information at hand.
    Busy,
}

/// One pen this host knows, as far as the announcement decision needs
/// it. `serial` is `None` while the pen's identity has not been read
/// (or never could be): the decision then falls back to link state.
pub struct Known {
    pub paired: bool,
    pub present: bool,
    pub serial: Option<u32>,
}

/// Place one announcement. A serial match against a paired pen wins
/// even while that pen's link is down — the pen is asleep, not gone,
/// and its old identity is no more worth pairing than its new one.
/// Without a match, any live link keeps the quiet behaviour the daemon
/// has always had: announcements are for the pen in range, and the pen
/// in range is the one already connected.
pub fn announce_choice(hovering: Option<u32>, known: impl IntoIterator<Item = Known>) -> Choice {
    let mut busy = false;
    for pen in known {
        if pen.paired && hovering.is_some() && pen.serial == hovering {
            return Choice::SamePen;
        }
        busy |= pen.present;
    }
    if busy { Choice::Busy } else { Choice::Pair }
}

/// The serial out of the pen's answer to feature report `0x40`: the
/// 32-bit transducer serial, then the 16-bit vendor, both
/// little-endian, the whole thing preceded by the report id when the
/// reader kept it (`HidD_GetFeature` does, a GATT read does not).
/// Anything that does not carry the expected vendor right behind the
/// serial is not this pen's identity, whatever else it is.
pub fn parse_identity_report(value: &[u8]) -> Option<u32> {
    identity_at(value).or_else(|| identity_at(value.get(1..)?))
}

fn identity_at(body: &[u8]) -> Option<u32> {
    let serial = le32(body.get(..4)?)?;
    let vendor = u16::from_le_bytes(body.get(4..6)?.try_into().ok()?);
    (serial != 0 && vendor == VENDOR).then_some(serial)
}

fn le32(bytes: &[u8]) -> Option<u32> {
    let serial = u32::from_le_bytes(bytes.try_into().ok()?);
    (serial != 0).then_some(serial)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic pen: this number names no device that exists.
    const SYNTHETIC: u32 = 0x1122_3344;

    /// One report 7 frame: id, button byte, then padding to the serial.
    fn report7(in_range: bool, serial: u32) -> Vec<u8> {
        let mut frame = vec![REPORT_PEN, u8::from(in_range)];
        frame.resize(REPORT7_SERIAL_END - 4, 0);
        frame.extend(serial.to_le_bytes());
        frame.extend([0x0A, 0x33, 0x00, 0x00]);
        frame
    }

    #[test]
    fn report_seven_names_the_pen_in_range_and_only_that_one() {
        let mut hover = HoverSerial::default();
        assert_eq!(hover.hovering(), None, "cold start knows nothing");
        hover.on_frame(&report7(true, SYNTHETIC));
        assert_eq!(hover.hovering(), Some(SYNTHETIC));
        hover.on_frame(&report7(false, SYNTHETIC));
        assert_eq!(hover.hovering(), None, "an out-of-range frame clears it");
        hover.on_frame(&report7(true, SYNTHETIC));
        hover.on_frame(&[REPORT_PEN, 0x01]);
        assert_eq!(
            hover.hovering(),
            Some(SYNTHETIC),
            "a frame too short to carry the number says nothing"
        );
    }

    #[test]
    fn report_thirty_four_names_the_pen_walking_in() {
        let mut hover = HoverSerial::default();
        let mut frame = vec![REPORT_PRESENCE];
        frame.extend(SYNTHETIC.to_le_bytes());
        frame.resize(16, 0);
        hover.on_frame(&frame);
        assert_eq!(hover.hovering(), Some(SYNTHETIC));
        hover.on_frame(&[REPORT_PRESENCE, 0x00, 0x00]);
        assert_eq!(
            hover.hovering(),
            Some(SYNTHETIC),
            "a too-short frame says nothing"
        );
    }

    #[test]
    fn other_reports_do_not_claim_a_pen() {
        let mut hover = HoverSerial::default();
        hover.on_frame(&report7(true, SYNTHETIC));
        let announcement = [0x6E, 1, 2, 3, 4, 5, 6, 0x03];
        hover.on_frame(&announcement);
        hover.on_frame(&[0x01, 0x00, 0x05]);
        assert_eq!(
            hover.hovering(),
            Some(SYNTHETIC),
            "only the pen reports speak"
        );
    }

    #[test]
    fn the_identity_feature_answers_the_digitizers_number() {
        // The shape Windows read off the pen (findings/006 §2): report
        // id, serial, vendor, a model number, then padding.
        let mut with_id = vec![0x40];
        with_id.extend(SYNTHETIC.to_le_bytes());
        with_id.extend([0x0A, 0x33, 0x0F, 0x0C, 0x03, 0x00]);
        with_id.resize(81, 0);
        assert_eq!(parse_identity_report(&with_id), Some(SYNTHETIC));
        assert_eq!(
            parse_identity_report(&with_id[1..]),
            Some(SYNTHETIC),
            "a GATT read without the report id parses too"
        );
    }

    #[test]
    fn the_pens_other_report_answers_are_not_an_identity() {
        // Reading every Report characteristic is only safe because the
        // vendor check rejects the rest. The haptics capabilities
        // feature opens with the non-zero auto-trigger control
        // 0x000D0042, and the current-state feature is five bytes —
        // shapes taken from the pen's live answers (findings/006 §2).
        let mut capabilities = (0x000D_0042u32).to_le_bytes().to_vec();
        capabilities.extend([0x03, 0x00, 0x04, 0x00]);
        capabilities.resize(84, 0);
        assert_eq!(parse_identity_report(&capabilities), None);
        assert_eq!(parse_identity_report(&[0x00, 0xFF, 0x01, 0x00, 0x00]), None);
    }

    #[test]
    fn anything_without_the_known_vendor_is_not_an_identity() {
        let mut body = SYNTHETIC.to_le_bytes().to_vec();
        body.extend([0x99, 0x99, 0x0F, 0x0C]);
        assert_eq!(parse_identity_report(&body), None, "wrong vendor");
        let mut zeroed = vec![0u8; 6];
        zeroed.extend([0x0A, 0x33]);
        assert_eq!(
            parse_identity_report(&zeroed),
            None,
            "a zero serial claims nothing"
        );
        assert_eq!(parse_identity_report(&[0x0A, 0x33]), None, "too short");
        assert_eq!(parse_identity_report(&[]), None);
    }

    fn pen(paired: bool, present: bool, serial: Option<u32>) -> Known {
        Known {
            paired,
            present,
            serial,
        }
    }

    #[test]
    fn a_paired_pens_old_identity_pairs_nothing_even_asleep() {
        // The scene this exists for: the pen is paired and connected
        // under one address; the digitizer announces another address
        // carrying the same transducer serial.
        let connected = [pen(true, true, Some(SYNTHETIC))];
        assert_eq!(announce_choice(Some(SYNTHETIC), connected), Choice::SamePen);
        let asleep = [pen(true, false, Some(SYNTHETIC))];
        assert_eq!(
            announce_choice(Some(SYNTHETIC), asleep),
            Choice::SamePen,
            "asleep is not gone: its old identity is still nobody to pair"
        );
    }

    #[test]
    fn a_link_holds_the_quiet_behaviour_when_identity_cannot_place_it() {
        let connected = [pen(true, true, Some(0x0000_0001))];
        let other = 0x0000_0002u32;
        assert_eq!(announce_choice(Some(other), connected), Choice::Busy);
        assert_eq!(
            announce_choice(None, [pen(true, true, None)]),
            Choice::Busy,
            "no serial anywhere: fall back to link state"
        );
    }

    #[test]
    fn an_unknown_pen_with_nobody_connected_pairs() {
        assert_eq!(announce_choice(Some(0x0000_0001), []), Choice::Pair);
        assert_eq!(announce_choice(None, []), Choice::Pair);
        assert_eq!(
            announce_choice(Some(0x0000_0001), [pen(false, false, Some(0x0000_0002))]),
            Choice::Pair,
            "a known pen that is neither paired nor present blocks nothing"
        );
    }
}
