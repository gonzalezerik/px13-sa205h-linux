// SPDX-License-Identifier: AGPL-3.0-or-later
//! Which status note a pen deserves, and when (docs/settings.md).
//!
//! A set-down pen drops its link within seconds and a charger report —
//! if there is one — lands within three seconds of the pen's own sleep
//! log (telemetry.rs), in either order around the link drop. So a drop
//! can only be named after waiting for both: link down plus a charger
//! report is "Docked · Charging", link down alone is "Idle", and a pen
//! back inside the window was only picked up again. One note per drop,
//! never an Idle that a late report then contradicts.
//!
//! A pen coming back says its battery and nothing else, after waiting
//! briefly for a fresh reading rather than repeating the stale value
//! from before the sleep. A pen that was never read stays quiet: a
//! note with no number in it is not a sentence.
//!
//! Pure logic on an injected clock: the service loop feeds it pen
//! changes and its own `now` every tick, the tests feed synthetic
//! times. Nothing here touches the bus; the daemon rounds percentages,
//! titles the note with the pen's name and sends it (service.rs).

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// The wait for the dock decision after a link drop: the charger window
/// of telemetry.rs plus a beat, so a report landing at its edge still
/// belongs to this drop.
pub const DROP_WINDOW: Duration = Duration::from_secs(3);

/// The wait for a fresh reading after a reconnect; after it the last
/// known value speaks, and a pen nobody ever read says nothing.
pub const WAKE_WINDOW: Duration = Duration::from_secs(3);

/// The note chosen for one pen. The daemon words these and titles them
/// with the pen's name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Note {
    /// Set down, no charger.
    Idle { percent: u8 },
    /// Asleep on its charger.
    Docked { percent: u8 },
    /// Picked up again: the battery, nothing else.
    Awake { percent: u8 },
}

#[derive(Debug)]
struct Entry {
    phase: Phase,
    /// The newest reading; every note speaks it.
    percent: Option<u8>,
    /// A charger report has been seen and the pen has not been lifted
    /// since — the next drop is a docking.
    charger: bool,
}

#[derive(Debug)]
enum Phase {
    /// Linked, nothing pending.
    Linked,
    /// Link down, waiting for the dock decision.
    Dropping {
        at: Instant,
    },
    /// The Idle or Docked note was sent; quiet until the pen returns.
    Idle,
    Docked,
    /// Linked again, waiting for a fresh reading.
    Waking {
        at: Instant,
    },
}

/// One entry per known pen, keyed by the daemon's pen object path.
#[derive(Default)]
pub struct Tracker {
    pens: BTreeMap<String, Entry>,
}

impl Tracker {
    /// Baseline at daemon start or first sighting: a linked pen starts
    /// `Linked`, a merely known one idle. The sighting itself never
    /// notes — a new pen already arrives with a pairing notification
    /// (docs/dbus.md), and a daemon must not announce the pen it woke
    /// up next to.
    pub fn sighted(&mut self, path: &str, present: bool) {
        self.pens.insert(
            path.to_string(),
            Entry {
                phase: if present { Phase::Linked } else { Phase::Idle },
                percent: None,
                charger: false,
            },
        );
    }

    /// BlueZ no longer knows the pen; every pending decision dies with it.
    pub fn forgotten(&mut self, path: &str) {
        self.pens.remove(path);
    }

    /// The link dropped. The name waits: `docked` within the window, or
    /// `tick` at its end.
    pub fn link_down(&mut self, path: &str, percent: Option<u8>, now: Instant) -> Option<Note> {
        let entry = self.pens.get_mut(path)?;
        if let Some(percent) = percent {
            entry.percent = Some(percent);
        }
        // A charger report that arrived before the link even went down
        // already decided this drop.
        if entry.charger {
            entry.phase = Phase::Docked;
            return entry.percent.map(|percent| Note::Docked { percent });
        }
        entry.phase = Phase::Dropping { at: now };
        None
    }

    /// A charger report (`Report::Docked`, telemetry.rs). During the
    /// drop window it names the drop right away; before any drop it is
    /// merely remembered, and after an Idle was already sent it is too
    /// late — one note per drop, never Idle contradicted by Docked.
    pub fn docked(&mut self, path: &str, percent: Option<u8>) -> Option<Note> {
        let entry = self.pens.get_mut(path)?;
        entry.charger = true;
        if let Some(percent) = percent {
            entry.percent = Some(percent);
        }
        match entry.phase {
            Phase::Dropping { .. } => {
                entry.phase = Phase::Docked;
                entry.percent.map(|percent| Note::Docked { percent })
            }
            _ => None,
        }
    }

