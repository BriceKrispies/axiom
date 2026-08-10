//! Backend-neutral **sky**: a vertical gradient with an optional celestial body
//! (a moon or a sun) sitting in it, a soft halo around that body, and an optional
//! layer of cloud drawn in front of both.
//!
//! This exists because a flat clear colour cannot be a light. A night scene lit
//! only by a directional light and a hemisphere ambient has nothing in frame that
//! *is* the source — the sky is a uniform field, so the eye reads the whole image
//! as "dark" rather than "moonlit", however carefully the light values are tuned.
//! Giving the frame a real sky puts the source on screen, and gives the horizon a
//! colour for depth fog to fade into that is not the same colour as the zenith.
//!
//! The cloud layer exists for the same reason one level up. A gradient plus a body
//! is a sky with *nothing in it*: however well the two colours are chosen, an
//! outdoor frame whose upper half is a clean two-stop wash reads as a backdrop
//! rather than as weather, and no amount of scene geometry below the horizon fixes
//! that. Cloud belongs **here**, in the sky's own evaluation, and not in the app as
//! billboard cards: cards need alpha the rasterizer does not have, would sort
//! against the depth fog, and — being ordinary textured geometry — would survive on
//! a backend that has already declared it drops [`crate::RenderCapability::Sky`],
//! which is precisely the silent divergence the capability system exists to
//! prevent. Carried here, cloud degrades with the sky it belongs to, by
//! declaration.
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
    cloud_coverage: f32,
    cloud_scale: f32,
    haze_height: f32,
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
            // Zero coverage puts the density threshold above the field's maximum,
            // so "no cloud" is exactly zero everywhere and needs no branch and no
            // separate representation — the same posture the body takes.
            cloud_coverage: 0.0,
            cloud_scale: 1.0,
            // The identity haze height: `haze_lift` collapses to `up` exactly at
            // this value, so a sky that never mentions the parameter evaluates
            // bit-for-bit as it did before the parameter existed.
            haze_height: DEFAULT_HAZE_HEIGHT,
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

    /// Put a layer of cloud in the sky.
    ///
    /// `coverage` runs `0` (a clear sky — the default, and exactly clear, not
    /// nearly) to `1` (overcast). `scale` sets how large the lumps read: it
    /// multiplies the cloud plane's coordinates, so *larger* is *smaller and
    /// busier* cloud. Around `0.5` gives the broad cumulus of a wide outdoor shot.
    ///
    /// The cloud takes no colour of its own. It is lit by the two things the sky
    /// already carries — the gradient behind it fills its shaded body, and the
    /// body's colour lights its sunward face — so one authored cloud layer is
    /// correct under a moon and under a midday sun without being re-tuned, and a
    /// cloud can never disagree with the sky it is sitting in.
    pub const fn with_clouds(mut self, coverage: Ratio, scale: Ratio) -> Self {
        self.cloud_coverage = coverage.get();
        self.cloud_scale = scale.get();
        self
    }

    /// Set how high the **horizon haze** reaches — the gradient's *shape*, as
    /// distinct from the two colours it runs between.
    ///
    /// `half_height` is the up-component (the sine of the elevation angle) at
    /// which the gradient stands exactly halfway between [`Self::horizon`] and
    /// [`Self::zenith`]. The default is `0.5` — 30° of elevation — which is
    /// precisely the plain `smoothstep(dir.y)` this evaluated before the
    /// parameter existed, so an existing sky is unchanged to the bit.
    ///
    /// ## Why the two stops were not enough on their own
    ///
    /// A two-stop dome parameterised on the raw up-component puts its midpoint
    /// at 30° and cannot be moved. That is fine for a camera that looks at the
    /// sky and wrong for every camera that does not: a chase camera on a road,
    /// a third-person camera behind a runner, anything shot near eye level sees
    /// only the *bottom* of the curve. Concretely, a frame whose top row sits
    /// 32° up reaches a blend of `0.57` and no further — the authored zenith is
    /// never reached anywhere on screen, and more than half of what the frame
    /// shows is the horizon stop.
    ///
    /// The app-side workaround is to stop authoring the zenith as a colour and
    /// start authoring it as a slope: push it far past the darkest pixel it is
    /// meant to produce so that 57% of it lands somewhere near right. That is a
    /// lie with two costs. It runs out — a channel cannot be pushed below zero,
    /// so once the zenith stop bottoms out the gradient simply cannot be made
    /// any steeper, whatever the frame needs. And it makes the sky wrong for
    /// any camera that ever looks up, and wrong for anything matched to it (the
    /// depth-fog target, the hemisphere ambient), because the number in the
    /// zenith slot is no longer the colour of the sky overhead.
    ///
    /// Real air does not have its midpoint at 30° either. Optical depth along a
    /// ray goes as roughly `1 / sin(elevation)`, so the haze band is *tight*:
    /// a clear day collapses to its zenith blue within the first 15–20° and
    /// holds it the rest of the way up. Carrying the band's height here — one
    /// authored number, with the shape it implies evaluated in one place — is
    /// what lets the zenith stop go back to meaning the sky overhead.
    pub const fn with_haze_height(mut self, half_height: Ratio) -> Self {
        self.haze_height = half_height.get();
        self
    }

    /// The up-component at which the gradient stands halfway between the
    /// horizon and the zenith. See [`Self::with_haze_height`].
    ///
    /// Returned as authored; both this crate's [`Self::radiance`] and a
    /// backend's mirror clamp it to the interior band they can evaluate, the
    /// same posture [`Self::cloud_coverage`] and [`Self::body_angular_radius`]
    /// already take.
    pub const fn haze_height(&self) -> Ratio {
        Ratio::finite_or_zero(self.haze_height)
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

    /// How much of the sky the cloud layer covers, `0` (clear) to `1` (overcast).
    pub const fn cloud_coverage(&self) -> Ratio {
        Ratio::finite_or_zero(self.cloud_coverage)
    }

    /// The cloud field's scale — larger is smaller, busier cloud.
    pub const fn cloud_scale(&self) -> Ratio {
        Ratio::finite_or_zero(self.cloud_scale)
    }

    /// **The sky, evaluated.** The linear-RGB radiance looking along `view`.
    ///
    /// Four terms: the vertical gradient, the body's disc and the body's halo
    /// added together as the sky *behind*, then the cloud layer composited in
    /// front of that by its density. Compositing rather than adding is what makes
    /// a cloud a cloud: at full density it replaces what is behind it, so cloud
    /// drifting across the body occludes the disc instead of glowing through it.
    ///
    /// All of it is pure arithmetic with no branches — which is required here
    /// (this is a layer) and is also exactly what makes it portable to a shader
    /// unchanged.
    ///
    /// The gradient uses the up-component rather than an angle, so it costs no
    /// trigonometry: [`haze_lift`] reshapes it by the authored haze height, and
    /// the result is smoothstepped so the horizon band is soft rather than a
    /// visible seam. Below the horizon it holds the horizon colour: there is no
    /// ground hemisphere here, because the ground is geometry.
    pub fn radiance(&self, view: [f32; 3]) -> [f32; 3] {
        let dir = normalize_or(view, [0.0, 1.0, 0.0]);
        let up = dir[1].clamp(0.0, 1.0);
        let blend = smoothstep(haze_lift(up, self.haze_height));

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

        let density = self.cloud_density(dir);
        // The cloud's sunward face: one broad forward-scattering lobe about the
        // body, so the whole sun side of the sky carries lit tops and the far side
        // stays in the gradient's own shade. This is why a cloud needs no authored
        // colour — its light is the body's light.
        let sunlit = cos_angle.max(0.0).powf(CLOUD_FORWARD) * CLOUD_SUN_GAIN;

        [0, 1, 2].map(|c| {
            let gradient = lerp(self.horizon[c], self.zenith[c], blend);
            let behind = gradient + self.body_color[c] * emission;
            // A cumulus is brighter than the sky it covers even in shade, which is
            // the fill gain, plus whatever the body throws on its sunward face.
            let cloud = gradient * CLOUD_FILL_GAIN + self.body_color[c] * sunlit;
            lerp(behind, cloud, density)
        })
    }

    /// How much cloud stands between the eye and the sky along the unit ray `dir`,
    /// `0`..`1`.
    ///
    /// The field is sampled on a **cloud plane** — where the ray meets a plane one
    /// unit overhead — rather than on the dome. That is what makes the layer read
    /// as weather at a distance instead of wallpaper: the lumps foreshorten and
    /// crowd together toward the horizon exactly as real cumulus do, and open out
    /// overhead. The up-component is floored so a ray at or below the horizon
    /// lands somewhere finite and very far instead of at infinity, and the density
    /// is faded out across that same band so the layer dissolves into the horizon
    /// haze rather than shimmering along a seam.
    fn cloud_density(&self, dir: [f32; 3]) -> f32 {
        let reach = self.cloud_scale.max(0.0) / dir[1].max(CLOUD_HORIZON_FLOOR);
        let field = cloud_field([dir[0] * reach, dir[2] * reach]);
        // The threshold the field must beat, laid out so both ends are exact
        // rather than nearly: at coverage `0` it is `1.0`, which the field (whose
        // maximum is exactly `1.0`) cannot beat at all — a provably clear sky, with
        // no branch. At coverage `1` it sits a full edge-width below zero, which
        // the field (whose minimum is `0.0`) always beats — overcast.
        let threshold = 1.0 - self.cloud_coverage.clamp(0.0, 1.0) * (1.0 + CLOUD_EDGE);
        smoothstep((field - threshold) / CLOUD_EDGE) * smoothstep(dir[1] / CLOUD_HORIZON_FADE)
    }
}

