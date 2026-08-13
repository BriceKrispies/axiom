//! The per-tick execution of a committed runback move.
//!
//! Split out of [`super::stage`], which owns the status read and the move
//! commitment, so each file stays narrowly owned. Pure relocation — a second
//! inherent `impl SimState` block, identical methods.

use axiom::prelude::Vec3;
use crate::config::DT;
use crate::events::SimEvent;
use crate::field::OffenseFrame;
use crate::identity::PlayerId;
use crate::player::AnimState;
use crate::state::SimState;
use super::charge;
use super::evade::{self, ThreatVerdict};
use super::read;
use super::{ActiveMove, RunbackMove, RunbackStatus};

impl SimState {
    /// One tick of **running through people**.
    ///
    /// Everybody the back touches while the charge lasts is knocked aside and
    /// counted as a broken tackle. There is no contest to lose: the contest was
    /// the old design and it was unusable, because it asked for a press timed
    /// finer than a human reaction. What the arithmetic still decides is *how
    /// hard* each man goes — the same impulse-against-resistance terms, now
    /// setting the size of the hit rather than gatekeeping whether it happens —
    /// so a big back at full speed visibly flattens people and a tired one
    /// merely shoves them off.
    pub(super) fn carry_charge(&mut self, back: PlayerId) {
        let runback = self.runback_tuning;
        let runner = self.players[back.index()];
        let hits: Vec<PlayerId> = self
            .players
            .iter()
            .filter(|p| p.team != runner.team && p.anim.can_act())
            .filter(|p| {
                let reach = runner.archetype.body_radius
                    + p.archetype.body_radius
                    + runback.shoulder_reach;
                Vec3::new(p.pos.x - runner.pos.x, 0.0, p.pos.z - runner.pos.z).length() <= reach
            })
            .map(|p| p.id)
            .collect();

        for defender in hits {
            let resolution = charge::resolve(
                &self.players[back.index()],
                &self.players[defender.index()],
                runback.charge_ideal_lead_ticks,
                &runback,
            );
            self.runback.last_charge = Some(resolution);
            let knock = runback.charge_knock_speed * resolution.overload.clamp(0.4, 2.5);
            let flattened = resolution.overload >= runback.charge_airborne_overload;
            let hit = &mut self.players[defender.index()];
            hit.balance = 0.0;
            hit.impact_strength = (resolution.overload * 0.5).clamp(0.15, 1.0);
            hit.vel = resolution.direction.mul_scalar(knock);
            match flattened {
                true => {
                    hit.vertical_vel = self.tuning.launch_up_speed * 0.7;
                    hit.set_anim(AnimState::AirborneFall);
                }
                false => hit.set_anim(AnimState::Stumble),
            }
            // Contact costs the runner something even when he wins it, so a
            // charge through four men leaves him walking rather than flying.
            let carrier = &mut self.players[back.index()];
            carrier.vel = carrier.vel.mul_scalar(runback.charge_win_keep);
            self.runback.broken += 1;
            self.runback.last_success = Some((RunbackMove::Shoulder.code(), self.tick));
            self.events.emit(SimEvent::TackleBroken {
                runner: back,
                defender,
                impulse: resolution.impulse,
                resistance: resolution.resistance,
            });
        }
    }

    /// Integrate the leap's arc, watch who goes underneath, and land him.
    pub(super) fn carry_jump(&mut self, back: PlayerId) {
        let runback = self.runback_tuning;
        self.runback.vertical -= runback.jump_gravity * DT;
        let height = self.players[back.index()].pos.y + self.runback.vertical * DT;
        let landed = height <= 0.0 && self.runback.vertical < 0.0;
        let player = &mut self.players[back.index()];
        player.pos = Vec3::new(player.pos.x, height.max(0.0), player.pos.z);

        let mut watches = core::mem::take(&mut self.runback.hurdles);
        evade::watch_hurdles(
            &self.players[back.index()],
            &self.players,
            &mut watches,
            &runback,
        );
        self.runback.hurdles = watches;

        if landed {
            self.land_jump(back);
        }
    }

    /// The landing: back on the turf, back into the run, and every defender who
    /// went beneath is now a cleared one — provided he landed still carrying.
    pub(super) fn land_jump(&mut self, back: PlayerId) {
        let runback = self.runback_tuning;
        self.runback.airborne = false;
        self.runback.vertical = 0.0;
        let player = &mut self.players[back.index()];
        player.pos = Vec3::new(player.pos.x, 0.0, player.pos.z);
        let carrying = self.ball.carrier() == Some(back) && self.players[back.index()].anim.can_act();
        let cleared = core::mem::take(&mut self.runback.hurdles);
        cleared
            .into_iter()
            .filter(|_| carrying)
            .for_each(|watch| {
                self.runback.hurdled += 1;
                self.runback.last_success = Some((RunbackMove::Jump.code(), self.tick));
                self.events.emit(SimEvent::DefenderHurdled {
                    runner: back,
                    defender: watch.defender,
                    clearance: watch.clearance,
                });
            });
        // The leap costs no extra recovery beyond its own cooldown: he lands
        // running, which is the promise the move makes.
        let _ = runback;
        self.runback.active = None;
        self.runback.ready_at = self.tick;
        self.players[back.index()].set_anim(AnimState::Sprint);
    }

    /// End the live move and set the shared recovery.
    pub(super) fn finish_move(&mut self, recovery_ticks: u32) {
        self.runback.active = None;
        self.runback.ready_at = self.tick + u64::from(recovery_ticks);
    }

    /// Judge every pending dodge threat against what the field did.
    pub(super) fn judge_threats(&mut self, back: PlayerId) {
        let runback = self.runback_tuning;
        let forward = self.frame.forward();
        let carrying = self.ball.carrier() == Some(back);
        let tick = self.tick;
        let pending = core::mem::take(&mut self.runback.threats);
        let mut kept = Vec::with_capacity(pending.len());
        for threat in pending {
            let verdict = evade::judge_threat(
                threat,
                &self.players[back.index()],
                &self.players[threat.defender.index()],
                forward,
                tick,
                carrying,
                &runback,
            );
            match verdict {
                ThreatVerdict::Pending => kept.push(threat),
                ThreatVerdict::Dodged => {
                    self.runback.dodges += 1;
                    self.runback.last_success = Some((threat.via, tick));
                    self.events.emit(SimEvent::TackleDodged {
                        runner: back,
                        defender: threat.defender,
                        gap: Vec3::new(
                            self.players[threat.defender.index()].pos.x
                                - self.players[back.index()].pos.x,
                            0.0,
                            self.players[threat.defender.index()].pos.z
                                - self.players[back.index()].pos.z,
                        )
                        .length(),
                    });
                }
                ThreatVerdict::Expired => {}
            }
        }
        self.runback.threats = kept;
    }
}
