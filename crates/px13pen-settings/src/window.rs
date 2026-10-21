// SPDX-License-Identifier: AGPL-3.0-or-later
//! The window. Each visible pen becomes a card, and controls appear
//! only for the interfaces `px13pend` exported for that pen — haptics
//! rows need `Haptics1`, shortcut-button entries need `Buttons1` — so a
//! pen never shows a control that cannot work.
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
use px13pen_client::{Pen, WatchEvent};

use crate::runtime::{self, BusHandle, Command, UiEvent};

/// How often the widget thread drains the event queue and flushes a
/// coalesced slider value. 120 ms is invisible in a slider and keeps a
/// full drag at one or two Bluetooth writes.
const POLL: Duration = Duration::from_millis(120);

/// The whole window state the poll needs, one card per visible pen.
/// A pen with the shortcut button gets a second group under its card;
/// `button_rows` pairs each row with the gesture name the daemon uses.
struct PenCard {
    pen: Pen,
    group: adw::PreferencesGroup,
    button_group: Option<adw::PreferencesGroup>,
    button_rows: Vec<(&'static str, adw::EntryRow)>,
    haptics_switch: Option<adw::SwitchRow>,
    intensity_row: Option<adw::ActionRow>,
    intensity: Option<gtk::Scale>,
    while_inking: Option<adw::SwitchRow>,
    while_interacting: Option<adw::SwitchRow>,
    ink_texture: Option<adw::ComboRow>,
    /// The combo's texture names in order, so a selection maps back to a name.
    textures: Vec<String>,
    /// The slider's latest value, not yet sent. Flushed by the poll.
    pending: Rc<RefCell<Option<u8>>>,
}

impl PenCard {
    fn new(pen: Pen, bus: &BusHandle, syncing: &Rc<Cell<bool>>) -> Self {
        let group = adw::PreferencesGroup::builder()
            .title(pen.name.clone())
            .build();
        group.set_description(Some(&describe(&pen)));
        let haptics = pen.haptics.clone();
        let on = haptics.as_ref().is_some_and(|h| h.enabled);
        let wire = ToggleWire {
            bus,
            path: &pen.path,
            syncing,
        };

        // Row order mirrors the gating: the master switch first, then
        // what it gates — strength, the two waveform classes, and the
        // texture the inking class plays.
        let haptics_switch = haptics.as_ref().map(|haptics| {
            let row = toggle_row(
                "Haptic feedback",
                None,
                haptics.enabled,
                true,
                &wire,
                |path, enabled| Command::SetHapticsEnabled { path, enabled },
            );
            group.add(&row);
            row
        });

        let pending = Rc::new(RefCell::new(None));
        let (intensity_row, intensity) = match haptics.as_ref() {
            Some(haptics) => {
                let scale = gtk::Scale::builder().draw_value(true).build();
                scale.set_range(0.0, 100.0);
                scale.set_increments(5.0, 10.0);
                scale.set_digits(0);
                scale.set_value(f64::from(haptics.intensity));
                scale.set_hexpand(true);
                scale.update_property(&[gtk::accessible::Property::Label("Haptic intensity")]);
                let row = adw::ActionRow::builder()
                    .title("Intensity")
                    .sensitive(on)
                    .build();
                // add_suffix, not the builder's .child(): that sets the
                // ListBoxRow child and would replace the row's own layout,
                // title included.
                row.add_suffix(&scale);
                let syncing = Rc::clone(syncing);
                let pending = Rc::clone(&pending);
                scale.connect_value_changed(move |scale| {
                    if syncing.get() {
                        return;
                    }
                    *pending.borrow_mut() = Some(clamp_intensity(scale.value()));
                });
                group.add(&row);
                (Some(row), Some(scale))
            }
            None => (None, None),
        };

        let while_inking = haptics.as_ref().map(|haptics| {
            let row = toggle_row(
                "While inking",
                Some("A texture plays while the tip touches the screen"),
                haptics.while_inking,
                on,
                &wire,
                |path, on| Command::SetWhileInking { path, on },
            );
            group.add(&row);
            row
        });

        let while_interacting = haptics.as_ref().map(|haptics| {
            let row = toggle_row(
                "While interacting",
                Some("Short ticks for clicks, presses and hovers"),
                haptics.while_interacting,
                on,
                &wire,
                |path, on| Command::SetWhileInteracting { path, on },
            );
            group.add(&row);
            row
        });

        let textures = ink_textures(&pen);
        let ink_texture = match (haptics.as_ref(), textures.is_empty()) {
            (Some(haptics), false) => {
                let names: Vec<&str> = textures.iter().map(String::as_str).collect();
                let strings = gtk::StringList::new(&names);
                let row = adw::ComboRow::builder()
                    .title("Ink texture")
                    .subtitle("The texture that plays while you write")
                    .model(&strings)
                    .sensitive(on && haptics.while_inking)
                    .build();
                if let Some(index) = textures
                    .iter()
                    .position(|name| *name == haptics.ink_texture)
                {
                    row.set_selected(index as u32);
                }
                let bus = bus.clone();
                let path = pen.path.clone();
                let syncing = Rc::clone(syncing);
                let names = textures.clone();
                row.connect_selected_notify(move |row| {
                    if syncing.get() {
                        return;
                    }
                    let index = usize::try_from(row.selected()).unwrap_or(usize::MAX);
                    if let Some(texture) = names.get(index) {
                        bus.send(Command::SetInkTexture {
                            path: path.clone(),
                            texture: texture.clone(),
                        });
                    }
                });
                group.add(&row);
                Some(row)
            }
            _ => None,
        };

        // The shortcut button lives in its own group under the card:
        // it edits commands, not the haptics policy above it. A pen the
        // daemon exports no `Buttons1` for never sees fields that could
        // not be saved.
        let (button_group, button_rows) = match pen.buttons.as_ref() {
            Some(actions) => {
                let group = adw::PreferencesGroup::builder()
                    .title("Shortcut button")
                    .description(BUTTON_NOTE)
                    .build();
                let mut rows = Vec::new();
                for (gesture, title, current) in [
                    ("single", "Single press", &actions.single),
                    ("double", "Double press", &actions.double),
                    ("long", "Long press", &actions.long),
                ] {
                    let row = adw::EntryRow::builder().title(title).text(current).build();
                    let bus = bus.clone();
                    let path = pen.path.clone();
                    row.connect_apply(move |row| {
                        bus.send(Command::SetButtonAction {
                            path: path.clone(),
                            gesture,
                            command: row.text().to_string(),
                        });
                    });
                    group.add(&row);
                    rows.push((gesture, row));
                }
                (Some(group), rows)
            }
            None => (None, Vec::new()),
        };

        PenCard {
            pen,
            group,
            button_group,
            button_rows,
            haptics_switch,
            intensity_row,
            intensity,
            while_inking,
            while_interacting,
            ink_texture,
            textures,
            pending,
        }
    }