/// The haze height that reproduces a plain `smoothstep(dir.y)` gradient exactly:
/// the midpoint at an up-component of `0.5`, which is [`haze_lift`]'s identity.
const DEFAULT_HAZE_HEIGHT: f32 = 0.5;

/// The interior band a haze height is evaluated in.
///
/// Both ends are excluded because both are degenerate: at `0` the bias
/// coefficient is `0` and the lift reads `0 / 0` looking straight at the
/// horizon; at `1` the coefficient is unbounded. Clamping rather than rejecting
/// is what keeps this branch-free, and it keeps a mis-authored sky a *usable*
/// sky rather than a frame of NaN — the same posture the body's radius and the
/// cloud coverage take.
const HAZE_HEIGHT_MIN: f32 = 0.02;
const HAZE_HEIGHT_MAX: f32 = 0.98;

/// Reshape the raw up-component so the gradient's midpoint lands at
/// `half_height` instead of at a fixed `0.5`.
///
/// The curve is `up / (up + (1 - up) * k)` with `k = h / (1 - h)`: a rational
/// bias, deliberately **not** `up.powf(p)`. Three properties earn it that shape,
/// and all three matter here rather than being taste:
///
/// * **The default is the identity, to the bit.** At `h = 0.5` the coefficient
///   is `k = 1` and the expression collapses to `up / (up + 1 - up)`, which is
///   `up` exactly. That is what lets a haze height be *added to the definition*
///   of the sky rather than forked around it: every sky authored before this
///   parameter existed evaluates to the same bits it always did.
/// * **Both ends stay exact for every `h`.** `0` maps to `0` and `1` maps to
///   `1`, so looking straight up still returns the zenith colour exactly and the
///   horizon still returns the horizon colour exactly. The second of those is
///   load-bearing: depth fog fades distant geometry into the horizon colour, and
///   a gradient that returned *almost* that colour at eye level would draw a
///   seam along the horizon of every outdoor frame.
/// * **It is four arithmetic operations**, so a shader mirror is the same four
///   and cannot drift. A `pow` mirror can: WGSL defines `pow(x, y)` as
///   `exp2(y * log2(x))`, which for `y = 1` is not required to return `x`.
///
/// `half_height` is clamped into [`HAZE_HEIGHT_MIN`]..=[`HAZE_HEIGHT_MAX`], so
/// the denominator is bounded below by `min(k, 1) > 0` for every `up` in
/// `0..=1` and the result is always finite.
fn haze_lift(up: f32, half_height: f32) -> f32 {
    let h = half_height.clamp(HAZE_HEIGHT_MIN, HAZE_HEIGHT_MAX);
    let k = h / (1.0 - h);
    up / (up + (1.0 - up) * k)
}

