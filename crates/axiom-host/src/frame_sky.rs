//! Backend-neutral **sky**: a vertical gradient with an optional celestial body
//! (a moon or a sun) sitting in it, and a soft halo around that body.
//!
//! This exists because a flat clear colour cannot be a light. A night scene lit
//! only by a directional light and a hemisphere ambient has nothing in frame that
//! *is* the source — the sky is a uniform field, so the eye reads the whole image
//! as "dark" rather than "moonlit", however carefully the light values are tuned.
//! Giving the frame a real sky puts the source on screen, and gives the horizon a
//! colour for depth fog to fade into that is not the same colour as the zenith.
//!
//! Carried as neutral frame data — like [`crate::FrameAmbient`] and
//! [`crate::FrameDepthFog`] — so the *definition* of the sky is one piece of
//! arithmetic ([`FrameSky::radiance`]) that a backend either evaluates or
//! declares it dropped. [`Self::radiance`] is the reference implementation and is
//! what the GPU sky shader mirrors; keeping it here rather than only in WGSL is
//! what makes it testable without a GPU, and what would let a software backend
//! substitute it later without re-deriving the maths.
//!
//! Colours are **linear** RGB, like every other colour crossing this boundary.

use axiom_kernel::{Radians, Ratio};

/// A gradient sky with an optional celestial body.
///
/// The body is described by the *direction toward it* rather than a position:
/// a moon is effectively at infinity, so only its direction and angular size are
/// meaningful, and a direction cannot go stale as the camera moves through the
/// world the way a position would.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameSky {
    zenith: [f32; 3],
    horizon: [f32; 3],
    body_direction: [f32; 3],
    body_angular_radius: f32,
    body_color: [f32; 3],
    halo_falloff: f32,
    halo_strength: f32,
}

impl FrameSky {
    /// A plain gradient sky with no body in it: `zenith` overhead fading to
    /// `horizon` at eye level, both linear RGB.
    pub const fn gradient(zenith: [f32; 3], horizon: [f32; 3]) -> Self {
        FrameSky {
            zenith,
            horizon,
            // A body with zero angular radius and zero halo contributes nothing,
            // so "no body" needs no separate representation and no branch.
            body_direction: [0.0, 1.0, 0.0],
            body_angular_radius: 0.0,
            body_color: [0.0, 0.0, 0.0],
            halo_falloff: 1.0,
            halo_strength: 0.0,
        }
    }

    /// Place a celestial body in the sky.
    ///
    /// `direction` points **toward** the body and need not be normalised.
    /// `angular_radius` is in radians — the real moon is about `0.0045`, which
    /// is a convincing but very small disc; a game moon is usually several times
    /// that. `halo_falloff` is the exponent on the angular cosine, so larger is
    /// tighter; `halo_strength` scales the halo against the body's own colour.
    pub const fn with_body(
        mut self,
        direction: [f32; 3],
        angular_radius: Radians,
        color: [f32; 3],
        halo_falloff: Ratio,
        halo_strength: Ratio,
    ) -> Self {
        self.body_direction = direction;
        self.body_angular_radius = angular_radius.get();
        self.body_color = color;
        self.halo_falloff = halo_falloff.get();
        self.halo_strength = halo_strength.get();
        self
    }

    /// The overhead tint (linear RGB).
    pub const fn zenith(&self) -> [f32; 3] {
        self.zenith
    }

    /// The eye-level tint (linear RGB). Depth fog usually fades into this.
    pub const fn horizon(&self) -> [f32; 3] {
        self.horizon
    }

    /// The unit direction toward the celestial body.
    pub fn body_direction(&self) -> [f32; 3] {
        normalize_or(self.body_direction, [0.0, 1.0, 0.0])
    }

    /// The body's angular radius.
    pub const fn body_angular_radius(&self) -> Radians {
        Radians::finite_or_zero(self.body_angular_radius)
    }

    /// The body's linear-RGB colour.
    pub const fn body_color(&self) -> [f32; 3] {
        self.body_color
    }

    /// The halo's cosine exponent — larger hugs the disc more tightly.
    pub const fn halo_falloff(&self) -> Ratio {
        Ratio::finite_or_zero(self.halo_falloff)
    }

    /// How strongly the halo is added, against the body's colour.
    pub const fn halo_strength(&self) -> Ratio {
        Ratio::finite_or_zero(self.halo_strength)
    }

