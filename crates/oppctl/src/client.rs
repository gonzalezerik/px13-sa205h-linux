// SPDX-License-Identifier: AGPL-3.0-or-later
//! The two commands: `status` takes one look at the pens, `watch`
//! follows them. The bus work itself lives in `opp-client`; this
//! module wires it to the output contract in `json` and to the
//! readable lines in `text`.

use std::time::Instant;

use opp_client::{Client, WatchEvent};

use crate::json::{self, Event};
use crate::text;

/// One look at the pens, filtered and sorted, printed and done.
pub async fn status(all: bool, system: bool, as_json: bool) -> Result<(), String> {
    let client = Client::connect(system).await?;
    let mut pens = client.pens(all).await?;
    pens.sort_by(|a, b| a.name.cmp(&b.name).then(a.path.cmp(&b.path)));
    if as_json {
        if let Some(line) = json::status_json(&pens) {
            println!("{line}");
        }
    } else {
        for pen in &pens {
            println!("{}", json::status_line(pen));
        }
    }
    Ok(())
}

/// Follow the pens until the daemon goes away: readable lines by
/// default, JSON Lines with `--json`. The match rule is registered
/// before the snapshot is read — `Client::follow` keeps that order so a
/// signal can be duplicated but never lost.
pub async fn watch(all: bool, system: bool, as_json: bool) -> Result<(), String> {
    let client = Client::connect(system).await?;
    let start = Instant::now();
    client
        .follow(all, |event| {
            if as_json {
                print!("{}", json::event_line(&view(&event)));
            } else {
                let prefix = text::prefix(start.elapsed().as_secs_f64());
                for line in text::event_lines(&event) {
                    println!("{prefix}{line}");
                }
            }
        })
        .await
}

/// One domain event as one contract record.
fn view(event: &WatchEvent) -> Event {
    match event {
        WatchEvent::Snapshot { pens } => Event::Snapshot { pens: pens.clone() },
        WatchEvent::Added { pen } => Event::PenAdded { pen: pen.clone() },
        WatchEvent::Removed { path, address } => Event::PenRemoved {
            path: path.clone(),
            address: address.clone(),
        },
        WatchEvent::Changed { pen } => Event::PenChanged { pen: pen.clone() },
        WatchEvent::Pairing { kind, detail } => Event::Pairing {
            kind: kind.clone(),
            detail: detail.clone(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opp_client::Pen;

    fn sample_pen() -> Pen {
        Pen {
            path: "/org/openpenprotocol/pen/dev_0a_0b_0c_0d_0e_0f".to_string(),
            name: "SA205H".to_string(),
            address: String::new(),
            present: true,
            paired: true,
            trusted: true,
            battery: Some(50),
            haptics: None,
            capabilities: None,
        }
    }

    #[test]
    fn every_domain_event_maps_to_its_contract_tag() {
        let events = [
            WatchEvent::Snapshot {
                pens: vec![sample_pen()],
            },
            WatchEvent::Added { pen: sample_pen() },
            WatchEvent::Removed {
                path: "/org/openpenprotocol/pen/dev_0a_0b_0c_0d_0e_0f".to_string(),
                address: String::new(),
            },
            WatchEvent::Changed { pen: sample_pen() },
            WatchEvent::Pairing {
                kind: "searching".to_string(),
                detail: String::new(),
            },
        ];
        let expected = [
            "snapshot",
            "pen.added",
            "pen.removed",
            "pen.changed",
            "pairing",
        ];
        for (event, tag) in events.iter().zip(expected) {
            let line = json::event_line(&view(event));
            let value: serde_json::Value =
                serde_json::from_str(line.trim_end()).expect("a JSON line");
            assert_eq!(value["type"], tag);
        }
    }
}
