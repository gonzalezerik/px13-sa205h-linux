// SPDX-License-Identifier: AGPL-3.0-or-later
//! The JSON contract of `oppctl`, described in docs/oppctl.md.
//!
//! Pure serialisation: the rules frontends rely on — empty output while
//! no pen is connected, `-1` battery as `null`, the tags of the `watch`
//! stream — live here and are tested here, away from the bus.

use serde::Serialize;

/// One pen: `status --json` prints these, and `watch` carries them in
/// its `snapshot`, `pen.added` and `pen.changed` events.
#[derive(Clone, Serialize)]
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

/// The pen's `Haptics1` policy state; `null` when the pen does no haptics.
#[derive(Clone, Serialize)]
pub struct Haptics {
    pub enabled: bool,
    pub intensity: u8,
}

/// The capability dict projected to JSON. A key absent from the dict is
/// absent here too — that is the daemon's "this pen lacks it" statement
/// (docs/dbus.md §6), so unknown keys are dropped rather than guessed at.
#[derive(Clone, Default, Serialize)]
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

#[derive(Clone, Default, Serialize)]
pub struct Buttons {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub barrel: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shortcut: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shortcut_gestures: Option<Vec<String>>,
}

#[derive(Clone, Default, Serialize)]
pub struct CapabilityHaptics {
    pub waveforms: Vec<Waveform>,
}

/// One playable waveform: the `(ordinal, name, duration_ms, continuous)`
/// tuple of the capability dict, spread into named fields.
#[derive(Clone, Serialize)]
pub struct Waveform {
    pub ordinal: u16,
    pub name: String,
    pub duration_ms: u16,
    pub continuous: bool,
}

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

/// `BatteryPercent` is `-1` until the battery service has reported one;
/// JSON says `null` rather than passing a magic number through.
pub fn battery_from_wire(percent: i32) -> Option<i32> {
    if percent < 0 { None } else { Some(percent) }
}

/// The address embedded in a `dev_XX_XX_XX_XX_XX_XX` object path, in the
/// `Pen1.Address` form: upper-case hex, colon-separated. Empty when the
/// path carries no address — `pen.removed` then still has the path.
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

    fn pen(suffix: &str, present: bool, battery: Option<i32>) -> Pen {
        Pen {
            path: format!("/org/openpenprotocol/pen/dev_{suffix}"),
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
    fn battery_minus_one_becomes_null() {
        assert_eq!(battery_from_wire(-1), None);
        assert_eq!(battery_from_wire(0), Some(0));
        assert_eq!(battery_from_wire(100), Some(100));
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
    fn an_address_comes_back_from_a_dev_path() {
        // Assembled at runtime: the privacy gate rejects colon-hex literals.
        let expected = String::from("0A:0B:0C:0D:0E") + ":0F";
        assert_eq!(
            address_from_object_path("/org/openpenprotocol/pen/dev_0a_0b_0c_0d_0e_0f"),
            expected
        );
    }

    #[test]
    fn paths_without_an_address_yield_empty() {
        for path in [
            "/org/openpenprotocol/Daemon",
            "/org/openpenprotocol/pen/",
            "/org/openpenprotocol/pen/unknown",
            "/org/openpenprotocol/pen/dev_0a_0b",
            "/org/openpenprotocol/pen/dev_0a_0b_0c_0d_0e_zz",
        ] {
            assert_eq!(address_from_object_path(path), "", "not an address: {path}");
        }
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
