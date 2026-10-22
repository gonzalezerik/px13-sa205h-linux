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
//! A pen coming back says its battery and nothing else, the moment the
//! link is up, with the newest value it has; if a fresh reading lands
//! within a few seconds and differs, the same note is updated in place
//! (the daemon replaces it by id). A pen back before its drop was even
//! named was only shifted in the hand and says nothing. A pen that was
//! never read stays quiet until its first reading: a note with no
//! number in it is not a sentence.
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

/// How long after a reconnect a fresh reading still corrects the wake
/// note; after it, readings are routine and say nothing.
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
    /// Linked again; a fresh reading inside the window may still
    /// correct the wake note. `said` is the percent already shown.
    Waking {
        at: Instant,
        said: Option<u8>,
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

    /// The link is back. A pen whose drop was named (Idle or Docked)
    /// says its battery at once; a drop not yet named is cancelled
    /// silently, so a short flap sends neither Idle nor Awake.
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
            Phase::Dropping { .. } => {
                entry.phase = Phase::Linked;
                None
            }
            Phase::Idle | Phase::Docked => {
                let said = entry.percent;
                entry.phase = Phase::Waking { at: now, said };
                said.map(|percent| Note::Awake { percent })
            }
        }
    }

    /// A fresh battery reading. Inside the wake window it corrects the
    /// wake note when the number changed — or is the wake note, for a
    /// pen that had no number to say yet.
    pub fn battery(&mut self, path: &str, percent: Option<u8>) -> Option<Note> {
        let entry = self.pens.get_mut(path)?;
        if let Some(percent) = percent {
            entry.percent = Some(percent);
        }
        match entry.phase {
            Phase::Waking { said, .. } if entry.percent.is_some() && entry.percent != said => {
                entry.phase = Phase::Linked;
                entry.percent.map(|percent| Note::Awake { percent })
            }
            _ => None,
        }
    }

    /// The service loop's heartbeat: every pending decision whose
    /// window has closed resolves — a drop becomes Idle (said only with
    /// a number), and a wake window simply closes.
    pub fn tick(&mut self, now: Instant) -> Vec<(String, Note)> {
        let mut due = Vec::new();
        for (path, entry) in &mut self.pens {
            match entry.phase {
                Phase::Dropping { at } if now >= at + DROP_WINDOW => {
                    entry.phase = Phase::Idle;
                    if let Some(percent) = entry.percent {
                        due.push((path.clone(), Note::Idle { percent }));
                    }
                }
                Phase::Waking { at, .. } if now >= at + WAKE_WINDOW => {
                    entry.phase = Phase::Linked;
                }
                _ => {}
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
    fn waking_says_the_battery_at_once_and_corrects_it_in_place() {
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
            Some(Note::Awake { percent: 84 }),
            "no waiting: the newest value speaks the moment the link is up"
        );
        assert_eq!(
            tracker.battery("/p", Some(83)),
            Some(Note::Awake { percent: 83 }),
            "a fresh reading that differs corrects the note"
        );
        assert_eq!(tracker.battery("/p", Some(82)), None, "only once");
    }

    #[test]
    fn a_fresh_reading_with_the_same_number_says_nothing_more() {
        let t0 = base();
        let mut tracker = tracker(true);
        tracker.link_down("/p", Some(84), t0);
        tracker.tick(t0 + DROP_WINDOW);
        let back = t0 + Duration::from_secs(30);
        tracker.link_up("/p", Some(84), back);
        assert_eq!(tracker.battery("/p", Some(84)), None);
        assert!(tracker.tick(back + WAKE_WINDOW).is_empty());
    }

    #[test]
    fn a_reading_after_the_wake_window_is_routine() {
        let t0 = base();
        let mut tracker = tracker(true);
        tracker.link_down("/p", Some(84), t0);
        tracker.tick(t0 + DROP_WINDOW);
        let back = t0 + Duration::from_secs(30);
        tracker.link_up("/p", Some(84), back);
        assert!(tracker.tick(back + WAKE_WINDOW).is_empty());
        assert_eq!(tracker.battery("/p", Some(80)), None);
    }

    #[test]
    fn a_flap_quick_enough_says_nothing_at_all() {
        let t0 = base();
        let mut tracker = tracker(true);
        tracker.link_down("/p", Some(84), t0);
        assert_eq!(
            tracker.link_up("/p", Some(84), t0 + Duration::from_millis(500)),
            None
        );
        assert!(
            tracker.tick(t0 + DROP_WINDOW).is_empty(),
            "the pending Idle is gone"
        );
        assert_eq!(tracker.battery("/p", Some(83)), None);
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
        assert_eq!(notes, vec![Note::Idle { percent: 84 }]);
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
        assert_eq!(
            tracker.link_up("/p", None, t0 + Duration::from_secs(10)),
            None
        );
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
        assert_eq!(
            tracker.link_up("/p", None, t0),
            None,
            "nothing to say before the first reading"
        );
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
