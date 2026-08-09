//! Everything the rest of the game is allowed to *see* of an attempt.
//!
//! One file of readers, and no writers anywhere in it. That is the point: the
//! presentation layers — the camera, the scene, the overlay, the debug view —
//! all reach in through here, and a file that contains no `&mut self` cannot
//! become a back door into the simulation however many of them there are.

use crate::figure::{KickPlan, Swing};
use crate::shot::{ResolvedShot, ShotIntent};
use crate::tuning::Tuning;

use crate::play::ball::Ball;
use crate::play::keeper::Keeper;
use crate::play::phase::Phase;
use crate::play::resolution::{ShotResult, Tally};

use super::Session;

impl Session {
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn phase_tick(&self) -> u32 {
        self.phase_tick
    }
    pub fn tick(&self) -> u64 {
        self.tick
    }
    pub fn intent(&self) -> &ShotIntent {
        &self.intent
    }
    pub fn shot(&self) -> &ResolvedShot {
        &self.shot
    }
    pub fn ball(&self) -> &Ball {
        &self.ball
    }
    pub fn keeper(&self) -> &Keeper {
        &self.keeper
    }
    pub fn kick(&self) -> &KickPlan {
        &self.kick
    }
    pub fn swing(&self) -> &Swing {
        &self.swing
    }
    /// Ticks since the run-up began — the kick's own clock, which keeps running
    /// through the flight so the follow-through never restarts.
    pub fn kick_tick(&self) -> u32 {
        self.kick_tick
    }
    pub fn result(&self) -> Option<ShotResult> {
        self.result
    }
    /// The speed the ball left at, metres per second, once it has.
    pub fn struck_speed(&self) -> Option<f32> {
        self.struck
    }
    pub fn tally(&self) -> Tally {
        self.tally
    }
    pub fn tuning(&self) -> &Tuning {
        &self.tuning
    }
}
