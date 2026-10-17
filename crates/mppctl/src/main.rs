// SPDX-License-Identifier: AGPL-3.0-or-later
//! Command-line client for mppd. Not functional yet: the protocol work comes first (see README).

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("--version") | Some("-V") => println!("mppctl {}", env!("CARGO_PKG_VERSION")),
        _ => {
            println!("mppctl {}: not implemented yet.", env!("CARGO_PKG_VERSION"));
            println!("Supported digitizers:");
            for (id, name) in mpp_core::SUPPORTED_DIGITIZERS {
                println!("  {:04x}:{:04x}  {name}", id.vendor, id.product);
            }
        }
    }
}
