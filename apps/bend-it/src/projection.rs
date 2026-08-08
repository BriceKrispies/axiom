//! Screen ↔ world, built from the same camera the renderer is given.
//!
//! Two directions, and the game needs both. **Forward** puts the goal's corners,
//! the authored path and the keeper's read on the screen so the overlay can
//! outline them. **Backward** turns a touch into a point on the goal plane,
//! which is how "put it in that corner" becomes a [`GoalTarget`].
//!
//! The matrices are composed exactly as the engine composes them — a
//! right-handed `look_at` under a right-handed perspective at the surface's own
//! aspect — so a finger on a post is a finger on *that* post and not on a
//! plausible-looking approximation of it.
//!
//! [`GoalTarget`]: crate::shot::GoalTarget

use axiom::prelude::{Mat4, Vec2, Vec3, Vec4};

use crate::camera::CameraPose;

/// A camera resolved into a screen mapping.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenProjection {
    clip_from_world: Mat4,
    world_from_clip: Mat4,
    eye: Vec3,
    viewport: Vec2,
}

impl ScreenProjection {
    /// Resolve a camera against a surface, in physical pixels.
    pub fn new(camera: &CameraPose, viewport: Vec2) -> ScreenProjection {
        let viewport = Vec2::new(viewport.x.max(1.0), viewport.y.max(1.0));
        let view = Mat4::look_at(camera.eye, camera.target, Vec3::UNIT_Y)
            .unwrap_or(Mat4::IDENTITY);
        let projection = Mat4::perspective(
            camera.fov_degrees.clamp(1.0, 179.0).to_radians(),
            viewport.x / viewport.y,
            0.1,
            400.0,
        )
        .unwrap_or(Mat4::IDENTITY);
        let clip_from_world = projection.multiply(view);
        ScreenProjection {
            world_from_clip: clip_from_world.inverse().unwrap_or(Mat4::IDENTITY),
            clip_from_world,
            eye: camera.eye,
            viewport,
        }
    }

    /// The surface this mapping is measured in, physical pixels.
    pub fn viewport(&self) -> Vec2 {
        self.viewport
    }

    /// Project a world point to physical pixels. `None` when it is behind the
    /// camera, which callers use to drop a segment rather than draw it folded.
    pub fn project(&self, world: Vec3) -> Option<Vec2> {
        let clip = self
            .clip_from_world
            .transform_vec4(Vec4::new(world.x, world.y, world.z, 1.0));
        (clip.w > 1.0e-4).then(|| {
            let ndc = Vec2::new(clip.x / clip.w, clip.y / clip.w);
            Vec2::new(
                (ndc.x * 0.5 + 0.5) * self.viewport.x,
                (0.5 - ndc.y * 0.5) * self.viewport.y,
            )
        })
    }

    /// The world-space ray under a screen point.
    pub fn ray(&self, screen: Vec2) -> (Vec3, Vec3) {
        let ndc = Vec2::new(
            (screen.x / self.viewport.x) * 2.0 - 1.0,
            1.0 - (screen.y / self.viewport.y) * 2.0,
        );
        let unproject = |depth: f32| {
            let p = self
                .world_from_clip
                .transform_vec4(Vec4::new(ndc.x, ndc.y, depth, 1.0));
            let w = [p.w, 1.0][usize::from(p.w.abs() < 1.0e-6)];
            Vec3::new(p.x / w, p.y / w, p.z / w)
        };
        let near = unproject(-1.0);
        let direction = unproject(1.0)
            .subtract(near)
            .normalize()
            .unwrap_or(Vec3::new(0.0, 0.0, -1.0));
        (near, direction)
    }

    /// Where a screen point lands on the goal plane (`z = 0`).
    ///
    /// `None` only when the ray runs parallel to or away from the plane — which
    /// this game's camera, planted behind the ball and aimed at the goal, never
    /// does. Everything else, including a touch well outside the posts, returns a
    /// point: clamping it into the mouth is the caller's job, and doing it there
    /// rather than here is what makes a sloppy touch land on the nearest legal
    /// corner instead of doing nothing at all.
    pub fn goal_plane_hit(&self, screen: Vec2) -> Option<Vec3> {
        let (origin, direction) = self.ray(screen);
        (direction.z < -1.0e-5).then(|| {
            let t = -origin.z / direction.z;
            origin.add(direction.mul_scalar(t))
        })
    }