    /// Bring the card's widgets in line with a daemon view of the pen.
    fn apply(&mut self, pen: &Pen, syncing: &Cell<bool>) {
        self.pen = pen.clone();
        self.group.set_description(Some(&describe(pen)));
        let haptics = pen.haptics.clone();
        let on = haptics.as_ref().is_some_and(|h| h.enabled);
        syncing.set(true);
        if let Some(row) = &self.haptics_switch {
            row.set_active(on);
        }
        if let Some(row) = &self.intensity_row {
            row.set_sensitive(on);
        }
        // While the user holds the slider, their value wins; it goes
        // out on the next flush and comes back as an ordinary change.
        if let Some(scale) = &self.intensity
            && self.pending.borrow().is_none()
        {
            scale.set_value(haptics.as_ref().map_or(0.0, |h| f64::from(h.intensity)));
        }
        if let Some(row) = &self.while_inking {
            row.set_active(haptics.as_ref().is_some_and(|h| h.while_inking));
            row.set_sensitive(on);
        }
        if let Some(row) = &self.while_interacting {
            row.set_active(haptics.as_ref().is_some_and(|h| h.while_interacting));
            row.set_sensitive(on);
        }
        if let Some(row) = &self.ink_texture {
            if let Some(index) = haptics
                .as_ref()
                .and_then(|h| self.textures.iter().position(|name| *name == h.ink_texture))
            {
                row.set_selected(index as u32);
            }
            row.set_sensitive(on && haptics.as_ref().is_some_and(|h| h.while_inking));
        }
        if let Some(actions) = pen.buttons.as_ref() {
            for (gesture, row) in &self.button_rows {
                let current = match *gesture {
                    "single" => &actions.single,
                    "double" => &actions.double,
                    _ => &actions.long,
                };
                // Typing wins over an echo: a row being edited is left
                // alone, and an unchanged row is not touched at all.
                if !row.has_focus() && row.text().as_str() != current.as_str() {
                    row.set_text(current);
                }
            }
        }
        syncing.set(false);
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
    window.set_title(Some("PX13 Pen"));
    window.set_default_size(480, 600);

    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&adw::WindowTitle::new("PX13 Pen", "")));
    // The primary menu sits in the right corner it always sits in; the
    // search button takes the spot next to the title.
    header.pack_end(&menu_button());
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

