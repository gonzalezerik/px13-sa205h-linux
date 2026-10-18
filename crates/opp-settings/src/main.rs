// SPDX-License-Identifier: AGPL-3.0-or-later
//! `opp-settings` — the pen settings window.
//!
//! The widget thread owns nothing but widgets. One background thread
//! (`runtime`) holds the `oppd` connection: it follows the daemon's
//! signals and carries out the commands the window sends, so the UI
//! never blocks on the bus and the bus never touches GTK.

use gtk::gio::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;

mod runtime;
mod window;

const USAGE: &str = "\
opp-settings — pen settings for the Open Pen Protocol daemon

usage: opp-settings [--system]

  --system   read and write oppd on the system bus (default: session bus)
  -h, --help show this help
";

fn main() -> gtk::glib::ExitCode {
    let argv: Vec<String> = std::env::args().collect();
    if argv
        .iter()
        .skip(1)
        .any(|arg| arg == "--help" || arg == "-h")
    {
        print!("{USAGE}");
        return gtk::glib::ExitCode::SUCCESS;
    }
    let system = argv.iter().any(|arg| arg == "--system");
    let args: Vec<&str> = argv
        .iter()
        .map(String::as_str)
        .filter(|arg| *arg != "--system")
        .collect();

    let app = adw::Application::new(
        Some("org.openpenprotocol.Settings"),
        gtk::gio::ApplicationFlags::empty(),
    );
    app.connect_activate(move |app| {
        window::build(app, system);
    });
    app.run_with_args(&args)
}
