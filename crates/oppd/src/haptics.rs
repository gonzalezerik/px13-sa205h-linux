// SPDX-License-Identifier: AGPL-3.0-or-later
//! Play a haptic effect on a paired pen.
//!
//! One 7-byte output report per effect (docs/protocol/haptics.md §2.2,
//! wire layout findings/006 §4); nothing is written unless `--send` is
//! given — without it the exact bytes are printed first, which is also
//! the approval step for the first live sends.
//!
//! Route on Linux: hidraw → kernel HID core → uhid → BlueZ turns the
//! report into a GATT write to the pen's HID Report characteristic
//! (Windows used ATT Write Command to handle 0x0021; the pen accepts
//! Write Request as well). That BlueZ forwards this report is
//! [HYP until the first live run].

use hidapi::HidApi;
use opp_core::haptics::{self, Effect};

use crate::hex_bytes;

/// What the command line asked for.
#[derive(Debug, PartialEq, Eq)]
enum Command {
    List,
    Play {
        name: String,
        effect: Effect,
        percent: u8,
        path: Option<String>,
        send: bool,
    },
}

/// Entry point for `oppd haptic`.
pub fn haptic(args: &[String]) -> Result<(), String> {
    match parse(&args[1..])? {
        Command::List => {
            print_table();
            Ok(())
        }
        Command::Play {
            name,
            effect,
            percent,
            path,
            send,
        } => {
            let report = effect.to_output_report();
            println!("{name} → output report: {}", hex_bytes(&report));
            if !send {
                println!("no write performed — repeat with --send");
                return Ok(());
            }
            send_to_pen(path.as_deref(), &report)?;
            println!("sent — {name} at {percent} %");
            Ok(())
        }
    }
}

/// Parse the arguments after `haptic`.
fn parse(args: &[String]) -> Result<Command, String> {
    if args.first().map(String::as_str) == Some("--list") {
        return Ok(Command::List);
    }
    let mut ordinal = None;
    let mut name = None;
    let mut percent: u8 = 100;
    let mut repeat: u8 = haptics::PLAY_ONCE;
    let mut cutoff_ms: u16 = 0;
    let mut retrigger_ms: u16 = 0;
    let mut path = None;
    let mut send = false;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let mut take_value = |flag: &str, maximum: u32| -> Result<u32, String> {
            let raw = iter
                .next()
                .ok_or_else(|| format!("{flag} needs a number up to {maximum}"))?;
            let value: u32 = raw
                .parse()
                .map_err(|_| format!("{flag} is not a number: {raw}"))?;
            if value > maximum {
                return Err(format!("{flag} must be 0..{maximum}, got {value}"));
            }
            Ok(value)
        };
        match arg.as_str() {
            "--intensity" => percent = take_value("--intensity", 100)? as u8,
            "--repeat" => repeat = take_value("--repeat", u32::from(u8::MAX))? as u8,
            "--cutoff-ms" => cutoff_ms = take_value("--cutoff-ms", u32::from(u16::MAX))? as u16,
            "--retrigger-ms" => {
                retrigger_ms = take_value("--retrigger-ms", u32::from(u16::MAX))? as u16;
            }
            "--path" => path = Some(iter.next().ok_or("--path needs a hidraw path")?.clone()),
            "--send" => send = true,
            other if other.starts_with("--") => return Err(format!("unknown option {other}")),
            effect => {
                if name.is_some() {
                    return Err(format!("a second effect name makes no sense: {effect}"));
                }
                ordinal = Some(haptics::resolve(effect).ok_or_else(|| {
                    format!("unknown effect {effect} — see `oppd haptic --list`")
                })?);
                name = Some(effect.to_lowercase());
            }
        }
    }
    let ordinal = ordinal.ok_or("which effect? `oppd haptic --list` shows the table")?;
    Ok(Command::Play {
        name: name.unwrap_or_default(),
        effect: Effect {
            ordinal,
            intensity: haptics::intensity_byte(percent),
            repeat,
            cutoff_ms,
            retrigger_ms,
        },
        percent,
        path,
        send,
    })
}

