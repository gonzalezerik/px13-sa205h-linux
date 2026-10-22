// SPDX-License-Identifier: AGPL-3.0-or-later
//! The tray's accessibility gate, part of `cargo test`. A tray icon
//! is a strange surface for a screen reader: the host renders an
//! icon, a title, a tooltip and a dbusmenu, and each host shows a
//! different subset. The decisions document answers that by
//! redundancy — every fact in words as well as in shape — and these
//! tests hold the menu and the texts to it (docs/decisions.md).

use px13pen_client::{BatteryDetail, Haptics, Pen};
use px13pen_tray::menu::{MenuItem, StandardItem};
use px13pen_tray::{PenTray, Status, summary};

fn sample(
    name: &str,
    present: bool,
    battery: Option<i32>,
    detail: Option<(f64, i32)>,
    docked: bool,
    haptics: Option<(bool, u8)>,
) -> Pen {
    Pen {
        path: "/io/github/gonzalezerik/Px13Pen/pen/dev_00_01_02_03_04_05".to_string(),
        name: name.to_string(),
        address: String::new(),
        present,
        paired: true,
        trusted: true,
        firmware: None,
        battery,
        battery_detail: detail.map(|(percent, millivolts)| BatteryDetail {
            percent: Some(percent),
            millivolts: Some(millivolts),
            docked,
        }),
        haptics: haptics.map(|(enabled, intensity)| Haptics {
            enabled,
            intensity,
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

/// Everything the menu says, collected by role: what it claims, not
/// what it paints.
#[derive(Default)]
struct Spoken {
    facts: Vec<(String, bool)>,
    checks: Vec<(String, bool)>,
    submenus: Vec<String>,
    radios: Vec<(Vec<String>, usize)>,
}

fn listen(items: &[MenuItem], spoken: &mut Spoken) {
    for item in items {
        match item {
            MenuItem::Standard(StandardItem { label, enabled, .. }) => {
                spoken.facts.push((label.clone(), *enabled));
            }
            MenuItem::Checkmark(item) => spoken.checks.push((item.label.clone(), item.checked)),
            MenuItem::SubMenu(item) => {
                spoken.submenus.push(item.label.clone());
                listen(&item.submenu, spoken);
            }
            MenuItem::RadioGroup(group) => spoken.radios.push((
                group.options.iter().map(|o| o.label.clone()).collect(),
                group.selected,
            )),
            MenuItem::Separator => {}
        }
    }
}

fn spoken_of(tray: &PenTray) -> Spoken {
    let mut spoken = Spoken::default();
    listen(&tray.menu(), &mut spoken);
    spoken
}

#[test]
fn every_menu_label_names_itself_in_words() {
    let tray = PenTray::preview(vec![sample(
        "SA205H",
        true,
        Some(78),
        Some((78.73, 4105)),
        false,
        Some((true, 50)),
    )]);
    let spoken = spoken_of(&tray);
    let all: Vec<&str> = spoken
        .facts
        .iter()
        .map(|(label, _)| label.as_str())
        .chain(spoken.checks.iter().map(|(label, _)| label.as_str()))
        .chain(spoken.submenus.iter().map(String::as_str))
        .chain(
            spoken
                .radios
                .iter()
                .flat_map(|(o, _)| o.iter().map(String::as_str)),
        )
        .collect();
    assert!(!all.is_empty(), "a menu with no labels says nothing");
    for label in &all {
        assert!(!label.trim().is_empty(), "a menu label must not be empty");
        assert!(!label.ends_with('_'), "dangling accelerator in {label:?}");
    }
}

#[test]
fn facts_are_shown_but_click_proof_controls_are_real() {
    let tray = PenTray::preview(vec![sample(
        "SA205H",
        true,
        Some(78),
        Some((78.73, 4105)),
        false,
        Some((true, 50)),
    )]);
    let spoken = spoken_of(&tray);
    // The pen's facts, as words, disabled because there is nothing
    // to do with them.
    for expected in ["SA205H", "battery 78.73 %", "Connected"] {
        assert!(
            spoken
                .facts
                .iter()
                .any(|(label, enabled)| { label == expected && !enabled }),
            "expected a read-only {expected:?} line, got {:?}",
            spoken.facts
        );
    }
    // The controls, and only the controls, enabled.
    for expected in ["Haptics", "Pair a pen…", "Settings…"] {
        let enabled = spoken
            .checks
            .iter()
            .find(|(label, _)| label == expected)
            .map(|_| true)
            .or_else(|| {
                spoken
                    .facts
                    .iter()
                    .find(|(label, _)| label == expected)
                    .map(|(_, enabled)| *enabled)
            });
        assert_eq!(
            enabled,
            Some(true),
            "{expected:?} must be an enabled control"
        );
    }
}

#[test]
fn state_is_conveyed_as_check_and_radio_not_as_colour() {
    let tray = PenTray::preview(vec![sample(
        "SA205H",
        true,
        Some(78),
        None,
        false,
        Some((true, 50)),
    )]);
    let spoken = spoken_of(&tray);
    assert_eq!(spoken.checks, vec![("Haptics".to_string(), true)]);
    assert_eq!(spoken.submenus, ["Intensity"]);
    assert_eq!(
        spoken.radios,
        vec![(
            vec![
                "Low (25 %)".to_string(),
                "Medium (50 %)".to_string(),
                "High (100 %)".to_string(),
            ],
            1
        )],
        "the radio group marks the preset the pen is at, and spells the percentages"
    );
}

#[test]
fn a_pen_without_haptics_grows_no_switches() {
    let tray = PenTray::preview(vec![sample("SA205H", true, Some(78), None, false, None)]);
    let spoken = spoken_of(&tray);
    assert!(spoken.checks.is_empty());
    assert!(spoken.submenus.is_empty());
    assert!(spoken.radios.is_empty());
}

#[test]
fn title_and_tooltip_carry_the_name_and_the_battery_as_text() {
    let pen = sample("SA205H", true, Some(78), Some((78.73, 4105)), false, None);
    let tray = PenTray::preview(vec![pen]);
    let title = tray.title();
    assert!(title.contains("SA205H"), "title {title:?} lacks the name");
    assert!(
        title.contains("78.73"),
        "title {title:?} lacks the battery number"
    );
    let tip = tray.tool_tip();
    assert_eq!(tip.title, "SA205H");
    assert!(tip.description.contains("connected"));
    assert!(tip.description.contains("battery"));
    assert!(tip.description.contains("4105 mV"));
}

#[test]
fn battery_levels_are_told_apart_by_icon_shape() {
    let icons: Vec<String> = [90u8, 60, 30, 10]
        .iter()
        .map(|percent| {
            PenTray::preview(vec![sample(
                "SA205H",
                true,
                Some(i32::from(*percent)),
                None,
                false,
                None,
            )])
            .icon_name()
        })
        .chain([
            PenTray::preview(vec![sample("SA205H", true, None, None, false, None)]).icon_name(),
        ])
        .collect();
    let mut unique = icons.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(
        unique.len(),
        icons.len(),
        "levels must differ by shape: {icons:?}"
    );
}

#[test]
fn a_hidden_tray_declares_itself_passive() {
    let pen = sample("SA205H", true, Some(78), None, false, None);
    assert_eq!(PenTray::preview(vec![pen.clone()]).status(), Status::Active);
    assert_eq!(
        PenTray::preview(vec![pen]).hidden().status(),
        Status::Passive
    );
    // No pens at all is passive even while "shown": nothing is here.
    assert_eq!(PenTray::preview(Vec::new()).status(), Status::Passive);
}

#[test]
fn the_empty_tray_still_names_itself() {
    let tray = PenTray::preview(Vec::new());
    assert!(!tray.title().trim().is_empty());
    assert!(!tray.tool_tip().description.trim().is_empty());
    let spoken = spoken_of(&tray);
    assert!(
        spoken
            .facts
            .iter()
            .any(|(label, _)| label == "No pen connected"),
    );
}

#[test]
fn the_intensity_presets_are_the_decided_ones() {
    assert_eq!(
        summary::INTENSITY_PRESETS,
        [
            ("Low (25 %)", 25),
            ("Medium (50 %)", 50),
            ("High (100 %)", 100),
        ]
    );
}
