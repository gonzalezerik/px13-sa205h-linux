// SPDX-License-Identifier: AGPL-3.0-or-later
//! The running tray: follow `px13pend` on the session bus and keep
//! one StatusNotifierItem current from what comes back (docs/dbus.md)
//! — never the hardware. The hide rule is the decided one: the icon
//! appears while a pen holds a link and stays for a grace period
//! after the last one drops, because these pens fall off Bluetooth
//! seconds after being set down and are back on the next touch
//! (findings/010 §4). A pen asleep on its charger is hidden exactly
//! like one asleep on the desk: it has no link, so there is no icon.
//! Pairing news is the daemon's to tell (docs/dbus.md §4), so this
//! process sends no notifications of its own.

use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use ksni::blocking::TrayMethods;
use px13pen_client::{Client, Pen, WatchEvent, block_on};
use px13pen_tray::PenTray;

/// How long the icon waits after the last link drops. The measured
/// idle drop is around 6–15 s after the pen is set down, so the icon
/// would otherwise blink on every thought about coffee; a minute
/// covers the way the pen is actually used.
const GRACE: Duration = Duration::from_secs(60);

fn main() -> ExitCode {
    let client = match block_on(Client::connect(false)) {
        Ok(client) => client,
        Err(error) => {
            eprintln!("px13pen-tray: {error}");
            return ExitCode::FAILURE;
        }
    };
    // A desktop without a status-notifier host must not become a
    // restart loop: asked to assume one may appear, the tray waits
    // for the watcher instead of failing here.
    let handle = match PenTray::new(client.clone())
        .assume_sni_available(true)
        .spawn()
    {
        Ok(handle) => handle,
        Err(error) => {
            eprintln!("px13pen-tray: no status notifier host: {error}");
            return ExitCode::FAILURE;
        }
    };

    let mut pens: Vec<Pen> = Vec::new();
    let mut shown = false;
    // An armed hide carries the generation it was armed against;
    // showing the icon again bumps the generation, and a timer that
    // wakes up against an old one does nothing.
    let mut pending_hide: Option<u64> = None;
    let generation = Arc::new(AtomicU64::new(0));

    // Every event carries a whole pen, so the list is applied, never
    // diffed; `follow` returns only when the connection dies, and a
    // non-zero exit lets the service restart into the new daemon.
    let outcome = block_on(client.follow(true, |event| {
        match event {
            WatchEvent::Snapshot { pens: fresh } => pens = fresh,
            WatchEvent::Added { pen } | WatchEvent::Changed { pen } => upsert(&mut pens, pen),
            WatchEvent::Removed { path, .. } => pens.retain(|pen| pen.path != path),
            // The daemon owns pairing notifications, and a button
            // press is not something an icon reacts to.
            WatchEvent::Button { .. } | WatchEvent::Pairing { .. } => {}
        }
        if pens.iter().any(|pen| pen.present) {
            if pending_hide.take().is_some() {
                generation.fetch_add(1, Ordering::Relaxed);
            }
            shown = true;
        }
        if shown {
            handle.update(|tray| tray.set_state(pens.clone(), true));
        } else if pending_hide.is_none() {
            let armed = generation.load(Ordering::Relaxed);
            pending_hide = Some(armed);
            let handle = handle.clone();
            let generation = Arc::clone(&generation);
            std::thread::spawn(move || {
                std::thread::sleep(GRACE);
                if generation.load(Ordering::Relaxed) == armed {
                    handle.update(|tray| tray.hide());
                }
            });
        }
    }));
    if let Err(error) = outcome {
        eprintln!("px13pen-tray: {error}");
    }
    ExitCode::FAILURE
}

fn upsert(pens: &mut Vec<Pen>, pen: Pen) {
    match pens.iter_mut().find(|known| known.path == pen.path) {
        Some(known) => *known = pen,
        None => pens.push(pen),
    }
}
