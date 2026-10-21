// SPDX-License-Identifier: AGPL-3.0-or-later
//! The pen's debug log (findings/010).
//!
//! The SA205H streams a plain-text firmware log over the Nordic UART
//! Service TX characteristic to any bonded host that subscribes. Nothing
//! has to be written to the pen: enabling notifications is enough. The
//! log carries a fuel-gauge reading about once a second while the pen is
//! awake, and a short sequence when the pen goes to sleep.
//!
//! Notifications do not follow line boundaries: one `printf` may arrive
//! as several notifications (`"<i> : "`, `"[bat meter] "`, the values,
//! `"\r\n"`), and some records end without a newline. `LogReader`
//! reassembles them; `parse_record` turns one record into an `Event`.

/// The Nordic UART Service TX characteristic: pen → host notifications.
pub const NUS_TX_UUID: &str = "6e400003-b5a3-f393-e0a9-e50e24dcca9e";

/// Longest partial record kept between notifications. The longest record
/// seen is the 62-byte gauge line plus its prefixes; anything far beyond
/// that is noise and is dropped rather than grown without bound.
const MAX_PENDING: usize = 512;

/// One thing the pen reported.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// A fuel-gauge reading.
    Gauge(Gauge),
    /// `Charge State: <state>` — logged right before sleep, on the
    /// charger and off it alike; captures only ever show "Not Charging"
    /// (findings/010 §3, §4).
    ChargeState(String),
    /// `PEN ENTER SLEEP` — the pen is about to drop the link.
    Sleep,
    /// `wake reason = N`. The meaning of `N` is not known; `4` is the
    /// only value seen, on every motion-driven wake.
    Wake(u8),
}

/// A fuel-gauge reading. Both log lines repeat the same measurement
/// every second; only the `[bat meter]` line has the fractional state of
/// charge, so the `vbat1 = …` line leaves `percent_centi` empty rather
/// than round it away.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gauge {
    pub millivolts: u16,
    /// State of charge in hundredths of a percent (`7456` = 74.56 %).
    pub percent_centi: Option<u16>,
    /// Battery current in microamps; negative while discharging.
    pub microamps: i32,
}

/// Reassembles notifications into records.
#[derive(Debug, Default)]
pub struct LogReader {
    pending: String,
}

impl LogReader {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one notification; returns the events it completed.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<Event> {
        self.push_lines(chunk)
            .into_iter()
            .filter_map(|line| parse_record(&line))
            .collect()
    }

    /// Feed one notification; returns the records it completed as the pen
    /// logged them (whitespace trimmed), including records no rule parses.
    /// Records that arrive without a line ending (`PEN ENTER SLEEP`) count
    /// as complete when the whole buffer parses as one; a fragment such as
    /// `"<i> : "` is kept for the next notification.
    pub fn push_lines(&mut self, chunk: &[u8]) -> Vec<String> {
        self.pending.push_str(&String::from_utf8_lossy(chunk));
        let mut lines = Vec::new();
        while let Some(end) = self.pending.find('\n') {
            let line: String = self.pending.drain(..=end).collect();
            let line = line.trim();
            if !line.is_empty() {
                lines.push(line.to_string());
            }
        }
        // Records without a newline arrive as one whole notification;
        // a fragment such as "<i> : " is kept for the next one.
        if parse_record(&self.pending).is_some() {
            lines.push(self.pending.trim().to_string());
            self.pending.clear();
        }
        if self.pending.len() > MAX_PENDING {
            self.pending.clear();
        }
        lines
    }

    /// Forget a partial record, e.g. when the link drops.
    pub fn reset(&mut self) {
        self.pending.clear();
    }
}

/// Parse one record. Unknown text is not an error: the log has other
/// lines (the IMU interrupt counter, for one) that carry nothing a host
/// acts on.
pub fn parse_record(record: &str) -> Option<Event> {
    let text = record.trim();
    if text == "PEN ENTER SLEEP" {
        return Some(Event::Sleep);
    }
    if let Some(state) = text.strip_prefix("Charge State:") {
        return Some(Event::ChargeState(state.trim().to_string()));
    }
    if let Some(reason) = text.strip_prefix("wake reason =") {
        return reason.trim().parse().ok().map(Event::Wake);
    }
    if let Some(start) = text.find("[bat meter]") {
        return parse_bat_meter(&text[start + "[bat meter]".len()..]).map(Event::Gauge);
    }
    if text.starts_with("vbat1 =") {
        return parse_vbat(text).map(Event::Gauge);
    }
    None
}

/// `adc 4080 vbat 4070mV ocv 82% soc 74.56% t 1.0s i -2.3 -2.3mA`
fn parse_bat_meter(rest: &str) -> Option<Gauge> {
    let words: Vec<&str> = rest.split_whitespace().collect();
    let after = |key: &str| {
        words
            .iter()
            .position(|word| *word == key)
            .and_then(|index| words.get(index + 1))
            .copied()
    };
    let millivolts = after("vbat")?.strip_suffix("mV")?.parse().ok()?;
    let percent_centi = centi(after("soc")?.strip_suffix('%')?)?;
    // `i` is followed by two readings; the second carries the unit.
    let index = words.iter().position(|word| *word == "i")?;
    let milliamps: f64 = words.get(index + 2)?.strip_suffix("mA")?.parse().ok()?;
    Some(Gauge {
        millivolts,
        percent_centi: Some(percent_centi),
        microamps: (milliamps * 1000.0).round() as i32,
    })
}

