//! The goal: the frame the ball can hit, and the net it hits behind it.
//!
//! The frame is three capsules — two posts and a crossbar — and they are the
//! *same* capsules the renderer draws as cylinders, so "the ball hit the post"
//! and "the ball looks like it hit the post" cannot disagree.
//!
//! The net is a grid of short strands. It carries no cloth simulation: a strike
//! stamps one radial impulse into it and every strand reads its own displacement
//! out of that impulse's field, which is enough to make the goal *feel* scored
//! and costs one closed-form evaluation per strand.

use axiom::prelude::Vec3;

use crate::contact::{sweep, Capsule, Contact};

use super::coordinates::{GOAL_HALF_WIDTH, GOAL_HEIGHT, NET_DEPTH, POST_RADIUS};

/// Which part of the frame was struck.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameMember {
    LeftPost,
    RightPost,
    Crossbar,
}

/// A struck frame member and where.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameHit {
    pub member: FrameMember,
    pub contact: Contact,
}

/// The three capsules of the frame, in the order they are tested.
pub fn frame_capsules() -> [(FrameMember, Capsule); 3] {
    let top = GOAL_HEIGHT - POST_RADIUS;
    [
        (
            FrameMember::LeftPost,
            Capsule::new(
                Vec3::new(-GOAL_HALF_WIDTH, 0.0, 0.0),
                Vec3::new(-GOAL_HALF_WIDTH, top, 0.0),
                POST_RADIUS,
            ),
        ),
        (
            FrameMember::RightPost,
            Capsule::new(
                Vec3::new(GOAL_HALF_WIDTH, 0.0, 0.0),
                Vec3::new(GOAL_HALF_WIDTH, top, 0.0),
                POST_RADIUS,
            ),
        ),
        (
            FrameMember::Crossbar,
            Capsule::new(
                Vec3::new(-GOAL_HALF_WIDTH, GOAL_HEIGHT, 0.0),
                Vec3::new(GOAL_HALF_WIDTH, GOAL_HEIGHT, 0.0),
                POST_RADIUS,
            ),
        ),
    ]
}

/// Test one tick of ball travel against the frame, returning the earliest hit.
pub fn frame_hit(from: Vec3, to: Vec3, ball_radius: f32) -> Option<FrameHit> {
    frame_capsules()
        .into_iter()
        .filter_map(|(member, capsule)| {
            sweep(from, to, ball_radius, capsule).map(|contact| FrameHit { member, contact })
        })
        .min_by(|a, b| {
            a.contact
                .travel
                .partial_cmp(&b.contact.travel)
                .unwrap_or(core::cmp::Ordering::Equal)
        })
}

/// Whether a point on the goal plane is inside the mouth (used to tell a goal
/// from a shot that squeezed past the frame).
pub fn inside_mouth(point: Vec3, ball_radius: f32) -> bool {
    (point.x.abs() <= GOAL_HALF_WIDTH - POST_RADIUS + ball_radius)
        & (point.y <= GOAL_HEIGHT - POST_RADIUS + ball_radius)
        & (point.y >= -ball_radius)
}

/// Columns and rows of the back-net strand grid.
pub const NET_COLUMNS: usize = 15;
pub const NET_ROWS: usize = 7;
/// Strands down each side net.
pub const SIDE_COLUMNS: usize = 5;

/// One drawn piece of netting: where its centre rests, and how long it is along
/// its own axis. `horizontal` picks which way the strand lies.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NetStrand {
    pub rest: Vec3,
    pub length: f32,
    pub horizontal: bool,
    /// Which surface the strand belongs to: `0` back, `-1` left side, `+1` right
    /// side, `2` roof. The impulse response is softest on the back panel, which
    /// is the one that bulges.
    pub panel: i8,
}

/// Build the whole net as a strand list. Rebuilt never — the ripple moves the
/// strands, it does not re-create them.
pub fn net_strands() -> Vec<NetStrand> {
    let top = GOAL_HEIGHT - POST_RADIUS;
    let width = (GOAL_HALF_WIDTH - POST_RADIUS) * 2.0;
    let column_step = width / (NET_COLUMNS - 1) as f32;
    let row_step = top / (NET_ROWS - 1) as f32;
    let back_z = -NET_DEPTH;

    // Back panel: verticals then horizontals, on the plane the ball bulges.
    let verticals = (0..NET_COLUMNS).map(|c| NetStrand {
        rest: Vec3::new(
            -width * 0.5 + c as f32 * column_step,
            top * 0.5,
            back_z,
        ),
        length: top,
        horizontal: false,
        panel: 0,
    });
    let horizontals = (0..NET_ROWS).map(|r| NetStrand {
        rest: Vec3::new(0.0, r as f32 * row_step, back_z),
        length: width,
        horizontal: true,
        panel: 0,
    });

    // Side panels: verticals hanging from the roof line back to the ground, and
    // horizontal runs receding from the goal line to the back panel.
    let sides = [-1.0f32, 1.0].into_iter().flat_map(move |side| {
        let x = side * (GOAL_HALF_WIDTH - POST_RADIUS);
        let depth_step = NET_DEPTH / SIDE_COLUMNS as f32;
        let runs = (0..NET_ROWS).map(move |r| NetStrand {
            rest: Vec3::new(x, r as f32 * row_step, -NET_DEPTH * 0.5),
            length: NET_DEPTH,
            horizontal: true,
            panel: side as i8,
        });
        let posts = (1..=SIDE_COLUMNS).map(move |d| NetStrand {
            rest: Vec3::new(x, top * 0.5, -(d as f32) * depth_step),
            length: top,
            horizontal: false,
            panel: side as i8,
        });
        runs.chain(posts)
    });

    // Roof: runs across the top, sloping from the crossbar down to the back.
    let roof = (1..=SIDE_COLUMNS).map(move |d| NetStrand {
        rest: Vec3::new(
            0.0,
            top - (d as f32 / SIDE_COLUMNS as f32) * 0.32,
            -(d as f32) * (NET_DEPTH / SIDE_COLUMNS as f32),
        ),
        length: width,
        horizontal: true,
        panel: 2,
    });

    verticals
        .chain(horizontals)
        .chain(sides)
        .chain(roof)
        .collect()
}

