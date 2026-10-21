// SPDX-License-Identifier: AGPL-3.0-or-later
//! The sentences and the icon the tray shows, decided from a pen read
//! and nothing else. Kept apart from the bus so a test can hold every
//! state up against the accessibility rules (docs/decisions.md): the
//! pen's name and battery in the title and tooltip, battery states on
//! distinct icon *shapes*, and every number in the text as well — a
//! host that renders only names, or a user who cannot tell two greens
//! apart, loses nothing.

use px13pen_client::Pen;

/// The intensity presets the menu offers, label and value together:
/// Low, Medium and High are 25, 50 and 100 %. The labels spell the
/// percent out so a screen reader says the number, and choosing one
/// writes that exact value.
pub const INTENSITY_PRESETS: [(&str, u8); 3] = [
    ("Low (25 %)", 25),
    ("Medium (50 %)", 50),
    ("High (100 %)", 100),
];

/// The battery line: the pen's own fuel gauge to the hundredth when
/// the daemon has it (the `battery-detail` path), the battery
/// service's rounded number otherwise, and "unknown" before the
/// first reading — never a silent zero.
pub fn battery_text(pen: &Pen) -> String {
    if let Some(percent) = pen
        .battery_detail
        .as_ref()
        .and_then(|detail| detail.percent)
    {
        return format!("battery {percent:.2} %");
    }
    match pen.battery {
        Some(percent) => format!("battery {percent} %"),
        None => "battery unknown".to_string(),
    }
}

/// What the link is doing. A pen asleep on its charger is not
/// "down" — it is charging, and the Windows page says so; a pen
/// asleep on the desk is simply not there.
pub fn link_text(pen: &Pen) -> &'static str {
    if pen.present {
        "Connected"
    } else if pen
        .battery_detail
        .as_ref()
        .is_some_and(|detail| detail.docked)
    {
        "On charger"
    } else {
        "Link down"
    }
}

/// The one percent the icon is chosen by: the battery service's
/// number, falling back to the fuel gauge rounded up to whole
/// percent.
fn icon_percent(pen: &Pen) -> Option<u8> {
    pen.battery
        .map(|percent| u8::try_from(percent.clamp(0, 100)).unwrap_or(0))
        .or_else(|| {
            pen.battery_detail
                .as_ref()
                .and_then(|detail| detail.percent)
                .map(|percent| (percent.round().clamp(0.0, 100.0)) as u8)
        })
}

/// Battery state as icon *shape*: the freedesktop battery names fill
/// a different amount at every level, in their `-symbolic` form so
/// even a monochrome host shows the difference. Without any reading
/// the icon is the tablet, not a full battery lying about it.
pub fn icon_name(pen: Option<&Pen>) -> &'static str {
    let Some(percent) = pen.and_then(icon_percent) else {
        return "input-tablet";
    };
    if percent >= 80 {
        "battery-full-symbolic"
    } else if percent >= 50 {
        "battery-good-symbolic"
    } else if percent >= 25 {
        "battery-low-symbolic"
    } else {
        "battery-caution-symbolic"
    }
}

/// The title: name and battery, so both survive a host that renders
/// the tooltip as the item's only text.
pub fn title(pens: &[Pen]) -> String {
    pens.first().map_or_else(
        || "PX13 Pen".to_string(),
        |pen| format!("{} · {}", pen.name, battery_text(pen)),
    )
}

/// The tooltip's second line, in a sentence: link, battery, and the
/// fuel gauge's voltage where there is one.
pub fn tooltip_description(pen: &Pen) -> String {
    let volts = pen
        .battery_detail
        .as_ref()
        .and_then(|detail| detail.millivolts)
        .map_or_else(String::new, |millivolts| format!(" ({millivolts} mV)"));
    format!(
        "{} · {}{volts}",
        link_text(pen).to_lowercase(),
        battery_text(pen)
    )
}

