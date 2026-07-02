//! Pass 10 — deterministic impact polish (net wobble, post/crossbar shake, save
//! impact + fake deflection, miss drift, crowd reaction, camera juice, result
//! banner, score popup).
//!
//! Every effect is a pure function of `(result, effect_tick, final ball pose,
//! award)` — fixed constants and small lookup tables, no wall-clock, no
//! randomness, no frame-duration dependence, no trig dependency. This is **not**
//! physics, **not** ragdoll, and **not** a particle/effects engine: it produces
//! app-local *descriptors* the app turns into extra render items and offsets.

use axiom_math::Vec3;

use crate::soccer_penalty::penalty_result::{PenaltyShotResult, PenaltyShotResultDetail, PenaltyShotResultKind};
use crate::soccer_penalty::penalty_scene::{GOAL_HALF_WIDTH, GOAL_HEIGHT, GOAL_LINE_Z, GROUND_Y, NET_DEPTH};

// --- fixed durations --------------------------------------------------------

pub const GOAL_TICKS: u32 = 72;
pub const SAVE_TICKS: u32 = 54;
pub const POST_TICKS: u32 = 54;
pub const MISS_TICKS: u32 = 42;
pub const SESSION_COMPLETE_TICKS: u32 = 90;

// --- fixed effect constants -------------------------------------------------

pub const NET_WOBBLE_COLS: u32 = 5;
pub const NET_WOBBLE_ROWS: u32 = 4;
pub const WOBBLE_AMPLITUDE: f32 = 0.35;
pub const DEFLECT_TICKS: u32 = 20;
pub const CAMERA_SHAKE_TICKS: u32 = 16;
pub const POP_TICKS: u32 = 12;

// Fixed oscillation / bounce / shake tables (no trig dependency).
const OSC_LUT: [i32; 8] = [0, 7, 10, 7, 0, -7, -10, -7];
const BOUNCE_LUT: [i32; 8] = [0, 6, 10, 9, 6, 3, 1, 0];
const SHAKE_LUT: [i32; 4] = [10, -8, 6, -4];

fn osc(i: u32) -> f32 {
    OSC_LUT[(i % 8) as usize] as f32 / 10.0
}
fn bounce(i: u32) -> f32 {
    BOUNCE_LUT[(i % 8) as usize] as f32 / 10.0
}
fn shake(i: u32) -> f32 {
    SHAKE_LUT[(i % 4) as usize] as f32 / 100.0
}
fn decay(tick: u32, span: u32) -> f32 {
    if tick < span { (span - tick) as f32 / span as f32 } else { 0.0 }
}
fn lerp(a: Vec3, b: Vec3, t: f32) -> Vec3 {
    a.add(b.subtract(a).mul_scalar(t))
}

// --- effect kind + timeline -------------------------------------------------

/// Which impact effect is playing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PenaltyImpactEffectKind {
    Goal,
    Save,
    Post,
    Miss,
    SessionComplete,
}

impl PenaltyImpactEffectKind {
    /// The fixed effect duration in ticks.
    pub fn duration(self) -> u32 {
        match self {
            PenaltyImpactEffectKind::Goal => GOAL_TICKS,
            PenaltyImpactEffectKind::Save => SAVE_TICKS,
            PenaltyImpactEffectKind::Post => POST_TICKS,
            PenaltyImpactEffectKind::Miss => MISS_TICKS,
            PenaltyImpactEffectKind::SessionComplete => SESSION_COMPLETE_TICKS,
        }
    }

    /// The effect kind for a resolved shot result.
    pub fn from_result(kind: PenaltyShotResultKind) -> Self {
        match kind {
            PenaltyShotResultKind::Goal => PenaltyImpactEffectKind::Goal,
            PenaltyShotResultKind::Save => PenaltyImpactEffectKind::Save,
            PenaltyShotResultKind::Post => PenaltyImpactEffectKind::Post,
            PenaltyShotResultKind::Miss => PenaltyImpactEffectKind::Miss,
        }
    }

    /// The banner text for this effect.
    pub fn banner_text(self) -> &'static str {
        match self {
            PenaltyImpactEffectKind::Goal => "GOAL",
            PenaltyImpactEffectKind::Save => "SAVE",
            PenaltyImpactEffectKind::Post => "POST",
            PenaltyImpactEffectKind::Miss => "MISS",
            PenaltyImpactEffectKind::SessionComplete => "FINAL SCORE",
        }
    }
}