/// A struck-net impulse: where the ball went in, how hard, and how long ago.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NetImpulse {
    pub point: Vec3,
    pub strength: f32,
    pub age: f32,
}

impl NetImpulse {
    /// How far this strand is pushed along `-Z` (deeper into the net) right now.
    ///
    /// A Gaussian bell around the entry point, ringing once and decaying — a
    /// closed-form standing wave rather than a solver. Side and roof panels take
    /// a fraction of it, so the bulge stays where the ball is.
    pub fn displacement(&self, strand: &NetStrand) -> f32 {
        let panel_gain = [0.30f32, 1.0, 0.30, 0.22][(strand.panel + 1).clamp(0, 3) as usize];
        let distance = strand.rest.subtract(self.point).length();
        let bell = (-(distance * distance) / 0.85).exp();
        let ring = (self.age * 21.0 - distance * 2.4).cos();
        let decay = (-self.age * 4.6).exp();
        // The first quarter-cycle is a pure push, not a ring: the net takes the
        // ball before it argues with it.
        let onset = (self.age * 26.0).min(1.0);
        -self.strength * panel_gain * bell * decay * onset * (0.55 + 0.45 * ring)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shot_into_the_post_is_reported_as_the_post() {
        let hit = frame_hit(
            Vec3::new(-3.55, 1.0, 0.45),
            Vec3::new(-3.72, 1.0, -0.45),
            0.11,
        )
        .expect("the path crosses the left post");
        assert_eq!(hit.member, FrameMember::LeftPost);
    }

    #[test]
    fn a_shot_into_the_bar_is_reported_as_the_bar() {
        let hit = frame_hit(Vec3::new(0.0, 2.4, 0.4), Vec3::new(0.0, 2.44, -0.4), 0.11)
            .expect("the path crosses the crossbar");
        assert_eq!(hit.member, FrameMember::Crossbar);
    }

    #[test]
    fn a_shot_through_the_middle_hits_nothing() {
        assert_eq!(
            frame_hit(Vec3::new(0.0, 1.2, 0.5), Vec3::new(0.0, 1.2, -0.5), 0.11),
            None
        );
        // ... and the right post is reachable too, so the search covers all three.
        assert_eq!(
            frame_hit(
                Vec3::new(3.55, 1.0, 0.45),
                Vec3::new(3.72, 1.0, -0.45),
                0.11
            )
            .map(|h| h.member),
            Some(FrameMember::RightPost)
        );
    }

    #[test]
    fn the_mouth_test_bounds_the_frame() {
        assert!(inside_mouth(Vec3::new(0.0, 1.0, 0.0), 0.11));
        assert!(!inside_mouth(Vec3::new(5.0, 1.0, 0.0), 0.11));
        assert!(!inside_mouth(Vec3::new(0.0, 3.0, 0.0), 0.11));
        assert!(!inside_mouth(Vec3::new(0.0, -0.5, 0.0), 0.11));
    }

    #[test]
    fn the_net_is_built_once_and_covers_every_panel() {
        let strands = net_strands();
        assert_eq!(
            strands.len(),
            NET_COLUMNS + NET_ROWS + 2 * (NET_ROWS + SIDE_COLUMNS) + SIDE_COLUMNS
        );
        assert!(strands.iter().any(|s| s.panel == 0));
        assert!(strands.iter().any(|s| s.panel == -1));
        assert!(strands.iter().any(|s| s.panel == 1));
        assert!(strands.iter().any(|s| s.panel == 2));
        assert!(strands.iter().all(|s| s.rest.z <= 0.0));
    }

    #[test]
    fn the_impulse_bulges_the_back_panel_where_the_ball_went_in_and_fades() {
        let strands = net_strands();
        let near = strands
            .iter()
            .filter(|s| s.panel == 0)
            .min_by(|a, b| {
                a.rest
                    .x
                    .abs()
                    .partial_cmp(&b.rest.x.abs())
                    .unwrap_or(core::cmp::Ordering::Equal)
            })
            .copied()
            .expect("a back strand near the middle");
        let fresh = NetImpulse {
            point: near.rest,
            strength: 0.5,
            age: 0.05,
        };
        assert!(fresh.displacement(&near) < 0.0, "the net is pushed back");
        let far = NetStrand {
            rest: Vec3::new(3.0, 2.0, -1.85),
            ..near
        };
        assert!(fresh.displacement(&far).abs() < fresh.displacement(&near).abs());
        let old = NetImpulse { age: 2.0, ..fresh };
        assert!(old.displacement(&near).abs() < 1.0e-3, "it settles");
        // A side strand answers the same impulse, but softly.
        let side = NetStrand { panel: 1, ..near };
        assert!(fresh.displacement(&side).abs() < fresh.displacement(&near).abs());
    }
}
