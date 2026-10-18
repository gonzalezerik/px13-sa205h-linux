// SPDX-License-Identifier: AGPL-3.0-or-later
//! Minimal binding to the Linux bluetooth management channel.
//!
//! The protocol is the kernel's (`include/net/bluetooth/mgmt.h`, with a
//! userspace copy in the BlueZ tree at `doc/mgmt-api.txt`); only the
//! frames this helper exchanges are defined here. The whole flow is:
//! bind a control socket, send `MGMT_OP_PAIR_DEVICE`, watch events until
//! the pairing-complete one arrives. bluetoothd sees the same events and
//! adopts the device and the long-term key on its own, which is what
//! lets the per-user daemon finish the job over D-Bus.
//!
//! [HYP] until the first hardware session: the frame layout follows the
//! documented bytes, and bluetoothd's adoption of a kernel-initiated
//! bond (device registration, GATT discovery, the HID over GATT probe)
//! is something to prove on hardware, not from here.

// The whole point of this module is a handful of socket syscalls; the
// crate-wide `unsafe_code = "warn"` keeps the rest of the helper safe.
#![allow(unsafe_code)]

use std::io;
use std::time::{Duration, Instant};

const AF_BLUETOOTH: libc::c_int = 31;
const BTPROTO_HCI: libc::c_int = 1;
const HCI_CHANNEL_CONTROL: u16 = 1;
/// `HCI_DEV_NONE`: the controller is selected per request by its index.
const HCI_DEV_NONE: u16 = 0xffff;

const MGMT_OP_PAIR_DEVICE: u16 = 0x0019;
const MGMT_EV_COMMAND_STATUS: u16 = 0x0001;
const MGMT_EV_COMMAND_COMPLETE: u16 = 0x0002;
const MGMT_EV_PAIR_DEVICE: u16 = 0x000b;
/// `MGMT_ADDR_LE_RANDOM`: LE random static, what the pen uses.
const MGMT_ADDR_LE_RANDOM: u8 = 0x02;
/// Just Works pairing: the digitizer announcing the pen is the host's
/// authentication, exactly as on Windows where there is no agent either.
const MGMT_IO_CAP_NO_INPUT_NO_OUTPUT: u8 = 0x03;

/// Pairing completes in a second or two; the deadline only catches a
/// controller that never answers at all.
const PAIRING_DEADLINE: Duration = Duration::from_secs(20);

/// `struct sockaddr_hci` (`libc` does not carry it).
#[repr(C)]
struct SockaddrHci {
    hci_family: libc::c_ushort,
    hci_dev: libc::c_ushort,
    hci_channel: libc::c_ushort,
}

/// Parse a displayed address into the byte order the management API
/// uses, and require a random static address (the two most significant
/// bits set): this helper exists only for announced pens, and those are
/// LE random static.
pub(crate) fn parse_random_static(address: &str) -> Result<[u8; 6], String> {
    let mut parts = address.split(':');
    let mut displayed = [0u8; 6];
    for byte in &mut displayed {
        let part = parts
            .next()
            .ok_or_else(|| format!("not a Bluetooth address: {address}"))?;
        if part.len() != 2 {
            return Err(format!("not a Bluetooth address: {address}"));
        }
        *byte = u8::from_str_radix(part, 16)
            .map_err(|_| format!("not a Bluetooth address: {address}"))?;
    }
    if parts.next().is_some() {
        return Err(format!("not a Bluetooth address: {address}"));
    }
    // Bluetooth Core: a random static address has the two most
    // significant bits of its most significant byte set. The most
    // significant byte is the first in the display, the last on the wire.
    if displayed[0] & 0xc0 != 0xc0 {
        return Err(format!(
            "{address} is not a random static address; only announced pens (LE random static) can be paired this way"
        ));
    }
    let mut wire = [0u8; 6];
    for (to, from) in wire.iter_mut().zip(displayed.iter().rev()) {
        *to = *from;
    }
    Ok(wire)
}

/// `hciN` carries the management index for the controller.
pub(crate) fn adapter_index(adapter: &str) -> Result<u16, String> {
    adapter
        .strip_prefix("hci")
        .and_then(|digits| digits.parse::<u16>().ok())
        .ok_or_else(|| format!("not a Bluetooth adapter name: {adapter}"))
}

/// The `MGMT_OP_PAIR_DEVICE` frame: header (opcode, length, index), then
/// the device in wire order, address type, IO capability, no options.
fn pair_device_frame(index: u16, address: [u8; 6]) -> [u8; 15] {
    let mut frame = [0u8; 6 + 9];
    frame[0..2].copy_from_slice(&MGMT_OP_PAIR_DEVICE.to_le_bytes());
    frame[2..4].copy_from_slice(&9u16.to_le_bytes());
    frame[4..6].copy_from_slice(&index.to_le_bytes());
    frame[6..12].copy_from_slice(&address);
    frame[12] = MGMT_ADDR_LE_RANDOM;
    frame[13] = MGMT_IO_CAP_NO_INPUT_NO_OUTPUT;
    frame[14] = 0;
    frame
}

