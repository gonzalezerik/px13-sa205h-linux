// SPDX-License-Identifier: AGPL-3.0-or-later
//! `px13pen-settings` — the pen settings window.
//!
//! The widget thread owns nothing but widgets. One background thread
//! (`runtime`) holds the `px13pend` connection: it follows the daemon's
//! signals and carries out the commands the window sends, so the UI
//! never blocks on the bus and the bus never touches GTK.

use gtk::gio::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;

mod runtime;
mod window;

const USAGE: &str = "\
px13pen-settings — pen settings for the PX13 SA205H Linux daemon

usage: px13pen-settings [--system]

  --system   read and write px13pend on the system bus (default: session bus)
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
        Some("io.github.gonzalezerik.Px13Pen.Settings"),
        gtk::gio::ApplicationFlags::empty(),
    );
    app.connect_activate(move |app| {
        window::build(app, system);
    });
    app.run_with_args(&args)
}
