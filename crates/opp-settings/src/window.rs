// SPDX-License-Identifier: AGPL-3.0-or-later
//! The window. Each visible pen becomes a card; a card only offers
//! haptics rows when `oppd` reported a `Haptics1` interface for that
//! pen, so a pen that does not do haptics never shows a control that
//! cannot work.
//!
//! Widget state is written from exactly two places: the daemon events
//! the poll drains, and the user's own edits. Edits are sent as
//! commands and echoed back by the daemon as change events; a guard
//! flag keeps our own writes from bouncing off a switch mid-toggle.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::Duration;

use gtk4 as gtk;
use libadwaita as adw;

use adw::prelude::*;
use opp_client::{Pen, WatchEvent};

use crate::runtime::{self, BusHandle, Command, UiEvent};

/// How often the widget thread drains the event queue and flushes a
/// coalesced slider value. 120 ms is invisible in a slider and keeps a
/// full drag at one or two Bluetooth writes.
const POLL: Duration = Duration::from_millis(120);

/// The whole window state the poll needs, one card per visible pen.
struct PenCard {
    pen: Pen,
    group: adw::PreferencesGroup,
    haptics_switch: Option<adw::SwitchRow>,
    intensity: Option<gtk::Scale>,
    /// The slider's latest value, not yet sent. Flushed by the poll.
    pending: Rc<RefCell<Option<u8>>>,
}

impl PenCard {
    fn new(pen: Pen, bus: &BusHandle, syncing: &Rc<Cell<bool>>) -> Self {
        let group = adw::PreferencesGroup::builder()
            .title(pen.name.clone())
            .build();
        group.set_description(Some(&describe(&pen)));

        let haptics_switch = pen.haptics.as_ref().map(|haptics| {
            let row = adw::SwitchRow::builder()
                .title("Haptic feedback")
                .active(haptics.enabled)
                .build();
            let bus = bus.clone();
            let path = pen.path.clone();
            let syncing = Rc::clone(syncing);
            row.connect_active_notify(move |row| {
                if syncing.get() {
                    return;
                }
                bus.send(Command::SetHapticsEnabled {
                    path: path.clone(),
                    enabled: row.is_active(),
                });
            });
            row
        });

        let pending = Rc::new(RefCell::new(None));
        let intensity = pen.haptics.as_ref().map(|haptics| {
            let scale = gtk::Scale::builder().draw_value(true).build();
            scale.set_range(0.0, 100.0);
            scale.set_increments(5.0, 10.0);
            scale.set_digits(0);
            scale.set_value(f64::from(haptics.intensity));
            scale.set_hexpand(true);
            scale.update_property(&[gtk::accessible::Property::Label("Haptic intensity")]);
            let row = adw::ActionRow::builder()
                .title("Intensity")
                .child(&scale)
                .build();
            let syncing = Rc::clone(syncing);
            let pending = Rc::clone(&pending);
            scale.connect_value_changed(move |scale| {
                if syncing.get() {
                    return;
                }
                *pending.borrow_mut() = Some(clamp_intensity(scale.value()));
            });
            group.add(&row);
            scale
        });
        if let Some(row) = &haptics_switch {
            group.add(row);
        }

        PenCard {
            pen,
            group,
            haptics_switch,
            intensity,
            pending,
        }
    }

    /// Bring the card's widgets in line with a daemon view of the pen.
    fn apply(&mut self, pen: &Pen, syncing: &Cell<bool>) {
        self.pen = pen.clone();
        self.group.set_description(Some(&describe(pen)));
        if let Some(row) = &self.haptics_switch {
            syncing.set(true);
            row.set_active(pen.haptics.as_ref().is_some_and(|h| h.enabled));
            syncing.set(false);
        }
        // While the user holds the slider, their value wins; it goes
        // out on the next flush and comes back as an ordinary change.
        if let Some(scale) = &self.intensity
            && self.pending.borrow().is_none()
        {
            syncing.set(true);
            scale.set_value(pen.haptics.as_ref().map_or(0.0, |h| f64::from(h.intensity)));
            syncing.set(false);
        }
    }

    /// Send the coalesced slider value, if the daemon does not have it.
    fn flush(&mut self, bus: &BusHandle) {
        let Some(value) = self.pending.borrow_mut().take() else {
            return;
        };
        if self.pen.haptics.as_ref().map(|h| h.intensity) != Some(value) {
            bus.send(Command::SetIntensity {
                path: self.pen.path.clone(),
                intensity: value,
            });
        }
    }
}

