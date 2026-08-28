//! Every number that defines how the player feels, in one place.
//!
//! Ported from `C:/dev/Claude-of-Duty/src/player/tuning.js:1-301` — the whole
//! file.
//!
//! Calibration notes — these are matched to Modern Warfare (2019) / MWII, which
//! are authored in inches at 20 units = 1 ft:
//!
//! | | source | converted |
//! |---|---|---|
//! | base run        | 150 u/s | 4.57 m/s |
//! | sprint           | 230 u/s | 7.01 m/s |
//! | tactical sprint  | 275 u/s | 8.38 m/s |
//! | crouch walk      | 80 u/s  | 2.44 m/s |
//! | prone            | 33 u/s  | 1.01 m/s |
//! | jump apex        | ~39 u   | 0.60 m (with 800 u/s² → ~20.6 m/s² gravity) |
//! | slide burst      | 290 u/s | 8.84 m/s, ~0.9 s to bleed out |
//! | stance change    | ~0.2 s crouch↔stand, ~0.75 s to/from prone |
//!
//! `GRAVITY` comes from [`crate::config::UNITS`]`.gravity` (the source's
//! `UNITS.gravity`, -20.6 m/s²) so the jump arc matches the rest of the game's
//! physics rather than a private constant — per the port recipe, used as-is,
//! not redefined.
//!
//! **Formerly a divergence, now fixed at the root.** [`crate::config::UNITS`]
//! used to store these five numbers as `f32`-backed kernel quantities, so
//! every value here inherited an `f32` rounding the pure-`f64` JavaScript
//! never had. That is the storage-width trap: `config.js`'s `UNITS` are plain
//! JavaScript numbers and the simulation integrates them in `f64` 120 times a
//! second, so narrowing at the *source* rather than at a carrier put the
//! player's feet 1.2e-11 m out after a single step and grew from there. It
//! broke three assertions in `tests/player_system_port.rs`. `UNITS` is `f64`
//! now and there is no cast on this page — see `config.rs`'s module doc
//! comment for the argument.

use std::sync::LazyLock;

use crate::config::UNITS;
use crate::player::springs::DEG;

/// `tuning.js:22`. Negative. [`crate::config::UNITS`]`.gravity` as-is — no
/// cast, because `UNITS` is `f64`. See the module doc comment.
pub const GRAVITY: f64 = UNITS.gravity;

/// `tuning.js:23`.
pub const JUMP_APEX: f64 = 0.6;

/// `v = sqrt(2 g h)` — solved from the apex so tuning the apex is meaningful.
/// `tuning.js:25`.
///
/// A `LazyLock`, not a `const`: `f64::sqrt` is not a stable `const fn`, so this
/// mirrors the source's "computed once, at module load" semantics with the
/// nearest stable Rust shape rather than a private re-derivation at every call
/// site.
pub static JUMP_SPEED: LazyLock<f64> = LazyLock::new(|| (2.0 * GRAVITY.abs() * JUMP_APEX).sqrt());

/// One physical stance: stand, crouch, or prone. `tuning.js:27-52`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stance {
    Stand,
    Crouch,
    Prone,
}

/// One row of `STANCE`. `tuning.js:27-52`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StanceDef {
    pub name: &'static str,
    pub height: f64,
    pub eye: f64,
    pub speed: f64,
    pub step_height: f64,
    pub stride_length: f64,
}

/// `STANCE.stand`. `height`/`eye` are [`UNITS`]'s metres as-is; see
/// [`GRAVITY`] for why there is no cast.
pub const STAND: StanceDef = StanceDef {
    name: "stand",
    height: UNITS.player_height,
    eye: UNITS.player_height - UNITS.eye_offset,
    speed: 4.57,
    step_height: 0.42,
    stride_length: 1.48,
};

/// `STANCE.crouch`.
pub const CROUCH: StanceDef = StanceDef {
    name: "crouch",
    height: UNITS.player_crouch_height,
    eye: UNITS.player_crouch_height - 0.1,
    speed: 2.44,
    step_height: 0.3,
    stride_length: 1.05,
};

