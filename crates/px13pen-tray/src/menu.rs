// SPDX-License-Identifier: AGPL-3.0-or-later
//! The dbusmenu, rebuilt on every open so it always says what the
//! last pen read said. Three read-only lines per pen, then the
//! haptics policy — only for a pen that has haptics, so a pen that
//! cannot buzz never grows a switch that would do nothing — and the
//! two commands every pen needs. Controls carry their state as a
//! check mark or a radio dot, never as colour.

use ksni::menu::{CheckmarkItem, MenuItem, RadioGroup, RadioItem, StandardItem, SubMenu};
use px13pen_client::Pen;

use crate::PenTray;
use crate::summary::{self, INTENSITY_PRESETS};

pub(super) fn build(tray: &PenTray) -> Vec<MenuItem<PenTray>> {
    let mut items = Vec::new();
    if tray.pens.is_empty() {
        items.push(info("No pen connected".to_string()));
    }
    for pen in &tray.pens {
        items.push(info(pen.name.clone()));
        items.push(info(summary::battery_text(pen)));
        items.push(info(summary::link_text(pen).to_string()));
    }
    items.push(MenuItem::Separator);
    // The policy the daemon keeps is per user, so one set of controls
    // driven by the first pen that has them.
    if let Some(pen) = tray.pens.iter().find(|pen| pen.haptics.is_some()) {
        items.extend(haptics_controls(pen));
    }
    items.push(MenuItem::Separator);
    items.push(
        StandardItem {
            label: "Pair a pen…".to_string(),
            activate: Box::new(|tray: &mut PenTray| tray.start_pairing()),
            ..Default::default()
        }
        .into(),
    );
    items.push(
        StandardItem {
            label: "Settings…".to_string(),
            activate: Box::new(|tray: &mut PenTray| tray.open_settings()),
            ..Default::default()
        }
        .into(),
    );
    items
}

/// The haptics switch and the intensity presets for one pen, writing
/// the values the menu did not show as stale: a click carries the
/// value it displayed.
fn haptics_controls(pen: &Pen) -> Vec<MenuItem<PenTray>> {
    let haptics = pen
        .haptics
        .clone()
        .expect("only called for a pen with haptics");
    let switch_path = pen.path.clone();
    let target = !haptics.enabled;
    let mut items = vec![
        CheckmarkItem {
            label: "Haptics".to_string(),
            checked: haptics.enabled,
            activate: Box::new(move |tray: &mut PenTray| {
                tray.set_haptics_enabled(switch_path.clone(), target);
            }),
            ..Default::default()
        }
        .into(),
    ];
    let intensity_path = pen.path.clone();
    items.push(
        SubMenu {
            label: "Intensity".to_string(),
            submenu: vec![
                RadioGroup {
                    selected: summary::preset_index(haptics.intensity),
                    select: Box::new(move |tray: &mut PenTray, index| {
                        tray.set_intensity(intensity_path.clone(), INTENSITY_PRESETS[index].1);
                    }),
                    options: INTENSITY_PRESETS
                        .iter()
                        .map(|(label, _)| RadioItem {
                            label: (*label).to_string(),
                            ..Default::default()
                        })
                        .collect(),
                }
                .into(),
            ],
            ..Default::default()
        }
        .into(),
    );
    items
}

/// A fact, not a control: shown to every reader, click-proof.
fn info(label: String) -> MenuItem<PenTray> {
    StandardItem {
        label,
        enabled: false,
        ..Default::default()
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(present: bool, haptics: Option<bool>) -> Pen {
        Pen {
            path: "/io/github/gonzalezerik/Px13Pen/pen/dev_00_01_02_03_04_05".to_string(),
            name: "SA205H".to_string(),
            address: String::new(),
            present,
            paired: true,
            trusted: true,
            firmware: None,
            battery: Some(78),
            battery_detail: None,
            haptics: haptics.map(|enabled| px13pen_client::Haptics {
                enabled,
                intensity: 50,
                while_inking: true,
                while_interacting: true,
                ink_texture: "ink".to_string(),
            }),
            buttons: None,
            capabilities: None,
        }
    }

    #[test]
    fn a_connected_pen_names_itself_and_its_state() {
        let tray = PenTray::preview(vec![sample(true, None)]);
        let menu = build(&tray);
        let labels: Vec<String> = menu
            .iter()
            .filter_map(|item| match item {
                MenuItem::Standard(standard) => Some(standard.label.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            labels,
            [
                "SA205H",
                "battery 78 %",
                "Connected",
                "Pair a pen…",
                "Settings…"
            ]
        );
    }

    #[test]
    fn an_empty_tray_says_so_and_still_offers_pairing() {
        let tray = PenTray::preview(Vec::new());
        let menu = build(&tray);
        let labels: Vec<String> = menu
            .iter()
            .filter_map(|item| match item {
                MenuItem::Standard(standard) => Some(standard.label.clone()),
                _ => None,
            })
            .collect();
        assert!(labels.contains(&"No pen connected".to_string()));
        assert!(labels.contains(&"Pair a pen…".to_string()));
    }
}
