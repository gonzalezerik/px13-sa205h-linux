// SPDX-License-Identifier: AGPL-3.0-or-later
//! Pen haptics: the single output report and the effect table.
//!
//! The model is one 7-byte output report per effect — repeat, intensity,
//! a waveform *ordinal*, cutoff and retrigger — matching what the Windows
//! host writes (docs/protocol/haptics.md §2.2, wire layout verified
//! §2.7). Ordinals index the device's own waveform list; ordinals 1
//! (None) and 2 (Stop) are implicit. The `SA205H_WAVES` table is this
//! pen's capability report as read on the wire (findings/006 §4); the
//! generic feature-report parser comes with a captured BLE descriptor.

/// The only report the host ever writes to the pen (§2.2: five usages,
/// one write; feature reports stay read-only, §2.3).
pub const OUTPUT_REPORT_ID: u8 = 0x41;

/// Total length including the report-id byte.
pub const OUTPUT_REPORT_BYTES: usize = 7;

/// Implicit ordinal 1: no-op, must not disturb ongoing playback.
pub const ORDINAL_NONE: u8 = 1;

/// Implicit ordinal 2: stop = "play Stop" (§2.2). Not yet exercised on
/// this pen (findings/006 §4 open item).
pub const ORDINAL_STOP: u8 = 2;

/// The intensity byte for full strength; the registry slider 1.0 maps
/// to `FF` (findings/006 §4).
pub const FULL_INTENSITY: u8 = 0xFF;

/// The repeat byte a single felt click used: `41 01 ff 03 00 00 00`
/// was one click (findings/006 §4). The HID guide calls this field
/// "Repeat Count, 0 = play once"; on this firmware the verified single
/// play is `01`, so `01` is the default until proven otherwise.
pub const PLAY_ONCE: u8 = 1;

/// One entry of a pen's ordinal → waveform table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wave {
    pub ordinal: u8,
    pub id: u16,
    pub name: &'static str,
    pub duration_ms: u16,
}

impl Wave {
    /// Continuous waveforms play at the device's discretion (tip
    /// contact); discrete ones start immediately on the report.
    pub fn continuous(&self) -> bool {
        (0x100B..=0x1011).contains(&self.id)
    }
}

/// The ASUS Pen 3.0 (SA205H) waveform list as reported by feature 0x42
/// and felt on the wire (findings/006 §4): ordinals 3–15, discrete
/// first, then the seven continuous textures. Durations come from the
/// device's own list — the continuous entries are ≈2 s, not the guide's
/// "0 = continuous".
pub const SA205H_WAVES: &[Wave] = &[
    Wave {
        ordinal: 3,
        id: 0x1003,
        name: "click",
        duration_ms: 45,
    },
    Wave {
        ordinal: 4,
        id: 0x1006,
        name: "press",
        duration_ms: 60,
    },
    Wave {
        ordinal: 5,
        id: 0x1007,
        name: "release",
        duration_ms: 60,
    },
    Wave {
        ordinal: 6,
        id: 0x1008,
        name: "hover",
        duration_ms: 35,
    },
    Wave {
        ordinal: 7,
        id: 0x1009,
        name: "success",
        duration_ms: 145,
    },
    Wave {
        ordinal: 8,
        id: 0x100A,
        name: "error",
        duration_ms: 185,
    },
    Wave {
        ordinal: 9,
        id: 0x100B,
        name: "ink",
        duration_ms: 1995,
    },
    Wave {
        ordinal: 10,
        id: 0x100C,
        name: "pencil",
        duration_ms: 1995,
    },
    Wave {
        ordinal: 11,
        id: 0x100D,
        name: "marker",
        duration_ms: 1995,
    },
    Wave {
        ordinal: 12,
        id: 0x100E,
        name: "chisel",
        duration_ms: 1995,
    },
    Wave {
        ordinal: 13,
        id: 0x100F,
        name: "brush",
        duration_ms: 1980,
    },
    Wave {
        ordinal: 14,
        id: 0x1010,
        name: "eraser",
        duration_ms: 1995,
    },
    Wave {
        ordinal: 15,
        id: 0x1011,
        name: "sparkle",
        duration_ms: 1980,
    },
];

/// Resolve an effect name to a playback ordinal: the table plus the two
/// implicit ordinals. Case-insensitive.
pub fn resolve(name: &str) -> Option<u8> {
    let name = name.to_lowercase();
    match name.as_str() {
        "none" => return Some(ORDINAL_NONE),
        "stop" => return Some(ORDINAL_STOP),
        _ => {}
    }
    SA205H_WAVES
        .iter()
        .find(|wave| wave.name == name)
        .map(|wave| wave.ordinal)
}