/// Write the report to the pen's HID node (uhid names it after the
/// GATT name, so "pen" is in it; the digitizer is excluded by id).
pub(crate) fn send_to_pen(path: Option<&str>, report: &[u8]) -> Result<(), String> {
    let api = HidApi::new().map_err(|error| format!("cannot open the HID subsystem: {error}"))?;
    let (open, node_name) = match path {
        Some(explicit) => {
            let c_path = std::ffi::CString::new(explicit)
                .map_err(|_| format!("not a usable path: {explicit}"))?;
            (api.open_path(&c_path), explicit.to_string())
        }
        None => {
            let found: Vec<&hidapi::DeviceInfo> = api
                .device_list()
                .filter(|info| {
                    let id = opp_core::HidId {
                        vendor: info.vendor_id(),
                        product: info.product_id(),
                    };
                    let name = info.product_string().unwrap_or("").to_lowercase();
                    opp_core::supported_digitizer(id).is_none() && name.contains("pen")
                })
                .collect();
            let chosen = match found.as_slice() {
                [] => {
                    return Err(
                        "no pen HID node — pair and wake the pen first (its effects ride the \
                         Bluetooth HID side)"
                            .to_string(),
                    );
                }
                [only] => *only,
                many => {
                    let listed: Vec<String> = many
                        .iter()
                        .map(|info| info.path().to_string_lossy().into_owned())
                        .collect();
                    return Err(format!(
                        "several pen-named HID nodes ({}); pick one with --path",
                        listed.join(", ")
                    ));
                }
            };
            let shown = chosen.path().to_string_lossy().into_owned();
            (api.open_path(chosen.path()), shown)
        }
    };
    let device = open.map_err(|error| format!("cannot open {node_name}: {error}"))?;
    let written = device
        .write(report)
        .map_err(|error| format!("output report to {node_name} failed: {error}"))?;
    if written != report.len() {
        return Err(format!(
            "short write to {node_name}: {written} of {} bytes",
            report.len()
        ));
    }
    Ok(())
}

/// The SA205H effect table plus the two implicit ordinals.
fn print_table() {
    println!("ordinal  effect    waveform  duration  class");
    println!("      1  none      —         —         no-op");
    println!("      2  stop      —         —         stop playback (unconfirmed on this pen)");
    for wave in haptics::SA205H_WAVES {
        let class = if wave.continuous() {
            "continuous (felt while the tip touches)"
        } else {
            "discrete"
        };
        println!(
            "     {:>2}  {:<9}  {:04x}     {:>5} ms  {class}",
            wave.ordinal, wave.name, wave.id, wave.duration_ms
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    fn play(values: &[&str]) -> (String, Effect, u8, bool) {
        match parse(&args(values)).unwrap() {
            Command::Play {
                name,
                effect,
                percent,
                send,
                ..
            } => (name, effect, percent, send),
            Command::List => panic!("expected a play command"),
        }
    }

    #[test]
    fn defaults_match_the_verified_felt_send() {
        let (name, effect, percent, send) = play(&["click"]);
        assert_eq!(name, "click");
        assert_eq!(
            effect.to_output_report(),
            [0x41, 0x01, 0xff, 0x03, 0x00, 0x00, 0x00]
        );
        assert_eq!(percent, 100);
        assert!(!send, "nothing is written without --send");
    }

    #[test]
    fn options_land_in_the_report() {
        let (_, effect, _, send) = play(&[
            "success",
            "--intensity",
            "25",
            "--repeat",
            "3",
            "--cutoff-ms",
            "1000",
            "--retrigger-ms",
            "20",
            "--send",
        ]);
        assert_eq!(
            effect.to_output_report(),
            [0x41, 0x03, 0x40, 0x07, 0x64, 0x14, 0x00]
        );
        assert!(send);
    }

    #[test]
    fn list_is_its_own_command() {
        assert_eq!(parse(&args(&["--list"])).unwrap(), Command::List);
    }

    #[test]
    fn unknown_effect_names_hint_at_the_table() {
        let error = parse(&args(&["collide"])).unwrap_err();
        assert!(error.contains("--list"), "{error}");
    }

    #[test]
    fn bad_numbers_and_options_are_refused() {
        assert!(parse(&args(&["click", "--intensity"])).is_err());
        assert!(parse(&args(&["click", "--intensity", "101"])).is_err());
        assert!(parse(&args(&["click", "--intensity", "lots"])).is_err());
        assert!(parse(&args(&["click", "--cutoff-ms", "70000"])).is_err());
        assert!(parse(&args(&["click", "--wat"])).is_err());
        assert!(parse(&args(&["click", "press"])).is_err());
        assert!(parse(&args(&[])).is_err());
    }
}
