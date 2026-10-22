// SPDX-License-Identifier: AGPL-3.0-or-later
//! While-inking arming: the digitizer's report 7 decides when the pen's
//! continuous texture gets armed and cut (docs/dbus.md §7).
//!
//! Verified on hardware (findings/009 §3): one write of a continuous
//! waveform stays armed across separate tip contacts — the pen itself
//! gates it on the tip switch — and the None ordinal 1 both cuts
//! playback and disarms until the next write. So the daemon arms as
//! soon as the pen comes into hover range, for the end that is
//! hovering (the Invert bit says which), and writes nothing more until
//! the pen leaves range, flips ends or the policy closes. Arming on
//! contact instead left the start of every stroke begun from out of
//! range — and all of a short tap — without texture, waiting for the
//! write to cross Bluetooth.
//!
//! The state machine is pure: digitizer frames in, write actions out,
//! outcomes back in — the whole policy is testable without a pen. The
//! caller (`service.rs`) owns the actual writes and calls `start_tick`
//! once per drain tick; at most one write is attempted per tick, so a
//! pen whose link is still coming up retries at the tick rate instead
//! of hammering the HID stack at the digitizer's report rate.

/// Which end of the pen is making contact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Tip,
    Eraser,
}

/// What the drain loop should write to the pen, if anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// No write.
    Nothing,
    /// Arm this continuous ordinal for this tool.
    Arm { tool: Tool, ordinal: u8 },
    /// Cut and disarm: the None ordinal 1 (findings/009 §3).
    Disarm,
}

/// Report 7 button byte: pen in range.
pub(crate) const IN_RANGE: u8 = 0x01;
/// Report 7 button byte: tip touching.
#[cfg(test)]
const TIP_CONTACT: u8 = 0x02;
/// Report 7 button byte: Invert, the eraser end is the one in range
/// (hovering 0x09 measured on hardware).
const INVERT: u8 = 0x08;
/// Report 7 button byte: eraser end touching.
const ERASER_CONTACT: u8 = 0x20;

/// Which end is in range on a report 7 button byte: the eraser end
/// when Invert or eraser contact is set, the tip otherwise; `None` out
/// of range.
fn tool_of(buttons: u8) -> Option<Tool> {
    if buttons & IN_RANGE == 0 {
        None
    } else if buttons & (INVERT | ERASER_CONTACT) != 0 {
        Some(Tool::Eraser)
    } else {
        Some(Tool::Tip)
    }
}

/// The arming state for the pen the daemon drives.
#[derive(Default)]
pub struct Arming {
    /// The ordinal armed on the pen, with the tool that armed it.
    armed: Option<(Tool, u8)>,
    /// The end in range on the last report 7 seen.
    in_contact: Option<Tool>,
    /// A write was already attempted this tick.
    attempted: bool,
}

impl Arming {
    /// Called once per drain tick before the frames: re-arms the
    /// once-per-tick write budget.
    pub fn start_tick(&mut self) {
        self.attempted = false;
    }

    /// Feed one digitizer frame. `ordinal_for` answers which ordinal
    /// the policy wants armed for a tool right now — `None` means the
    /// policy is closed for it (master off, `WhileInking` off, or the
    /// texture unresolvable), which disarms an armed pen.
    pub fn on_frame(&mut self, frame: &[u8], ordinal_for: impl Fn(Tool) -> Option<u8>) -> Action {
        if frame.is_empty() || frame[0] != 0x07 {
            return Action::Nothing;
        }
        let in_range = frame.len() >= 2 && frame[1] & IN_RANGE != 0;
        self.in_contact = if in_range { tool_of(frame[1]) } else { None };
        if self.attempted {
            return Action::Nothing;
        }
        let desired = if !in_range {
            self.armed.as_ref().map(|_| Action::Disarm)
        } else if let Some((tool, armed)) = self.armed {
            // The end in range decides: flipping to the other end
            // re-arms for its texture, and a texture change or a
            // policy close lands on the next report.
            let tool = self.in_contact.unwrap_or(tool);
            match ordinal_for(tool) {
                None => Some(Action::Disarm),
                Some(want) if want != armed => Some(Action::Arm {
                    tool,
                    ordinal: want,
                }),
                Some(_) => None,
            }
        } else {
            self.in_contact
                .and_then(|tool| ordinal_for(tool).map(|ordinal| Action::Arm { tool, ordinal }))
        };
        match desired {
            Some(action) => {
                self.attempted = true;
                action
            }
            None => Action::Nothing,
        }
    }