    /// **The sky, evaluated.** The linear-RGB radiance looking along `view`.
    ///
    /// Three terms, added: the vertical gradient, the body's disc, and the body's
    /// halo. All of it is pure arithmetic with no branches — which is required
    /// here (this is a layer) and is also exactly what makes it portable to a
    /// shader unchanged.
    ///
    /// The gradient uses the *raw* up-component rather than an angle, so it costs
    /// no trigonometry, and is smoothstepped so the horizon band is soft rather
    /// than a visible seam. Below the horizon it holds the horizon colour: there
    /// is no ground hemisphere here, because the ground is geometry.
    pub fn radiance(&self, view: [f32; 3]) -> [f32; 3] {
        let dir = normalize_or(view, [0.0, 1.0, 0.0]);
        let up = dir[1].clamp(0.0, 1.0);
        let blend = smoothstep(up);

        let cos_angle = dot(dir, self.body_direction());
        // The disc: a smooth step across the limb rather than a hard cut, so the
        // edge does not alias. The softness is a fraction of the radius, so a
        // bigger body gets a proportionally softer edge.
        let limb = self.body_angular_radius.max(MIN_ANGULAR_RADIUS);
        let inner = (limb * (1.0 - LIMB_SOFTNESS)).cos();
        let outer = limb.cos();
        let disc = inverse_lerp(outer, inner, cos_angle);
        // The halo: the angular cosine raised to a power, so it falls off around
        // the body without a second radius to keep in sync.
        let halo = cos_angle.max(0.0).powf(self.halo_falloff.max(1.0)) * self.halo_strength;
        let emission = disc + halo;

        [0, 1, 2].map(|c| {
            let gradient = lerp(self.horizon[c], self.zenith[c], blend);
            gradient + self.body_color[c] * emission
        })
    }
}

/// The smallest angular radius a body is evaluated at, so a zero-radius body
/// cannot produce a degenerate limb window.
const MIN_ANGULAR_RADIUS: f32 = 1.0e-4;

/// How much of the body's radius is spent softening its edge.
const LIMB_SOFTNESS: f32 = 0.25;

/// Hermite smoothstep on an already-`0..1` value.
fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Where `value` sits between `from` and `to`, clamped to `0..1`, for a window
/// running in either direction. A collapsed window degrades to a hard step at
/// `from` rather than dividing by zero — which cannot arise from
/// [`FrameSky::radiance`] anyway, since the limb window is built from a radius
/// floored at [`MIN_ANGULAR_RADIUS`].
fn inverse_lerp(from: f32, to: f32, value: f32) -> f32 {
    let span = to - from;
    let safe = span.abs().max(f32::EPSILON) * sign_or_positive(span);
    ((value - from) / safe).clamp(0.0, 1.0)
}

/// `1.0` for a positive or zero input, `-1.0` for a negative one — without a
/// branch, and without `signum`'s zero-handling surprise.
fn sign_or_positive(x: f32) -> f32 {
    1.0 - 2.0 * f32::from(x < 0.0)
}

