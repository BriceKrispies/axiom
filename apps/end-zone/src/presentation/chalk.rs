//! Pre-snap route chalk: the selected play's offensive routes drawn on the turf
//! as dotted chalk lines, so the player sees the play they called laid out on
//! the field before the snap — the field twin of the huddle's chalkboard.
//!
//! The simulation publishes the routes on the snapshot (only while the offense
//! is set pre-snap; see [`super::snapshot`]); this module only turns each
//! polyline into pooled chalk dots, procedural like every ring and juice effect.

use axiom::prelude::Vec3;
use axiom_math::{Quat, Transform};

use super::snapshot::PresentationSnapshot;

/// Which chalk a dot belongs to: an ordinary route, or the primary read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChalkMaterial {
    Line,
    Primary,
}

/// One chalk dot: where it sits and which line it belongs to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChalkDot {
    pub transform: Transform,
    pub material: ChalkMaterial,
}

/// Interpolated dots per route segment (the segment endpoints are drawn too).
const STEPS: usize = 6;
/// Height above the turf, yd — clear of the surface so it reads as painted-on
/// but sits above the yard lines it crosses.
const CHALK_HEIGHT: f32 = 0.12;
/// Dot size along a segment / at the final waypoint, yd. Chunky so the route
/// reads from the low arcade camera despite downfield foreshortening.
const DOT_SIZE: f32 = 0.32;
const WAYPOINT_SIZE: f32 = 0.62;

/// Pool headroom: 3-4 route runners, ~2 segments each, STEPS dots + waypoints.
pub const CHALK_LINE_POOL: usize = 220;
pub const CHALK_PRIMARY_POOL: usize = 72;

fn dot(pos: Vec3, size: f32, material: ChalkMaterial) -> ChalkDot {
    ChalkDot {
        transform: Transform::new(
            Vec3::new(pos.x, CHALK_HEIGHT, pos.z),
            Quat::IDENTITY,
            Vec3::new(size, 0.06, size),
        ),
        material,
    }
}

/// Build the chalk dots for this tick's pre-snap routes (empty otherwise).
pub fn chalk_instances(snapshot: &PresentationSnapshot, out: &mut Vec<ChalkDot>) {
    out.clear();
    for route in &snapshot.pre_snap_routes {
        let material = if route.primary {
            ChalkMaterial::Primary
        } else {
            ChalkMaterial::Line
        };
        let Some((&start, rest)) = route.points.split_first() else {
            continue;
        };
        out.push(dot(start, DOT_SIZE, material));
        let mut previous = start;
        for (index, waypoint) in rest.iter().enumerate() {
            let last = index + 1 == rest.len();
            for step in 1..STEPS {
                let t = step as f32 / STEPS as f32;
                let between = Vec3::new(
                    previous.x + (waypoint.x - previous.x) * t,
                    0.0,
                    previous.z + (waypoint.z - previous.z) * t,
                );
                out.push(dot(between, DOT_SIZE, material));
            }
            let size = if last { WAYPOINT_SIZE } else { DOT_SIZE * 1.4 };
            out.push(dot(*waypoint, size, material));
            previous = *waypoint;
        }
    }
}
