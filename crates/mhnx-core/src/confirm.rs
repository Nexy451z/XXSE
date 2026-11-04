//! The two shapes of "this action needs a second answer".
//!
//! Both live here rather than in the console UI so that the rule itself - how
//! long a key has to be held, how many presses a destructive button takes - can
//! be tested without a console. The UI only has to ask.
//!
//! The two are deliberately separate: a press is released between the two
//! answers, a hold is not, and mixing them was how one 0.5 second hold came to
//! throw away a whole save with no way back.

/// A destructive action that runs on the second identical press.
///
/// The first press only arms it and reports false, so the caller can say what
/// the second press would do. Any other action should call [ConfirmPress::clear],
/// otherwise the offer would outlive the thing it was offered for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ConfirmPress {
    armed: bool,
}

impl ConfirmPress {
    pub const fn new() -> Self {
        ConfirmPress { armed: false }
    }

    /// Is the next press the one that runs the action?
    pub const fn is_armed(&self) -> bool {
        self.armed
    }

    /// Take the offer away without running anything.
    pub fn clear(&mut self) {
        self.armed = false;
    }

    /// One press. True means "run it now", false means "this press only asked".
    pub fn press(&mut self) -> bool {
        if self.armed {
            self.armed = false;
            true
        } else {
            self.armed = true;
            false
        }
    }
}

/// What a held key did on the frame it crossed the threshold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoldAction {
    /// Still too short to mean anything.
    None,
    /// Long enough: the action is offered, not run.
    Arm,
    /// Long enough, and it had already been offered once: run it now.
    Run,
}

/// How long a key must be held before it means the destructive action.
///
/// One second at the ~16ms frame the console loop sleeps for: the two-step undo
/// is only reached by somebody who meant to hold the key, and the half second
/// it used to take was inside the range of a normal press that lingered.
pub const HOLD_CONFIRM_FRAMES: u32 = 60;

/// A key that must be held, released, and held again before it runs.
///
/// The frame counter is per hold ([HoldConfirm::press] starts one), the arm is
/// not: that is what makes the second hold different from the first.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HoldConfirm {
    frames: u32,
    fired: bool,
    armed: bool,
}

impl HoldConfirm {
    pub const fn new() -> Self {
        HoldConfirm {
            frames: 0,
            fired: false,
            armed: false,
        }
    }

    /// A new press of the key begins.
    pub fn press(&mut self) {
        self.frames = 0;
        self.fired = false;
    }

    /// One frame with the key still down. Reports at most once per hold.
    pub fn tick(&mut self) -> HoldAction {
        if self.fired {
            return HoldAction::None;
        }
        self.frames += 1;
        if self.frames < HOLD_CONFIRM_FRAMES {
            return HoldAction::None;
        }
        self.fired = true;
        if self.armed {
            self.armed = false;
            HoldAction::Run
        } else {
            self.armed = true;
            HoldAction::Arm
        }
    }

    /// Did the hold already reach the threshold?
    pub const fn fired(&self) -> bool {
        self.fired
    }

    /// Is the action offered, waiting for the second hold?
    pub const fn is_armed(&self) -> bool {
        self.armed
    }

    /// Take the offer away (the user did something else instead).
    pub fn clear_arm(&mut self) {
        self.armed = false;
    }

    /// The key came up. True means it was a plain tap: too short to have fired.
    pub fn release(&mut self) -> bool {
        let tap = !self.fired;
        self.frames = 0;
        self.fired = false;
        tap
    }
}

/// Does this frame need the full-screen hit that stops taps which miss a panel?
///
/// The keyboard and the MIB picker cover most of the frame but not all of it:
/// the header band above them and the footer band below them stay live, and the
/// footer's undo pill sits in that band. A tap there used to run a whole-save
/// undo with no confirmation. While any of the three panels is up the frame
/// carries one hit that covers everything; the panel's own targets are pushed
/// after it, so a tap that lands on the panel still reaches it.
pub const fn needs_modal_guard(mib_picker: bool, osk: bool, review: bool) -> bool {
    mib_picker || osk || review
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_press_only_asks() {
        let mut c = ConfirmPress::new();
        assert!(!c.press());
        assert!(c.is_armed());
        assert!(c.press());
        assert!(!c.is_armed());
        assert!(!c.press(), "a confirmed action asks again from scratch");
    }

    #[test]
    fn clearing_takes_the_offer_away() {
        let mut c = ConfirmPress::new();
        assert!(!c.press());
        c.clear();
        assert!(!c.press());
    }

    #[test]
    fn a_short_hold_is_a_tap() {
        let mut h = HoldConfirm::new();
        h.press();
        for _ in 0..(HOLD_CONFIRM_FRAMES - 1) {
            assert_eq!(h.tick(), HoldAction::None);
        }
        assert!(h.release());
        assert!(!h.is_armed());
    }

    #[test]
    fn the_first_long_hold_only_arms() {
        let mut h = HoldConfirm::new();
        h.press();
        let mut last = HoldAction::None;
        for _ in 0..HOLD_CONFIRM_FRAMES {
            last = h.tick();
        }
        assert_eq!(last, HoldAction::Arm);
        assert!(h.is_armed());
        assert!(!h.release(), "a hold is not also a tap");
    }

    #[test]
    fn the_second_long_hold_runs() {
        let mut h = HoldConfirm::new();
        for expected in [HoldAction::Arm, HoldAction::Run] {
            h.press();
            let mut last = HoldAction::None;
            for _ in 0..HOLD_CONFIRM_FRAMES {
                last = h.tick();
            }
            assert_eq!(last, expected);
            let _ = h.release();
        }
    }

    #[test]
    fn holding_forever_reports_the_threshold_once() {
        let mut h = HoldConfirm::new();
        h.press();
        for _ in 0..HOLD_CONFIRM_FRAMES {
            h.tick();
        }
        for _ in 0..120 {
            assert_eq!(h.tick(), HoldAction::None);
        }
    }

    #[test]
    fn another_action_does_not_leave_the_undo_armed() {
        let mut h = HoldConfirm::new();
        h.press();
        for _ in 0..HOLD_CONFIRM_FRAMES {
            h.tick();
        }
        h.clear_arm();
        let _ = h.release();
        h.press();
        let mut last = HoldAction::None;
        for _ in 0..HOLD_CONFIRM_FRAMES {
            last = h.tick();
        }
        assert_eq!(last, HoldAction::Arm, "it has to be offered again");
    }
}
