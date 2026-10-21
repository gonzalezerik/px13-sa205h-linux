// SPDX-License-Identifier: AGPL-3.0-or-later
//! Host-side haptics policy, persisted per user (docs/dbus.md §7).
//!
//! Windows keeps the same policy in the registry (findings/006 §4);
//! here it is one small key=value file under the user's config
//! directory. Parsing is deliberately forgiving: unknown keys and bad
//! values fall back to the defaults, so a file written by a newer
//! daemon never upsets an older one.

use std::fs;
use std::io::Write as _;
use std::path::PathBuf;

use px13pen_core::haptics::SA205H_WAVES;

/// The whole policy. One instance is shared by every pen: the toggles
/// are per user, not per pen, exactly like the registry values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HapticsSettings {
    /// Master switch; `false` means no report is ever written.
    pub enabled: bool,
    /// Default strength for the `255` sentinel, 0–100 percent.
    pub intensity: u8,
    /// The tip's contact arms the ink texture.
    pub while_inking: bool,
    /// Discrete effects (the UI ticks) get through.
    pub while_interacting: bool,
    /// Which continuous texture the tip arms; `is_ink_texture` names.
    pub ink_texture: String,
}

impl Default for HapticsSettings {
    /// The Windows defaults: everything on, full strength, plain ink.
    fn default() -> Self {
        HapticsSettings {
            enabled: true,
            intensity: 100,
            while_inking: true,
            while_interacting: true,
            ink_texture: "ink".to_string(),
        }
    }
}

impl HapticsSettings {
    /// Whether an ordinal may reach the pen under this policy. The
    /// master switch wins; continuous waveforms (the ink textures)
    /// need `while_inking`, discrete ones need `while_interacting`.
    /// The implicit ordinals (none, stop) always pass while the master
    /// is on — stopping must never be gated.
    pub fn plays(&self, ordinal: u8) -> bool {
        if !self.enabled {
            return false;
        }
        match SA205H_WAVES.iter().find(|wave| wave.ordinal == ordinal) {
            Some(wave) if wave.continuous() => self.while_inking,
            Some(_) => self.while_interacting,
            None => true,
        }
    }
}

/// The texture names `InkTexture` accepts: the continuous waveforms,
/// minus `eraser` — the eraser end always arms the eraser texture by
/// itself, so it is never a user choice (docs/dbus.md §7).
pub fn ink_textures() -> Vec<&'static str> {
    SA205H_WAVES
        .iter()
        .filter(|wave| wave.continuous() && wave.name != "eraser")
        .map(|wave| wave.name)
        .collect()
}

/// Whether a (lower-case) name is a selectable ink texture.
pub fn is_ink_texture(name: &str) -> bool {
    ink_textures().contains(&name)
}

/// Where the file lives: `px13pen/haptics.conf` under
/// `$XDG_CONFIG_HOME` (`$HOME/.config` fallback). `None` when neither
/// is set — the daemon then keeps the policy in memory only.
pub fn config_path() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join("haptics.conf"))
}

/// The shortcut button policy: one shell command per top-button
/// gesture, `""` meaning do nothing. The pen's own Win+F18/19/20 press
/// reaches the compositor either way (docs/dbus.md §7).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ButtonSettings {
    pub single: String,
    pub double: String,
    pub long: String,
}

/// Where the button file lives: `px13pen/buttons.conf`, same directory
/// rules as the haptics file.
pub fn button_config_path() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join("buttons.conf"))
}

/// The directory both policy files share.
fn config_dir() -> Option<PathBuf> {
    let root = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => PathBuf::from(std::env::var_os("HOME")?).join(".config"),
    };
    Some(root.join("px13pen"))
}

/// Read the button policy; a missing file is "no actions", and
/// anything unparseable falls back key by key, like the haptics file.
pub fn load_buttons() -> ButtonSettings {
    let Some(path) = button_config_path() else {
        return ButtonSettings::default();
    };
    match fs::read_to_string(&path) {
        Ok(text) => parse_buttons(&text),
        Err(error) if path.exists() => {
            eprintln!("settings: {}: {error} — using defaults", path.display());
            ButtonSettings::default()
        }
        Err(_) => ButtonSettings::default(),
    }
}

/// Write the button policy, temp file plus rename like the haptics one.
pub fn save_buttons(settings: &ButtonSettings) -> Result<(), String> {
    let Some(path) = button_config_path() else {
        return Ok(());
    };
    let dir = path
        .parent()
        .ok_or_else(|| "config path has no parent".to_string())?;
    fs::create_dir_all(dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    let tmp = path.with_extension("conf.tmp");
    let result = (|| -> std::io::Result<()> {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(buttons_to_text(settings).as_bytes())?;
        file.flush()?;
        fs::rename(&tmp, &path)
    })();
    result.map_err(|error| format!("{}: {error}", path.display()))
}

/// Serialise the button policy, one `key=value` per line. A command
/// lives on one line, so the editor fields are single-line too.
pub fn buttons_to_text(settings: &ButtonSettings) -> String {
    format!(
        "# px13pen shortcut button actions — written by px13pend; see docs/settings.md.\n\
         # One shell command per gesture; empty means do nothing.\n\
         # The pen's Win+F18/19/20 always reach the compositor too.\n\
         single={}\n\
         double={}\n\
         long={}\n",
        settings.single, settings.double, settings.long,
    )
}

/// Parse the `key=value` lines; commands keep everything after the
/// first `=`, so they may contain `=` themselves. Commands carrying a
/// newline (impossible from the editor) are ignored.
pub fn parse_buttons(text: &str) -> ButtonSettings {
    let mut settings = ButtonSettings::default();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "single" => settings.single = value.to_string(),
            "double" => settings.double = value.to_string(),
            "long" => settings.long = value.to_string(),
            _ => {}
        }
    }
    settings
}

