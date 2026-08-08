//! One fixed simulation step — the single ordering of the whole per-tick
//! pipeline, kept apart from the state it advances so the order is readable in
//! one screen: commands, intents, movement, the world, then bookkeeping.
//!
//! The order is load-bearing. Intents are decided from LAST tick's perception
//! (so a reaction delay is a real delay), movement is integrated before
//! collision, and the ball is stepped around the rig so a carrier's hand and
//! the ball agree within the tick rather than a frame apart.

use crate::config::DT;
use crate::events::StampedEvent;
use crate::player::controller;
use crate::state::{PlayPhase, SimCommand, SimState};

impl SimState {
    /// Advance one fixed step under `commands`, returning this tick's events.
    pub fn step(&mut self, commands: &[SimCommand]) -> &[StampedEvent] {
        self.events.begin_tick(self.tick);
        for command in commands {
            self.apply_command(*command);
        }
        let prev_possession = self.possession;

        self.decide_intents();
        let phase = self.phase;
        controller::integrate_movement(&mut self.players, &self.intents, phase, &self.tuning, DT);
        self.collision.resolve(&mut self.players, self.tick);

        // Pre-snap the world HOLDS. The clock still runs and players may still
        // walk into a new alignment, but contacts, ball physics, the rig and
        // idle animation are frozen, so the beat before the snap reads as a
        // deliberate pause to call a play in — not as a play already underway.
        // Nothing skipped here has anything to do before the snap: there are no
        // contacts to resolve, the ball is at rest, nobody can run out of
        // bounds. What is left is exactly the ambient motion that made a held
        // moment feel live.
        let held = phase == PlayPhase::PreSnap;
        // The rig always mirrors, so a player who shifts carries his body with
        // him; it simply does not SIMULATE while the world is held.
        self.rig.mirror_players(&self.players);
        if !held {
            // The running back's move, before contact: a won charge must have
            // already put its defender on the turf, and a leap must already be
            // at this tick's height, by the time the tackle stage looks.
            self.advance_runback();
            self.resolve_contacts();
            self.ball_pre_physics();
            self.rig.step(self.tick);
            self.ball_post_physics();
            self.check_carrier_bounds();
        }
        if self.possession.is_some() && self.possession != prev_possession {
            self.possession_since = self.tick;
        }

        for player in &mut self.players {
            player.anim_ticks = match held {
                true => player.anim_ticks,
                false => player.anim_ticks.saturating_add(1),
            };
        }
        self.push_perception();
        self.update_throwable();
        self.throw_commanded = false;
        self.tick += 1;
        self.events.events()
    }
}