/// The fixed timeline of an effect kind (duration + normalized progress).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PenaltyImpactEffectTimeline {
    pub kind: PenaltyImpactEffectKind,
}

impl PenaltyImpactEffectTimeline {
    pub fn new(kind: PenaltyImpactEffectKind) -> Self {
        Self { kind }
    }

    pub fn duration(&self) -> u32 {
        self.kind.duration()
    }

    /// Normalized progress `0..=1000` at a local tick (clamped).
    pub fn progress(&self, tick: u32) -> u32 {
        tick.min(self.duration()) * 1000 / self.duration()
    }
}

/// The live impact-effect state for one resolved shot (or a completed session).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenaltyImpactEffectState {
    pub kind: PenaltyImpactEffectKind,
    pub tick: u32,
    /// The impact point (the frozen final ball pose / contact point).
    pub impact: Vec3,
    pub detail: PenaltyShotResultDetail,
    pub award_total: u32,
    /// For `SessionComplete`: whether to celebrate (final score > 0).
    pub celebrate: bool,
}

impl PenaltyImpactEffectState {
    /// The effect for a resolved shot.
    pub fn for_result(result: PenaltyShotResult, impact: Vec3, award_total: u32) -> Self {
        Self {
            kind: PenaltyImpactEffectKind::from_result(result.kind),
            tick: 0,
            impact,
            detail: result.detail,
            award_total,
            celebrate: false,
        }
    }

    /// The effect for a completed session.
    pub fn session_complete(final_score: u32) -> Self {
        Self {
            kind: PenaltyImpactEffectKind::SessionComplete,
            tick: 0,
            impact: Vec3::ZERO,
            detail: PenaltyShotResultDetail::Scored,
            award_total: final_score,
            celebrate: final_score > 0,
        }
    }

    /// The fixed timeline for this effect.
    pub fn timeline(&self) -> PenaltyImpactEffectTimeline {
        PenaltyImpactEffectTimeline::new(self.kind)
    }

    pub fn duration(&self) -> u32 {
        self.kind.duration()
    }

    pub fn progress(&self) -> u32 {
        self.timeline().progress(self.tick)
    }

    /// Advance the effect one tick.
    pub fn advanced(self) -> Self {
        Self { tick: self.tick + 1, ..self }
    }

    /// Build the full deterministic descriptor bundle for the current tick.
    pub fn describe(&self) -> PenaltyEffectDescriptor {
        PenaltyEffectDescriptor {
            kind: self.kind,
            tick: self.tick,
            progress: self.progress(),
            net_wobble: self.net_wobble(),
            frame_shake: self.frame_shake(),
            ball_deflection: self.ball_deflection(),
            crowd: self.crowd(),
            camera: self.camera(),
            banner: self.banner(),
            score_popup: self.score_popup(),
            foreground: self.foreground(),
        }
    }

    // --- net wobble (Goal only) ---------------------------------------------

    fn net_wobble(&self) -> Option<PenaltyNetWobble> {
        (self.kind == PenaltyImpactEffectKind::Goal).then(|| PenaltyNetWobble {
            rear: wobble_panel(self.impact, self.tick, GOAL_LINE_Z - NET_DEPTH),
            front: wobble_panel(self.impact, self.tick, GOAL_LINE_Z),
        })
    }

    // --- goal-frame shake (Post only) ---------------------------------------

    fn frame_shake(&self) -> Option<PenaltyGoalFrameShake> {
        (self.kind == PenaltyImpactEffectKind::Post).then(|| {
            let target = match self.detail {
                PenaltyShotResultDetail::HitLeftPost => PenaltyGoalFramePart::LeftPost,
                PenaltyShotResultDetail::HitRightPost => PenaltyGoalFramePart::RightPost,
                _ => PenaltyGoalFramePart::Crossbar,
            };
            let d = decay(self.tick, POST_TICKS);
            PenaltyGoalFrameShake {
                target,
                offset: Vec3::new(shake(self.tick) * d, shake(self.tick + 1) * d, 0.0),
            }
        })
    }

    // --- fake ball deflection (Save) / drift (Miss) -------------------------