    /// Report the outcome of the action returned for this tick. A
    /// failed write leaves the state untouched, so the next tick
    /// re-decides — that is the retry path for a pen whose BLE link
    /// is still coming up while its tip already touches.
    pub fn on_done(&mut self, action: &Action, ok: bool) {
        if !ok {
            return;
        }
        match action {
            Action::Arm { tool, ordinal } => self.armed = Some((*tool, *ordinal)),
            Action::Disarm => self.armed = None,
            Action::Nothing => {}
        }
    }
}

/// Hover-out edge detector, for the opt-in hover tick. The ticks seen
/// at Windows' hover-stroke ends (findings/007 §4) land as the pen
/// leaves range — two back-to-back strokes with the pen staying in
/// range produced none — so this answers true on the one report 7
/// frame that takes the pen from in range to out. It starts cold: a
/// daemon that boots while a pen hovers has not seen a stroke and
/// never ticks on the first leave.
#[derive(Default)]
pub struct HoverOut {
    in_range: bool,
}

impl HoverOut {
    /// Feed one digitizer frame; true exactly on the frame that ends
    /// the hover, false on every other.
    pub fn on_frame(&mut self, frame: &[u8]) -> bool {
        if frame.is_empty() || frame[0] != 0x07 {
            return false;
        }
        let in_range = frame.len() >= 2 && frame[1] & IN_RANGE != 0;
        let left = self.in_range && !in_range;
        self.in_range = in_range;
        left
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Frames as the digitizer sends them: report id 7 plus the button
    /// byte (docs/protocol/digitizer-tlc.md report 7).
    fn frame(buttons: u8) -> Vec<u8> {
        vec![0x07, buttons]
    }

    const HOVER: u8 = IN_RANGE;
    const TIP: u8 = IN_RANGE | TIP_CONTACT;
    const ERASER: u8 = IN_RANGE | ERASER_CONTACT;

    /// The SA205H shape: tip arms 9, eraser always arms 14.
    fn sa205h(tool: Tool) -> Option<u8> {
        match tool {
            Tool::Tip => Some(9),
            Tool::Eraser => Some(14),
        }
    }

    /// Drive one tick: start, feed the frames, return the actions.
    fn tick(arming: &mut Arming, frames: &[Vec<u8>]) -> Vec<Action> {
        arming.start_tick();
        frames
            .iter()
            .map(|frame| arming.on_frame(frame, sa205h))
            .collect()
    }

    #[test]
    fn hovering_arms_before_the_tip_lands() {
        let mut arming = Arming::default();
        assert_eq!(
            tick(&mut arming, &[frame(HOVER)]),
            vec![Action::Arm {
                tool: Tool::Tip,
                ordinal: 9
            }]
        );
        arming.on_done(
            &Action::Arm {
                tool: Tool::Tip,
                ordinal: 9,
            },
            true,
        );
        assert_eq!(
            tick(&mut arming, &[frame(TIP)]),
            vec![Action::Nothing],
            "the touch finds the texture already armed"
        );
    }

    #[test]
    fn the_eraser_end_hovering_arms_the_eraser_texture() {
        let mut arming = Arming::default();
        assert_eq!(
            tick(&mut arming, &[frame(IN_RANGE | INVERT)]),
            vec![Action::Arm {
                tool: Tool::Eraser,
                ordinal: 14
            }]
        );
    }

    #[test]
    fn tip_contact_arms_once_and_stays_armed() {
        let mut arming = Arming::default();
        assert_eq!(
            tick(&mut arming, &[frame(TIP)]),
            vec![Action::Arm {
                tool: Tool::Tip,
                ordinal: 9
            }]
        );
        arming.on_done(
            &Action::Arm {
                tool: Tool::Tip,
                ordinal: 9,
            },
            true,
        );
        // The verified behaviour: separate strokes need no re-arm.
        assert_eq!(
            tick(&mut arming, &[frame(HOVER), frame(TIP), frame(TIP)]),
            vec![Action::Nothing, Action::Nothing, Action::Nothing]
        );
    }

    #[test]
    fn leaving_range_disarms() {
        let mut arming = Arming::default();
        tick(&mut arming, &[frame(TIP)]);
        arming.on_done(
            &Action::Arm {
                tool: Tool::Tip,
                ordinal: 9,
            },
            true,
        );
        assert_eq!(tick(&mut arming, &[frame(0x00)]), vec![Action::Disarm]);
        arming.on_done(&Action::Disarm, true);
        assert_eq!(arming.armed, None);
        assert_eq!(tick(&mut arming, &[frame(0x00)]), vec![Action::Nothing]);
    }

    #[test]
    fn a_failed_write_retries_next_tick() {
        let mut arming = Arming::default();
        assert_eq!(
            tick(&mut arming, &[frame(TIP)]),
            vec![Action::Arm {
                tool: Tool::Tip,
                ordinal: 9
            }]
        );
        arming.on_done(
            &Action::Arm {
                tool: Tool::Tip,
                ordinal: 9,
            },
            false,
        );
        // The link is still coming up: the next tick tries again while
        // the contact holds, and only once per tick.
        assert_eq!(
            tick(&mut arming, &[frame(TIP), frame(TIP)]),
            vec![
                Action::Arm {
                    tool: Tool::Tip,
                    ordinal: 9
                },
                Action::Nothing
            ]
        );
    }

    #[test]
    fn eraser_contact_arms_the_eraser_texture() {
        let mut arming = Arming::default();
        assert_eq!(
            tick(&mut arming, &[frame(ERASER)]),
            vec![Action::Arm {
                tool: Tool::Eraser,
                ordinal: 14
            }]
        );
    }

    #[test]
    fn switching_ends_re_arms() {
        let mut arming = Arming::default();
        tick(&mut arming, &[frame(TIP)]);
        arming.on_done(
            &Action::Arm {
                tool: Tool::Tip,
                ordinal: 9,
            },
            true,
        );
        assert_eq!(
            tick(&mut arming, &[frame(ERASER)]),
            vec![Action::Arm {
                tool: Tool::Eraser,
                ordinal: 14
            }]
        );
    }

    #[test]
    fn the_policy_closing_disarms_an_armed_pen() {
        let mut arming = Arming::default();
        tick(&mut arming, &[frame(TIP)]);
        arming.on_done(
            &Action::Arm {
                tool: Tool::Tip,
                ordinal: 9,
            },
            true,
        );
        // WhileInking (or the master switch) just went off.
        arming.start_tick();
        assert_eq!(
            arming.on_frame(&frame(TIP), |_| None),
            Action::Disarm,
            "an armed pen must go quiet the moment the policy closes"
        );
    }

    #[test]
    fn a_texture_change_re_arms_the_new_texture() {
        let mut arming = Arming::default();
        tick(&mut arming, &[frame(TIP)]);
        arming.on_done(
            &Action::Arm {
                tool: Tool::Tip,
                ordinal: 9,
            },
            true,
        );
        arming.start_tick();
        assert_eq!(
            arming.on_frame(&frame(HOVER), |_| Some(13)),
            Action::Arm {
                tool: Tool::Tip,
                ordinal: 13
            },
            "picking a new texture takes effect without waiting for a stroke"
        );
    }

    #[test]
    fn non_pen_frames_are_ignored() {
        let mut arming = Arming::default();
        arming.start_tick();
        assert_eq!(arming.on_frame(&[], sa205h), Action::Nothing);
        assert_eq!(
            arming.on_frame(&[0x6e, 0x01, 0x02], sa205h),
            Action::Nothing,
            "pairing announcements are not pen reports"
        );
    }

    #[test]
    fn hover_out_fires_once_when_the_pen_leaves_range() {
        let mut hover = HoverOut::default();
        assert!(
            !hover.on_frame(&frame(HOVER)),
            "entering range is not leaving it"
        );
        assert!(
            hover.on_frame(&frame(0x00)),
            "the stroke ends as the pen lifts out"
        );
        assert!(
            !hover.on_frame(&frame(0x00)),
            "one tick per stroke, not one per frame"
        );
        assert!(
            !hover.on_frame(&frame(TIP)),
            "contact is not a hover-out either"
        );
        assert!(hover.on_frame(&frame(0x00)));
    }

    #[test]
    fn a_pen_already_hovering_at_start_gets_no_tick() {
        // Cold start: the daemon never saw the stroke, so the first
        // leave must be silent.
        let mut hover = HoverOut::default();
        assert!(!hover.on_frame(&frame(0x00)));
        assert!(!hover.on_frame(&frame(0x00)));
    }

    #[test]
    fn hover_out_ignores_other_reports_and_keeps_its_edge() {
        let mut hover = HoverOut::default();
        hover.on_frame(&frame(HOVER));
        assert!(
            !hover.on_frame(&[0x6e, 0x01, 0x02]),
            "other reports are invisible to it"
        );
        assert!(
            hover.on_frame(&frame(0x00)),
            "the edge survives frames of other reports"
        );
    }
}
