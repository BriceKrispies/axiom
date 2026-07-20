//! The lab's animation catalog: every [`AnimState`] the player figure can
//! strike, in a fixed order, each paired with a short label, a drive script
//! ([`Path`]) that says how the isolated actor moves while the clip plays, and
//! how many ticks before an in-place override pose replays.
//!
//! The drive scripts exist so the *real* locomotion animator sees the same
//! thing it sees in a game: the running poses only animate when the actor
//! actually travels (the gait advances on resolved displacement), so the
//! moving clips run a continuous path and the still clips let a tick-driven
//! override pose play out and loop.

use crate::player::AnimState;

/// How the lab actor moves while a clip plays.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Path {
    /// Stationary. Idle/ready stances settle in place; the action, hit, and
    /// fall overrides pose themselves from their tick counter and loop.
    Still,
    /// Runs a large, gentle circle at `speed` yd/s with the given `radius` yd.
    /// A continuous loop keeps the actor framed with no wrap cut while the
    /// gait advances on real displacement — the honest way to see foot-plant
    /// (or foot-skate) against the moving turf.
    Circle { speed: f32, radius: f32 },
    /// Straight backpedal at `speed` yd/s for `reach` yд, then re-anchors the
    /// feet and repeats — the quarterback drop-back.
    Backpedal { speed: f32, reach: f32 },
}

/// One selectable animation in the lab.
#[derive(Debug, Clone, Copy)]
pub struct LabClip {
    /// The authoritative animation state fed to the real animator.
    pub anim: AnimState,
    /// The short label shown on the lab's picker button.
    pub label: &'static str,
    /// How the actor moves while this clip plays.
    pub path: Path,
    /// Ticks before an in-place override pose restarts (`0` = never loops /
    /// not an override state). Ignored for the moving clips.
    pub loop_ticks: u32,
}

const fn clip(anim: AnimState, label: &'static str, path: Path, loop_ticks: u32) -> LabClip {
    LabClip {
        anim,
        label,
        path,
        loop_ticks,
    }
}

/// The full ordered catalog: the five holdable/locomotion states first (idle,
/// stances, and the moving clips), then every action / hit / fall override.
pub fn catalog() -> Vec<LabClip> {
    vec![
        clip(AnimState::Idle, "Idle", Path::Still, 0),
        clip(AnimState::ReadyStance, "Ready Stance", Path::Still, 0),
        clip(
            AnimState::Jog,
            "Jog",
            Path::Circle {
                speed: 4.6,
                radius: 15.0,
            },
            0,
        ),
        clip(
            AnimState::Sprint,
            "Sprint",
            Path::Circle {
                speed: 8.4,
                radius: 26.0,
            },
            0,
        ),
        clip(
            AnimState::DropBack,
            "Drop Back",
            Path::Backpedal {
                speed: 3.0,
                reach: 7.0,
            },
            0,
        ),
        clip(AnimState::Throw, "Throw", Path::Still, 72),
        clip(AnimState::Catch, "Catch", Path::Still, 72),
        clip(AnimState::Block, "Block", Path::Still, 96),
        clip(AnimState::Tackle, "Tackle", Path::Still, 72),
        clip(AnimState::Dive, "Dive", Path::Still, 96),
        clip(AnimState::HitReaction, "Hit Reaction", Path::Still, 54),
        clip(AnimState::Stumble, "Stumble", Path::Still, 72),
        clip(AnimState::AirborneFall, "Airborne Fall", Path::Still, 84),
        clip(AnimState::GroundImpact, "Ground Impact", Path::Still, 84),
        clip(AnimState::Recovery, "Recovery", Path::Still, 96),
    ]
}