/// One haptic playback request, in host units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Effect {
    pub ordinal: u8,
    /// Raw 0..=255 strength byte; see `intensity_byte`.
    pub intensity: u8,
    pub repeat: u8,
    /// Ceiling on total playback; encoded in 10 ms units, so the
    /// representable range ends at 2550 ms.
    pub cutoff_ms: u16,
    /// Gap (or restart period) between repeats.
    pub retrigger_ms: u16,
}

impl Effect {
    /// Play the ordinal once at full strength, no cutoff — the shape of
    /// every verified direct send.
    pub fn of(ordinal: u8) -> Self {
        Effect {
            ordinal,
            intensity: FULL_INTENSITY,
            repeat: PLAY_ONCE,
            cutoff_ms: 0,
            retrigger_ms: 0,
        }
    }

    /// The 7-byte output report: `41 <repeat> <intensity> <ordinal>
    /// <cutoff ×10 ms> <retrigger lo> <hi>` (findings/006 §4).
    pub fn to_output_report(&self) -> [u8; OUTPUT_REPORT_BYTES] {
        let cutoff_units = (self.cutoff_ms / 10).min(u8::MAX as u16) as u8;
        [
            OUTPUT_REPORT_ID,
            self.repeat,
            self.intensity,
            self.ordinal,
            cutoff_units,
            self.retrigger_ms as u8,
            (self.retrigger_ms >> 8) as u8,
        ]
    }
}

/// Map a 0–100 percent slider value to the intensity byte
/// (`byte ≈ slider/100 × 255`, rounded; the 25 % slider produced `40`
/// on the wire — findings/006 §4).
pub fn intensity_byte(percent: u8) -> u8 {
    let percent = percent.min(100) as u32;
    ((percent * 255 + 50) / 100) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_verified_single_click_encodes_byte_for_byte() {
        // `41 01 FF 03 00 00 00` = one click at full intensity, felt as
        // such (findings/006 §4).
        let report = Effect::of(3).to_output_report();
        assert_eq!(report, [0x41, 0x01, 0xFF, 0x03, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn cutoff_is_in_ten_millisecond_units_and_clamps() {
        let mut effect = Effect::of(9);
        effect.cutoff_ms = 1995;
        assert_eq!(effect.to_output_report()[4], 199);
        effect.cutoff_ms = 9999;
        assert_eq!(effect.to_output_report()[4], 255);
    }

    #[test]
    fn retrigger_is_little_endian_16_bit() {
        let mut effect = Effect::of(3);
        effect.retrigger_ms = 1000;
        let report = effect.to_output_report();
        assert_eq!(&report[5..7], &[0xE8, 0x03]);
    }

    #[test]
    fn intensity_rounds_like_the_windows_slider() {
        assert_eq!(intensity_byte(100), 0xFF);
        assert_eq!(intensity_byte(25), 0x40);
        assert_eq!(intensity_byte(49), 0x7D);
        assert_eq!(intensity_byte(0), 0x00);
        // Out-of-range saturates instead of wrapping.
        assert_eq!(intensity_byte(250), 0xFF);
    }

    #[test]
    fn sa205h_table_is_the_captured_capability_list() {
        assert_eq!(SA205H_WAVES.len(), 13);
        let ordinals: Vec<u8> = SA205H_WAVES.iter().map(|wave| wave.ordinal).collect();
        assert_eq!(ordinals.first().copied(), Some(3));
        assert_eq!(ordinals.last().copied(), Some(15));
        assert!(ordinals.windows(2).all(|pair| pair[1] == pair[0] + 1));
        let continuous: Vec<&Wave> = SA205H_WAVES.iter().filter(|w| w.continuous()).collect();
        assert_eq!(continuous.len(), 7);
        assert_eq!(SA205H_WAVES[0].name, "click");
        assert_eq!(SA205H_WAVES[0].duration_ms, 45);
    }

    #[test]
    fn resolve_handles_implicit_ordinals_and_case() {
        assert_eq!(resolve("none"), Some(ORDINAL_NONE));
        assert_eq!(resolve("stop"), Some(ORDINAL_STOP));
        assert_eq!(resolve("Click"), Some(3));
        assert_eq!(resolve("sparkle"), Some(15));
        assert_eq!(
            resolve("collide"),
            None,
            "the pen lacks it (findings/006 §4)"
        );
    }
}