/// Whether the pairing listener is running, and the buttons that show
/// it — the header has one, the empty state has the other.
struct Pairing {
    active: bool,
    buttons: Vec<gtk::Button>,
}

/// Build and present the window.
pub fn build(app: &adw::Application, system: bool) -> adw::ApplicationWindow {
    let (bus, events) = runtime::spawn(system);

    let window = adw::ApplicationWindow::new(app);
    window.set_title(Some("Open Pen"));
    window.set_default_size(480, 600);

    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&adw::WindowTitle::new("Open Pen", "")));
    let search_button = pair_button();
    header.pack_end(&search_button);
    toolbar.add_top_bar(&header);

    let stack = gtk::Stack::new();
    let page = adw::PreferencesPage::new();
    let empty = adw::StatusPage::builder()
        .icon_name("input-tablet-symbolic")
        .title("No pen connected")
        .description("Turn the pen on, then press “Search for pens”.")
        .build();
    let empty_button = pair_button();
    empty.set_child(Some(&empty_button));
    stack.add_named(&page, Some("pens"));
    stack.add_named(&empty, Some("empty"));
    stack.set_visible_child_name("empty");

    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&stack));
    toolbar.set_content(Some(&toasts));
    window.set_content(Some(&toolbar));

    let pairing = Rc::new(RefCell::new(Pairing {
        active: false,
        buttons: vec![search_button, empty_button],
    }));
    for button in pairing.borrow().buttons.clone() {
        let bus = bus.clone();
        let pairing = Rc::clone(&pairing);
        button.connect_clicked(move |_| {
            let mut pairing = pairing.borrow_mut();
            pairing.active = !pairing.active;
            bus.send(if pairing.active {
                Command::StartPairing
            } else {
                Command::StopPairing
            });
        });
    }

    let cards: Rc<RefCell<BTreeMap<String, PenCard>>> = Rc::new(RefCell::new(BTreeMap::new()));
    let syncing = Rc::new(Cell::new(false));
    let shown_pairing = Cell::new(false);

    gtk::glib::timeout_add_local(POLL, {
        let bus = bus;
        let cards = Rc::clone(&cards);
        let events = events;
        let page = page;
        let pairing = Rc::clone(&pairing);
        let stack = stack;
        let syncing = Rc::clone(&syncing);
        let toasts = toasts;
        move || {
            while let Ok(event) = events.try_recv() {
                match event {
                    UiEvent::Watch(WatchEvent::Snapshot { pens }) => {
                        let mut cards = cards.borrow_mut();
                        for card in cards.values() {
                            page.remove(&card.group);
                        }
                        cards.clear();
                        for pen in pens {
                            add_pen(&page, &mut cards, pen, &bus, &syncing);
                        }
                    }
                    UiEvent::Watch(WatchEvent::Added { pen }) => {
                        let mut cards = cards.borrow_mut();
                        if !cards.contains_key(&pen.path) {
                            add_pen(&page, &mut cards, pen, &bus, &syncing);
                        }
                    }
                    UiEvent::Watch(WatchEvent::Removed { path, .. }) => {
                        if let Some(card) = cards.borrow_mut().remove(&path) {
                            page.remove(&card.group);
                        }
                    }
                    UiEvent::Watch(WatchEvent::Changed { pen }) => {
                        refresh_pen(&page, &mut cards.borrow_mut(), pen, &bus, &syncing)
                    }
                    UiEvent::Watch(WatchEvent::Pairing { kind, detail }) => {
                        if kind == "bonded" || kind == "failed" {
                            pairing.borrow_mut().active = false;
                        }
                        toast(&toasts, &pairing_message(&kind, &detail), 5);
                    }
                    UiEvent::Failure(message) => toast(&toasts, &message, 5),
                    // Stay open, stay stale, say so — and keep the
                    // toast until the user has read it.
                    UiEvent::LostConnection(reason) => {
                        pairing.borrow_mut().active = false;
                        toast(&toasts, &format!("Lost oppd: {reason}"), 0);
                    }
                }
            }
            for card in cards.borrow_mut().values_mut() {
                card.flush(&bus);
            }
            let active = pairing.borrow().active;
            if active != shown_pairing.get() {
                shown_pairing.set(active);
                for button in pairing.borrow().buttons.iter() {
                    button.set_label(if active {
                        "Stop searching"
                    } else {
                        "Search for pens"
                    });
                }
            }
            stack.set_visible_child_name(if cards.borrow().is_empty() {
                "empty"
            } else {
                "pens"
            });
            gtk::glib::ControlFlow::Continue
        }
    });

    window.present();
    window
}