/// The smallest angular radius a body is evaluated at, so a zero-radius body
/// cannot produce a degenerate limb window.
const MIN_ANGULAR_RADIUS: f32 = 1.0e-4;

/// How much of the body's radius is spent softening its edge.
const LIMB_SOFTNESS: f32 = 0.25;

/// The width of the cloud field's coverage window — how much field value separates
/// a clear pixel from a fully opaque one. Wide enough that a cumulus has a soft,
/// wispy limb rather than a paper edge; narrow enough that it still reads as a
/// distinct puff rather than a smear.
const CLOUD_EDGE: f32 = 0.22;

/// The smallest up-component the cloud plane is sampled at, so a ray at or below
/// the horizon lands somewhere finite rather than at infinity.
const CLOUD_HORIZON_FLOOR: f32 = 0.06;

/// The up-component over which cloud density fades in from the horizon, so the
/// layer dissolves into the haze instead of ending at a seam.
const CLOUD_HORIZON_FADE: f32 = 0.10;

/// How much brighter a cloud's shaded body is than the sky directly behind it.
/// Above one because a cumulus in shade still out-scatters clear air.
const CLOUD_FILL_GAIN: f32 = 1.6;

/// How much of the body's colour lands on a cloud's sunward face.
const CLOUD_SUN_GAIN: f32 = 0.35;