/// `vbat1 = 4070 v2 = 4080, soc = 74, i_bat = -2310`
fn parse_vbat(text: &str) -> Option<Gauge> {
    let field = |key: &str| {
        let start = text.find(key)? + key.len();
        let value = text[start..].trim_start().strip_prefix('=')?.trim_start();
        let end = value
            .find(|c: char| c != '-' && !c.is_ascii_digit())
            .unwrap_or(value.len());
        value[..end].parse::<i64>().ok()
    };
    Some(Gauge {
        millivolts: u16::try_from(field("vbat1")?).ok()?,
        percent_centi: None,
        microamps: i32::try_from(field("i_bat")?).ok()?,
    })
}

/// `"74.56"` → `7456`, without going through a float.
fn centi(text: &str) -> Option<u16> {
    let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
    let fraction = match fraction.len() {
        0 => 0,
        1 => fraction.parse::<u16>().ok()? * 10,
        2 => fraction.parse::<u16>().ok()?,
        _ => return None,
    };
    whole
        .parse::<u16>()
        .ok()?
        .checked_mul(100)?
        .checked_add(fraction)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The notifications of one gauge cycle, exactly as captured.
    const CYCLE: [&[u8]; 5] = [
        b"<i> : ",
        b"[bat meter] ",
        b"adc 4080 vbat 4070mV ocv 82% soc 74.56% t 1.0s i -2.3 -2.3mA",
        b"\r\n",
        b"vbat1 = 4070 v2 = 4080, soc = 74, i_bat = -2310",
    ];

    #[test]
    fn a_split_gauge_line_is_reassembled() {
        let mut reader = LogReader::new();
        let events: Vec<Event> = CYCLE.iter().flat_map(|chunk| reader.push(chunk)).collect();
        assert_eq!(
            events,
            vec![
                Event::Gauge(Gauge {
                    millivolts: 4070,
                    percent_centi: Some(7456),
                    microamps: -2300,
                }),
                Event::Gauge(Gauge {
                    millivolts: 4070,
                    percent_centi: None,
                    microamps: -2310,
                }),
            ]
        );
    }

    #[test]
    fn the_dock_sequence_parses() {
        let mut reader = LogReader::new();
        let mut events = reader.push(b"Charge State: Not Charging");
        events.extend(reader.push(b"<i> : "));
        events.extend(reader.push(b"[bat meter] "));
        events
            .extend(reader.push(b"adc 4115 vbat 4105mV ocv 86% soc 78.30% t 1.0s i +0.00 +0.00mA"));
        events.extend(reader.push(b"\r\n"));
        events.extend(reader.push(b"PEN ENTER SLEEP"));
        assert_eq!(
            events,
            vec![
                Event::ChargeState("Not Charging".to_string()),
                Event::Gauge(Gauge {
                    millivolts: 4105,
                    percent_centi: Some(7830),
                    microamps: 0,
                }),
                Event::Sleep,
            ]
        );
    }

    #[test]
    fn push_lines_returns_the_records_as_logged() {
        let mut reader = LogReader::new();
        let mut lines: Vec<String> = Vec::new();
        for chunk in CYCLE {
            lines.extend(reader.push_lines(chunk));
        }
        assert_eq!(
            lines,
            vec![
                "<i> : [bat meter] adc 4080 vbat 4070mV ocv 82% soc 74.56% t 1.0s i -2.3 -2.3mA",
                "vbat1 = 4070 v2 = 4080, soc = 74, i_bat = -2310",
            ]
        );
        // A record without a line ending arrives whole and is returned at once.
        assert_eq!(
            reader.push_lines(b"PEN ENTER SLEEP"),
            vec!["PEN ENTER SLEEP".to_string()]
        );
        // Records no rule parses are lines too — the IMU counter included.
        assert_eq!(
            reader.push_lines(b"imu int cnt = 85800\r\n"),
            vec!["imu int cnt = 85800".to_string()]
        );
    }

    #[test]
    fn wake_reasons_and_unknown_lines() {
        assert_eq!(parse_record("wake reason = 4"), Some(Event::Wake(4)));
        assert_eq!(parse_record("imu int cnt = 85800"), None);
        assert_eq!(parse_record("wake reason = x"), None);
    }

    #[test]
    fn a_fragment_waits_for_the_rest() {
        let mut reader = LogReader::new();
        assert!(reader.push(b"<i> : ").is_empty());
        assert!(reader.push(b"[bat meter] ").is_empty());
        reader.reset();
        // After a reset the orphaned values line carries no prefix and
        // is not mistaken for a reading.
        assert!(
            reader
                .push(b"adc 4080 vbat 4070mV ocv 82% soc 74.56% t 1.0s i -2.3 -2.3mA\r\n")
                .is_empty()
        );
    }

    #[test]
    fn runaway_text_is_dropped() {
        let mut reader = LogReader::new();
        reader.push(&[b'x'; 600]);
        assert!(reader.pending.is_empty());
    }

    #[test]
    fn centi_handles_every_precision() {
        assert_eq!(centi("74.56"), Some(7456));
        assert_eq!(centi("74.5"), Some(7450));
        assert_eq!(centi("74"), Some(7400));
        assert_eq!(centi("74.567"), None);
        assert_eq!(centi("-1"), None);
    }
}
