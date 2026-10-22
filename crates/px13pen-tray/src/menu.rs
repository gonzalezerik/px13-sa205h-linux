// SPDX-License-Identifier: AGPL-3.0-or-later
//! The dbusmenu, rebuilt on every open so it always says what the
//! last pen read said. Three read-only lines per pen, then the
//! haptics policy — only for a pen that has haptics, so a pen that
//! cannot buzz never grows a switch that would do nothing — and the
//! two commands every pen needs. Controls carry their state as a
//! check mark or a radio dot, never as colour.
//!
//! The menu is plain data: what a click does is an [`Action`] value,
//! so a test reads it the way a host does (src/host.rs).

use px13pen_client::Pen;

use crate::PenTray;
use crate::summary::{self, INTENSITY_PRESETS};

/// One menu entry, as a dbusmenu host is told about it.
#[derive(Clone, Debug, PartialEq)]
pub enum MenuItem {
    Standard(StandardItem),
    Checkmark(CheckmarkItem),
    SubMenu(SubMenu),
    RadioGroup(RadioGroup),
    Separator,
}

/// A line: a fact when `action` is `None` (shown, not clickable), a
/// command otherwise.
#[derive(Clone, Debug, PartialEq)]
pub struct StandardItem {
    pub label: String,
    pub enabled: bool,
    pub action: Option<Action>,
}

/// A switch, its state a check mark.
#[derive(Clone, Debug, PartialEq)]
pub struct CheckmarkItem {
    pub label: String,
    pub checked: bool,
    pub action: Action,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SubMenu {
    pub label: String,
    pub submenu: Vec<MenuItem>,
}

/// Mutually exclusive choices; `selected` indexes `options`.
#[derive(Clone, Debug, PartialEq)]
pub struct RadioGroup {
    pub selected: usize,
    pub options: Vec<RadioItem>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RadioItem {
    pub label: String,
    pub action: Action,
}

/// What a click asks for. A click carries the value its entry
/// displayed, never a toggle of whatever the daemon holds by then.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    SetHaptics { path: String, enabled: bool },
    SetIntensity { path: String, intensity: u8 },
    SetStatusNotifications { path: String, enabled: bool },
    Pair,
    Settings,
}

pub(super) fn build(tray: &PenTray) -> Vec<MenuItem> {
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
    // The idle / docked / picked-up notes, the same switch as the
    // settings window's; one user policy, read from the first pen.
    if let Some(pen) = tray.pens.first() {
        items.push(MenuItem::Checkmark(CheckmarkItem {
            label: "Status notifications".to_string(),
            checked: pen.status_notifications,
            action: Action::SetStatusNotifications {
                path: pen.path.clone(),
                enabled: !pen.status_notifications,
            },
        }));
    }
    items.push(MenuItem::Separator);
    items.push(command("Pair a pen…", Action::Pair));
    items.push(command("Settings…", Action::Settings));
    items
}

/// The haptics switch and the intensity presets for one pen.
fn haptics_controls(pen: &Pen) -> Vec<MenuItem> {
    let haptics = pen
        .haptics
        .clone()
        .expect("only called for a pen with haptics");
    vec![
        MenuItem::Checkmark(CheckmarkItem {
            label: "Haptics".to_string(),
            checked: haptics.enabled,
            action: Action::SetHaptics {
                path: pen.path.clone(),
                enabled: !haptics.enabled,
            },
        }),
        MenuItem::SubMenu(SubMenu {
            label: "Intensity".to_string(),
            submenu: vec![MenuItem::RadioGroup(RadioGroup {
                selected: summary::preset_index(haptics.intensity),
                options: INTENSITY_PRESETS
                    .iter()
                    .map(|(label, intensity)| RadioItem {
                        label: (*label).to_string(),
                        action: Action::SetIntensity {
                            path: pen.path.clone(),
                            intensity: *intensity,
                        },
                    })
                    .collect(),
            })],
        }),
    ]
}

/// A fact, not a control: shown to every reader, click-proof.
fn info(label: String) -> MenuItem {
    MenuItem::Standard(StandardItem {
        label,
        enabled: false,
        action: None,
    })
}

fn command(label: &str, action: Action) -> MenuItem {
    MenuItem::Standard(StandardItem {
        label: label.to_string(),
        enabled: true,
        action: Some(action),
    })
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
                hover_tick: false,
                ink_texture: "ink".to_string(),
            }),
            buttons: None,
            status_notifications: true,
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