fn add_pen(
    page: &adw::PreferencesPage,
    cards: &mut BTreeMap<String, PenCard>,
    pen: Pen,
    bus: &BusHandle,
    syncing: &Rc<Cell<bool>>,
) {
    let card = PenCard::new(pen, bus, syncing);
    page.add(&card.group);
    cards.insert(card.pen.path.clone(), card);
}

/// Update the card for `pen`, or build one if it does not exist yet —
/// a change can only precede its `Added` if we missed that signal.
fn refresh_pen(
    page: &adw::PreferencesPage,
    cards: &mut BTreeMap<String, PenCard>,
    pen: Pen,
    bus: &BusHandle,
    syncing: &Rc<Cell<bool>>,
) {
    let updated = if let Some(card) = cards.get_mut(&pen.path) {
        card.apply(&pen, syncing);
        true
    } else {
        false
    };
    if !updated {
        add_pen(page, cards, pen, bus, syncing);
    }
}

fn pair_button() -> gtk::Button {
    gtk::Button::builder()
        .label("Search for pens")
        .css_classes(["suggested-action"])
        .build()
}

fn toast(overlay: &adw::ToastOverlay, message: &str, timeout: u32) {
    overlay.add_toast(
        adw::Toast::builder()
            .title(message)
            .timeout(timeout)
            .build(),
    );
}

/// The card's one-line state under the pen name.
fn describe(pen: &Pen) -> String {
    let battery = match pen.battery {
        Some(percent) => format!("battery {percent} %"),
        None => "battery unknown".to_string(),
    };
    let state = if pen.present {
        "connected"
    } else {
        "link down"
    };
    if pen.address.is_empty() {
        format!("{state} · {battery}")
    } else {
        format!("{} · {state} · {battery}", pen.address)
    }
}

/// A `PairingEvent` as one toast. Unknown kinds pass their name
/// through; the daemon will not invent a quieter one for us.
fn pairing_message(kind: &str, detail: &str) -> String {
    match kind {
        "announced" => "A pen answered the search — completing the pairing".to_string(),
        "bonded" => "Pen paired".to_string(),
        "failed" if detail.is_empty() => "Pairing failed".to_string(),
        "failed" => format!("Pairing failed: {detail}"),
        other => format!("Pairing: {other}"),
    }
}

/// The slider is a float; the bus wants 0–100.
fn clamp_intensity(value: f64) -> u8 {
    value.round().clamp(0.0, 100.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_pen() -> Pen {
        Pen {
            path: "/org/openpenprotocol/pen/dev_0a_0b_0c_0d_0e_0f".to_string(),
            name: "SA205H".to_string(),
            address: String::new(),
            present: true,
            paired: true,
            trusted: true,
            battery: None,
            haptics: None,
            capabilities: None,
        }
    }

    #[test]
    fn the_slider_value_lands_inside_the_bus_range() {
        assert_eq!(clamp_intensity(-3.0), 0);
        assert_eq!(clamp_intensity(41.4), 41);
        assert_eq!(clamp_intensity(41.5), 42);
        assert_eq!(clamp_intensity(100.9), 100);
        assert_eq!(clamp_intensity(f64::NAN), 0);
    }

    #[test]
    fn the_description_names_what_is_known() {
        let mut pen = sample_pen();
        assert_eq!(describe(&pen), "connected · battery unknown");
        pen.battery = Some(87);
        pen.address = String::from("<address>");
        assert_eq!(describe(&pen), "<address> · connected · battery 87 %");
        pen.present = false;
        assert_eq!(describe(&pen), "<address> · link down · battery 87 %");
    }

    #[test]
    fn pairing_kinds_read_as_sentences() {
        assert_eq!(
            pairing_message("announced", ""),
            "A pen answered the search — completing the pairing"
        );
        assert_eq!(pairing_message("bonded", ""), "Pen paired");
        assert_eq!(
            pairing_message("failed", "timeout"),
            "Pairing failed: timeout"
        );
        assert_eq!(pairing_message("failed", ""), "Pairing failed");
        assert_eq!(pairing_message("later", ""), "Pairing: later");
    }
}
