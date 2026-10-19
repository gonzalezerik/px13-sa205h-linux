// SPDX-License-Identifier: AGPL-3.0-or-later
//! Raw evdev view of the pen, to learn what its BLE HID reports become on
//! Linux. After pairing, BlueZ registers the pen with the kernel over
//! uhid, and the top-button report (findings 006 §5) surfaces here as
//! ordinary input events. The key codes are still unknown — the
//! button-matrix capture is pending — so this prints events verbatim:
//! press each gesture and read off the codes.

use std::io::Read;
use std::path::Path;

/// `struct input_event` on 64-bit little-endian Linux: timeval (2 × i64),
/// type u16, code u16, value i32.
pub const RECORD_BYTES: usize = 24;

/// One decoded evdev record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Event {
    pub seconds: i64,
    pub microseconds: i64,
    pub kind: u16,
    pub code: u16,
    pub value: i32,
}

impl Event {
    /// One-line rendering. Codes stay raw kernel numbers on purpose —
    /// mapping them is what this tool exists to discover.
    pub fn format(&self) -> String {
        let kind = event_kind_name(self.kind).unwrap_or("OTHER");
        format!(
            "[{:>10}.{:06}] {kind:<4} code {:#06x} value {}",
            self.seconds, self.microseconds, self.code, self.value
        )
    }
}

/// Parse one evdev record from little-endian bytes; short slices (a read
/// split across a boundary) are None.
pub fn parse_record(bytes: &[u8]) -> Option<Event> {
    let raw: &[u8; RECORD_BYTES] = bytes.try_into().ok()?;
    let i64_at = |offset: usize| -> Option<i64> {
        let chunk: [u8; 8] = raw[offset..offset + 8].try_into().ok()?;
        Some(i64::from_le_bytes(chunk))
    };
    Some(Event {
        seconds: i64_at(0)?,
        microseconds: i64_at(8)?,
        kind: u16::from_le_bytes([raw[16], raw[17]]),
        code: u16::from_le_bytes([raw[18], raw[19]]),
        value: i32::from_le_bytes([raw[20], raw[21], raw[22], raw[23]]),
    })
}

/// Names for the event types a HID pen actually produces.
pub fn event_kind_name(kind: u16) -> Option<&'static str> {
    match kind {
        0x0000 => Some("SYN"),
        0x0001 => Some("KEY"),
        0x0002 => Some("REL"),
        0x0003 => Some("ABS"),
        0x0004 => Some("MSC"),
        0x0005 => Some("SW"),
        0x0011 => Some("LED"),
        _ => None,
    }
}

/// Pen-looking evdev nodes: entries under `<sysroot>/input` whose device
/// name mentions the pen (uhid names the node after the GATT device).
/// Returns (event node name, device name) sorted by node name.
pub fn find_pen_nodes(sysroot: &Path) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(sysroot.join("input")) else {
        return found;
    };
    for entry in entries.flatten() {
        let node = entry.file_name().to_string_lossy().into_owned();
        if !node.starts_with("event") {
            continue;
        }
        let Ok(device_name) = std::fs::read_to_string(entry.path().join("device/name")) else {
            continue;
        };
        let device_name = device_name.trim().to_string();
        if device_name.to_lowercase().contains("pen") {
            found.push((node, device_name));
        }
    }
    found.sort();
    found
}

/// Stream events from the pen's evdev node until stopped.
pub fn watch_buttons(list_only: bool) -> Result<(), String> {
    let nodes = find_pen_nodes(Path::new("/sys"));
    if list_only {
        if nodes.is_empty() {
            println!("no pen-named evdev node");
        }
        for (node, name) in &nodes {
            println!("/dev/input/{node}  {name}");
        }
        return Ok(());
    }
    let first = match nodes.as_slice() {
        [] => {
            return Err(
                "no pen-named evdev node — is the pen paired and awake? BlueZ creates the node on \
                 connect; reading /dev/input needs input group membership"
                    .to_string(),
            );
        }
        [only] => only,
        [first, rest @ ..] => {
            for (node, name) in rest {
                println!("also present: /dev/input/{node}  {name}");
            }
            first
        }
    };
    let (node, name) = first;
    println!("reading /dev/input/{node} ({name}) — Ctrl-C to stop");
    let mut file = std::fs::File::open(format!("/dev/input/{node}"))
        .map_err(|error| format!("cannot open /dev/input/{node}: {error} (input group?)"))?;
    let mut buffer = vec![0u8; 4096];
    let mut pending: Vec<u8> = Vec::new();
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("read from /dev/input/{node} failed: {error}"))?;
        if read == 0 {
            return Ok(());
        }
        pending.extend_from_slice(&buffer[..read]);
        let complete = pending.len() - pending.len() % RECORD_BYTES;
        for chunk in pending[..complete].as_chunks::<RECORD_BYTES>().0 {
            if let Some(event) = parse_record(chunk) {
                println!("{}", event.format());
            }
        }
        pending.drain(..complete);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn record(kind: u16, code: u16, value: i32) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&7i64.to_le_bytes());
        bytes.extend_from_slice(&123456i64.to_le_bytes());
        bytes.extend_from_slice(&kind.to_le_bytes());
        bytes.extend_from_slice(&code.to_le_bytes());
        bytes.extend_from_slice(&value.to_le_bytes());
        bytes
    }

    #[test]
    fn parses_a_full_record() {
        let event = parse_record(&record(0x0001, 0x01c, 1)).unwrap();
        assert_eq!(
            event,
            Event {
                seconds: 7,
                microseconds: 123456,
                kind: 0x0001,
                code: 0x01c,
                value: 1,
            }
        );
        assert!(event.format().contains("KEY"));
        assert!(event.format().contains("7.123456"));
    }

    #[test]
    fn short_reads_are_not_records() {
        assert_eq!(parse_record(&[]), None);
        assert_eq!(parse_record(&record(0x0000, 0x0000, 0)[..23]), None);
    }

    #[test]
    fn finds_pen_nodes_in_a_fake_sysfs() {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "px13pend-buttons-test-{}",
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        let mk = |node: &str, name: &str| {
            let dir = root.join("input").join(node).join("device");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("name"), format!("{name}\n")).unwrap();
        };
        mk("event9", "ASUS Pen Keyboard");
        mk("event8", "Sleep Button");
        mk("mouse3", "ELAN Touchpad");
        assert_eq!(
            find_pen_nodes(&root),
            vec![("event9".to_string(), "ASUS Pen Keyboard".to_string())]
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
}
