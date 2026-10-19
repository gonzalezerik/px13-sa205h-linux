// SPDX-License-Identifier: AGPL-3.0-or-later
//! While-inking arming: the digitizer's report 7 decides when the pen's
//! continuous texture gets armed and cut (docs/dbus.md §7).
//!
//! Verified on hardware (findings/009 §3): one write of a continuous
//! waveform stays armed across separate tip contacts — the pen itself
//! gates it on the tip switch — and the None ordinal 1 both cuts
//! playback and disarms until the next write. So the daemon arms once
//! per tool activation and writes nothing until the pen leaves range
//! or the policy closes.
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
const IN_RANGE: u8 = 0x01;
/// Report 7 button byte: tip touching.
const TIP_CONTACT: u8 = 0x02;
/// Report 7 button byte: eraser end touching (the pen inverted).
const ERASER_CONTACT: u8 = 0x20;

/// Which end is touching on a report 7 button byte; the eraser wins
/// if a firmware ever sets both.
fn tool_of(buttons: u8) -> Option<Tool> {
    if buttons & ERASER_CONTACT != 0 {
        Some(Tool::Eraser)
    } else if buttons & TIP_CONTACT != 0 {
        Some(Tool::Tip)
    } else {
        None
    }
}

/// The arming state for the pen the daemon drives.
#[derive(Default)]
pub struct Arming {
    /// The ordinal armed on the pen, with the tool that armed it.
    armed: Option<(Tool, u8)>,
    /// The tool in contact on the last report 7 seen.
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
            // The end in contact decides: flipping to the other end
            // re-arms for its texture. Hovering keeps the armed tool's
            // texture current, so a texture change or a policy close
            // lands on the next report.
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
    fn hovering_arms_nothing() {
        let mut arming = Arming::default();
        assert_eq!(tick(&mut arming, &[frame(HOVER)]), vec![Action::Nothing]);
        assert_eq!(arming.armed, None);
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
}