    /// Roughly how many pixels one metre spans at a world point — the number the
    /// overlay uses to size a handle so it stays the same physical size on the
    /// screen however far away the thing it marks is.
    pub fn pixels_per_metre(&self, at: Vec3) -> f32 {
        let along = self.eye.subtract(at).normalize().unwrap_or(Vec3::UNIT_Y);
        let sideways = along.cross(Vec3::UNIT_Y).normalize().unwrap_or(Vec3::UNIT_X);
        self.project(at)
            .zip(self.project(at.add(sideways)))
            .map(|(a, b)| b.subtract(a).length())
            .unwrap_or(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera;
    use crate::pitch::{ball_spot, GoalMouth, GOAL_HALF_WIDTH, GOAL_HEIGHT};
    use crate::tuning::Tuning;

    fn projection(w: f32, h: f32) -> ScreenProjection {
        let tuning = Tuning::DEFAULT;
        let pose = camera::frame(
            Vec2::new(w, h),
            &GoalMouth::new(tuning.goal.inset),
            ball_spot(tuning.flight.ball_radius),
            Vec3::new(-1.04, 0.0, 14.2),
            0.0,
            &tuning.camera,
        );
        ScreenProjection::new(&pose, Vec2::new(w, h))
    }

    #[test]
    fn a_touch_on_the_goal_maps_back_to_the_point_it_touched() {
        let p = projection(390.0, 844.0);
        for target in [
            Vec3::new(0.0, 1.2, 0.0),
            Vec3::new(-3.0, 0.3, 0.0),
            Vec3::new(3.0, 2.2, 0.0),
        ] {
            let screen = p.project(target).expect("the goal is on screen");
            let back = p.goal_plane_hit(screen).expect("the ray meets the plane");
            assert!(
                back.subtract(target).length() < 0.02,
                "{target:?} round-tripped to {back:?}"
            );
        }
    }

    #[test]
    fn world_plus_x_is_screen_right_and_world_up_is_screen_up() {
        let p = projection(390.0, 844.0);
        let centre = p.project(Vec3::new(0.0, 1.0, 0.0)).expect("on screen");
        let right = p.project(Vec3::new(2.0, 1.0, 0.0)).expect("on screen");
        let above = p.project(Vec3::new(0.0, 2.2, 0.0)).expect("on screen");
        assert!(right.x > centre.x, "+X must draw to the right");
        assert!(above.y < centre.y, "+Y must draw upward (screen y is down)");
    }

    #[test]
    fn the_goal_fills_a_readable_part_of_a_phone_screen() {
        let p = projection(390.0, 844.0);
        let left = p.project(Vec3::new(-GOAL_HALF_WIDTH, 0.0, 0.0)).expect("on");
        let right = p.project(Vec3::new(GOAL_HALF_WIDTH, 0.0, 0.0)).expect("on");
        let top = p.project(Vec3::new(0.0, GOAL_HEIGHT, 0.0)).expect("on");
        let base = p.project(Vec3::new(0.0, 0.0, 0.0)).expect("on");
        assert!(
            right.x - left.x > 390.0 * 0.55,
            "the goal is only {} px wide",
            right.x - left.x
        );
        assert!(base.y - top.y > 60.0, "the goal is only {} px tall", base.y - top.y);
        // ... and it sits in the upper half of the screen.
        assert!(base.y < 844.0 * 0.62, "the goal line is at {}", base.y);
    }

    #[test]
    fn a_point_behind_the_camera_does_not_project() {
        let p = projection(390.0, 844.0);
        assert_eq!(p.project(Vec3::new(0.0, 1.0, 400.0)), None);
        assert!(p.pixels_per_metre(Vec3::new(0.0, 1.0, 0.0)) > 10.0);
        // A degenerate viewport does not divide by zero.
        let tiny = ScreenProjection::new(
            &crate::camera::CameraPose {
                eye: Vec3::new(0.0, 3.0, 17.0),
                target: Vec3::new(0.0, 1.3, 3.0),
                fov_degrees: 55.0,
            },
            Vec2::ZERO,
        );
        assert_eq!(tiny.viewport(), Vec2::ONE);
    }

    #[test]
    fn a_touch_well_outside_the_posts_still_lands_on_the_plane() {
        let p = projection(390.0, 844.0);
        let hit = p
            .goal_plane_hit(Vec2::new(2.0, 40.0))
            .expect("a ray from the top corner of the screen still meets the plane");
        assert!(hit.z.abs() < 1.0e-3);
        assert!(hit.x < -GOAL_HALF_WIDTH, "it lands wide, ready to be clamped");
    }
}
