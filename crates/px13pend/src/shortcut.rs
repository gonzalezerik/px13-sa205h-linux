// SPDX-License-Identifier: AGPL-3.0-or-later
//! The pen's top shortcut button, read from the pen's own HID node.
//!
//! The pen classifies the gesture itself and reports it as one
//! boot-keyboard press (findings/006 §5): Left-GUI plus F20 for a single
//! press, F19 for a double, F18 for a long press, then an empty release
//! frame. Reading the reports off the pen's hidraw node is deliberate —
//! the compositor holds the matching evdev keyboard node under an
//! exclusive grab, while hidraw hands every reader its own copy of
//! every report (findings/010 §4).
//!
//! The node lives and dies with the Bluetooth link, so this never holds
//! a closed file: `resync` re-enumerates on the service loop's slower
//! tick and `poll` drains the open nodes without blocking on the faster
//! one. Reads on Linux carry the report id as the first byte, so
//! `parse_report` screens for the keyboard report before parsing.

use std::collections::{HashMap, HashSet};

use hidapi::{HidApi, HidDevice};
use px13pen_core::SUPPORTED_PENS;

/// Reports drained from one node per poll; the button is rare and the
/// loop has a digitizer stream to keep up with too.
const POLL_LIMIT: usize = 16;
/// Report bytes a pen report can occupy; keyboard reports use eight.
const READ_BYTES: usize = 64;

/// A gesture the pen has already classified. The key code in the report
/// *is* the gesture; nothing here re-times anything (findings/006 §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gesture {
    Single,
    Double,
    Long,
}

impl Gesture {
    /// The name the capability list and the `ButtonEvent` signal use.
    pub fn name(&self) -> &'static str {
        match self {
            Gesture::Single => "single",
            Gesture::Double => "double",
            Gesture::Long => "long",
        }
    }

    fn from_keycode(code: u8) -> Option<Self> {
        match code {
            0x6f => Some(Gesture::Single), // HID usage: keyboard F20
            0x6e => Some(Gesture::Double), // F19
            0x6d => Some(Gesture::Long),   // F18
            _ => None,
        }
    }
}

/// One button gesture, with the Bluetooth address the node reported
/// (empty for nodes that name no serial — a lone pen then claims it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub address: Option<String>,
    pub gesture: Gesture,
}

/// The report id of the pen's boot-keyboard collection, the one the
/// top button reports on (findings/006 §5).
const KEYBOARD_REPORT: u8 = 0x43;

/// One report as read from the node: the report id, then a
/// boot-keyboard payload — modifiers, a reserved byte, key codes. A
/// top-button press is the keyboard report carrying Left-GUI (0x08)
/// and the gesture's key code; the release frame is all zeroes and
/// yields nothing, and so does every other report id: the pen streams
/// more collections over the same node.
pub fn parse_report(report: &[u8]) -> Option<Gesture> {
    if report.first() != Some(&KEYBOARD_REPORT) {
        return None;
    }
    Gesture::from_keycode(*report.get(3)?)
}

/// A pen's HID node, open and non-blocking.
struct Node {
    device: HidDevice,
    /// The node's serial — for a Bluetooth pen its address, which
    /// matches `Pen1.Address`.
    address: Option<String>,
}

/// Every supported pen's HID node, followed across links coming and
/// going.
pub struct Watcher {
    api: HidApi,
    open: HashMap<String, Node>,
}

impl Watcher {
    pub fn new() -> Result<Self, String> {
        HidApi::new()
            .map(|api| Watcher {
                api,
                open: HashMap::new(),
            })
            .map_err(|error| format!("cannot open the HID subsystem: {error}"))
    }

    /// Open every supported pen node not open yet, drop the ones that
    /// vanished with their link. A node that refuses to open (gone
    /// between enumeration and open) is retried on the next resync.
    pub fn resync(&mut self) {
        let _ = self.api.refresh_devices();
        let mut present = HashSet::new();
        let mut arrivals = Vec::new();
        for info in self.api.device_list().filter(|info| {
            SUPPORTED_PENS
                .iter()
                .any(|(id, _)| id.vendor == info.vendor_id() && id.product == info.product_id())
        }) {
            let shown = info.path().to_string_lossy().into_owned();
            if present.insert(shown.clone()) && !self.open.contains_key(&shown) {
                arrivals.push((
                    shown,
                    info.path(),
                    info.serial_number()
                        .filter(|serial| !serial.is_empty())
                        .map(|serial| serial.to_uppercase()),
                ));
            }
        }
        for (shown, path, address) in arrivals {
            match self.api.open_path(path) {
                Ok(device) => match device.set_blocking_mode(false) {
                    Ok(()) => {
                        self.open.insert(shown, Node { device, address });
                    }
                    Err(error) => eprintln!("shortcut {shown}: {error}"),
                },
                Err(error) => eprintln!("shortcut {shown}: {error}"),
            }
        }
        self.open.retain(|path, _| present.contains(path));
    }

    /// Everything read since the last call, without blocking; a node
    /// that errors is dropped (its link died) and returns on the next
    /// resync.
    pub fn poll(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        let mut closed = Vec::new();
        for (path, node) in &mut self.open {
            let mut buffer = [0u8; READ_BYTES];
            for _ in 0..POLL_LIMIT {
                match node.device.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(read) => {
                        if let Some(gesture) = parse_report(&buffer[..read]) {
                            events.push(Event {
                                address: node.address.clone(),
                                gesture,
                            });
                        }
                    }
                    Err(_) => {
                        closed.push(path.clone());
                        break;
                    }
                }
            }
        }
        for path in closed {
            self.open.remove(&path);
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A press as a Linux read delivers it: the keyboard report id,
    /// Left-GUI and the gesture's key in the boot-keyboard payload.
    /// (Captured from the pen's node, findings/006 §5.)
    fn press(key: u8) -> Vec<u8> {
        vec![KEYBOARD_REPORT, 0x08, 0x00, key, 0, 0, 0, 0, 0]
    }

    #[test]
    fn the_key_code_is_the_gesture() {
        assert_eq!(parse_report(&press(0x6f)), Some(Gesture::Single));
        assert_eq!(parse_report(&press(0x6e)), Some(Gesture::Double));
        assert_eq!(parse_report(&press(0x6d)), Some(Gesture::Long));
    }

    #[test]
    fn a_release_is_not_a_gesture() {
        assert_eq!(
            parse_report(&[KEYBOARD_REPORT, 0, 0, 0, 0, 0, 0, 0, 0]),
            None
        );
    }

    #[test]
    fn other_reports_on_the_same_node_are_not_gestures() {
        // The pen streams other collections over the same node: a
        // gesture key under a different report id, a different key
        // under this one, and a modifier-only keyboard frame all read
        // as nothing.
        assert_eq!(parse_report(&[0x41, 0x08, 0x00, 0x6f, 0, 0, 0, 0, 0]), None);
        assert_eq!(parse_report(&press(0x04)), None);
        assert_eq!(parse_report(&press(0x00)), None);
    }

    #[test]
    fn short_frames_are_not_gestures() {
        assert_eq!(parse_report(&[]), None);
        assert_eq!(parse_report(&[KEYBOARD_REPORT, 0x08, 0x00]), None);
    }

    #[test]
    fn gesture_names_match_the_capability_list() {
        assert_eq!(Gesture::Single.name(), "single");
        assert_eq!(Gesture::Double.name(), "double");
        assert_eq!(Gesture::Long.name(), "long");
    }
}