/// `STANCE.prone`. `height`/`eye` are the source's own private literals (not
/// derived from `UNITS`) — `0.7` is a comment-documented approximation of
/// `2 * radius = 0.64`, ported verbatim.
pub const PRONE: StanceDef = StanceDef {
    name: "prone",
    height: 0.7,
    eye: 0.4,
    speed: 1.01,
    step_height: 0.14,
    stride_length: 0.78,
};

impl Stance {
    /// `STANCE[this.stance]`.
    pub fn def(self) -> StanceDef {
        match self {
            Stance::Stand => STAND,
            Stance::Crouch => CROUCH,
            Stance::Prone => PRONE,
        }
    }
}

/// `MOVE.slide`. `tuning.js:91-111`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SlideTuning {
    pub entry_speed: f64,
    pub min_entry: f64,
    pub exit_speed: f64,
    pub duration: f64,
    pub drag: f64,
    pub brake: f64,
    pub cooldown: f64,
    pub min_speed_to_start: f64,
    pub steer: f64,
    pub slope_assist: f64,
}

/// `MOVE.mantle`. `tuning.js:113-136`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MantleTuning {
    pub auto_vault_max: f64,
    pub min_height: f64,
    pub max_height: f64,
    pub reach: f64,
    pub land_depth: f64,
    pub vault_time: f64,
    pub mantle_time: f64,
    pub high_mantle_time: f64,
    pub cooldown: f64,
    pub auto_speed: f64,
    pub proactive_distance: f64,
    pub proactive_lookahead: f64,
}

/// `MOVE.lean`. `tuning.js:138-145`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LeanTuning {
    pub offset: f64,
    pub roll: f64,
    pub drop: f64,
    pub rate: f64,
    pub probe_radius: f64,
}

/// `MOVE.stanceTau`. `tuning.js:148-152`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StanceTauTuning {
    pub stand_crouch: f64,
    pub crouch_stand: f64,
    pub prone: f64,
}

/// `MOVE`. `tuning.js:54-153`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MoveTuning {
    pub sprint_speed: f64,
    pub tac_sprint_speed: f64,
    pub strafe_scale: f64,
    pub back_scale: f64,
    pub ads_scale: f64,
    pub ground_accel: f64,
    pub ground_decel: f64,
    pub stop_decel: f64,
    pub air_accel_scale: f64,
    pub air_speed_cap: f64,
    pub terminal_speed: f64,
    pub coyote_time: f64,
    pub jump_buffer: f64,
    pub jump_cooldown: f64,
    pub tac_sprint_tap_window: f64,
    pub tac_sprint_max_time: f64,
    pub tac_sprint_recovery: f64,
    pub sprint_forward_dot: f64,
    pub sprint_start_delay: f64,
    pub slide: SlideTuning,
    pub mantle: MantleTuning,
    pub lean: LeanTuning,
    pub stance_tau: StanceTauTuning,
}

/// `tuning.js:54-153`, value for value.
pub const MOVE: MoveTuning = MoveTuning {
    sprint_speed: 7.01,
    tac_sprint_speed: 8.38,
    strafe_scale: 0.92,
    back_scale: 0.8,
    ads_scale: 0.5,
    ground_accel: 92.0,
    ground_decel: 52.0,
    stop_decel: 30.0,
    air_accel_scale: 0.25,
    air_speed_cap: 3.4,
    terminal_speed: 55.0,
    coyote_time: 0.09,
    jump_buffer: 0.13,
    jump_cooldown: 0.28,
    tac_sprint_tap_window: 0.32,
    tac_sprint_max_time: 6.0,
    tac_sprint_recovery: 1.6,
    sprint_forward_dot: 0.55,
    sprint_start_delay: 0.05,
    slide: SlideTuning {
        entry_speed: 8.84,
        min_entry: 6.2,
        exit_speed: 2.95,
        duration: 0.9,
        drag: 0.75,
        brake: 0.85,
        cooldown: 0.55,
        min_speed_to_start: 5.2,
        steer: 2.6,
        slope_assist: 9.0,
    },
    mantle: MantleTuning {
        auto_vault_max: 0.72,
        min_height: 0.34,
        max_height: 1.85,
        reach: 0.62,
        land_depth: 0.46,
        vault_time: 0.34,
        mantle_time: 0.62,
        high_mantle_time: 0.82,
        cooldown: 0.2,
        auto_speed: 2.4,
        proactive_distance: 0.2,
        proactive_lookahead: 0.035,
    },
    lean: LeanTuning {
        offset: 0.34,
        roll: 13.0 * DEG,
        drop: 0.035,
        rate: 0.085,
        probe_radius: 0.17,
    },
    stance_tau: StanceTauTuning {
        stand_crouch: 0.062,
        crouch_stand: 0.072,
        prone: 0.16,
    },
};

