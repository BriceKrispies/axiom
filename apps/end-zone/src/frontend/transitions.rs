//! The frontend transition system: screens declare transition intents; this
//! module executes them with bounded, deterministic progress from the
//! frontend tick — completing at exactly `1.0` — and swaps every large
//! movement for a short fade under reduced motion.

use crate::frontend::state::Screen;

/// The transition vocabulary screens may declare.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionKind {
    /// Full-screen fade.
    Fade,
    /// Horizontal panel wipe (the arcade slab sweep).
    Wipe,
    /// Angled panel slide (menu-to-menu).
    AngledSlide,
    /// Scale impact (into gameplay).
    ScaleImpact,
}

/// Full-motion durations, in frontend ticks.
fn full_duration(kind: TransitionKind) -> u32 {
    match kind {
        TransitionKind::Fade => 18,
        TransitionKind::Wipe => 26,
        TransitionKind::AngledSlide => 22,
        TransitionKind::ScaleImpact => 30,
    }
}

/// One running transition between two screens.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActiveTransition {
    pub kind: TransitionKind,
    pub from: Screen,
    pub to: Screen,
    pub tick: u32,
    pub duration: u32,
}

impl ActiveTransition {
    /// Start a transition; reduced motion replaces every kind with a short
    /// fade (large sweeps and zoom impacts are removed, feedback stays).
    pub fn start(kind: TransitionKind, from: Screen, to: Screen, reduced_motion: bool) -> Self {
        let (kind, duration) = if reduced_motion {
            (TransitionKind::Fade, 8)
        } else {
            (kind, full_duration(kind))
        };
        ActiveTransition {
            kind,
            from,
            to,
            tick: 0,
            duration: duration.max(1),
        }
    }

    /// Advance one tick; returns `true` while still running.
    pub fn advance(&mut self) -> bool {
        self.tick = self.tick.saturating_add(1);
        self.tick < self.duration
    }

    /// Progress in `0.0..=1.0`, exactly `1.0` on the final tick.
    pub fn progress(&self) -> f32 {
        (self.tick as f32 / self.duration as f32).clamp(0.0, 1.0)
    }

    pub fn finished(&self) -> bool {
        self.tick >= self.duration
    }
}

/// The presenter-facing view of a transition.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransitionView {
    pub kind: TransitionKind,
    pub progress: f32,
}