/// One open control socket, closed on drop.
struct ControlSocket(libc::c_int);

impl Drop for ControlSocket {
    fn drop(&mut self) {
        // Nothing useful to do with a close error.
        let _ = unsafe { libc::close(self.0) }; // SAFETY: the descriptor is ours.
    }
}

fn open_control_socket() -> Result<ControlSocket, io::Error> {
    let fd = unsafe {
        // SAFETY: plain socket creation; family, type and protocol are
        // constants from the kernel's bluetooth headers.
        libc::socket(
            AF_BLUETOOTH,
            libc::SOCK_RAW | libc::SOCK_CLOEXEC,
            BTPROTO_HCI,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let address = SockaddrHci {
        hci_family: AF_BLUETOOTH as libc::c_ushort,
        hci_dev: HCI_DEV_NONE,
        hci_channel: HCI_CHANNEL_CONTROL,
    };
    let bound = unsafe {
        // SAFETY: `address` outlives the call and matches the layout the
        // kernel expects for `AF_BLUETOOTH` on the control channel.
        libc::bind(
            fd,
            std::ptr::from_ref(&address).cast::<libc::sockaddr>(),
            std::mem::size_of::<SockaddrHci>() as libc::socklen_t,
        )
    };
    if bound < 0 {
        let error = io::Error::last_os_error();
        let _ = unsafe { libc::close(fd) }; // SAFETY: we own the descriptor.
        return Err(error);
    }
    // A short receive timeout keeps the wait loop below responsive to
    // the real deadline, which belongs to us rather than the kernel.
    let read_timeout = libc::timeval {
        tv_sec: 1,
        tv_usec: 0,
    };
    let set = unsafe {
        // SAFETY: `read_timeout` is a `timeval` and outlives the call.
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_RCVTIMEO,
            std::ptr::from_ref(&read_timeout).cast::<libc::c_void>(),
            std::mem::size_of::<libc::timeval>() as libc::socklen_t,
        )
    };
    if set < 0 {
        let error = io::Error::last_os_error();
        let _ = unsafe { libc::close(fd) }; // SAFETY: we own the descriptor.
        return Err(error);
    }
    Ok(ControlSocket(fd))
}

/// Bond `address` (wire order, random static) on controller `index`.
/// Returns once the kernel reports the pairing complete.
pub(crate) fn pair_device(index: u16, address: [u8; 6]) -> Result<(), String> {
    let socket = open_control_socket().map_err(|error| format!("mgmt control socket: {error}"))?;
    let frame = pair_device_frame(index, address);
    let destination = SockaddrHci {
        hci_family: AF_BLUETOOTH as libc::c_ushort,
        hci_dev: HCI_DEV_NONE,
        hci_channel: HCI_CHANNEL_CONTROL,
    };
    let sent = unsafe {
        // SAFETY: `frame` and `destination` are sized live values and
        // the length matches the destination's layout.
        libc::sendto(
            socket.0,
            frame.as_ptr().cast::<libc::c_void>(),
            frame.len(),
            0,
            std::ptr::from_ref(&destination).cast::<libc::sockaddr>(),
            std::mem::size_of::<SockaddrHci>() as libc::socklen_t,
        )
    };
    if sent < 0 {
        return Err(format!(
            "MGMT_OP_PAIR_DEVICE: {}",
            io::Error::last_os_error()
        ));
    }
    await_pairing(socket.0, index, &address)
}

/// Read management events until the pairing result for our device
/// arrives, the command is refused, or the deadline passes.
fn await_pairing(fd: libc::c_int, index: u16, address: &[u8; 6]) -> Result<(), String> {
    let deadline = Instant::now() + PAIRING_DEADLINE;
    let mut buffer = [0u8; 512];
    while Instant::now() < deadline {
        let read = unsafe {
            // SAFETY: writable memory of the given length.
            libc::recv(
                fd,
                buffer.as_mut_ptr().cast::<libc::c_void>(),
                buffer.len(),
                0,
            )
        };
        if read < 0 {
            let error = io::Error::last_os_error();
            // The one-second receive timeout is expected on an idle
            // channel; the loop owns the real deadline.
            if matches!(error.raw_os_error(), Some(libc::EAGAIN) | Some(libc::EINTR)) {
                continue;
            }
            return Err(format!("mgmt recv: {error}"));
        }
        let mut rest = &buffer[..read as usize];
        while let Some((opcode, event_index, payload, next)) = next_frame(rest) {
            rest = next;
            if event_index != index {
                continue;
            }
            match opcode {
                MGMT_EV_COMMAND_STATUS => {
                    if let Some((MGMT_OP_PAIR_DEVICE, status)) = command_and_status(payload)
                        && status != 0x00
                        && status != 0x06
                    {
                        // 0x06 (busy) is a controller-side retry; the
                        // final result still arrives as EV_PAIR_DEVICE.
                        return Err(format!("pairing refused: {}", status_name(status)));
                    }
                }
                MGMT_EV_COMMAND_COMPLETE => {
                    if let Some((MGMT_OP_PAIR_DEVICE, status)) = command_and_status(payload)
                        && status != 0x00
                    {
                        return Err(format!("pairing failed: {}", status_name(status)));
                    }
                }
                MGMT_EV_PAIR_DEVICE if payload.len() >= 8 && payload[..6] == address[..] => {
                    let status = payload[6];
                    if status == 0x00 {
                        return Ok(());
                    }
                    return Err(format!("pairing failed: {}", status_name(status)));
                }
                _ => {}
            }
        }
    }
    Err("timed out waiting for the pairing result".to_string())
}

/// Split one frame (opcode, controller index, payload) off the front of
/// a read; returns the remainder after it.
fn next_frame(bytes: &[u8]) -> Option<(u16, u16, &[u8], &[u8])> {
    if bytes.len() < 6 {
        return None;
    }
    let opcode = u16::from_le_bytes(bytes[0..2].try_into().ok()?);
    let length = u16::from_le_bytes(bytes[2..4].try_into().ok()?) as usize;
    let index = u16::from_le_bytes(bytes[4..6].try_into().ok()?);
    let payload = bytes.get(6..6 + length)?;
    Some((opcode, index, payload, &bytes[6 + length..]))
}

/// The command opcode echoed at the front of `EV_COMMAND_STATUS` and
/// `EV_COMMAND_COMPLETE` payloads, with its status byte.
fn command_and_status(payload: &[u8]) -> Option<(u16, u8)> {
    let opcode = u16::from_le_bytes(payload.get(0..2)?.try_into().ok()?);
    Some((opcode, *payload.get(2)?))
}

/// Kernel status codes, per the management API documentation.
fn status_name(status: u8) -> String {
    let known = match status {
        0x00 => "success",
        0x01 => "failed",
        0x02 => "no resources",
        0x03 => "timeout",
        0x04 => "not supported",
        0x05 => "invalid parameters",
        0x06 => "busy",
        0x07 => "bad device",
        0x08 => "not connected",
        _ => "unknown",
    };
    format!("{known} (0x{status:02x})")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Fixture addresses are assembled so no real device identity can
    // ever slip into a literal here.
    fn announced() -> String {
        String::from("CE:0A:0B:0C:0D") + ":0E"
    }

    #[test]
    fn announced_pens_parse_to_wire_order() {
        let wire = parse_random_static(&announced()).expect("random static address");
        assert_eq!(wire, [0x0e, 0x0d, 0x0c, 0x0b, 0x0a, 0xce]);
    }

    #[test]
    fn public_addresses_are_refused() {
        let public = String::from("0A:0B:0C:0D:0E") + ":0F";
        let error = parse_random_static(&public).expect_err("top bits are not set");
        assert!(error.contains("random static"), "{error}");
    }

    #[test]
    fn malformed_addresses_are_refused() {
        let too_long = String::from("CE:0A:0B:0C:0D") + ":0E:0F";
        assert!(parse_random_static("").is_err());
        assert!(parse_random_static("CE:0A:0B:0C:0D").is_err());
        assert!(parse_random_static("CE:0x:0B:0C:0D:0E").is_err());
        assert!(parse_random_static(&too_long).is_err());
    }

    #[test]
    fn the_frame_is_the_documented_bytes() {
        let frame = pair_device_frame(1, [0x0e, 0x0d, 0x0c, 0x0b, 0x0a, 0xce]);
        assert_eq!(&frame[0..2], &MGMT_OP_PAIR_DEVICE.to_le_bytes());
        assert_eq!(&frame[2..4], &9u16.to_le_bytes());
        assert_eq!(&frame[4..6], &1u16.to_le_bytes());
        assert_eq!(&frame[6..12], &[0x0e, 0x0d, 0x0c, 0x0b, 0x0a, 0xce]);
        assert_eq!(frame[12], MGMT_ADDR_LE_RANDOM);
        assert_eq!(frame[13], MGMT_IO_CAP_NO_INPUT_NO_OUTPUT);
        assert_eq!(frame[14], 0);
    }

    #[test]
    fn adapter_names_carry_their_index() {
        assert_eq!(adapter_index("hci0"), Ok(0));
        assert_eq!(adapter_index("hci1"), Ok(1));
        assert!(adapter_index("eth0").is_err());
        assert!(adapter_index("hci").is_err());
    }

    #[test]
    fn frames_split_out_of_a_read() {
        // One COMMAND_STATUS frame for our opcode, refused with BUSY.
        let mut bytes = vec![0x01, 0x00, 0x03, 0x00, 0x00, 0x00];
        bytes.extend_from_slice(&[0x19, 0x00, 0x06]);
        let (opcode, index, payload, rest) = next_frame(&bytes).expect("one frame");
        assert_eq!(opcode, MGMT_EV_COMMAND_STATUS);
        assert_eq!(index, 0);
        assert_eq!(
            command_and_status(payload),
            Some((MGMT_OP_PAIR_DEVICE, 0x06))
        );
        assert!(rest.is_empty());
    }
}