/// `CAMERA.bob`. `tuning.js:162-172`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BobTuning {
    pub amp_x: f64,
    pub amp_y: f64,
    pub amp_z: f64,
    pub roll: f64,
    pub pitch: f64,
    pub speed_exp: f64,
    pub speed_cap: f64,
    pub ads_scale: f64,
    pub air_fade: f64,
}

/// `CAMERA.step`. `tuning.js:174-180`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StepTuning {
    pub impulse: f64,
    pub freq: f64,
    pub damping: f64,
    pub sprint_scale: f64,
}

/// `CAMERA.land`. `tuning.js:182-196`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LandTuning {
    pub min_speed: f64,
    pub full_speed: f64,
    pub dip_impulse: f64,
    pub pitch: f64,
    pub roll: f64,
    pub freq: f64,
    pub damping: f64,
    pub trauma: f64,
    pub damage_speed: f64,
    pub damage_per_speed: f64,
}

/// `CAMERA.roll`. `tuning.js:198-206`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RollTuning {
    pub strafe: f64,
    pub yaw_rate: f64,
    pub yaw_rate_max: f64,
    pub tau: f64,
    pub slide: f64,
    pub air: f64,
}

/// `CAMERA.recoil`. `tuning.js:208-216`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecoilTuning {
    pub freq: f64,
    pub damping: f64,
    pub residual_tau: f64,
    pub residual_share: f64,
    pub punch_freq: f64,
    pub punch_damping: f64,
}

/// `CAMERA.shake`. `tuning.js:218-223`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShakeTuning {
    pub decay: f64,
    pub rot: f64,
    pub pos: f64,
    pub freq: f64,
}

/// `CAMERA.breath`. `tuning.js:225-237`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreathTuning {
    pub freq_a: f64,
    pub freq_b: f64,
    pub amp: f64,
    pub pos_amp: f64,
    pub ads_scale: f64,
    pub low_health_scale: f64,
    pub move_damp: f64,
    pub suppression_scale: f64,
}

/// `CAMERA.fov`. `tuning.js:239-248`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FovTuning {
    pub sprint: f64,
    pub tac_sprint: f64,
    pub slide: f64,
    pub air: f64,
    pub ads_tau: f64,
    pub move_tau: f64,
}

/// `CAMERA`. `tuning.js:155-253`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraTuning {
    pub bob: BobTuning,
    pub step: StepTuning,
    pub land: LandTuning,
    pub roll: RollTuning,
    pub recoil: RecoilTuning,
    pub shake: ShakeTuning,
    pub breath: BreathTuning,
    pub fov: FovTuning,
    pub wall_pad: f64,
    pub pitch_limit: f64,
}