/// Endpoint-exact linear interpolation.
///
/// `a + (b - a) * t` is the familiar form and it is *wrong at the ends*: at
/// `t = 1` it returns `a + (b - a)`, which for `0.1` and `0.5` is `0.099999994`
/// rather than `0.1`. Looking straight up must return the zenith colour exactly,
/// not almost — otherwise the sky and anything matched to it (the fog target,
/// the clear colour) disagree in the last bit.
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a * (1.0 - t) + b * t
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Normalise, falling back to `fallback` for a degenerate or poisoned vector.
///
/// The fallback is chosen by **table index**, not by arithmetic. The obvious
/// branchless form — `v * usable + fallback * (1 - usable)` — does not work
/// here: a NaN component multiplied by zero is still NaN, so a poisoned input
/// would sail straight through the guard that exists to catch it and poison
/// every pixel of the sky. Indexing never touches the bad value.
fn normalize_or(v: [f32; 3], fallback: [f32; 3]) -> [f32; 3] {
    let length = dot(v, v).sqrt();
    let usable = length.is_finite() & (length > f32::EPSILON);
    let scaled = [0, 1, 2].map(|c| v[c] / length.max(f32::EPSILON));
    [0, 1, 2].map(|c| [fallback[c], scaled[c]][usize::from(usable)])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests author the body's angle and halo as plain scalars.
    fn rad(v: f32) -> Radians {
        Radians::finite_or_zero(v)
    }

    fn q(v: f32) -> Ratio {
        Ratio::finite_or_zero(v)
    }

    fn moonlit() -> FrameSky {
        FrameSky::gradient([0.02, 0.03, 0.06], [0.06, 0.08, 0.13]).with_body(
            [0.0, 0.2, 1.0],
            rad(0.05),
            [0.9, 0.94, 1.0],
            q(180.0),
            q(0.5),
        )
    }

    #[test]
    fn a_plain_gradient_runs_from_horizon_to_zenith() {
        let sky = FrameSky::gradient([0.1, 0.2, 0.4], [0.5, 0.4, 0.3]);
        let overhead = sky.radiance([0.0, 1.0, 0.0]);
        let level = sky.radiance([0.0, 0.0, 1.0]);
        assert_eq!(overhead, [0.1, 0.2, 0.4], "straight up is the zenith");
        assert_eq!(level, [0.5, 0.4, 0.3], "the horizon is the horizon");
        // And it is monotone in between, on every channel.
        let mid = sky.radiance([0.0, 0.5, 0.75]);
        assert!(mid[0] > overhead[0] && mid[0] < level[0], "{mid:?}");
        assert!(mid[2] < overhead[2] && mid[2] > level[2], "{mid:?}");
    }

    #[test]
    fn below_the_horizon_holds_the_horizon_colour() {
        let sky = FrameSky::gradient([0.1, 0.2, 0.4], [0.5, 0.4, 0.3]);
        // The ground is geometry; the sky does not invent a second hemisphere.
        assert_eq!(sky.radiance([0.0, -1.0, 0.0]), [0.5, 0.4, 0.3]);
        assert_eq!(sky.radiance([0.0, -0.3, 0.95]), [0.5, 0.4, 0.3]);
    }

    #[test]
    fn a_gradient_sky_carries_no_body() {
        let sky = FrameSky::gradient([0.1, 0.2, 0.4], [0.5, 0.4, 0.3]);
        assert_eq!(sky.body_color(), [0.0, 0.0, 0.0]);
        assert_eq!(sky.halo_strength().get(), 0.0);
        // Looking exactly where a body would be is still just the gradient.
        assert_eq!(sky.radiance([0.0, 1.0, 0.0]), [0.1, 0.2, 0.4]);
    }

    #[test]
    fn the_body_is_brightest_at_its_centre_and_fades_outward() {
        let sky = moonlit();
        let centre = sky.radiance(sky.body_direction());
        let limb = sky.radiance([0.06, 0.2, 1.0]);
        let away = sky.radiance([1.0, 0.2, 0.0]);
        assert!(centre[0] > limb[0], "centre {centre:?} vs limb {limb:?}");
        assert!(limb[0] > away[0], "limb {limb:?} vs away {away:?}");
        // The disc genuinely reads as the body's colour, not a tint on the sky.
        assert!(centre[2] > 0.9, "the moon is bright: {centre:?}");
    }

    #[test]
    fn the_halo_reaches_beyond_the_disc_but_dies_away() {
        let sky = moonlit();
        // Just outside the disc: no disc term, but halo still present.
        let near = sky.radiance([0.12, 0.2, 1.0]);
        let far = sky.radiance([0.0, 1.0, 0.0]);
        assert!(near[0] > far[0], "halo near {near:?} vs far {far:?}");
        // A body with no halo has none of that reach.
        let bare = FrameSky::gradient([0.02, 0.03, 0.06], [0.06, 0.08, 0.13])
            .with_body([0.0, 0.2, 1.0], rad(0.05), [0.9, 0.94, 1.0], q(180.0), q(0.0));
        let bare_near = bare.radiance([0.12, 0.2, 1.0]);
        assert!(bare_near[0] < near[0], "no halo is dimmer outside the disc");
    }

    #[test]
    fn a_tighter_falloff_makes_a_smaller_halo() {
        let loose = FrameSky::gradient([0.0; 3], [0.0; 3])
            .with_body([0.0, 0.0, 1.0], rad(0.05), [1.0; 3], q(20.0), q(1.0));
        let tight = FrameSky::gradient([0.0; 3], [0.0; 3])
            .with_body([0.0, 0.0, 1.0], rad(0.05), [1.0; 3], q(400.0), q(1.0));
        let probe = [0.15, 0.0, 1.0];
        assert!(
            loose.radiance(probe)[0] > tight.radiance(probe)[0],
            "a tighter exponent hugs the disc"
        );
        // A falloff below one is clamped up rather than inverting the halo.
        let silly = FrameSky::gradient([0.0; 3], [0.0; 3])
            .with_body([0.0, 0.0, 1.0], rad(0.05), [1.0; 3], q(0.0), q(1.0));
        assert!(silly.radiance(probe)[0].is_finite());
        assert!(silly.radiance([1.0, 0.0, 0.0])[0] <= 1.0e-6, "and still dies at 90 degrees");
    }

    #[test]
    fn a_bigger_body_covers_more_sky() {
        let small = FrameSky::gradient([0.0; 3], [0.0; 3])
            .with_body([0.0, 0.0, 1.0], rad(0.02), [1.0; 3], q(4_000.0), q(0.0));
        let big = FrameSky::gradient([0.0; 3], [0.0; 3])
            .with_body([0.0, 0.0, 1.0], rad(0.10), [1.0; 3], q(4_000.0), q(0.0));
        let probe = [0.05, 0.0, 1.0];
        assert!(small.radiance(probe)[0] < 0.5, "outside the small disc");
        assert!(big.radiance(probe)[0] > 0.5, "inside the big one");
    }

    #[test]
    fn accessors_round_trip_and_the_direction_comes_back_normalised() {
        let sky = moonlit();
        assert_eq!(sky.zenith(), [0.02, 0.03, 0.06]);
        assert_eq!(sky.horizon(), [0.06, 0.08, 0.13]);
        assert_eq!(sky.body_angular_radius().get(), 0.05);
        assert_eq!(sky.body_color(), [0.9, 0.94, 1.0]);
        assert_eq!(sky.halo_falloff().get(), 180.0);
        assert_eq!(sky.halo_strength().get(), 0.5);
        let dir = sky.body_direction();
        let length = dot(dir, dir).sqrt();
        assert!((length - 1.0).abs() < 1.0e-5, "unit: {length}");
        assert!(format!("{sky:?}").contains("FrameSky"));
        assert_eq!(sky, moonlit());
        assert_ne!(sky, FrameSky::gradient([0.0; 3], [0.0; 3]));
    }

    /// A degenerate or poisoned input must produce a usable sky, not a NaN that
    /// propagates into every pixel of the frame.
    #[test]
    fn degenerate_directions_fall_back_instead_of_poisoning_the_frame() {
        let sky = moonlit();
        let finite = |c: [f32; 3]| c.iter().all(|v| v.is_finite());
        assert!(finite(sky.radiance([0.0, 0.0, 0.0])), "a zero view direction");
        assert!(finite(sky.radiance([f32::NAN, 1.0, 0.0])), "a poisoned one");
        assert!(finite(sky.radiance([f32::INFINITY, 0.0, 0.0])));
        // A zero body direction falls back to straight up rather than NaN.
        let headless = FrameSky::gradient([0.1; 3], [0.2; 3])
            .with_body([0.0, 0.0, 0.0], rad(0.05), [1.0; 3], q(100.0), q(1.0));
        assert_eq!(headless.body_direction(), [0.0, 1.0, 0.0]);
        assert!(finite(headless.radiance([0.0, 1.0, 0.0])));
    }

    #[test]
    fn the_helpers_behave_at_their_edges() {
        assert_eq!(smoothstep(-1.0), 0.0);
        assert_eq!(smoothstep(2.0), 1.0);
        assert!((smoothstep(0.5) - 0.5).abs() < 1.0e-6);
        assert_eq!(inverse_lerp(0.0, 1.0, -5.0), 0.0);
        assert_eq!(inverse_lerp(0.0, 1.0, 5.0), 1.0);
        // A collapsed window is a hard step at `from`, not a division by zero.
        assert_eq!(inverse_lerp(1.0, 1.0, 1.0), 0.0);
        assert_eq!(inverse_lerp(1.0, 1.0, 2.0), 1.0);
        assert_eq!(inverse_lerp(1.0, 0.0, -1.0), 1.0, "a descending window still works");
        assert_eq!(inverse_lerp(1.0, 0.0, 2.0), 0.0);
        assert_eq!(sign_or_positive(0.0), 1.0);
        assert_eq!(sign_or_positive(3.0), 1.0);
        assert_eq!(sign_or_positive(-3.0), -1.0);
        assert_eq!(lerp(2.0, 4.0, 0.5), 3.0);
        assert_eq!(normalize_or([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]), [1.0, 0.0, 0.0]);
        assert_eq!(normalize_or([0.0, 3.0, 0.0], [1.0, 0.0, 0.0]), [0.0, 1.0, 0.0]);
    }
}
