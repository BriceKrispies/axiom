//! Player-vs-player contact as real rigid-body de-penetration over the
//! `physical-animation` crowd surface: one dynamic collision sphere per player
//! sharing a single deterministic world. Each tick every standing player is
//! snapped to its authoritative `(position, velocity)` and the world is stepped
//! once; only the *collision-induced delta* — the de-penetration push plus the
//! momentum the solver exchanged — is written back. A player with no neighbor
//! in contact reads back exactly its snapped free-flight, so open-field motion
//! is left bit-identical.
//!
//! This replaces the old positional-only `resolve_overlaps`: colliding players
//! now trade momentum (a defender running into a blocker is genuinely slowed and
//! has to rebuild speed), not merely separate. The isolation of *just* the
//! collision response is what keeps the scripted showcase stable — nothing moves
//! a free receiver off his route.
//!
//! The shared crowd world (`PhysicsApi::new()` defaults) has gravity but no
//! damping and no ground plane. Zero damping is the load-bearing property: with
//! no drag and no horizontal force, a non-contacting body's XZ state after one
//! step is exactly its snapped state advanced by `vel * dt`, so subtracting that
//! free-flight baseline leaves only the contact response. Gravity acts on Y
//! alone; every sphere is pinned to one height on snap and the Y read-back is
//! discarded, so de-penetration stays purely horizontal — matching the old
//! XZ-only overlap resolve.
//!
//! Faults are recorded, never panicked on (`unwrap`/`expect`-free), exactly like
//! [`crate::physics_rig::PhysicsRig`].

use axiom::prelude::Vec3;
use axiom_kernel::{Meters, Tick};
use axiom_physical_animation::{HumanoidHandle, PhysicalAnimationApi};

use crate::config::DT;
use crate::identity::PlayerId;
use crate::player::{AnimState, PlayerSim};

/// The height (yards) every collision sphere's center is pinned to on snap, so
/// the spheres stay coplanar and push apart purely in XZ.
const COLLISION_Y: f32 = 1.0;

/// Park a non-acting (stumbling/airborne/downed) player's body far out of play
/// so it collides with no one. Spread per index so parked bodies never overlap
/// one another and manufacture spurious contacts.
fn park(index: usize) -> Vec3 {
    Vec3::new(0.0, -1000.0 - index as f32 * 10.0, 0.0)
}

/// The player-collision world: one bare dynamic sphere per player, indexed in
/// lockstep with the sim's player array. A body is `None` only if its bind
/// failed at construction (recorded in `fault`); that player simply skips
/// de-penetration rather than panicking.
#[derive(Debug)]
pub struct CollisionRig {
    crowd: PhysicalAnimationApi,
    /// One handle per player slot (`None` = bind failed). Stored as `Option`
    /// because `HumanoidHandle` is construct-only inside the module.
    bodies: Vec<Option<HumanoidHandle>>,
    /// First recorded fault (subsystem: operation); inspectable, never fatal.
    pub fault: Option<&'static str>,
}

impl CollisionRig {
    /// Build the world: one collision sphere per player at its formation spot,
    /// sized to that player's body radius.
    pub fn new(players: &[PlayerSim]) -> Self {
        let mut crowd = PhysicalAnimationApi::new();
        let mut fault = None;
        let bodies = players
            .iter()
            .map(|player| {
                let origin = Vec3::new(player.pos.x, COLLISION_Y, player.pos.z);
                let radius = Meters::finite_or_zero(player.archetype.body_radius);
                match crowd.bind_colliding_body(origin, radius) {
                    Ok(handle) => Some(handle),
                    Err(_) => {
                        fault = fault.or(Some("collision_rig: bind body"));
                        None
                    }
                }
            })
            .collect();
        CollisionRig {
            crowd,
            bodies,
            fault,
        }
    }

    /// Record the first fault from a crowd operation.
    fn note(&mut self, op: &'static str, ok: bool) {
        if !ok && self.fault.is_none() {
            self.fault = Some(op);
        }
    }

    /// Resolve one tick of player-vs-player contact in place of the old
    /// positional overlap pass: snap the standing players to their authoritative
    /// state, park the rest, step the shared world once, and add back only the
    /// collision delta to the standing players.
    pub fn resolve(&mut self, players: &mut [PlayerSim], tick: u64) {
        let placements: Vec<(HumanoidHandle, Vec3, Vec3)> = self
            .bodies
            .iter()
            .zip(players.iter())
            .enumerate()
            .filter_map(|(index, (handle, player))| {
                handle.map(|handle| {
                    // Standing players AND committed divers are placed in the
                    // world at their real 3D position (the diver's arc height
                    // lifts its body above the grounded carrier, so a dive that
                    // sails over the top stays out of contact). Only the truly
                    // down / recovering are parked out of play. Divers are not
                    // read back — their motion is owned by the contact framework;
                    // they are here only so their body can actually touch the
                    // carrier's.
                    if player.anim.can_act() || player.anim == AnimState::Dive {
                        (
                            handle,
                            Vec3::new(player.pos.x, COLLISION_Y + player.pos.y, player.pos.z),
                            Vec3::new(player.vel.x, 0.0, player.vel.z),
                        )
                    } else {
                        (handle, park(index), Vec3::ZERO)
                    }
                })
            })
            .collect();

        let stepped = self.crowd.resolve_crowd(&placements, Tick::new(tick)).is_ok();
        self.note("collision_rig: resolve", stepped);
        if !stepped {
            return;
        }

        for (handle, player) in self.bodies.iter().zip(players.iter_mut()) {
            if !player.anim.can_act() {
                continue;
            }
            let Some(handle) = handle else {
                continue;
            };
            let read = self
                .crowd
                .crowd_pelvis_transform(*handle)
                .and_then(|pose| {
                    self.crowd
                        .crowd_pelvis_velocity(*handle)
                        .map(|vel| (pose.translation, vel))
                });
            let (resolved_pos, resolved_vel) = match read {
                Ok(pair) => pair,
                Err(_) => {
                    if self.fault.is_none() {
                        self.fault = Some("collision_rig: readback");
                    }
                    continue;
                }
            };
            // Free-flight XZ (zero damping, no horizontal force) is exactly the
            // snapped state advanced one step; the surplus is the contact push.
            let free_x = player.pos.x + player.vel.x * DT;
            let free_z = player.pos.z + player.vel.z * DT;
            player.pos = Vec3::new(
                player.pos.x + (resolved_pos.x - free_x),
                player.pos.y,
                player.pos.z + (resolved_pos.z - free_z),
            );
            player.vel = Vec3::new(resolved_vel.x, player.vel.y, resolved_vel.z);
        }
    }

    /// Whether players `a` and `b` had their collision bodies actually touching
    /// on the last [`Self::resolve`] step — the authoritative, physics-driven
    /// "are these two bodies colliding?" used to land tackles from real contact
    /// instead of a distance heuristic. `false` if either body failed to bind.
    pub fn in_contact(&self, a: PlayerId, b: PlayerId) -> bool {
        match (
            self.bodies.get(a.index()).copied().flatten(),
            self.bodies.get(b.index()).copied().flatten(),
        ) {
            (Some(ha), Some(hb)) => self.crowd.crowd_bodies_in_contact(ha, hb),
            _ => false,
        }
    }
}