    fn ball_deflection(&self) -> Option<PenaltyBallDeflectionVisual> {
        match self.kind {
            PenaltyImpactEffectKind::Save => {
                let bias = match self.detail {
                    PenaltyShotResultDetail::SavedByLeftHand => Vec3::new(0.7, 0.1, 0.4),
                    PenaltyShotResultDetail::SavedByRightHand => Vec3::new(-0.7, 0.1, 0.4),
                    _ => Vec3::new(0.0, -0.25, 0.55), // torso/body: down + forward
                };
                Some(PenaltyBallDeflectionVisual::new(self.impact, self.impact.add(bias), self.tick))
            }
            PenaltyImpactEffectKind::Miss => Some(PenaltyBallDeflectionVisual::new(
                self.impact,
                self.impact.add(Vec3::new(0.0, -0.1, -1.1)), // drift past the plane, down
                self.tick,
            )),
            _ => None,
        }
    }

    // --- crowd reaction -----------------------------------------------------

    fn crowd(&self) -> PenaltyCrowdReaction {
        let amplitude = match self.kind {
            PenaltyImpactEffectKind::Goal => 0.5,
            PenaltyImpactEffectKind::Save => 0.3,
            PenaltyImpactEffectKind::Post => 0.15,
            PenaltyImpactEffectKind::Miss => 0.05,
            PenaltyImpactEffectKind::SessionComplete => if self.celebrate { 0.6 } else { 0.0 },
        };
        PenaltyCrowdReaction { kind: self.kind, tick: self.tick, amplitude, duration: self.duration() }
    }

    // --- camera juice -------------------------------------------------------

    fn camera(&self) -> PenaltyCameraJuice {
        let intensity = match self.kind {
            PenaltyImpactEffectKind::Goal => 1.2,
            PenaltyImpactEffectKind::Save => 0.9,
            PenaltyImpactEffectKind::Post => 1.0,
            PenaltyImpactEffectKind::Miss => 0.5,
            PenaltyImpactEffectKind::SessionComplete => 0.6,
        };
        let d = decay(self.tick, CAMERA_SHAKE_TICKS);
        let offset = if self.tick < CAMERA_SHAKE_TICKS { {
                Vec3::new(
                    shake(self.tick) * intensity * d,
                    shake(self.tick + 2) * intensity * d,
                    0.0,
                )
            } } else { Vec3::ZERO };
        PenaltyCameraJuice { offset }
    }

    // --- banner + score popup ----------------------------------------------

    fn banner(&self) -> PenaltyResultBanner {
        let pop = decay(self.tick, POP_TICKS);
        PenaltyResultBanner {
            text: self.kind.banner_text(),
            scale: 1.0 + 0.5 * pop,
            pulse: osc(self.tick).abs(),
        }
    }

    fn score_popup(&self) -> Option<PenaltyScorePopup> {
        // Round effects pop the awarded points; SessionComplete uses the banner.
        (self.kind != PenaltyImpactEffectKind::SessionComplete).then(|| PenaltyScorePopup {
            points: self.award_total,
            scale: 1.0 + 0.4 * decay(self.tick, POP_TICKS),
        })
    }

    // --- foreground flashes -------------------------------------------------

    fn foreground(&self) -> Vec<PenaltyEffectRenderItem> {
        // A save impact flash at the contact point (fades over ~POP_TICKS).
        if self.kind == PenaltyImpactEffectKind::Save { {
                let a = decay(self.tick, POP_TICKS);
                vec![PenaltyEffectRenderItem {
                    ordinal: 0,
                    position: self.impact,
                    size: 0.3 + 0.4 * a,
                    alpha: a,
                    label: "impact.flash",
                }]
            } } else { Default::default() }
    }
}

fn wobble_panel(impact: Vec3, tick: u32, z: f32) -> Vec<PenaltyNetWobbleNode> {
    let mut nodes = Vec::new();
    (0..NET_WOBBLE_ROWS).for_each(|row| {
        (0..NET_WOBBLE_COLS).for_each(|col| {
            let ordinal = row * NET_WOBBLE_COLS + col;
            let x = -GOAL_HALF_WIDTH + (GOAL_HALF_WIDTH * 2.0) * (col as f32 / (NET_WOBBLE_COLS - 1) as f32);
            let y = GROUND_Y + GOAL_HEIGHT * (row as f32 / (NET_WOBBLE_ROWS - 1) as f32);
            let base = Vec3::new(x, y, z);
            // Bulge away from the camera (−z), strongest near the impact, decaying
            // with distance and with the effect tick, oscillating over time.
            let dx = x - impact.x;
            let dy = y - impact.y;
            let dist = (dx * dx + dy * dy).sqrt();
            let falloff = 1.0 / (1.0 + dist * 1.2);
            let disp = WOBBLE_AMPLITUDE * decay(tick, GOAL_TICKS) * falloff * osc(tick + ordinal);
            nodes.push(PenaltyNetWobbleNode {
                ordinal,
                base_position: base,
                displaced_position: base.add(Vec3::new(0.0, 0.0, -disp)),
            });
        });
    });
    nodes
}

