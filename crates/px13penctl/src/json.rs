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
    /// A top-button gesture (`Pen1.ButtonEvent`); the pen is present by
    /// definition, so the path is enough to find it in the snapshot.
    #[serde(rename = "pen.button")]
    PenButton { path: String, gesture: String },
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

/// The text form of `status`: `name  battery 87 %`, with the pen's own
/// firmware once the daemon has read it and the pen's own gauge in
/// brackets when it has that too. The link is spelled out only when the
/// pen is away — the default view only ever shows present pens, so the
/// suffix appears together with `--all`; a pen asleep on its charger
/// says so instead. Silenced status notifications get their own
/// suffix, and only when silenced: on is the default and needs no
/// announcement.
pub fn status_line(pen: &Pen) -> String {
    let battery = match pen.battery {
        Some(percent) => format!("battery {percent} %"),
        None => "battery unknown".to_string(),
    };
    let firmware = pen
        .firmware
        .as_deref()
        .map_or(String::new(), |text| format!("  firmware {text}"));
    let detail = pen
        .battery_detail
        .as_ref()
        .map(|detail| {
            let mut parts = Vec::new();
            if let Some(percent) = detail.percent {
                parts.push(format!("{percent:.2} %"));
            }
            if let Some(millivolts) = detail.millivolts {
                parts.push(format!("{millivolts} mV"));
            }
            if parts.is_empty() {
                String::new()
            } else {
                format!(" ({})", parts.join(", "))
            }
        })
        .unwrap_or_default();
    let docked = pen
        .battery_detail
        .as_ref()
        .is_some_and(|detail| detail.docked);
    let link = match (pen.present, docked) {
        (true, _) => "",
        (false, true) => "  on charger",
        (false, false) => "  link down",
    };
    let silence = if pen.status_notifications {
        ""
    } else {
        "  notifications off"
    };
    format!("{}  {battery}{detail}{firmware}{link}{silence}", pen.name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use px13pen_client::{BatteryDetail, Capabilities, CapabilityHaptics, Waveform};

    fn pen(suffix: &str, present: bool, battery: Option<i32>) -> Pen {
        Pen {
            path: format!("/io/github/gonzalezerik/Px13Pen/pen/dev_{suffix}"),
            name: "SA205H".to_string(),
            address: String::new(),
            present,
            paired: true,
            trusted: true,
            firmware: None,
            battery,
            battery_detail: None,
            haptics: None,
            buttons: None,
            status_notifications: true,
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
        assert!(out.get("firmware").is_none(), "unread: no key at all");
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

    #[test]
    fn text_status_shows_the_firmware_once_read() {
        let mut sample = pen("0a_0b_0c_0d_0e_0f", false, Some(87));
        sample.firmware = Some("2.0.37".to_string());
        assert_eq!(
            status_line(&sample),
            "SA205H  battery 87 %  firmware 2.0.37  link down"
        );
    }

    #[test]
    fn text_status_shows_the_pen_gauge_and_the_charger() {
        let mut awake = pen("0a_0b_0c_0d_0e_0f", true, Some(78));
        awake.battery_detail = BatteryDetail::from_wire(78.73, 4105, false);
        assert_eq!(
            status_line(&awake),
            "SA205H  battery 78 % (78.73 %, 4105 mV)"
        );
        let mut asleep = pen("0a_0b_0c_0d_0e_0f", false, Some(78));
        asleep.battery_detail = BatteryDetail::from_wire(78.3, 4105, true);
        assert_eq!(
            status_line(&asleep),
            "SA205H  battery 78 % (78.30 %, 4105 mV)  on charger"
        );
        let value = parse(&status_json(&[awake]).expect("one pen"));
        assert_eq!(value["pens"][0]["battery_detail"]["millivolts"], 4105);
        assert_eq!(value["pens"][0]["battery_detail"]["docked"], false);
    }

    #[test]
    fn silenced_notifications_show_and_on_stays_unsaid() {
        let mut sample = pen("0a_0b_0c_0d_0e_0f", true, Some(87));
        assert_eq!(
            status_line(&sample),
            "SA205H  battery 87 %",
            "on is the default and needs no announcement"
        );
        let value = parse(&status_json(&[sample.clone()]).expect("one pen"));
        assert!(
            value["pens"][0].get("status_notifications").is_none(),
            "and no key either"
        );
        sample.status_notifications = false;
        assert_eq!(
            status_line(&sample),
            "SA205H  battery 87 %  notifications off"
        );
        let value = parse(&status_json(&[sample]).expect("one pen"));
        assert_eq!(value["pens"][0]["status_notifications"], false);
    }

    #[test]
    fn no_detail_means_no_key() {
        let value =
            parse(&status_json(&[pen("0a_0b_0c_0d_0e_0f", true, Some(87))]).expect("one pen"));
        assert!(value["pens"][0].get("battery_detail").is_none());
    }
}
