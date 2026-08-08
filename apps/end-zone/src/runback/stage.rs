//! The orchestrator's **runback stage**, owned by this subsystem: commit the
//! move the player asked for, carry whatever move is live, resolve contact, and
//! judge what the field did about it.
//!
//! It runs after the controller has integrated the ordinary run and after
//! player-vs-player de-penetration, and *before* the contact stage — the order
//! is load-bearing three times over:
//!
//! 1. after the controller, because a move is a modification of the run, not a
//!    replacement for it: the back's forward velocity is the controller's, and
//!    the lateral displacement of a cut is added on top of it;
//! 2. after de-penetration, so a juke's displacement is not immediately undone
//!    by the crowd solver pushing bodies apart;
//! 3. before contact, so a *won* charge has already put the defender on the turf
//!    when [`crate::player::contact::resolve_tackle`] looks for a tackler, and a
//!    *lost* one has already spent the runner's balance and speed when it does.

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
    /// The read-only view of the back's move state.
    pub fn runback_status(&self) -> RunbackStatus {
        let height = self
            .runback
            .back
            .map(|id| self.players[id.index()].pos.y)
            .unwrap_or(0.0);
        RunbackStatus {
            back: self.runback.back,
            active: self.runback.active_move(),
            airborne: self.runback.airborne,
            height,
            jump_available: self.runback.jump_available(self.tick)
                && self.controlled_player() == self.runback.back
                && self.runback.back.is_some(),
            jump_cooldown_left: self.runback.jump_cooldown_left(self.tick),
            move_ready: self.runback.move_available(self.tick) && self.back_is_carrying(),
            charging: self.runback.charging(self.tick),
            charge_available: self.runback.charge_available(self.tick) && self.back_is_carrying(),
            charge_cooldown_left: self.runback.charge_cooldown_left(self.tick),
            charge_window: self.runback.charge_window,
            dodges: self.runback.dodges,
            hurdled: self.runback.hurdled,
            broken: self.runback.broken,
            last_success: self.runback.last_success,
        }
    }

    /// Whether the running back is currently the ball carrier the player drives.
    pub fn back_is_carrying(&self) -> bool {
        self.runback.back.is_some() && self.controlled_player() == self.runback.back
    }

    /// The one runback stage. Does nothing unless a live play has the back
    /// carrying — every move is a thing you do *with the ball*.
    pub(crate) fn advance_runback(&mut self) {
        // A move ordered while nobody is carrying is stale, exactly like a
        // pre-snap throw press: it is dropped rather than banked.
        let Some(back) = self.runback.back.filter(|_| self.back_is_carrying()) else {
            self.runback.pending = None;
            self.runback.charge_window = None;
            self.clear_move_state();
            return;
        };
        self.commit_move(back);
        self.carry_move(back);
        if self.runback.charging(self.tick) {
            self.carry_charge(back);
        }
        self.judge_threats(back);
        self.advance_charge_window(back);
    }

    /// Advance the charge tell. Last, so it reflects the field as this tick
    /// leaves it — the same field the player will be looking at when they decide
    /// whether to press.
    fn advance_charge_window(&mut self, back: PlayerId) {
        self.runback.charge_window = read::advance_charge_window(
            self,
            back,
            self.runback.charge_available(self.tick) && self.back_is_carrying(),
            self.runback.charge_window,
        );
    }

    /// A move in progress when possession or the play is lost simply stops; the
    /// back is put back on the turf so nothing is left airborne after a whistle.
    fn clear_move_state(&mut self) {
        let airborne = self.runback.airborne;
        let Some(back) = self.runback.back.filter(|_| airborne) else {
            return;
        };
        let player = &mut self.players[back.index()];
        player.pos = Vec3::new(player.pos.x, 0.0, player.pos.z);
        self.runback.airborne = false;
        self.runback.vertical = 0.0;
        self.runback.active = None;
    }

    /// Turn this tick's commanded move into a live one, if the rules allow it.
    fn commit_move(&mut self, back: PlayerId) {
        let Some(wanted) = self.runback.pending.take() else {
            return;
        };
        let tick = self.tick;
        // The charge is a window, not a blocking move: it has its own cooldown,
        // it does not occupy the move slot, and you can keep cutting and leaping
        // while it runs. That is the whole of what makes the run feel fluid.
        if wanted == RunbackMove::Shoulder {
            let runback = self.runback_tuning;
            if self.runback.charge_available(tick) {
                let speed = self.players[back.index()].speed();
                self.runback.charge_until = tick + u64::from(runback.charge_immunity_ticks);
                self.runback.charge_ready_at = tick + runback.charge_cooldown_ticks;
                self.players[back.index()].set_anim(AnimState::Shoulder);
                self.events.emit(SimEvent::RunbackMove {
                    runner: back,
                    move_code: wanted.code(),
                    speed,
                });
            }
            return;
        }
        let jump = wanted == RunbackMove::Jump;
        // Two gates, deliberately separate. Every move waits out the shared
        // recovery; only the jump additionally waits out its own cooldown, and
        // "already airborne" is folded into `jump_available` so a second leap
        // can never begin mid-arc.
        let allowed = match jump {
            true => self.runback.jump_available(tick),
            false => self.runback.move_available(tick),
        };
        if !allowed {
            return;
        }
        let runback = self.runback_tuning;
        let speed = self.players[back.index()].speed();
        let ends = tick
            + u64::from(match wanted {
                RunbackMove::JukeLeft | RunbackMove::JukeRight => runback.juke_ticks,
                // Unreachable: the charge returned above. Kept exhaustive so a
                // new move has to answer this question rather than inherit one.
                RunbackMove::Shoulder => runback.shoulder_ticks,
                // The leap ends when it lands, not on a timer; this is the
                // backstop that keeps a move from ever being unbounded.
                RunbackMove::Jump => runback.jump_cooldown_ticks as u32,
            });
        self.runback.active = Some(ActiveMove {
            kind: wanted,
            started: tick,
            ends,
        });
        match wanted {
            RunbackMove::JukeLeft | RunbackMove::JukeRight => self.begin_juke(back, wanted),
            RunbackMove::Shoulder => {}
            RunbackMove::Jump => self.begin_jump(back),
        }
        self.events.emit(SimEvent::RunbackMove {
            runner: back,
            move_code: wanted.code(),
            speed,
        });
    }

    /// Plant and cut: snapshot who the cut is being made against, scrub a little
    /// forward speed, and lock in the lateral direction the cut carries.
    fn begin_juke(&mut self, back: PlayerId, wanted: RunbackMove) {
        let runback = self.runback_tuning;
        self.runback.threats = evade::snapshot_threats(
            &self.players[back.index()],
            &self.players,
            self.tick,
            wanted.code(),
            &self.tuning,
            &runback,
        );
        self.runback.juke_dir = self
            .frame
            .right()
            .mul_scalar(wanted.juke_sign());
        let player = &mut self.players[back.index()];
        player.vel = player.vel.mul_scalar(runback.juke_forward_keep);
        player.set_anim(AnimState::Juke);
    }

    /// Leave the ground. Horizontal motion is untouched — the controller keeps
    /// running him forward through the whole arc, which is what makes timing a
    /// leap over an incoming defender useful rather than a stop.
    fn begin_jump(&mut self, back: PlayerId) {
        let runback = self.runback_tuning;
        self.runback.airborne = true;
        self.runback.vertical = runback.jump_launch_speed;
        self.runback.hurdles.clear();
        self.runback.jump_ready_at = self.tick + runback.jump_cooldown_ticks;
        self.players[back.index()].set_anim(AnimState::Leap);
    }

    /// Advance the live move by one tick.
    fn carry_move(&mut self, back: PlayerId) {
        let Some(active) = self.runback.active else {
            return;
        };
        match active.kind {
            RunbackMove::JukeLeft | RunbackMove::JukeRight => self.carry_juke(back, active),
            // The charge is not an active move — see `commit_move`.
            RunbackMove::Shoulder => {}
            RunbackMove::Jump => self.carry_jump(back),
        }
    }

    /// The cut's lateral displacement, applied as real movement on top of the
    /// run the controller integrated. Deliberately positional: writing it into
    /// `vel` instead would hand it straight back to the controller's steering,
    /// which would spend the next few ticks turning it off.
    fn carry_juke(&mut self, back: PlayerId, active: ActiveMove) {
        let runback = self.runback_tuning;
        let step = self.runback.juke_dir.mul_scalar(runback.juke_speed * DT);
        let player = &mut self.players[back.index()];
        player.pos = OffenseFrame::clamp_in_bounds(
            Vec3::new(player.pos.x + step.x, player.pos.y, player.pos.z + step.z),
            self.tuning.bounds_margin * 0.5,
        );
        if self.tick >= active.ends {
            self.finish_move(runback.juke_recovery_ticks);
        }
    }

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
    fn carry_charge(&mut self, back: PlayerId) {
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
    fn carry_jump(&mut self, back: PlayerId) {
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
    fn land_jump(&mut self, back: PlayerId) {
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
    fn finish_move(&mut self, recovery_ticks: u32) {
        self.runback.active = None;
        self.runback.ready_at = self.tick + u64::from(recovery_ticks);
    }

    /// Judge every pending dodge threat against what the field did.
    fn judge_threats(&mut self, back: PlayerId) {
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