    /// The link is back. A pen that dropped — seconds or days ago — now
    /// waits out `WAKE_WINDOW` for a fresh reading; a drop that had not
    /// been named yet is cancelled, so a short flap never sends Idle
    /// and Awake for the same set-down.
    pub fn link_up(&mut self, path: &str, percent: Option<u8>, now: Instant) -> Option<Note> {
        let entry = self.pens.get_mut(path)?;
        if let Some(percent) = percent {
            entry.percent = Some(percent);
        }
        // Lifted: a remembered charger report belongs to the last
        // docking, never to the next drop.
        entry.charger = false;
        match entry.phase {
            Phase::Linked | Phase::Waking { .. } => None,
            Phase::Dropping { .. } | Phase::Idle | Phase::Docked => {
                entry.phase = Phase::Waking { at: now };
                None
            }
        }
    }

    /// A fresh battery reading. The one that ends a wake wait is the
    /// wake note itself.
    pub fn battery(&mut self, path: &str, percent: Option<u8>) -> Option<Note> {
        let entry = self.pens.get_mut(path)?;
        if let Some(percent) = percent {
            entry.percent = Some(percent);
        }
        match entry.phase {
            Phase::Waking { .. } => {
                entry.phase = Phase::Linked;
                entry.percent.map(|percent| Note::Awake { percent })
            }
            _ => None,
        }
    }

