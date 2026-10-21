// SPDX-License-Identifier: AGPL-3.0-or-later
//! The bus side of the window. One thread owns the `px13pend` connection:
//! it follows the daemon's signals and applies the commands the window
//! sends. The two never meet directly — commands go out through one
//! unbounded channel, events come back through the other, and the
//! widget thread polls the event channel on its own clock.

use async_channel::{Receiver, Sender, unbounded};
use futures_util::future::{Either, select};
use px13pen_client::{Client, WatchEvent};

/// What the window asks the daemon to do.
pub enum Command {
    SetHapticsEnabled {
        path: String,
        enabled: bool,
    },
    SetIntensity {
        path: String,
        intensity: u8,
    },
    SetWhileInking {
        path: String,
        on: bool,
    },
    SetWhileInteracting {
        path: String,
        on: bool,
    },
    SetInkTexture {
        path: String,
        texture: String,
    },
    /// One shortcut-button gesture's command; `gesture` is one of the
    /// names the daemon's `ButtonEvent` signal carries.
    SetButtonAction {
        path: String,
        gesture: &'static str,
        command: String,
    },
    /// The daemon's status-notification switch (`Notifications1`);
    /// shared by every pen, like the haptics policy.
    SetStatusNotifications {
        path: String,
        enabled: bool,
    },
    StartPairing,
    StopPairing,
}

/// What the window needs to hear about: daemon events plus the two
/// failures that have no place in the event stream itself.
#[derive(Debug)]
pub enum UiEvent {
    /// A daemon event, passed through unchanged from `px13pen-client`.
    /// Boxed: a full pen snapshot inside the enum would make every
    /// event as big as the biggest pen list.
    Watch(Box<WatchEvent>),
    /// A command failed; the window shows the message and waits for
    /// the daemon's own state to settle.
    Failure(String),
    /// The follow loop ended. The window stays open, stale and marked.
    LostConnection(String),
}

/// The sending half, cheap to clone into widget signal handlers.
#[derive(Clone)]
pub struct BusHandle {
    tx: Sender<Command>,
}

impl BusHandle {
    /// Queue a command. The queue is unbounded, so this only fails once
    /// the bus thread is gone — and by then a `Failure` or
    /// `LostConnection` event is already on its way to the window.
    pub fn send(&self, command: Command) {
        let _ = self.tx.try_send(command);
    }
}

/// Start the bus thread against the same bus `px13pend serve` exports on.
pub fn spawn(system: bool) -> (BusHandle, Receiver<UiEvent>) {
    let (command_tx, command_rx) = unbounded();
    let (event_tx, event_rx) = unbounded();
    let spawned = std::thread::Builder::new()
        .name("px13pen-bus".to_string())
        .spawn({
            let events = event_tx.clone();
            move || px13pen_client::block_on(run(system, command_rx, events))
        });
    if spawned.is_err() {
        let _ = event_tx.try_send(UiEvent::Failure("cannot start the bus thread".to_string()));
    }
    (BusHandle { tx: command_tx }, event_rx)
}

/// Follow the daemon until it goes away, applying commands the whole
/// time. Either half ending ends the thread: the follow ending means
/// the bus is gone, the command queue emptying means the window is.
async fn run(system: bool, commands: Receiver<Command>, events: Sender<UiEvent>) {
    let client = match Client::connect(system).await {
        Ok(client) => client,
        Err(error) => {
            let _ = events.try_send(UiEvent::Failure(error));
            return;
        }
    };
    let follow = client.follow(false, |event| {
        let _ = events.try_send(UiEvent::Watch(Box::new(event)));
    });
    let pump = pump(&client, &commands, &events);
    match select(Box::pin(follow), Box::pin(pump)).await {
        Either::Left((outcome, _commands)) => {
            let reason = match outcome {
                Ok(()) => "the connection to px13pend closed".to_string(),
                Err(reason) => reason,
            };
            let _ = events.try_send(UiEvent::LostConnection(reason));
        }
        Either::Right((_pump, _follow)) => {}
    }
}

/// Apply commands until the window drops its handle. A failed command
/// is reported, never fatal — the next event tells the window what the
/// daemon actually has.
async fn pump(client: &Client, commands: &Receiver<Command>, events: &Sender<UiEvent>) {
    while let Ok(command) = commands.recv().await {
        let outcome = match command {
            Command::SetHapticsEnabled { path, enabled } => {
                client.set_haptics_enabled(&path, enabled).await
            }
            Command::SetIntensity { path, intensity } => {
                client.set_intensity(&path, intensity).await
            }
            Command::SetWhileInking { path, on } => client.set_while_inking(&path, on).await,
            Command::SetWhileInteracting { path, on } => {
                client.set_while_interacting(&path, on).await
            }
            Command::SetInkTexture { path, texture } => {
                client.set_ink_texture(&path, &texture).await
            }
            Command::SetButtonAction {
                path,
                gesture,
                command,
            } => client.set_button_action(&path, gesture, &command).await,
            Command::SetStatusNotifications { path, enabled } => {
                client.set_status_notifications(&path, enabled).await
            }
            Command::StartPairing => client.start_pairing().await.map(|_| ()),
            Command::StopPairing => client.stop_pairing().await.map(|_| ()),
        };
        if let Err(error) = outcome {
            let _ = events.try_send(UiEvent::Failure(error));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_survives_until_the_receiver_asks() {
        let (command_tx, command_rx) = unbounded::<Command>();
        let handle = BusHandle { tx: command_tx };
        handle.send(Command::StopPairing);
        let received = command_rx.try_recv();
        assert!(matches!(received, Ok(Command::StopPairing)));
    }
}
