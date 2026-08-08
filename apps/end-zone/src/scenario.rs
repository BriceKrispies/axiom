//! **Deterministic staged encounters** — known starting conditions for the one
//! thing a whole carry cannot reliably produce on demand: a *specific* geometry
//! between the running back and one defender.
//!
//! A carry is emergent. Which defender arrives, from what angle, at what closing
//! speed, is the product of two AI teams, a blocking contest and a physics
//! solver, and that is exactly what makes the game worth playing — but it is a
//! poor instrument for asking "does a badly-aligned charge lose?" So this module
//! plays the **real game** up to the moment control arrives, and then sets the
//! board: one defender, placed and pointed, everyone else moved out of the play.
//!
//! What it does **not** do is the important part. Nothing here injects a success,
//! sets a flag, moves anybody once the encounter has begun, or calls a mechanic
//! directly. The controls are the real controls, the collision is the real
//! collision, the AI keeps running both teams, the update loop is the real loop,
//! and the success detectors are the real detectors. The staging is the *initial
//! condition* only — the same thing a physics experiment does before it lets go.

use axiom::prelude::Vec3;

use crate::attempt::AttemptPhase;
use crate::field::OffensePoint;
use crate::identity::PlayerId;
use crate::launch::RunConfig;
use crate::showcase::ShowcaseRun;

/// The seed the agent's validation run plays. Nothing special about it beyond
/// being the one we quote, so a reader can reproduce the exact trace.
pub const VALIDATION_SEED: u64 = 7;

/// How far off the field a benched player is parked, yards downfield behind the
/// offense. Far enough to be out of every pursuit radius, on the field of play
/// so no boundary rule fires.
const BENCH_DEPTH: f32 = -34.0;

/// The board for one staged encounter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EncounterSetup {
    pub seed: u64,
    /// The run concept to call.
    pub concept: usize,
    /// How far downfield of the back the defender stands, yards.
    pub ahead: f32,
    /// How far to the back's right he stands, yards.
    pub lateral: f32,
    /// Whether he is squared up facing the runner (the braced case) or turned
    /// downfield (the caught-out case).
    pub squared: bool,
    /// How fast he is already moving *at* the runner, yd/s.
    pub closing: f32,
    /// The runner's speed at the moment the board is set, yd/s. Clamped to his
    /// archetype's top speed by the controller on the very next tick, so asking
    /// for more than he can run simply gets his best.
    pub runner_speed: f32,
}

impl EncounterSetup {
    /// A fast runner meeting one defender who is not set — the charge's
    /// favourable case.
    pub fn favourable_charge() -> Self {
        EncounterSetup {
            seed: VALIDATION_SEED,
            concept: 0,
            ahead: 2.6,
            lateral: 0.0,
            squared: false,
            closing: 1.0,
            runner_speed: 8.4,
        }
    }

    /// A slow runner meeting a defender squared up and coming hard — the
    /// charge's unfavourable case.
    pub fn unfavourable_charge() -> Self {
        EncounterSetup {
            runner_speed: 1.6,
            squared: true,
            closing: 7.0,
            ..EncounterSetup::favourable_charge()
        }
    }

    /// A defender arriving from the runner's right, close enough that his tackle
    /// is imminent on the current line — the dodge's case.
    pub fn imminent_tackle() -> Self {
        EncounterSetup {
            ahead: 1.6,
            lateral: 1.1,
            closing: 5.0,
            squared: true,
            ..EncounterSetup::favourable_charge()
        }
    }
}

/// A staged encounter: the live run, plus who is who.
#[derive(Debug)]
pub struct StagedEncounter {
    pub run: ShowcaseRun,
    pub back: PlayerId,
    pub defender: PlayerId,
}

/// Play the real game to the moment control arrives.
///
/// Calls `concept` the instant the card is up and then simply steps, exactly as
/// a player would: the shift, the snap, the mesh and the exchange are all the
/// game's own, so anything staged on top of this is staged on a real carry.
/// Returns the carrying run, or `None` if the play never reached the back (which
/// is a failure worth seeing rather than one worth papering over).
pub fn run_to_carrying(config: &RunConfig, concept: usize) -> Option<ShowcaseRun> {
    let mut run = ShowcaseRun::new_run(config);
    for _ in 0..600 {
        let step = run.attempt()?;
        if step.phase.accepts_call() {
            run.select_concept(concept);
        }
        if step.phase == AttemptPhase::Carrying && run.sim.back_is_carrying() {
            return Some(run);
        }
        run.step(&[]);
    }
    None
}

/// Set the board: one defender placed and pointed, every other defender benched,
/// and the runner put at the requested speed on his current heading.
///
/// The chosen defender is the one **nearest the back**, so the encounter is with
/// somebody who was genuinely in the play rather than with a body summoned for
/// it.
pub fn stage(setup: EncounterSetup) -> Option<StagedEncounter> {
    let config = RunConfig::new(setup.seed);
    let mut run = run_to_carrying(&config, setup.concept)?;
    let back = run.sim.runback.back?;
    let runner = run.sim.players[back.index()];
    let frame = run.sim.frame;

    let defender = run
        .sim
        .players
        .iter()
        .filter(|p| p.team != runner.team)
        .map(|p| {
            let to = Vec3::new(p.pos.x - runner.pos.x, 0.0, p.pos.z - runner.pos.z);
            (p.id, to.length())
        })
        .fold(None::<(PlayerId, f32)>, |best, (id, d)| {
            match best.map(|(_, b)| d < b).unwrap_or(true) {
                true => Some((id, d)),
                false => best,
            }
        })
        .map(|(id, _)| id)?;

    // Bench everybody else on the defense, well behind the play.
    let opponents: Vec<PlayerId> = run
        .sim
        .players
        .iter()
        .filter(|p| p.team != runner.team && p.id != defender)
        .map(|p| p.id)
        .collect();
    for (index, id) in opponents.iter().enumerate() {
        let spot = frame.to_world(OffensePoint::new(
            -20.0 + index as f32 * 8.0,
            BENCH_DEPTH,
        ));
        let benched = &mut run.sim.players[id.index()];
        benched.pos = spot;
        benched.vel = Vec3::ZERO;
    }

    // Place the one defender relative to the runner, in the offense frame.
    let here = frame.from_world(runner.pos);
    let spot = frame.to_world(OffensePoint::new(
        here.lateral + setup.lateral,
        here.downfield + setup.ahead,
    ));
    let toward_runner = Vec3::new(runner.pos.x - spot.x, 0.0, runner.pos.z - spot.z);
    let unit = toward_runner.mul_scalar(1.0 / toward_runner.length().max(1.0e-4));
    let facing = match setup.squared {
        true => unit,
        false => frame.forward(),
    };
    let placed = &mut run.sim.players[defender.index()];
    placed.pos = spot;
    placed.vel = unit.mul_scalar(setup.closing);
    placed.facing = facing.x.atan2(facing.z);

    // Put the runner at the requested speed, on the heading he already had.
    let heading = match runner.speed() > 0.1 {
        true => Vec3::new(runner.vel.x, 0.0, runner.vel.z)
            .mul_scalar(1.0 / runner.speed()),
        false => frame.forward(),
    };
    let carrier = &mut run.sim.players[back.index()];
    carrier.vel = heading.mul_scalar(setup.runner_speed);

    Some(StagedEncounter {
        run,
        back,
        defender,
    })
}