/// Read the policy; a missing file is the defaults, and anything the
/// parser cannot make sense of falls back per key.
pub fn load() -> HapticsSettings {
    let Some(path) = config_path() else {
        return HapticsSettings::default();
    };
    match fs::read_to_string(&path) {
        Ok(text) => parse(&text),
        Err(error) if path.exists() => {
            eprintln!("settings: {}: {error} — using defaults", path.display());
            HapticsSettings::default()
        }
        Err(_) => HapticsSettings::default(),
    }
}

/// Write the policy: temp file plus rename, so a crash cannot leave a
/// half-written config behind.
pub fn save(settings: &HapticsSettings) -> Result<(), String> {
    let Some(path) = config_path() else {
        return Ok(());
    };
    let dir = path
        .parent()
        .ok_or_else(|| "config path has no parent".to_string())?;
    fs::create_dir_all(dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    let tmp = path.with_extension("conf.tmp");
    let result = (|| -> std::io::Result<()> {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(to_text(settings).as_bytes())?;
        file.flush()?;
        fs::rename(&tmp, &path)
    })();
    result.map_err(|error| format!("{}: {error}", path.display()))
}

/// Serialise the policy, one `key=value` per line.
pub fn to_text(settings: &HapticsSettings) -> String {
    format!(
        "# px13pen haptics policy — written by px13pend; see docs/settings.md.\n\
         enabled={}\n\
         intensity={}\n\
         while_inking={}\n\
         while_interacting={}\n\
         ink_texture={}\n",
        settings.enabled,
        settings.intensity,
        settings.while_inking,
        settings.while_interacting,
        settings.ink_texture,
    )
}

/// Parse `key=value` lines. Unknown keys, malformed lines and values
/// out of range are ignored key by key.
pub fn parse(text: &str) -> HapticsSettings {
    let mut settings = HapticsSettings::default();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "enabled" => settings.enabled = parse_bool(value).unwrap_or(settings.enabled),
            "intensity" => {
                if let Ok(percent) = value.parse::<u8>()
                    && percent <= 100
                {
                    settings.intensity = percent;
                }
            }
            "while_inking" => {
                settings.while_inking = parse_bool(value).unwrap_or(settings.while_inking)
            }
            "while_interacting" => {
                settings.while_interacting = parse_bool(value).unwrap_or(settings.while_interacting)
            }
            "ink_texture" => {
                let name = value.to_lowercase();
                if is_ink_texture(&name) {
                    settings.ink_texture = name;
                }
            }
            _ => {}
        }
    }
    settings
}

/// The bool spellings the file accepts; anything else keeps the
/// current value.
fn parse_bool(value: &str) -> Option<bool> {
    match value {
        "true" | "1" | "yes" | "on" => Some(true),
        "false" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_windows_defaults() {
        let settings = HapticsSettings::default();
        assert!(settings.enabled);
        assert_eq!(settings.intensity, 100);
        assert!(settings.while_inking);
        assert!(settings.while_interacting);
        assert_eq!(settings.ink_texture, "ink");
    }

    #[test]
    fn the_file_round_trips() {
        let settings = HapticsSettings {
            enabled: false,
            intensity: 42,
            while_inking: true,
            while_interacting: false,
            ink_texture: "brush".to_string(),
        };
        assert_eq!(parse(&to_text(&settings)), settings);
    }

    #[test]
    fn bad_lines_fall_back_key_by_key() {
        let text = "enabled=maybe\nintensity=999\nink_texture=eraser\n\
                    while_inking=off\nnonsense\n# a comment\n=empty\n";
        let settings = parse(text);
        assert!(settings.enabled, "an unparseable bool keeps the default");
        assert_eq!(settings.intensity, 100, "out of range keeps the default");
        assert_eq!(settings.ink_texture, "ink", "eraser is not selectable");
        assert!(!settings.while_inking, "off parses");
    }

    #[test]
    fn eraser_is_not_a_selectable_texture() {
        assert!(is_ink_texture("ink"));
        assert!(is_ink_texture("sparkle"));
        assert!(
            !is_ink_texture("eraser"),
            "the eraser end arms its texture by itself"
        );
        assert!(
            !is_ink_texture("click"),
            "discrete waveforms are not textures"
        );
    }

    #[test]
    fn the_button_file_round_trips() {
        let settings = ButtonSettings {
            single: "notify-send hi".to_string(),
            double: "sh -c 'a=1; echo $a'".to_string(),
            long: String::new(),
        };
        assert_eq!(parse_buttons(&buttons_to_text(&settings)), settings);
    }

    #[test]
    fn unknown_button_keys_are_ignored() {
        let settings = parse_buttons("single=ls\nmiddle=whatever\nnonsense\n");
        assert_eq!(settings.single, "ls");
        assert_eq!(settings.double, "");
    }

    #[test]
    fn the_toggles_gate_by_waveform_class() {
        let mut settings = HapticsSettings::default();
        assert!(settings.plays(3), "discrete");
        assert!(settings.plays(9), "continuous");
        assert!(settings.plays(1), "the stop ordinal");
        settings.while_inking = false;
        assert!(!settings.plays(9));
        assert!(settings.plays(3));
        settings.while_inking = true;
        settings.while_interacting = false;
        assert!(!settings.plays(4));
        assert!(settings.plays(10));
        assert!(settings.plays(1), "stopping is never gated");
        settings.enabled = false;
        assert!(!settings.plays(3));
        assert!(!settings.plays(9));
        assert!(!settings.plays(1), "off means no writes at all");
    }
}
