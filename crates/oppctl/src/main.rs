// SPDX-License-Identifier: AGPL-3.0-or-later
//! Command-line client for `oppd`. `status` is one look at the pens,
//! `watch` is a JSON Lines stream of their changes, both over the D-Bus
//! interface of docs/dbus.md. The output contract is docs/oppctl.md and
//! contrib/ has the Waybar example; the empty-when-idle rule itself
//! lives in `json` and is tested there.

mod client;
mod json;

use std::process::ExitCode;

const USAGE: &str = "\
usage: oppctl <command> [options]

commands:
  status            one look at the pens: name, battery, link
      --json        one compact JSON object on stdout (docs/oppctl.md)
      --all         include pens that are paired but not connected
      --system      talk to oppd on the system bus (oppd serve --system)
  watch             stream events as JSON Lines: a snapshot, then
                    pen.added, pen.removed, pen.changed and pairing;
                    exits with an error when oppd goes away
      --all         keep link-down pens in the stream
      --system      talk to oppd on the system bus
  -h, --help        print this help
  -V, --version     print the version

An empty result is an empty stdout in every format, so a Waybar custom
module hides itself while no pen is connected. `watch` only ever emits
JSON; --json is accepted there for symmetry with status.
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(String::as_str).unwrap_or("").to_string();
    let outcome = match command.as_str() {
        "status" | "watch" => match Options::parse(&args) {
            Err(message) => Err(message),
            Ok(options) => opp_client::block_on(async {
                if command == "status" {
                    client::status(options.all, options.system, options.json).await
                } else {
                    client::watch(options.all, options.system).await
                }
            }),
        },
        "-h" | "--help" => {
            print!("{USAGE}");
            Ok(())
        }
        "-V" | "--version" => {
            println!("oppctl {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        _ => {
            eprint!("{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("oppctl: {message}");
            ExitCode::FAILURE
        }
    }
}

/// The shared option set; every flag works with both commands.
struct Options {
    all: bool,
    system: bool,
    json: bool,
}

impl Options {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut options = Options {
            all: false,
            system: false,
            json: false,
        };
        for arg in args.iter().skip(1) {
            match arg.as_str() {
                "--all" => options.all = true,
                "--system" => options.system = true,
                "--json" => options.json = true,
                other => return Err(format!("unknown option {other}")),
            }
        }
        Ok(options)
    }
}

#[cfg(test)]
mod tests {
    use super::Options;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn options_default_off_and_flags_accumulate() {
        let bare = Options::parse(&args(&["status"])).expect("no options");
        assert!(!bare.all && !bare.system && !bare.json);
        let everything =
            Options::parse(&args(&["watch", "--json", "--all", "--system"])).expect("flags");
        assert!(everything.all && everything.system && everything.json);
    }

    #[test]
    fn unknown_options_are_refused() {
        assert!(Options::parse(&args(&["status", "--battery"])).is_err());
    }
}
