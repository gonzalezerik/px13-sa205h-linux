// SPDX-License-Identifier: AGPL-3.0-or-later
//! System pairing helper for MPP 2.6 pens.
//!
//! Stock bluetoothd exports no way to bond a device that never
//! advertises: `Adapter1.ConnectDevice` has been `[experimental]` since
//! it was added. The per-user daemon therefore asks this helper, which
//! issues the same bond through the kernel management interface
//! (`MGMT_OP_PAIR_DEVICE`). bluetoothd watches the management events,
//! registers the device and stores the long-term key itself, so the pen
//! shows up as `Device1` and the daemon finishes with `Trusted` like any
//! other pen.
//!
//! The helper is D-Bus activated — nothing to enable — and does one
//! thing: pair an LE random static address on a named adapter. The
//! systemd unit grants it `CAP_NET_ADMIN` and nothing else; callers must
//! pass the `org.openpenprotocol.pair-pen` polkit check, which the
//! shipped policy grants to active local sessions — the same reach
//! BlueZ's own pairing has.

mod mgmt;

use std::collections::HashMap;
use std::process::ExitCode;

use zbus::wire::Variant;
use zbus::{Connection, Proxy, fdo, interface};

/// The polkit action callers must be allowed to perform.
const PAIR_ACTION: &str = "org.openpenprotocol.pair-pen";

/// The bus name, the object path and the unit's `BusName=` must agree.
const BUS_NAME: &str = "org.openpenprotocol.PairHelper1";
const OBJECT_PATH: &str = "/org/openpenprotocol/PairHelper1";

struct PairHelper;

#[interface(name = "org.openpenprotocol.PairHelper1")]
impl PairHelper {
    /// Pair `address` on `adapter` through the kernel. The polkit check
    /// runs against the bus sender of this very call, so it cannot be
    /// borrowed from another client's session.
    async fn pair_device(
        &self,
        #[zbus(connection)] connection: &Connection,
        #[zbus(header)] header: zbus::message::Header<'_>,
        address: String,
        adapter: String,
    ) -> fdo::Result<()> {
        authorize(connection, &header)
            .await
            .map_err(fdo::Error::Failed)?;
        let address = mgmt::parse_random_static(&address).map_err(fdo::Error::Failed)?;
        let index = mgmt::adapter_index(&adapter).map_err(fdo::Error::Failed)?;
        // The management exchange is a blocking loop of a couple of
        // seconds at most; a tap-to-pair announcement is one pairing at
        // a time by construction.
        mgmt::pair_device(index, address).map_err(fdo::Error::Failed)
    }
}

/// `CheckAuthorization` for the sender's bus name. Flags 0: no
/// interaction is offered, so a caller that is merely the subject of a
/// challenge (remote, or an inactive session) is simply refused.
async fn authorize(
    connection: &Connection,
    header: &zbus::message::Header<'_>,
) -> Result<(), String> {
    let sender = header
        .sender()
        .ok_or("the request carries no sender")?
        .to_string();
    let polkit = Proxy::new(
        connection,
        "org.freedesktop.PolicyKit1",
        "/org/freedesktop/PolicyKit1/Authority",
        "org.freedesktop.PolicyKit1.Authority",
    )
    .await
    .map_err(|error| format!("polkit is not reachable: {error}"))?;
    let mut subject_details = HashMap::new();
    subject_details.insert("name", Variant::from(sender.as_str()));
    let details: HashMap<&str, &str> = HashMap::new();
    // CheckAuthorization((sa{sv}) subject, s action_id, a{ss} details,
    // u flags, s cancellation_id) -> (b is_authorized, b is_challenge,
    // a{ss} details); the subject is its own struct.
    let check: zbus::Result<(bool, bool, HashMap<String, String>)> = polkit
        .call(
            "CheckAuthorization",
            &(
                ("system-bus-name", subject_details),
                PAIR_ACTION,
                details,
                0u32,
                "",
            ),
        )
        .await;
    let (authorized, _challenge, _details) =
        check.map_err(|error| format!("the polkit check failed: {error}"))?;
    if authorized {
        Ok(())
    } else {
        Err(format!("caller is not authorized for {PAIR_ACTION}"))
    }
}

/// Own the bus name and keep serving; the system bus starts this process
/// on demand and the name request is what completes the activation.
async fn serve() -> Result<(), String> {
    let connection = Connection::system()
        .await
        .map_err(|error| format!("system bus: {error}"))?;
    connection
        .object_server()
        .at(OBJECT_PATH, PairHelper)
        .await
        .map_err(|error| format!("exporting {OBJECT_PATH}: {error}"))?;
    connection
        .request_name(BUS_NAME)
        .await
        .map_err(|error| format!("owning {BUS_NAME}: {error}"))?;
    std::future::pending::<()>().await;
    Ok(())
}

fn main() -> ExitCode {
    match zbus::block_on(serve()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("opp-pair-helper: {message}");
            ExitCode::FAILURE
        }
    }
}
