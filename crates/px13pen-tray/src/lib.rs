// SPDX-License-Identifier: AGPL-3.0-or-later
//! The pen's tray icon: one StatusNotifierItem with a dbusmenu, a thin
//! client of `px13pend` like `px13penctl`, never the hardware
//! (docs/decisions.md). What the icon shows is decided here as plain
//! data — texts, icon choice, menu — so the accessibility test
//! (`tests/a11y.rs`) can hold it to the project's standard before any
//! host renders it: every fact also readable as a name or a number,
//! battery state by icon shape, nothing carried by colour alone.
//!
//! The running side (src/main.rs) keeps this state fresh from the
//! daemon and lets a menu click write through.

mod menu;
pub mod summary;

use std::future::Future;

use ksni::menu::MenuItem;
use ksni::{Status, ToolTip, Tray};
use px13pen_client::{Client, Pen, block_on};

/// Everything the icon shows: the visible pens as last read, and
/// whether the icon is currently shown or inside its hide timer.
pub struct PenTray {
    pens: Vec<Pen>,
    shown: bool,
    client: Option<Client>,
}

impl PenTray {
    /// The tray of a live session: `set_state` keeps it current, and
    /// menu actions write through `client`.
    pub fn new(client: Client) -> Self {
        PenTray {
            pens: Vec::new(),
            shown: false,
            client: Some(client),
        }
    }

    /// What the tray would show for `pens` with no bus connection.
    /// The menu is built exactly as the live one, so tests see the
    /// real thing minus the writes.
    pub fn preview(pens: Vec<Pen>) -> Self {
        PenTray {
            pens,
            shown: true,
            client: None,
        }
    }

    /// A `preview` with the icon hidden, for checking the hidden state.
    pub fn hidden(mut self) -> Self {
        self.shown = false;
        self
    }

    /// Replace the whole shown state: `pens` are the pens a view
    /// should show, `shown` whether the icon may appear at all. A pen
    /// with a link sorts first, because the title, the tooltip and the
    /// icon all speak for the first pen — the live one if there is
    /// one.
    pub fn set_state(&mut self, pens: Vec<Pen>, shown: bool) {
        let mut pens = pens;
        pens.sort_by_key(|pen| !pen.present);
        self.pens = pens;
        self.shown = shown;
    }

    /// The grace period ran out with no link left to show: the icon
    /// goes away until the next pen arrives.
    pub fn hide(&mut self) {
        self.shown = false;
    }

    /// One write to `px13pend`, run off the tray thread. A menu must
    /// never wait on Bluetooth: the daemon echoes the new value back
    /// as a pen change, which is where the menu re-reads it from. A
    /// write that failed is said once on stderr — the service's
    /// journal — and otherwise passes silently, like a button action.
    fn write<F, Fut>(&self, what: String, call: F)
    where
        F: FnOnce(Client) -> Fut + Send + 'static,
        Fut: Future<Output = Result<(), String>> + Send + 'static,
    {
        let Some(client) = self.client.clone() else {
            return;
        };
        std::thread::spawn(move || {
            if let Err(error) = block_on(call(client)) {
                eprintln!("px13pen-tray: {what}: {error}");
            }
        });
    }

    fn set_haptics_enabled(&self, path: String, enabled: bool) {
        self.write(
            format!("setting haptics on {path}"),
            move |client| async move { client.set_haptics_enabled(&path, enabled).await },
        );
    }

    fn set_intensity(&self, path: String, intensity: u8) {
        self.write(
            format!("setting intensity on {path}"),
            move |client| async move { client.set_intensity(&path, intensity).await },
        );
    }

    fn start_pairing(&self) {
        self.write("starting pairing".to_string(), |client| async move {
            client.start_pairing().await.map(|_| ())
        });
    }

    fn open_settings(&self) {
        // The same detachment a button action has: launched, then
        // its status is its own. The session's PATH carries the
        // package binary.
        if let Err(error) = std::process::Command::new("px13pen-settings").spawn() {
            eprintln!("px13pen-tray: opening settings: {error}");
        }
    }
}

impl Tray for PenTray {
    /// Left-click is how anyone reaches a tray's controls; opening
    /// the menu beats activating a window no one asked for.
    const MENU_ON_ACTIVATE: bool = true;

    fn id(&self) -> String {
        env!("CARGO_PKG_NAME").to_string()
    }

    fn title(&self) -> String {
        summary::title(&self.pens)
    }

    fn status(&self) -> Status {
        if self.shown && !self.pens.is_empty() {
            Status::Active
        } else {
            Status::Passive
        }
    }

    fn icon_name(&self) -> String {
        summary::icon_name(self.pens.first()).to_string()
    }

    fn tool_tip(&self) -> ToolTip {
        let pen = self.pens.first();
        ToolTip {
            icon_name: summary::icon_name(pen).to_string(),
            title: pen.map_or_else(|| "PX13 Pen".to_string(), |pen| pen.name.clone()),
            description: pen.map_or_else(
                || "No pen connected".to_string(),
                summary::tooltip_description,
            ),
            icon_pixmap: Vec::new(),
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        menu::build(self)
    }
}
