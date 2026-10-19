// SPDX-License-Identifier: AGPL-3.0-or-later
//! The JSON contract of `px13penctl`, described in docs/px13penctl.md.
//!
//! Pure serialisation: the pen shapes come from `px13pen-client`, and the
//! rules frontends rely on — empty output while no pen is connected,
//! an unreported battery as `null`, the tags of the `watch` stream —
//! live here and are tested here, away from the bus.

use px13pen_client::Pen;
use serde::Serialize;

/// One `watch` record: serialised as one compact line of JSON.
#[derive(Clone, Serialize)]
#[serde(tag = "type")]
pub enum Event {
    /// The complete visible pen set, emitted first so a consumer starts
    /// from a whole picture. An empty `pens` array is a statement, not
    /// silence: `watch` is a conversation, `status` is a render.
    #[serde(rename = "snapshot")]
    Snapshot { pens: Vec<Pen> },
    #[serde(rename = "pen.added")]
    PenAdded { pen: Pen },
    #[serde(rename = "pen.removed")]
    PenRemoved { path: String, address: String },
    #[serde(rename = "pen.changed")]
    PenChanged { pen: Pen },
    #[serde(rename = "pairing")]
    Pairing { kind: String, detail: String },
}

/// The `status --json` document: one object, one line. `None` — print
/// nothing at all, not even `{"pens":[]}` — when the pen list is empty,
/// which is what lets a bar module hide itself while no pen is connected.
pub fn status_json(pens: &[Pen]) -> Option<String> {
    #[derive(Serialize)]
    struct Status<'a> {
        pens: &'a [Pen],
    }

    if pens.is_empty() {
        return None;
    }
    // Serialising our own fixed shapes cannot fail: no field carries
    // user input that serde could choke on.
    Some(serde_json::to_string(&Status { pens }).expect("pens are serialisable"))
}

/// One `watch` record as one JSON Lines entry, newline included; the
/// line-buffered stdout flushes it the moment `print!` sees the newline.
pub fn event_line(event: &Event) -> String {
    let mut line = serde_json::to_string(event).expect("events are serialisable");
    line.push('\n');
    line
}

/// The text form of `status`: `name  battery 87 %`. The link is spelled
/// out only when the pen is away — the default view only ever shows
/// present pens, so the suffix appears together with `--all`.
pub fn status_line(pen: &Pen) -> String {
    let battery = match pen.battery {
        Some(percent) => format!("battery {percent} %"),
        None => "battery unknown".to_string(),
    };
    let link = if pen.present { "" } else { "  link down" };
    format!("{}  {battery}{link}", pen.name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use px13pen_client::{Capabilities, CapabilityHaptics, Waveform};

    fn pen(suffix: &str, present: bool, battery: Option<i32>) -> Pen {
        Pen {
            path: format!("/io/github/gonzalezerik/Px13Pen/pen/dev_{suffix}"),
            name: "SA205H".to_string(),
            address: String::new(),
            present,
            paired: true,
            trusted: true,
            battery,
            haptics: None,
            capabilities: None,
        }
    }

    fn parse(line: &str) -> serde_json::Value {
        serde_json::from_str(line).expect("the serialiser emits valid JSON")
    }

    #[test]
    fn status_prints_nothing_until_a_pen_shows_up() {
        assert!(status_json(&[]).is_none());
        let line = status_json(&[pen("0a_0b_0c_0d_0e_0f", true, Some(87))]).expect("one pen");
        assert!(!line.contains('\n'), "status is one line");
        let value = parse(&line);
        assert_eq!(value["pens"].as_array().expect("a pens array").len(), 1);
    }

    #[test]
    fn an_unreported_battery_is_null() {
        let line = status_json(&[pen("0a_0b_0c_0d_0e_0f", true, None)]).expect("one pen");
        assert!(parse(&line)["pens"][0]["battery"].is_null());
    }

    #[test]
    fn a_pen_omits_the_capability_keys_it_lacks() {
        let mut sample = pen("0a_0b_0c_0d_0e_0f", true, Some(50));
        sample.capabilities = Some(Capabilities {
            pairing: Some("seamless-touch".to_string()),
            ..Capabilities::default()
        });
        let line = status_json(&[sample]).expect("one pen");
        let out = &parse(&line)["pens"][0];
        assert_eq!(out["capabilities"]["pairing"], "seamless-touch");
        assert!(out["capabilities"].get("buttons").is_none());
        assert!(out["haptics"].is_null());
    }

    #[test]
    fn waveforms_keep_the_capability_tuple_fields() {
        let mut sample = pen("0a_0b_0c_0d_0e_0f", true, Some(50));
        sample.capabilities = Some(Capabilities {
            haptics: Some(CapabilityHaptics {
                waveforms: vec![Waveform {
                    ordinal: 3,
                    name: "click".to_string(),
                    duration_ms: 45,
                    continuous: false,
                }],
            }),
            ..Capabilities::default()
        });
        let line = status_json(&[sample]).expect("one pen");
        let wave = &parse(&line)["pens"][0]["capabilities"]["haptics"]["waveforms"][0];
        assert_eq!(wave["ordinal"], 3);
        assert_eq!(wave["name"], "click");
        assert_eq!(wave["duration_ms"], 45);
        assert_eq!(wave["continuous"], false);
    }

    #[test]
    fn watch_records_are_tagged_json_lines() {
        let added = event_line(&Event::PenAdded {
            pen: pen("0a_0b_0c_0d_0e_0f", true, Some(9)),
        });
        assert_eq!(added.matches('\n').count(), 1);
        assert_eq!(parse(added.trim_end())["type"], "pen.added");

        let changed = event_line(&Event::PenChanged {
            pen: pen("0a_0b_0c_0d_0e_0f", true, Some(9)),
        });
        assert_eq!(parse(changed.trim_end())["type"], "pen.changed");

        let pairing = event_line(&Event::Pairing {
            kind: "bonded".to_string(),
            detail: "done".to_string(),
        });
        let value = parse(pairing.trim_end());
        assert_eq!(value["type"], "pairing");
        assert_eq!(value["kind"], "bonded");

        // An empty snapshot is still a record: it tells a consumer the
        // visible set is empty now, unlike status printing nothing.
        let snapshot = event_line(&Event::Snapshot { pens: vec![] });
        assert_eq!(
            parse(snapshot.trim_end())["pens"]
                .as_array()
                .expect("a pens array")
                .len(),
            0
        );
    }

    #[test]
    fn text_status_shows_battery_and_link() {
        assert_eq!(
            status_line(&pen("0a_0b_0c_0d_0e_0f", true, Some(87))),
            "SA205H  battery 87 %"
        );
        assert_eq!(
            status_line(&pen("0a_0b_0c_0d_0e_0f", false, None)),
            "SA205H  battery unknown  link down"
        );
    }
}
