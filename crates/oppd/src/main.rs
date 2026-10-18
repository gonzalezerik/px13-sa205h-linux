// SPDX-License-Identifier: AGPL-3.0-or-later
//! Daemon for MPP 2.6 pens: pairing, haptics, battery, buttons. Not functional yet: the protocol work comes first (see README).

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("--version") | Some("-V") => println!("oppd {}", env!("CARGO_PKG_VERSION")),
        _ => {
            println!("oppd {}: not implemented yet.", env!("CARGO_PKG_VERSION"));
            println!("Supported digitizers:");
            for (id, name) in opp_core::SUPPORTED_DIGITIZERS {
                println!("  {:04x}:{:04x}  {name}", id.vendor, id.product);
            }
        }
    }
}