    /// The service loop's heartbeat: every pending decision whose
    /// window has closed resolves — a drop becomes Idle, a wake speaks
    /// the newest value it has, or, if it never had one, stays quiet.
    pub fn tick(&mut self, now: Instant) -> Vec<(String, Note)> {
        let mut due = Vec::new();
        for (path, entry) in &mut self.pens {
            let (at, waking) = match entry.phase {
                Phase::Dropping { at } => (at, false),
                Phase::Waking { at } => (at, true),
                _ => continue,
            };
            let window = if waking { WAKE_WINDOW } else { DROP_WINDOW };
            if now < at + window {
                continue;
            }
            entry.phase = if waking { Phase::Linked } else { Phase::Idle };
            let note = match (waking, entry.percent) {
                (true, Some(percent)) => Some(Note::Awake { percent }),
                (false, Some(percent)) => Some(Note::Idle { percent }),
                _ => None,
            };
            if let Some(note) = note {
                due.push((path.clone(), note));
            }
        }
        due
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Instant {
        Instant::now()
    }

    fn tracker(present: bool) -> Tracker {
        let mut tracker = Tracker::default();
        tracker.sighted("/p", present);
        tracker
    }

    #[test]
    fn a_set_down_pen_is_idle_once_the_charger_window_passes() {
        let t0 = base();
        let mut tracker = tracker(true);
        assert_eq!(tracker.link_down("/p", Some(84), t0), None);
        assert!(tracker.tick(t0 + Duration::from_millis(2900)).is_empty());
        assert_eq!(
            tracker.tick(t0 + DROP_WINDOW),
            vec![("/p".to_string(), Note::Idle { percent: 84 })]
        );
    }

    #[test]
    fn a_charger_report_before_the_drop_names_it_docked() {
        let t0 = base();
        let mut tracker = tracker(true);
        assert_eq!(
            tracker.docked("/p", Some(84)),
            None,
            "sleep is logged first"
        );
        assert_eq!(
            tracker.link_down("/p", Some(84), t0),
            Some(Note::Docked { percent: 84 })
        );
        assert!(
            tracker.tick(t0 + Duration::from_secs(30)).is_empty(),
            "the passed window cannot turn it Idle afterwards"
        );
    }

    #[test]
    fn a_charger_report_inside_the_window_still_names_it_docked() {
        let t0 = base();
        let mut tracker = tracker(true);
        assert_eq!(tracker.link_down("/p", Some(84), t0), None);
        assert_eq!(
            tracker.docked("/p", Some(84)),
            Some(Note::Docked { percent: 84 })
        );
        assert!(tracker.tick(t0 + Duration::from_secs(30)).is_empty());
    }

    #[test]
    fn waking_waits_for_the_fresh_reading_then_says_only_the_battery() {
        let t0 = base();
        let mut tracker = tracker(true);
        tracker.link_down("/p", Some(84), t0);
        assert_eq!(
            tracker.tick(t0 + DROP_WINDOW),
            vec![("/p".to_string(), Note::Idle { percent: 84 })]
        );
        let back = t0 + Duration::from_secs(60);
        assert_eq!(
            tracker.link_up("/p", Some(84), back),
            None,
            "the stale value is not said twice"
        );
        assert_eq!(
            tracker.battery("/p", Some(83)),
            Some(Note::Awake { percent: 83 })
        );
    }

    #[test]
    fn a_wake_with_no_new_reading_falls_back_after_the_window() {
        let t0 = base();
        let mut tracker = tracker(true);
        tracker.link_down("/p", Some(84), t0);
        tracker.tick(t0 + DROP_WINDOW);
        let back = t0 + Duration::from_secs(30);
        tracker.link_up("/p", Some(84), back);
        assert!(tracker.tick(back + Duration::from_millis(2900)).is_empty());
        assert_eq!(
            tracker.tick(back + WAKE_WINDOW),
            vec![("/p".to_string(), Note::Awake { percent: 84 })]
        );
    }

    #[test]
    fn a_flap_quick_enough_never_becomes_idle() {
        let t0 = base();
        let mut tracker = tracker(true);
        tracker.link_down("/p", Some(84), t0);
        assert_eq!(
            tracker.link_up("/p", Some(84), t0 + Duration::from_millis(500)),
            None
        );
        assert!(
            tracker.tick(t0 + DROP_WINDOW).is_empty(),
            "the pending Idle is gone, and the wake still waits"
        );
        assert_eq!(
            tracker.tick(t0 + Duration::from_millis(3500)),
            vec![("/p".to_string(), Note::Awake { percent: 84 })]
        );
    }

    #[test]
    fn down_up_down_is_one_note_per_change_of_mind() {
        let t0 = base();
        let mut tracker = tracker(true);
        tracker.link_down("/p", Some(84), t0);
        tracker.link_up("/p", Some(84), t0 + Duration::from_millis(500));
        let mut notes: Vec<Note> = tracker
            .tick(t0 + Duration::from_millis(3500))
            .into_iter()
            .map(|(_, note)| note)
            .collect();
        tracker.link_down("/p", Some(84), t0 + Duration::from_millis(3600));
        notes.extend(
            tracker
                .tick(t0 + Duration::from_millis(7000))
                .into_iter()
                .map(|(_, note)| note),
        );
        assert_eq!(
            notes,
            vec![Note::Awake { percent: 84 }, Note::Idle { percent: 84 }]
        );
    }

    #[test]
    fn a_pen_that_was_never_read_stays_silent() {
        let t0 = base();
        let mut tracker = tracker(true);
        tracker.link_down("/p", None, t0);
        assert!(
            tracker.tick(t0 + DROP_WINDOW).is_empty(),
            "an Idle without a number is not a sentence"
        );
        tracker.link_up("/p", None, t0 + Duration::from_secs(10));
        assert!(tracker.tick(t0 + Duration::from_secs(30)).is_empty());
    }

    #[test]
    fn first_sight_of_a_linked_pen_says_nothing() {
        let t0 = base();
        let mut tracker = tracker(true);
        assert_eq!(tracker.battery("/p", Some(90)), None);
        assert!(tracker.tick(t0 + Duration::from_secs(30)).is_empty());
    }

    #[test]
    fn a_known_absent_pen_announces_its_first_wake() {
        let t0 = base();
        let mut tracker = tracker(false);
        assert_eq!(tracker.link_up("/p", Some(80), t0), None);
        assert_eq!(
            tracker.battery("/p", Some(80)),
            Some(Note::Awake { percent: 80 })
        );
    }

    #[test]
    fn a_late_charger_report_never_turns_idle_into_docked() {
        let t0 = base();
        let mut tracker = tracker(true);
        tracker.link_down("/p", Some(84), t0);
        assert_eq!(
            tracker.tick(t0 + DROP_WINDOW),
            vec![("/p".to_string(), Note::Idle { percent: 84 })]
        );
        assert_eq!(tracker.docked("/p", Some(84)), None);
        assert!(tracker.tick(t0 + Duration::from_secs(30)).is_empty());
    }

    #[test]
    fn lifting_a_pen_forgets_the_charger_report() {
        let t0 = base();
        let mut tracker = tracker(true);
        tracker.docked("/p", Some(84));
        tracker.link_down("/p", Some(84), t0);
        let lifted = t0 + Duration::from_secs(60);
        tracker.link_up("/p", Some(84), lifted);
        tracker.battery("/p", Some(84));
        let set_down = lifted + Duration::from_secs(30);
        assert_eq!(
            tracker.link_down("/p", Some(84), set_down),
            None,
            "the next drop waits for a charger report of its own"
        );
        assert_eq!(
            tracker.tick(set_down + DROP_WINDOW),
            vec![("/p".to_string(), Note::Idle { percent: 84 })]
        );
    }
}