// --- descriptor sub-types ---------------------------------------------------

/// One net grid node with its base + wobbled world position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenaltyNetWobbleNode {
    pub ordinal: u32,
    pub base_position: Vec3,
    pub displaced_position: Vec3,
}

/// The wobbled net: ordered rear + front grid nodes (still fake line geometry).
#[derive(Debug, Clone, PartialEq)]
pub struct PenaltyNetWobble {
    pub rear: Vec<PenaltyNetWobbleNode>,
    pub front: Vec<PenaltyNetWobbleNode>,
}

/// Which goal-frame part shakes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PenaltyGoalFramePart {
    LeftPost,
    RightPost,
    Crossbar,
}

/// A deterministic goal-frame shake offset for one part.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenaltyGoalFrameShake {
    pub target: PenaltyGoalFramePart,
    pub offset: Vec3,
}

/// A fake ball-deflection / drift visual (no physics): the ball's final visual
/// pose slides from `start` to `end` over [`DEFLECT_TICKS`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenaltyBallDeflectionVisual {
    pub start: Vec3,
    pub end: Vec3,
    pub current: Vec3,
}

impl PenaltyBallDeflectionVisual {
    fn new(start: Vec3, end: Vec3, tick: u32) -> Self {
        let t = tick.min(DEFLECT_TICKS) as f32 / DEFLECT_TICKS as f32;
        Self { start, end, current: lerp(start, end, t) }
    }
}

/// A deterministic crowd reaction: a per-card bounce/pulse pattern.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenaltyCrowdReaction {
    pub kind: PenaltyImpactEffectKind,
    pub tick: u32,
    pub amplitude: f32,
    pub duration: u32,
}

impl PenaltyCrowdReaction {
    /// The world-space bounce offset for a crowd card (by stable ordinal).
    pub fn card_offset(&self, ordinal: u32) -> Vec3 {
        Vec3::new(0.0, self.amplitude * decay(self.tick, self.duration) * bounce(self.tick + ordinal), 0.0)
    }

    /// A `0..=1` color pulse for a crowd card.
    pub fn card_pulse(&self, ordinal: u32) -> f32 {
        if self.amplitude > 0.0 { decay(self.tick, self.duration) * osc(self.tick + ordinal).abs() } else { 0.0 }
    }
}

/// A small additive camera offset (juice), zero outside the shake window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenaltyCameraJuice {
    pub offset: Vec3,
}

impl PenaltyCameraJuice {
    pub const NONE: Self = Self { offset: Vec3::ZERO };
}

/// The result banner descriptor (unlit HUD).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenaltyResultBanner {
    pub text: &'static str,
    pub scale: f32,
    pub pulse: f32,
}

/// The score-popup descriptor (unlit HUD).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenaltyScorePopup {
    pub points: u32,
    pub scale: f32,
}

/// A foreground effect render item (e.g. a save impact flash).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenaltyEffectRenderItem {
    pub ordinal: u32,
    pub position: Vec3,
    pub size: f32,
    pub alpha: f32,
    pub label: &'static str,
}

/// The full bundle of effect descriptors at one tick.
#[derive(Debug, Clone, PartialEq)]
pub struct PenaltyEffectDescriptor {
    pub kind: PenaltyImpactEffectKind,
    pub tick: u32,
    pub progress: u32,
    pub net_wobble: Option<PenaltyNetWobble>,
    pub frame_shake: Option<PenaltyGoalFrameShake>,
    pub ball_deflection: Option<PenaltyBallDeflectionVisual>,
    pub crowd: PenaltyCrowdReaction,
    pub camera: PenaltyCameraJuice,
    pub banner: PenaltyResultBanner,
    pub score_popup: Option<PenaltyScorePopup>,
    pub foreground: Vec<PenaltyEffectRenderItem>,
}
