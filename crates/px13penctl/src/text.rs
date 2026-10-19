// SPDX-License-Identifier: AGPL-3.0-or-later
//! The human form of `watch`: one readable line per event, for a terminal
//! left open next to the pen. Scripts use `--json` (docs/px13penctl.md); this
//! wording is not a contract.

use px13pen_client::{Pen, WatchEvent};

use crate::json;

/// The lines one event prints, without the elapsed-time prefix.
pub fn event_lines(event: &WatchEvent) -> Vec<String> {
    match event {
        WatchEvent::Snapshot { pens } if pens.is_empty() => {
            vec!["watching px13pend: no pen connected yet".to_string()]
        }
        WatchEvent::Snapshot { pens } => {
            let mut lines = vec!["watching px13pend:".to_string()];
            lines.extend(pens.iter().map(|pen| format!("  {}", pen_state(pen))));
            lines
        }
        WatchEvent::Added { pen } => vec![format!("pen added: {}", pen_state(pen))],
        WatchEvent::Changed { pen } => vec![pen_state(pen)],
        WatchEvent::Removed { path, address } => {
            let who = if address.is_empty() { path } else { address };
            vec![format!("pen removed: {who}")]
        }
        WatchEvent::Pairing { kind, detail } => vec![match kind.as_str() {
            "announced" => format!("pen nearby ({detail}), pairing…"),
            "bonded" => format!("paired: {detail}"),
            "failed" => format!("pairing failed: {detail}"),
            _ => format!("pairing {kind}: {detail}"),
        }],
    }
}

/// `SA205H  battery 87 %  connected`.
fn pen_state(pen: &Pen) -> String {
    let link = if pen.present {
        "connected"
    } else {
        "link down"
    };
    let line = json::status_line(pen);
    // status_line already says "link down" for absent pens.
    if pen.present {
        format!("{line}  {link}")
    } else {
        line
    }
}

/// `[  12.3 s] ` — seconds since `watch` started.
pub fn prefix(seconds: f64) -> String {
    format!("[{seconds:>6.1} s] ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pen(present: bool) -> Pen {
        Pen {
            path: "/io/github/gonzalezerik/Px13Pen/pen/dev_0a_0b_0c_0d_0e_0f".to_string(),
            name: "SA205H".to_string(),
            address: "XX:XX:XX:XX:XX:XX".to_string(),
            present,
            paired: true,
            trusted: true,
            battery: Some(87),
            battery_detail: None,
            haptics: None,
            capabilities: None,
        }
    }

    #[test]
    fn an_empty_snapshot_says_so() {
        let lines = event_lines(&WatchEvent::Snapshot { pens: Vec::new() });
        assert_eq!(lines, ["watching px13pend: no pen connected yet"]);
    }

    #[test]
    fn a_connected_pen_reads_naturally() {
        let lines = event_lines(&WatchEvent::Changed { pen: pen(true) });
        assert_eq!(lines, ["SA205H  battery 87 %  connected"]);
        let lines = event_lines(&WatchEvent::Changed { pen: pen(false) });
        assert_eq!(lines, ["SA205H  battery 87 %  link down"]);
    }

    #[test]
    fn pairing_progress_is_spelled_out() {
        let event = |kind: &str, detail: &str| WatchEvent::Pairing {
            kind: kind.to_string(),
            detail: detail.to_string(),
        };
        assert_eq!(
            event_lines(&event("announced", "XX:XX:XX:XX:XX:XX")),
            ["pen nearby (XX:XX:XX:XX:XX:XX), pairing…"]
        );
        assert_eq!(
            event_lines(&event("bonded", "XX:XX:XX:XX:XX:XX")),
            ["paired: XX:XX:XX:XX:XX:XX"]
        );
        assert_eq!(
            event_lines(&event("failed", "XX:XX:XX:XX:XX:XX: timed out")),
            ["pairing failed: XX:XX:XX:XX:XX:XX: timed out"]
        );
    }

    #[test]
    fn the_prefix_is_fixed_width() {
        assert_eq!(prefix(1.31), "[   1.3 s] ");
        assert_eq!(prefix(123.0), "[ 123.0 s] ");
    }
}