    // Per-window action group, so a second application window would get
    // its own About dialog transient to itself. The dialog is built once
    // and re-presented; libadwaita dialogs are reusable.
    let actions = gtk::gio::SimpleActionGroup::new();
    let about_action = gtk::gio::SimpleAction::new("about", None);
    let about = about_dialog();
    let about_parent = window.clone();
    about_action.connect_activate(move |_, _| {
        about.present(Some(&about_parent));
    });
    actions.add_action(&about_action);
    window.insert_action_group("win", Some(&actions));

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
                    UiEvent::Watch(watch) => match *watch {
                        WatchEvent::Snapshot { pens } => {
                            let mut cards = cards.borrow_mut();
                            for card in cards.values() {
                                page.remove(&card.group);
                                if let Some(group) = &card.button_group {
                                    page.remove(group);
                                }
                            }
                            cards.clear();
                            for pen in pens {
                                add_pen(&page, &mut cards, pen, &bus, &syncing);
                            }
                        }
                        WatchEvent::Added { pen } => {
                            let mut cards = cards.borrow_mut();
                            if !cards.contains_key(&pen.path) {
                                add_pen(&page, &mut cards, pen, &bus, &syncing);
                            }
                        }
                        WatchEvent::Removed { path, .. } => {
                            if let Some(card) = cards.borrow_mut().remove(&path) {
                                page.remove(&card.group);
                                if let Some(group) = &card.button_group {
                                    page.remove(group);
                                }
                            }
                        }
                        WatchEvent::Changed { pen } => {
                            refresh_pen(&page, &mut cards.borrow_mut(), pen, &bus, &syncing)
                        }
                        WatchEvent::Pairing { kind, detail } => {
                            if kind == "bonded" || kind == "failed" {
                                pairing.borrow_mut().active = false;
                            }
                            toast(&toasts, &pairing_message(&kind, &detail), 5);
                        }
                        // A gesture changes nothing this window shows:
                        // its configured action runs in the daemon.
                        WatchEvent::Button { .. } => {}
                    },
                    UiEvent::Failure(message) => toast(&toasts, &message, 5),
                    // Stay open, stay stale, say so — and keep the
                    // toast until the user has read it.
                    UiEvent::LostConnection(reason) => {
                        pairing.borrow_mut().active = false;
                        toast(&toasts, &format!("Lost px13pend: {reason}"), 0);
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
    if let Some(group) = &card.button_group {
        page.add(group);
    }
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

/// The primary menu. About is the only entry today; the corner goes in
/// the place users look, so later entries move nothing.
fn menu_button() -> gtk::MenuButton {
    let menu = gtk::gio::Menu::new();
    menu.append(Some("About PX13 Pen"), Some("win.about"));
    let button = gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .tooltip_text("Main menu")
        .menu_model(&menu)
        .build();
    button.update_property(&[gtk::accessible::Property::Label("Main menu")]);
    button
}

/// What it is, who maintains it, which licence, where to file issues —
/// the version comes from the package metadata so it cannot drift.
fn about_dialog() -> adw::AboutDialog {
    adw::AboutDialog::builder()
        .application_name("PX13 Pen")
        .application_icon("io.github.gonzalezerik.Px13Pen.Settings")
        .developer_name("The PX13 SA205H Linux maintainers")
        .version(env!("CARGO_PKG_VERSION"))
        .comments("Pen settings for the PX13 SA205H Linux daemon.")
        .website("https://github.com/gonzalezerik/px13-sa205h-linux")
        .issue_url("https://github.com/gonzalezerik/px13-sa205h-linux/issues")
        .license_type(gtk::License::Agpl30)
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

/// The wiring every toggle shares: where commands go, which pen they
/// name, and the guard that keeps programmatic updates from echoing
/// back as user edits.
struct ToggleWire<'a> {
    bus: &'a BusHandle,
    path: &'a str,
    syncing: &'a Rc<Cell<bool>>,
}

/// A labelled switch that sends one command when toggled. The subtitle
/// is the row's only explanation; keep it to one line.
fn toggle_row(
    title: &str,
    subtitle: Option<&str>,
    active: bool,
    sensitive: bool,
    wire: &ToggleWire<'_>,
    command: fn(String, bool) -> Command,
) -> adw::SwitchRow {
    let row = adw::SwitchRow::builder()
        .title(title)
        .active(active)
        .sensitive(sensitive)
        .build();
    if let Some(subtitle) = subtitle {
        row.set_subtitle(subtitle);
    }
    let bus = wire.bus.clone();
    let path = wire.path.to_string();
    let syncing = Rc::clone(wire.syncing);
    row.connect_active_notify(move |row| {
        if syncing.get() {
            return;
        }
        bus.send(command(path.clone(), row.is_active()));
    });
    row
}

/// The textures the picker offers: the pen's own continuous waveforms
/// (docs/dbus.md §6) minus the eraser, which the daemon arms by itself
/// when the pen is inverted. No capability list, no picker.
fn ink_textures(pen: &Pen) -> Vec<String> {
    pen.capabilities
        .as_ref()
        .and_then(|caps| caps.haptics.as_ref())
        .map(|haptics| {
            haptics
                .waveforms
                .iter()
                .filter(|wave| wave.continuous && wave.name != "eraser")
                .map(|wave| wave.name.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// What every shortcut-button entry shares, said once as the group's
/// description: what a command is, how it saves, and the one trap.
const BUTTON_NOTE: &str = "A command runs when the pen's top button fires the gesture; \
                           leave one empty to do nothing. Press Enter to save. The pen's \
                           own Win+F18/19/20 keys still reach the desktop — bind a gesture \
                           here or in the compositor, not both.";

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
    use px13pen_client::{Capabilities, CapabilityHaptics, Waveform};

    fn waveform(ordinal: u16, name: &str, continuous: bool) -> Waveform {
        Waveform {
            ordinal,
            name: name.to_string(),
            duration_ms: 0,
            continuous,
        }
    }

    #[test]
    fn the_picker_offers_continuous_waveforms_without_the_eraser() {
        let mut pen = sample_pen();
        pen.capabilities = Some(Capabilities {
            haptics: Some(CapabilityHaptics {
                waveforms: vec![
                    waveform(3, "click", false),
                    waveform(9, "ink", true),
                    waveform(14, "eraser", true),
                    waveform(10, "pencil", true),
                ],
            }),
            ..Default::default()
        });
        assert_eq!(ink_textures(&pen), ["ink", "pencil"]);
        assert!(ink_textures(&sample_pen()).is_empty());
    }

    fn sample_pen() -> Pen {
        Pen {
            path: "/io/github/gonzalezerik/Px13Pen/pen/dev_0a_0b_0c_0d_0e_0f".to_string(),
            name: "SA205H".to_string(),
            address: String::new(),
            present: true,
            paired: true,
            trusted: true,
            battery: None,
            battery_detail: None,
            haptics: None,
            buttons: None,
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
