// SPDX-License-Identifier: AGPL-3.0-or-later
//! The D-Bus client library for `oppd`, shared by every frontend.
//!
//! One [`Client`] speaks the interface of docs/dbus.md: read the pens
//! once with [`Client::pens`], follow them with [`Client::follow`],
//! write the settings the daemon owns. The reading rules — full
//! re-reads on change, snapshot before subscription, absent interfaces
//! treated as absence — are written here once so frontends stay
//! displays.

mod bus;
mod pen;

pub use bus::{Client, WatchEvent};
pub use pen::{
    Buttons, Capabilities, CapabilityHaptics, Haptics, Pen, Waveform, address_from_object_path,
    battery_from_wire,
};
pub use zbus::block_on;