/// The forward-scattering lobe's exponent — small, because a cloud's sunward
/// brightening is broad, not a second halo.
const CLOUD_FORWARD: f32 = 6.0;

/// The cloud field's octaves as `[rotation, frequency, weight]`.
///
/// A sum of separable sinusoids, not a hashed lattice noise: it is the same eight
/// lines of arithmetic in Rust and in WGSL with no texture, no integer hashing and
/// no `fract` precision cliff, which is what keeps this function portable to the
/// shader unchanged the way the rest of [`FrameSky`] is.
///
/// Each octave is rotated by its own odd angle and scaled by a non-integer
/// frequency ratio. Both matter: axis-aligned harmonics of a common frequency
/// re-align into a visible grid, and a grid is the one thing a sky may not look
/// like. The weights sum to exactly `1.0`, which is what pins the field's range to
/// `0.0..=1.0` and lets the coverage threshold have exact ends.
const CLOUD_OCTAVES: [[f32; 3]; 4] = [
    [0.00, 1.00, 0.50],
    [1.13, 2.31, 0.25],
    [2.47, 4.73, 0.15],
    [3.71, 9.17, 0.10],
];

/// One octave of the cloud field: a separable sinusoid on a rotated lattice,
/// remapped to `0.0..=1.0`.
fn cloud_octave(p: [f32; 2], rotation: f32, frequency: f32) -> f32 {
    let (sin_r, cos_r) = (rotation.sin(), rotation.cos());
    let x = (p[0] * cos_r + p[1] * sin_r) * frequency;
    let y = (p[1] * cos_r - p[0] * sin_r) * frequency;
    x.sin() * y.sin() * 0.5 + 0.5
}