/// Which preset an intensity is closest to; the first on a tie, so
/// 75 % is read as Medium and the radio group has an answer for
/// whatever number the slider left behind.
pub fn preset_index(intensity: u8) -> usize {
    let mut best = 0usize;
    let mut best_gap = i32::MAX;
    for (index, (_, preset)) in INTENSITY_PRESETS.iter().enumerate() {
        let gap = (i32::from(intensity) - i32::from(*preset)).abs();
        if gap < best_gap {
            best = index;
            best_gap = gap;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use px13pen_client::BatteryDetail;

    fn sample(battery: Option<i32>, detail: Option<BatteryDetail>) -> Pen {
        Pen {
            path: "/io/github/gonzalezerik/Px13Pen/pen/dev_00_01_02_03_04_05".to_string(),
            name: "SA205H".to_string(),
            address: String::new(),
            present: true,
            paired: true,
            trusted: true,
            firmware: None,
            battery,
            battery_detail: detail,
            haptics: None,
            buttons: None,
            status_notifications: true,
            capabilities: None,
        }
    }

    #[test]
    fn the_fuel_gauge_wins_when_the_daemon_has_it() {
        let pen = sample(
            Some(78),
            Some(BatteryDetail {
                percent: Some(78.73),
                millivolts: Some(4105),
                docked: false,
            }),
        );
        assert_eq!(battery_text(&pen), "battery 78.73 %");
    }

    #[test]
    fn without_the_gauge_the_service_number_and_a_plain_unknown() {
        assert_eq!(battery_text(&sample(Some(78), None)), "battery 78 %");
        assert_eq!(battery_text(&sample(None, None)), "battery unknown");
    }

    #[test]
    fn a_pen_on_its_charger_is_charging_not_down() {
        let mut off_desk = sample(Some(50), None);
        off_desk.present = false;
        assert_eq!(link_text(&off_desk), "Link down");
        off_desk.battery_detail = Some(BatteryDetail {
            percent: None,
            millivolts: Some(4000),
            docked: true,
        });
        assert_eq!(link_text(&off_desk), "On charger");
        off_desk.present = true;
        assert_eq!(link_text(&off_desk), "Connected");
    }

    #[test]
    fn every_battery_level_gets_its_own_shape() {
        let at = |percent: u8| sample(Some(i32::from(percent)), None);
        let icons = [
            icon_name(Some(&at(90))),
            icon_name(Some(&at(60))),
            icon_name(Some(&at(30))),
            icon_name(Some(&at(10))),
            icon_name(None),
        ];
        for (left, right) in icons.iter().zip(&icons[1..]) {
            assert_ne!(left, right, "levels must differ by shape");
        }
        // Bucket borders: 80 is full, 79 is good, 50 is good, 49 is
        // low, 25 is low, and everything below reads as caution.
        assert_eq!(icon_name(Some(&at(80))), "battery-full-symbolic");
        assert_eq!(icon_name(Some(&at(79))), "battery-good-symbolic");
        assert_eq!(icon_name(Some(&at(50))), "battery-good-symbolic");
        assert_eq!(icon_name(Some(&at(49))), "battery-low-symbolic");
        assert_eq!(icon_name(Some(&at(25))), "battery-low-symbolic");
        assert_eq!(icon_name(Some(&at(24))), "battery-caution-symbolic");
        // A pen without any reading is the tablet, never a battery.
        assert_eq!(icon_name(Some(&sample(None, None))), "input-tablet");
    }

    #[test]
    fn the_tooltip_says_link_battery_and_voltage_as_words() {
        let pen = sample(
            Some(78),
            Some(BatteryDetail {
                percent: Some(78.73),
                millivolts: Some(4105),
                docked: false,
            }),
        );
        assert_eq!(
            tooltip_description(&pen),
            "connected · battery 78.73 % (4105 mV)"
        );
    }

    #[test]
    fn any_intensity_falls_on_the_nearest_preset() {
        assert_eq!(preset_index(25), 0);
        assert_eq!(preset_index(37), 0);
        assert_eq!(preset_index(38), 1);
        assert_eq!(preset_index(100), 2);
        assert_eq!(preset_index(75), 1, "a tie stays with the earlier preset");
    }
}