/// `tuning.js:155-253`, value for value.
pub const CAMERA: CameraTuning = CameraTuning {
    bob: BobTuning {
        amp_x: 0.0165,
        amp_y: 0.0115,
        amp_z: 0.006,
        roll: 0.42 * DEG,
        pitch: 0.16 * DEG,
        speed_exp: 0.85,
        speed_cap: 1.55,
        ads_scale: 0.22,
        air_fade: 0.11,
    },
    step: StepTuning {
        impulse: 0.085,
        freq: 5.4,
        damping: 0.62,
        sprint_scale: 1.7,
    },
    land: LandTuning {
        min_speed: 2.2,
        full_speed: 12.5,
        dip_impulse: 2.35,
        pitch: 3.4 * DEG,
        roll: 0.9 * DEG,
        freq: 3.05,
        damping: 0.52,
        trauma: 0.34,
        damage_speed: 15.0,
        damage_per_speed: 7.0,
    },
    roll: RollTuning {
        strafe: 1.05 * DEG,
        yaw_rate: 0.055,
        yaw_rate_max: 1.5 * DEG,
        tau: 0.11,
        slide: 5.2 * DEG,
        air: 0.9 * DEG,
    },
    recoil: RecoilTuning {
        freq: 9.5,
        damping: 0.5,
        residual_tau: 0.28,
        residual_share: 0.34,
        punch_freq: 12.0,
        punch_damping: 0.62,
    },
    shake: ShakeTuning {
        decay: 1.85,
        rot: 1.35,
        pos: 0.022,
        freq: 22.0,
    },
    breath: BreathTuning {
        freq_a: 0.235,
        freq_b: 0.155,
        amp: 0.0021,
        pos_amp: 0.0035,
        ads_scale: 1.85,
        low_health_scale: 2.6,
        move_damp: 0.78,
        suppression_scale: 2.2,
    },
    fov: FovTuning {
        sprint: 1.055,
        tac_sprint: 1.1,
        slide: 1.085,
        air: 1.015,
        ads_tau: 0.052,
        move_tau: 0.13,
    },
    wall_pad: 0.09,
    pitch_limit: 88.0 * DEG,
};

/// `HEALTH.suppression`. `tuning.js:267-277`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SuppressionTuning {
    pub per_near_miss: f64,
    pub per_hit: f64,
    pub per_explosion: f64,
    pub radius: f64,
    pub decay: f64,
    pub sway_scale: f64,
    pub shake_scale: f64,
}

/// `HEALTH.effect`. `tuning.js:279-289`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HealthEffectTuning {
    pub desaturate: f64,
    pub vignette: f64,
    pub tint: f64,
    pub heartbeat_min: f64,
    pub heartbeat_max: f64,
    pub pulse_gain: f64,
    pub hit_flash: f64,
    pub hit_flash_tau: f64,
}

/// `HEALTH`. `tuning.js:255-290`. Not consumed by `springs.js`/`movement.js`/
/// `camera.js`/`mantle.js` (health lives in a different, unported subsystem),
/// but it is part of the whole `tuning.js` file this module ports.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HealthTuning {
    pub max: f64,
    pub regen_delay: f64,
    pub regen_rate: f64,
    pub regen_ramp: f64,
    pub low_threshold: f64,
    pub critical_threshold: f64,
    pub indicator_time: f64,
    pub indicator_max: u32,
    pub suppression: SuppressionTuning,
    pub effect: HealthEffectTuning,
}

/// `tuning.js:255-290`, value for value.
pub const HEALTH: HealthTuning = HealthTuning {
    max: 100.0,
    regen_delay: 4.6,
    regen_rate: 34.0,
    regen_ramp: 0.55,
    low_threshold: 0.36,
    critical_threshold: 0.18,
    indicator_time: 1.8,
    indicator_max: 4,
    suppression: SuppressionTuning {
        per_near_miss: 0.28,
        per_hit: 0.5,
        per_explosion: 0.85,
        radius: 3.2,
        decay: 0.62,
        sway_scale: 1.5,
        shake_scale: 0.28,
    },
    effect: HealthEffectTuning {
        desaturate: 0.62,
        vignette: 0.55,
        tint: 0.3,
        heartbeat_min: 1.05,
        heartbeat_max: 2.05,
        pulse_gain: 0.42,
        hit_flash: 0.85,
        hit_flash_tau: 0.22,
    },
};

/// `FOOTSTEP`. `tuning.js:292-301`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FootstepTuning {
    pub lateral: f64,
    pub probe: f64,
    pub run_speed: f64,
    pub land_hold: f64,
}

/// `tuning.js:292-301`, value for value.
pub const FOOTSTEP: FootstepTuning = FootstepTuning {
    lateral: 0.13,
    probe: 0.9,
    run_speed: 5.4,
    land_hold: 0.12,
};
