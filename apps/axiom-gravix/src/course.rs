//! The **course**: a deterministic chain of shallow half-pipe segments that forms
//! the playable downhill run, plus the spawn, finish gate, and kill plane.
//!
//! Each [`Segment`] is a [`HalfPipeParams`] channel placed by a world `Transform`
//! (position + heading yaw + downhill pitch). Segments chain end-to-start so the
//! ball keeps its momentum across joints. The layout is a pure function of named
//! constants — no ambient randomness — so the course replays identically.

use axiom::prelude::{Transform, Vec3};
use axiom_math::Quat;

use crate::halfpipe::HalfPipeParams;
use crate::settings;

/// One placed half-pipe segment: its shape, its world pose, and the derived local
/// axes (run `forward` and surface `up`) the game reads for chaining + placement.
#[derive(Debug, Clone, Copy)]
pub struct Segment {
    pub params: HalfPipeParams,
    pub center: Vec3,
    pub rotation: Quat,
    pub forward: Vec3,
    pub up: Vec3,
    /// `true` for the uphill segment that rewards a spin-launch (debug/inspection).
    pub is_launch_reward: bool,
}

impl Segment {
    /// Place a segment centred at `center`, running along `heading` (yaw about +Y)
    /// and tilted down by `pitch` (positive = downhill, negative = uphill).
    fn placed(
        params: HalfPipeParams,
        center: Vec3,
        heading: f32,
        pitch: f32,
        is_launch_reward: bool,
    ) -> Self {
        let rotation = Quat::from_axis_angle(Vec3::UNIT_Y, heading)
            .expect("finite heading")
            .multiply(Quat::from_axis_angle(Vec3::UNIT_X, pitch).expect("finite pitch"));
        let forward = rotation.rotate(Vec3::UNIT_Z);
        let up = rotation.rotate(Vec3::UNIT_Y);
        Segment {
            params,
            center,
            rotation,
            forward,
            up,
            is_launch_reward,
        }
    }

    fn half_len(&self) -> f32 {
        self.params.half_extents().1
    }

    /// The world point at the centre of the segment's near (start) edge.
    pub fn start(&self) -> Vec3 {
        self.center
            .subtract(self.forward.mul_scalar(self.half_len()))
    }

    /// The world point at the centre of the segment's far (end) edge.
    pub fn end(&self) -> Vec3 {
        self.center.add(self.forward.mul_scalar(self.half_len()))
    }

    /// The world Transform the renderer + physics place the segment mesh/collider at.
    pub fn transform(&self) -> Transform {
        Transform::new(self.center, self.rotation, Vec3::ONE)
    }

    /// The world point on the channel-centre surface at the segment's end (finish /
    /// marker placement): the end edge lifted by the channel-centre height.
    pub fn end_surface(&self) -> Vec3 {
        self.end()
            .add(self.up.mul_scalar(self.params.centre_height()))
    }

    /// The world point on the channel-centre surface at the segment's start.
    pub fn start_surface(&self) -> Vec3 {
        self.start()
            .add(self.up.mul_scalar(self.params.centre_height()))
    }
}

/// The whole course: spawn, the ordered segment chain, the finish-gate centre, and
/// the kill plane below which a fall resets.
#[derive(Debug, Clone)]
pub struct Course {
    pub spawn: Vec3,
    pub segments: Vec<Segment>,
    pub finish: Vec3,
    pub kill_plane_y: f32,
}

/// Build the deterministic course: three chained downhill half-pipe segments (with
/// gentle bends), one flatter recovery segment, and one uphill segment that rewards
/// the spin-launch, ending at a finish gate.
pub fn generate() -> Course {
    // (length, heading-delta, pitch, is_launch_reward). Positive pitch = downhill.
    let specs: [(f32, f32, f32, bool); 5] = [
        (46.0, 0.00, 0.24, false),  // downhill 1
        (46.0, 0.22, 0.30, false),  // downhill 2 (bends right, steeper)
        (46.0, -0.18, 0.26, false), // downhill 3 (bends back)
        (32.0, 0.00, 0.04, false),  // flatter recovery
        (36.0, 0.00, -0.16, true),  // uphill: needs a spin-launch to clear
    ];

    let mut segments: Vec<Segment> = Vec::new();
    let mut heading = 0.0_f32;
    // The first segment's start edge sits here, high up; the course descends.
    let mut cursor = Vec3::new(0.0, 22.0, -4.0);
    let mut min_y = cursor.y;

    for &(length, dheading, pitch, reward) in &specs {
        heading += dheading;
        let params = HalfPipeParams::straight(length);
        // Provisional placement to read `forward`, then set `center` so the start
        // edge lands exactly on the running cursor (segments chain end→start).
        let probe = Segment::placed(params, cursor, heading, pitch, reward);
        let center = cursor.add(probe.forward.mul_scalar(probe.half_len()));
        let seg = Segment::placed(params, center, heading, pitch, reward);
        cursor = seg.end();
        min_y = min_y.min(seg.end().y).min(seg.center.y);
        segments.push(seg);
    }

    let first = segments[0];
    let last = *segments.last().unwrap();
    let spawn = first
        .start_surface()
        .add(first.up.mul_scalar(settings::BALL_RADIUS + 0.6));
    let finish = last.end_surface();

    Course {
        spawn,
        segments,
        finish,
        kill_plane_y: min_y - settings::KILL_PLANE_DROP,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_course_has_the_required_segments_and_a_launch_reward() {
        let c = generate();
        assert_eq!(c.segments.len(), 5, "3 downhill + recovery + launch-reward");
        // At least three descend (forward points downward), one is ~flat, one climbs.
        let downhill = c.segments.iter().filter(|s| s.forward.y < -0.05).count();
        assert!(
            downhill >= 3,
            "at least three downhill segments, got {downhill}"
        );
        assert!(
            c.segments
                .iter()
                .any(|s| s.is_launch_reward && s.forward.y > 0.02),
            "an uphill launch-reward segment"
        );
    }

    #[test]
    fn segments_chain_end_to_start_and_descend() {
        let c = generate();
        // Each segment's start edge coincides with the previous segment's end edge.
        for pair in c.segments.windows(2) {
            let gap = pair[0].end().subtract(pair[1].start()).length();
            assert!(gap < 1.0e-3, "segments are continuous, gap = {gap}");
        }
        // The run descends overall: the finish is well below the spawn.
        assert!(
            c.finish.y < c.spawn.y - 8.0,
            "the course drops: spawn {} -> finish {}",
            c.spawn.y,
            c.finish.y
        );
        // The kill plane sits below the whole track.
        assert!(c.kill_plane_y < c.finish.y - 5.0);
    }

    #[test]
    fn the_spawn_sits_above_the_first_segment_surface() {
        let c = generate();
        let s0 = c.segments[0];
        // The spawn is lifted above the start surface along the segment up axis.
        let lift = c.spawn.subtract(s0.start_surface()).dot(s0.up);
        assert!(
            lift > settings::BALL_RADIUS,
            "spawn rests above the surface, lift = {lift}"
        );
        assert_eq!(s0.transform().translation, s0.center);
    }
}