/// The cloud field at a point on the cloud plane, in `0.0..=1.0`.
fn cloud_field(p: [f32; 2]) -> f32 {
    CLOUD_OCTAVES
        .iter()
        .map(|octave| octave[2] * cloud_octave(p, octave[0], octave[1]))
        .sum()
}

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

    /// A blue day sky with broad cumulus and a sun — the shape an outdoor frame
    /// authors.
    fn daylit() -> FrameSky {
        FrameSky::gradient([0.10, 0.28, 0.75], [0.55, 0.72, 0.95])
            .with_body([0.45, 0.30, 1.0], rad(0.03), [3.0, 2.8, 2.4], q(600.0), q(0.6))
            .with_clouds(q(0.55), q(0.5))
    }

    /// The default is a *clear* sky, exactly — not nearly. This is the property
    /// that lets "no clouds" need no separate representation and no branch, and it
    /// is why the coverage threshold is laid out with exact ends.
    #[test]
    fn a_sky_with_no_clouds_is_exactly_the_sky_without_the_layer() {
        let clear = FrameSky::gradient([0.1, 0.2, 0.4], [0.5, 0.4, 0.3]);
        assert_eq!(clear.cloud_coverage().get(), 0.0);
        // Every direction, well above the horizon fade, is untouched arithmetic.
        [[0.0, 1.0, 0.0], [0.3, 0.6, 0.7], [0.0, 0.5, 0.87], [-0.4, 0.9, 0.2]]
            .into_iter()
            .for_each(|dir| {
                assert_eq!(clear.cloud_density(normalize_or(dir, [0.0, 1.0, 0.0])), 0.0);
            });
        assert_eq!(clear.radiance([0.3, 0.6, 0.7]), {
            let blend = smoothstep(normalize_or([0.3, 0.6, 0.7], [0.0, 1.0, 0.0])[1]);
            [0, 1, 2].map(|c| lerp([0.5, 0.4, 0.3][c], [0.1, 0.2, 0.4][c], blend))
        });
    }

    /// The field the whole layer is thresholded against must actually span its
    /// stated range, or neither end of the coverage window is exact.
    #[test]
    fn the_cloud_field_stays_inside_zero_to_one_and_is_not_a_flat_sheet() {
        let samples: Vec<f32> = (0..64)
            .map(|i| {
                let t = i as f32 * 0.37;
                cloud_field([t.cos() * 9.0 + t, t.sin() * 7.0 - t * 0.5])
            })
            .collect();
        assert!(samples.iter().all(|v| (0.0..=1.0).contains(v)), "{samples:?}");
        let lo = samples.iter().copied().fold(f32::INFINITY, f32::min);
        let hi = samples.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        assert!(hi - lo > 0.3, "the field varies rather than sitting flat: {lo}..{hi}");
        // The octaves are genuinely rotated against each other: a single unrotated
        // separable sinusoid is symmetric under swapping x and z, and the field
        // must not be.
        assert!((cloud_field([1.7, 0.4]) - cloud_field([0.4, 1.7])).abs() > 1.0e-3);
    }

    #[test]
    fn more_coverage_is_more_cloud_and_full_coverage_is_overcast() {
        let up = [0.2, 0.8, 0.5];
        let dir = normalize_or(up, [0.0, 1.0, 0.0]);
        let at = |c: f32| {
            FrameSky::gradient([0.1; 3], [0.2; 3])
                .with_clouds(q(c), q(0.5))
                .cloud_density(dir)
        };
        assert_eq!(at(0.0), 0.0, "clear");
        let full = at(1.0);
        assert!(full >= 1.0 - 1.0e-6, "overcast: {full}");
        let ramp: Vec<f32> = (0..=10).map(|i| at(i as f32 / 10.0)).collect();
        assert!(
            ramp.windows(2).all(|w| w[1] >= w[0]),
            "coverage is monotone: {ramp:?}"
        );
    }

    /// The cloud plane, not the dome: the layer must crowd toward the horizon and
    /// open out overhead, and it must fade out at the horizon rather than end at a
    /// seam.
    #[test]
    fn clouds_crowd_toward_the_horizon_and_dissolve_into_it() {
        let sky = daylit();
        // Below the horizon the layer is exactly absent, and it fades in across the
        // band above it rather than starting at full strength on a seam.
        assert_eq!(sky.cloud_density([0.0, -0.5, 0.86]), 0.0, "nothing below the horizon");
        let ray = |y: f32| normalize_or([0.0, y, 1.0], [0.0, 1.0, 0.0]);
        assert_eq!(sky.cloud_density(ray(0.0)), 0.0, "nor exactly on it");
        // Across the whole fade band the density is held under the fade itself, so
        // however dense the field is down there the layer arrives gradually.
        (0..=20).for_each(|i| {
            let y = i as f32 * 0.006;
            let d = sky.cloud_density(ray(y));
            let fade = smoothstep(ray(y)[1] / CLOUD_HORIZON_FADE);
            assert!(d <= fade + 1.0e-6, "at y={y}: density {d} exceeds the fade {fade}");
        });
        // Sweeping a fixed angular step across the sky crosses far more cloud
        // edges near the horizon than overhead — that is the foreshortening.
        let edges = |elevation: f32| {
            let samples: Vec<f32> = (0..96)
                .map(|i| {
                    let a = i as f32 * 0.02;
                    sky.cloud_density(normalize_or([a.sin(), elevation, a.cos()], [0.0, 1.0, 0.0]))
                })
                .collect();
            samples.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>()
        };
        // Bound to locals rather than recomputed inside the failure message: a
        // format argument is only evaluated when the assertion fails, so writing
        // it inline leaves a region no passing test can ever reach.
        let (low, high) = (edges(0.25), edges(2.5));
        assert!(low > high, "low sky is busier than high: {low} vs {high}");
    }

    /// The cloud takes its light from the body and the gradient — which is what
    /// lets one authored layer be correct under a moon and under a midday sun.
    #[test]
    fn a_cloud_is_lit_by_the_body_and_occludes_it_rather_than_glowing_through() {
        // Full overcast, so every ray is entirely cloud and the composite is the
        // cloud term alone.
        let overcast = |body: [f32; 3]| {
            FrameSky::gradient([0.10, 0.28, 0.75], [0.55, 0.72, 0.95])
                .with_body([0.0, 0.5, 1.0], rad(0.03), body, q(600.0), q(0.6))
                .with_clouds(q(1.0), q(0.5))
        };
        let sunward = normalize_or([0.0, 0.5, 1.0], [0.0, 1.0, 0.0]);
        let away = normalize_or([0.0, 0.5, -1.0], [0.0, 1.0, 0.0]);
        let sun = overcast([3.0, 2.8, 2.4]);
        let (lit, shaded) = (sun.radiance(sunward), sun.radiance(away));
        assert!(
            lit[0] > shaded[0] + 0.5,
            "the sunward face is lit: {lit:?} vs {shaded:?}"
        );
        // The disc is *behind* the layer: at full density it is covered, so the
        // pixel is nowhere near the body's own 3.0 radiance.
        assert!(lit[0] < 2.0, "{lit:?}");
        let clear_sky = FrameSky::gradient([0.10, 0.28, 0.75], [0.55, 0.72, 0.95])
            .with_body([0.0, 0.5, 1.0], rad(0.03), [3.0, 2.8, 2.4], q(600.0), q(0.6));
        assert!(
            clear_sky.radiance(sunward)[0] > sun.radiance(sunward)[0],
            "an uncovered disc is brighter than a covered one"
        );
        // A dim body (a moon) gives a dim cloud from the same authored layer: the
        // layer scales with the light rather than carrying a colour of its own.
        let moon = overcast([0.9, 0.94, 1.0]);
        assert!(moon.radiance(sunward)[0] < sun.radiance(sunward)[0]);
        // ...and it is still brighter than the sky it covers, in shade.
        assert!(moon.radiance(away)[0] > clear_sky.radiance(away)[0]);
    }

    #[test]
    fn cloud_accessors_round_trip_and_degenerate_input_cannot_poison_the_sky() {
        let sky = daylit();
        assert_eq!(sky.cloud_coverage().get(), 0.55);
        assert_eq!(sky.cloud_scale().get(), 0.5);
        assert_ne!(sky, daylit().with_clouds(q(0.55), q(0.9)));
        // Coverage outside its range is clamped, not extrapolated into a negative
        // or runaway density.
        let silly = FrameSky::gradient([0.1; 3], [0.2; 3]).with_clouds(q(9.0), q(0.0));
        let d = silly.cloud_density(normalize_or([0.2, 0.8, 0.5], [0.0, 1.0, 0.0]));
        assert!((0.0..=1.0).contains(&d), "{d}");
        let negative = FrameSky::gradient([0.1; 3], [0.2; 3]).with_clouds(q(-4.0), q(0.5));
        assert_eq!(negative.cloud_density(normalize_or([0.2, 0.8, 0.5], [0.0; 3])), 0.0);
        // A poisoned view direction is caught upstream by `normalize_or`, so the
        // cloud layer sees only usable rays and the frame stays finite.
        assert!(daylit().radiance([f32::NAN, 1.0, 0.0]).iter().all(|v| v.is_finite()));
        assert!(daylit().radiance([0.0, 0.0, 0.0]).iter().all(|v| v.is_finite()));
    }

    /// The property that lets a haze height be added to the *definition* of the
    /// sky rather than forked around it: at the default, the lift is the
    /// identity and every sky authored before the parameter existed evaluates to
    /// the bits it always did.
    #[test]
    fn the_default_haze_height_is_the_identity_to_the_bit() {
        let plain = FrameSky::gradient([0.1, 0.2, 0.4], [0.5, 0.4, 0.3]);
        assert_eq!(plain.haze_height().get(), DEFAULT_HAZE_HEIGHT);
        // Not "close to" `up` — exactly `up`, for every representable input.
        (0..=64).for_each(|i| {
            let up = i as f32 / 64.0;
            assert_eq!(haze_lift(up, DEFAULT_HAZE_HEIGHT), up, "at up={up}");
        });
        // ...and therefore the whole gradient is the plain smoothstep it was.
        [[0.0, 1.0, 0.0], [0.3, 0.6, 0.7], [0.0, -1.0, 0.0], [1.0, 0.05, 0.0]]
            .into_iter()
            .for_each(|view| {
                let blend = smoothstep(normalize_or(view, [0.0, 1.0, 0.0])[1].clamp(0.0, 1.0));
                let expected =
                    [0, 1, 2].map(|c| lerp([0.5, 0.4, 0.3][c], [0.1, 0.2, 0.4][c], blend));
                assert_eq!(plain.radiance(view), expected, "at {view:?}");
            });
    }

    /// What the parameter is *for*: pulling the gradient's midpoint down to the
    /// elevation a near-level camera actually shows, without touching either
    /// stop — and without moving the two ends the rest of the frame is matched
    /// to.
    #[test]
    fn a_lower_haze_height_moves_the_midpoint_and_leaves_both_ends_exact() {
        let (horizon, zenith) = ([0.5, 0.4, 0.3], [0.1, 0.2, 0.4]);
        let tight = FrameSky::gradient(zenith, horizon).with_haze_height(q(0.2));
        assert_eq!(tight.haze_height().get(), 0.2);

        // The midpoint is where it says it is: at `up == h` the lift is exactly
        // 0.5, and `smoothstep(0.5)` is exactly 0.5, so the gradient stands
        // exactly halfway between the two stops.
        assert_eq!(haze_lift(0.2, 0.2), 0.5);
        assert_eq!(smoothstep(haze_lift(0.2, 0.2)), 0.5);

        // Both ends survive untouched. The horizon end is load-bearing: depth fog
        // fades distant geometry into exactly this colour, so anything less than
        // exact draws a seam along the horizon.
        assert_eq!(tight.radiance([0.0, 0.0, 1.0]), horizon, "the horizon is exact");
        assert_eq!(tight.radiance([0.0, 1.0, 0.0]), zenith, "the zenith is exact");
        assert_eq!(tight.radiance([0.0, -1.0, 0.0]), horizon, "and below it too");

        // In between, a tighter haze is strictly nearer the zenith than the
        // default is — that is the whole point — and the lift stays monotone.
        let plain = FrameSky::gradient(zenith, horizon);
        let ramp: Vec<f32> = (0..=20).map(|i| haze_lift(i as f32 / 20.0, 0.2)).collect();
        assert!(ramp.windows(2).all(|w| w[1] > w[0]), "monotone: {ramp:?}");
        (1..20).for_each(|i| {
            let up = i as f32 / 20.0;
            assert!(
                haze_lift(up, 0.2) > up,
                "a tighter band lifts the blend at up={up}"
            );
        });
        // Sampled where this app's camera actually looks — 32 degrees up, the top
        // row of a chase-camera frame — the tighter band reaches far more of the
        // zenith stop than the default can. This is the number the two stops
        // alone could not reach: 0.57 of the way, against 0.88.
        let top = [0.0, 0.53, 0.848];
        assert!((smoothstep(haze_lift(0.53, DEFAULT_HAZE_HEIGHT)) - 0.545).abs() < 0.005);
        assert!((smoothstep(haze_lift(0.53, 0.2)) - 0.913).abs() < 0.005);
        // Measured as distance still to travel to the zenith stop, which is what
        // "the authored zenith is never reached on screen" actually means. The
        // default leaves 45% of the way to go at the top of frame; the tighter
        // band leaves under a tenth of it.
        let togo = |sky: &FrameSky| sky.radiance(top)[1] - zenith[1];
        assert!(
            togo(&tight) < togo(&plain) * 0.25,
            "the visible top of frame is now most of the way to the zenith: {} vs {}",
            togo(&tight),
            togo(&plain)
        );

        // And it reaches in the other direction too: a *higher* midpoint holds
        // the horizon colour further up, which is what a hazy or dusty day is.
        let wide = FrameSky::gradient(zenith, horizon).with_haze_height(q(0.8));
        assert!(haze_lift(0.5, 0.8) < 0.5, "a high midpoint holds the haze");
        assert!(wide.radiance(top)[1] > plain.radiance(top)[1]);
    }

    /// A mis-authored haze height must produce a usable sky, not a frame of NaN.
    /// Both ends of the range are genuinely degenerate — `0` divides `0` by `0`
    /// at the horizon, `1` is unbounded — so both are clamped away, branchlessly.
    #[test]
    fn a_degenerate_haze_height_is_clamped_rather_than_poisoning_the_frame() {
        let finite = |c: [f32; 3]| c.iter().all(|v| v.is_finite());
        [0.0, 1.0, -4.0, 9.0, f32::NAN, f32::INFINITY]
            .into_iter()
            .for_each(|h| {
                let sky = FrameSky::gradient([0.1, 0.2, 0.4], [0.5, 0.4, 0.3])
                    .with_haze_height(Ratio::finite_or_zero(h));
                [[0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [0.3, 0.6, 0.7], [0.0, -1.0, 0.0]]
                    .into_iter()
                    .for_each(|view| {
                        assert!(finite(sky.radiance(view)), "h={h} view={view:?}");
                    });
                // Clamped, so the ends are still exact whatever was authored.
                assert_eq!(sky.radiance([0.0, 0.0, 1.0]), [0.5, 0.4, 0.3], "h={h}");
                assert_eq!(sky.radiance([0.0, 1.0, 0.0]), [0.1, 0.2, 0.4], "h={h}");
            });
        assert_eq!(haze_lift(0.0, 0.0), 0.0, "the horizon is not 0/0");
        assert_eq!(haze_lift(1.0, 1.0), 1.0);
        assert_eq!(haze_lift(0.5, -4.0), haze_lift(0.5, HAZE_HEIGHT_MIN));
        assert_eq!(haze_lift(0.5, 9.0), haze_lift(0.5, HAZE_HEIGHT_MAX));
        // It participates in identity like every other authored field.
        let base = FrameSky::gradient([0.1; 3], [0.2; 3]);
        assert_ne!(base, base.with_haze_height(q(0.2)));
        assert_eq!(base.with_haze_height(q(0.2)), base.with_haze_height(q(0.2)));
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
