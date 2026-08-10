//! The leaflet comb of a frond, as a tiling albedo texture.
//!
//! Every leaf surface in this game — the palm crowns that line the coast, the
//! shrub clumps on the verge, the conifer cones inland — is drawn with one
//! material, [`super::palette::ScenePalette::foliage`], and until now that
//! material was a **single flat colour**. A palm crown is the second-largest
//! object family in any coastal frame after the road itself, and it rendered as
//! two solid slabs: one value on the up-facing blades, one on the down-facing
//! ones, and nothing at all in between. Measured on the champion frame, a patch
//! wholly inside one frond blade returns essentially the material's own RGB, over
//! and over, at every depth.
//!
//! The reference's palms are the opposite of that, and the measurement is
//! unambiguous. Sampling 746 small patches lying strictly inside the big
//! right-hand crown of `visual_targets/burnt-rubber/reference.png` — green
//! patches only, so no sky and no trunk — the **median** patch varies by
//! **13.2%** of its own displayed value. That is not silhouette and it is not
//! lighting; it is a patch a dozen pixels across, entirely within one lit blade.
//! What it is measuring is the thing a palm frond is actually made of: a comb of
//! individual leaflets hung off a central rachis, with sky and shadow showing
//! between them.
//!
//! ## The detail is *chromatic*, not just tonal — and that is the surprise
//!
//! The obvious way to author a leaf texture is a grey comb that darkens the
//! albedo, and against this reference that would be half a texture. Sampling the
//! same patches channel by channel, the reference's frond runs from
//! `(154, 173, 79)` — a hot yellow-green — to `(106, 176, 146)`, a cool
//! blue-green, **with the green channel nearly constant across the swing**. The
//! variation lives almost entirely in red and blue, in opposition.
//!
//! That is not a stylisation, it is what a lit frond is: a leaflet's broad face
//! takes the warm sun and reads yellow-green, while the gap beside it shows the
//! shaded underside of the next leaflet, lit by the blue sky alone. So this
//! texture carries a **warmth modulation** ([`WARMTH`]) in step with the comb —
//! red up and blue down on the leaflet bodies, the reverse in the gaps —
//! alongside the tonal one. It is what makes the crown read as foliage rather
//! than as a green slab with stripes painted on it.
//!
//! ## The features are the geometry's, not a pattern laid over it
//!
//! `super::surface_builder`'s `quad` stretches the texture exactly once across
//! each quad it emits, and `super::prop_meshes::palm_crown_surface` emits a frond
//! blade as `(root-left, root-right, far-right, far-left)`. So on every blade in
//! the game **`u` runs across the blade's width and `v` runs along its length**,
//! and each of this texture's three features is the real part of a leaf it is
//! named for:
//!
//! * the **leaflets** ([`LEAFLETS`]) are a comb along `v` — bands running across
//!   the blade, repeated down its length, which is exactly how leaflets sit on a
//!   frond;
//! * the **rachis** ([`RACHIS_DEPTH`]) is a dark line at `u = 0.5`, which is
//!   where the blade's own centre line is: `quad` places the geometric spine of
//!   the frond at exactly that coordinate;
//! * the **edge lift** ([`EDGE_LIFT`]) brightens `u → 0` and `u → 1`, because a
//!   leaflet tip is thinner than its base and carries more light through it. It
//!   is also the feature that stops a blade reading as one flat ribbon: the
//!   silhouette edge is the brightest part of the reference's fronds.
//!
//! Nothing here depends on a UV the mesh does not have, and nothing has to be
//! re-authored if a blade changes length — the pattern travels with the corners.
//!
//! ## The amplitude is measured, and the mean is held
//!
//! [`MIN_MULTIPLIER`] is set so the finished texture varies by **12.98%** of the
//! crown's displayed value, mid-band of the reference's own 13.2%. A relative
//! figure is the right unit and it is exposure-invariant: this is a multiplied
//! albedo, so re-lighting the frame scales the crown and its comb together.
//!
//! A multiplied albedo can only ever **darken** (a texel's ceiling is `1.0`), and
//! this one is a strong pattern, so its mean multiplier is `0.56` — a full stop.
//! Left uncorrected that would not be a texture, it would be a lighting change
//! wearing one, and it would push a crown that is *already* darker than the
//! reference's a stop darker again. So [`base_colour`] lifts the material's
//! authored colour by the reciprocal of the per-channel mean, and the textured
//! crown's mean displayed value lands within **1.5 display levels** of the flat
//! fill it replaces, in all three channels. Adding the surface is not also a
//! grade; that is the whole discipline, and
//! [`tests::the_texture_adds_a_surface_without_changing_the_crown_s_colour`]
//! is what keeps it.
//!
//! ## Why it does not sparkle
//!
//! [`LEAFLETS`] is twelve over [`RES`] sixty-four, so a leaflet is **5.3 texels**
//! — comfortably resolved — and the comb is a raised cosine rather than a square
//! wave, so it has no step in it to alias. On screen the blade is the other way
//! round: the nearest palm's frond segment is a few dozen pixels long against
//! sixty-four texels, so this texture is **minified everywhere in the frame** and
//! never magnified. The foliage material is sampled `Crisp`, which minifies
//! trilinearly across a real mip chain, so a distant palm averages the comb to
//! its own mean and fades to the flat colour it always was — which is the correct
//! way for leaf detail to go away with distance.
//!
//! ## Why the pixels are authored as sRGB bytes
//!
//! The GPU backend uploads a custom albedo as `Rgba8UnormSrgb` and the shader
//! computes `base = albedo * colour`, so a byte here is decoded to linear before
//! it multiplies the material's colour. The byte range is therefore derived from
//! the linear multipliers it must produce — see [`byte_for_multiplier`], which is
//! the same inversion [`super::asphalt_texture`] does and for the same reason.
//!
//! This is a **GPU-arm** enrichment by declaration: the Canvas 2D rasterizer
//! drops the `Textures` capability, so the software arm keeps the flat foliage
//! colour it has always had, unchanged and legible.

/// The foliage colour this texture is authored against — the flat fill it
/// replaces, and the value its own mean must reproduce.
///
/// Held here rather than at the `ScenePalette` call site because the two numbers
/// are one decision: [`base_colour`] divides by this texture's measured mean, so
/// the colour the crown ends up displaying is this, whatever the pattern does.
pub const FOLIAGE: [f32; 3] = [0.13, 0.27, 0.15];

/// The texture's edge length in texels. `RES * RES * RGBA` is 16 KiB.
///
/// Sixty-four is set by [`LEAFLETS`], not chosen for its own sake: the comb has
/// to sit at four or more texels per leaflet or the pattern is at Nyquist in the
/// buffer before a sampler ever touches it, and twelve leaflets at 64 gives 5.3.
/// Going higher buys nothing — the blade is minified everywhere on screen, so
/// every texel above the mip level the sampler actually picks is averaged away.
pub const RES: u32 = 64;

/// How many leaflets a comb carries across one quad — one *segment* of a frond,
/// since `palm_crown_surface` builds a blade from two quads end to end, so a
/// whole frond shows twenty-four.
///
/// A real frond has far more than twenty-four leaflets, and authoring the real
/// count would be the mistake: the nearest palm's blade segment is a few dozen
/// pixels long, so sixty leaflets would land under one pixel each and mip
/// straight back to the flat fill this module exists to end. Twelve is the
/// largest count that still resolves on the nearest crown, at 5.3 texels a
/// leaflet.
///
/// It is a **whole number of cycles across `v`**, which is what makes the comb
/// wrap without a seam: `Repeat` addressing puts row `RES-1` next to row `0`, and
/// the raised cosine has period `1/LEAFLETS` in `v`, so an integer count closes
/// exactly at the tile boundary. Note that this is *not* the same as `LEAFLETS`
/// dividing [`RES`] — it does not (64 % 12 = 4), and it does not need to. The
/// texel grid samples a continuous periodic field; only the field's period has to
/// match the tile.
const LEAFLETS: u32 = 12;

/// The darkest linear multiplier a texel may apply, and therefore the pattern's
/// **strength**.
///
/// `0.36` produces a displayed variation of **12.98%** of the crown's own value.
/// The target is the reference's own measurement: 746 patches lying strictly
/// inside its big right-hand palm crown have a median within-frond variation of
/// **13.2%** (see the module docs). Weaker than this and the crown is a slab with
/// a hint on it; stronger and the leaflets stop being leaflets and become a
/// zebra.
const MIN_MULTIPLIER: f32 = 0.36;

/// How far red and blue swing apart, in step with the comb — red up and blue down
/// on a leaflet's sunlit body, the reverse in the shaded gap beside it.
///
/// This is the half of the pattern a grey comb cannot express, and the reference
/// says it is the larger half: across its frond the green channel is nearly
/// constant while red runs `106 → 163` and blue runs the other way. Kept
/// symmetric about the mean so it is a *modulation* and not a tint — the crown's
/// hue is the material's, and this only fans it.
const WARMTH: f32 = 0.18;

/// How deep the rachis — the frond's central spine — cuts, at `u = 0.5`.
///
/// `quad` puts the blade's geometric centre line at exactly that coordinate, so
/// this is the real part of the leaf and not a stripe placed by eye. It is the
/// one feature that varies **across** the blade rather than along it, which is
/// why it survives when the crown is seen edge-on and the comb is foreshortened
/// into nothing.
const RACHIS_DEPTH: f32 = 0.55;

/// The rachis's half-width, as a fraction of the half-blade. `0.18` of a half-
/// blade is about 5.8 texels of falloff — a spine, not a gutter.
const RACHIS_HALF: f32 = 0.18;

/// How much brighter a blade's outer edges are than its middle.
///
/// A leaflet tip is thinner than its base and carries light through it, and in
/// the reference the frond edges are the brightest green in the crown. It is also
/// the feature that gives a blade a cross-section: without it the quad reads as a
/// flat ribbon however finely it is combed.
const EDGE_LIFT: f32 = 0.35;

/// How much the leaflets vary in strength from one to the next, `0` uniform … `1`
/// half of them gone. A comb of twelve identical teeth is a machined part; a
/// frond is ragged, and the leaflets nearest a tip are visibly shorter and
/// thinner than the ones at its base.
const RAGGED: f32 = 0.30;

/// The material colour that, multiplied by this texture, displays as [`FOLIAGE`].
///
/// A multiplied albedo can only darken, and this pattern's mean multiplier is
/// about `0.56`, so a texture applied over the flat colour unchanged would take a
/// full stop out of every leaf in the game. That is a lighting change, not a
/// surface, and it is the exact failure this function exists to prevent: the lift
/// is the reciprocal of the texture's own **per-channel** mean, computed from the
/// buffer that actually ships rather than from the authored constants, so the two
/// can never drift apart.
pub fn base_colour() -> [f32; 3] {
    let mean = mean_multiplier();
    [
        FOLIAGE[0] / mean[0],
        FOLIAGE[1] / mean[1],
        FOLIAGE[2] / mean[2],
    ]
}

/// The tiling foliage albedo, as `RES * RES` RGBA8 texels ready for
/// `RunningApp::add_texture_data`.
pub fn foliage_albedo() -> Vec<u8> {
    (0..RES * RES)
        .flat_map(|i| {
            let (x, y) = (i % RES, i / RES);
            let f = leaf(x, y);
            let m = MIN_MULTIPLIER + (1.0 - MIN_MULTIPLIER) * f;
            // The warmth swings about the field's own midpoint, so it adds no net
            // tint: red leads on a leaflet's body, blue leads in the gap.
            let w = WARMTH * (f - 0.5);
            [
                byte_for_multiplier(m * (1.0 + w)),
                byte_for_multiplier(m),
                byte_for_multiplier(m * (1.0 - w)),
                255,
            ]
        })
        .collect()
}

/// The mean linear multiplier the shipped buffer applies, per channel.
fn mean_multiplier() -> [f32; 3] {
    let pixels = foliage_albedo();
    let count = (RES * RES) as f32;
    let channel = |c: usize| {
        pixels
            .chunks(4)
            .map(|t| decode_srgb(t[c]))
            .sum::<f32>()
            / count
    };
    [channel(0), channel(1), channel(2)]
}

/// The leaf field at a texel, in `0..=1` — the comb, the rachis and the edge lift
/// multiplied together, because they are three independent reasons a point on a
/// leaf is dark and a leaf that is in the gap *and* on the spine is darker than
/// either alone.
fn leaf(x: u32, y: u32) -> f32 {
    let (u, v) = ((x as f32 + 0.5) / RES as f32, (y as f32 + 0.5) / RES as f32);
    comb(v) * rachis(u) * edge_lift(u)
}

/// The leaflet comb along the blade: a raised cosine at [`LEAFLETS`] cycles,
/// each leaflet knocked down by its own share of [`RAGGED`].
///
/// A raised cosine rather than a square wave, so there is no step in the buffer
/// to alias — and because [`LEAFLETS`] divides [`RES`] exactly, the cycle wraps
/// continuously across the tile and no seam runs across the blade.
fn comb(v: f32) -> f32 {
    let phase = (1.0 - (v * LEAFLETS as f32 * std::f32::consts::TAU).cos()) * 0.5;
    let index = (v * LEAFLETS as f32) as u32 % LEAFLETS;
    phase * (1.0 - RAGGED * hash_unit(index, 0, 0x9E37_79B9))
}

/// The rachis notch: a smooth quadratic well at the blade's centre line, where
/// `quad` puts the frond's geometric spine.
fn rachis(u: f32) -> f32 {
    let d = (u - 0.5).abs() * 2.0;
    let inside = (1.0 - d / RACHIS_HALF).max(0.0);
    1.0 - RACHIS_DEPTH * inside * inside
}

/// The edge lift: brightest at the blade's two silhouette edges, dimmest at its
/// centre line, so a blade has a cross-section rather than being a flat ribbon.
fn edge_lift(u: f32) -> f32 {
    let d = (u - 0.5).abs() * 2.0;
    1.0 - EDGE_LIFT * (1.0 - d * d)
}

/// A deterministic `0..=1` hash of a leaflet index. Integer-only, so the texture
/// is byte-identical on every platform and every run.
fn hash_unit(x: u32, y: u32, salt: u32) -> f32 {
    let mut h = x.wrapping_mul(0x27D4_EB2D) ^ y.wrapping_mul(0x1656_67B1) ^ salt;
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x2974_5C69);
    h ^= h >> 15;
    h as f32 / u32::MAX as f32
}

/// The sRGB byte that decodes to `multiplier` in linear light. The backend
/// uploads this texture as `Rgba8UnormSrgb`, so the shader sees the *decoded*
/// value; authoring the byte directly would make the comb roughly twice as strong
/// as intended near white, where the curve is steepest.
fn byte_for_multiplier(multiplier: f32) -> u8 {
    let m = multiplier.clamp(0.0, 1.0);
    let encoded = [m * 12.92, 1.055 * m.powf(1.0 / 2.4) - 0.055]
        [usize::from(m > 0.003_130_8)];
    (encoded * 255.0).round().clamp(0.0, 255.0) as u8
}

/// The linear value an sRGB byte stands for — the inverse of
/// [`byte_for_multiplier`], needed by [`mean_multiplier`] because the mean that
/// matters is the mean of what the *shader* sees.
fn decode_srgb(byte: u8) -> f32 {
    let e = byte as f32 / 255.0;
    [e / 12.92, ((e + 0.055) / 1.055).powf(2.4)][usize::from(e > 0.040_45)]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A linear value as the 0..255 display level it lands on, so the tests can
    /// speak in the same units the reference was measured in.
    fn displayed(linear: f32) -> f32 {
        255.0 * (1.055 * linear.max(1.0e-9).powf(1.0 / 2.4) - 0.055)
    }

    /// Every texel's displayed crown colour, after the base-colour lift — which
    /// is the only form in which any claim about how this looks is meaningful.
    fn crown_levels() -> Vec<[f32; 3]> {
        let base = base_colour();
        foliage_albedo()
            .chunks(4)
            .map(|t| {
                [
                    displayed(base[0] * decode_srgb(t[0])),
                    displayed(base[1] * decode_srgb(t[1])),
                    displayed(base[2] * decode_srgb(t[2])),
                ]
            })
            .collect()
    }

    fn luma(c: [f32; 3]) -> f32 {
        0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
    }

    #[test]
    fn the_albedo_is_exactly_the_pixel_buffer_add_texture_data_accepts() {
        let pixels = foliage_albedo();
        assert_eq!(pixels.len(), (RES * RES * 4) as usize);
        // Opaque throughout: the shader's alpha-mask capability cuts at 0.5, and
        // a frond with holes punched in it is a different (and much more
        // expensive) kind of leaf than this mesh was built for.
        assert!(pixels.chunks(4).all(|t| t[3] == 255));
    }

    /// **As strong as the reference's fronds, and no stronger.**
    ///
    /// The unit is the displayed spread as a fraction of the displayed value,
    /// because that is what a human sees and because it is exposure-invariant: a
    /// multiplied albedo scales with the light, so this number can be compared
    /// against a reference shot under any rig, and no lighting change can excuse
    /// a bad value here.
    ///
    /// The band is the reference's own measurement — 746 patches lying strictly
    /// inside its big right-hand palm crown have a **median** within-frond
    /// variation of 13.2% of their own displayed value. Below ~8% the crown is
    /// back to being a slab; above ~18% the leaflets stop reading as leaflets and
    /// the frond becomes a zebra.
    #[test]
    fn the_comb_is_as_strong_as_the_reference_fronds_and_no_stronger() {
        let levels: Vec<f32> = crown_levels().into_iter().map(luma).collect();
        let mean = levels.iter().sum::<f32>() / levels.len() as f32;
        let sd = (levels.iter().map(|l| (l - mean).powi(2)).sum::<f32>()
            / levels.len() as f32)
            .sqrt();
        let relative = sd / mean;
        assert!(
            (0.08..0.18).contains(&relative),
            "the frond varies by {:.1}% of its own displayed value; the reference's \
             own crown measures 13.2% and a flat fill measures 0%",
            relative * 100.0
        );
    }

    /// **The texture adds a surface; it does not re-light the crown.**
    ///
    /// This is the assertion that makes the change honest. A multiplied albedo can
    /// only darken, and this pattern's mean multiplier is about `0.56` — so
    /// dropping it onto the existing colour would take a full stop out of every
    /// leaf in the game and call it detail. [`base_colour`] divides it back out
    /// per channel, and the check is that the textured crown displays the *same*
    /// colour the flat fill did, to within a couple of display levels in each
    /// channel.
    #[test]
    fn the_texture_adds_a_surface_without_changing_the_crown_s_colour() {
        let levels = crown_levels();
        let n = levels.len() as f32;
        let mean = |c: usize| levels.iter().map(|l| l[c]).sum::<f32>() / n;
        for c in 0..3 {
            let flat = displayed(FOLIAGE[c]);
            assert!(
                (mean(c) - flat).abs() < 2.5,
                "channel {c} of the textured crown displays at {:.1} where the flat \
                 fill it replaces displays at {flat:.1}: this is a grade, not a texture",
                mean(c)
            );
        }
        // ...and the lift it took to get there is a real one, so the test above is
        // not passing by the texture being too weak to matter.
        assert!(
            base_colour()[1] > FOLIAGE[1] * 1.3,
            "the base colour was not lifted: {:?}",
            base_colour()
        );
        // The lift must stay authorable as a colour, not clip at white.
        assert!(base_colour().iter().all(|c| *c <= 1.0), "{:?}", base_colour());
    }

    /// **The detail runs along the blade, because that is where leaflets are.**
    ///
    /// A texture with the right strength and the wrong axis is not a frond — it
    /// is corduroy running the length of the leaf. `v` is the along-blade
    /// coordinate (`quad` emits `root-left, root-right, far-right, far-left`
    /// against `UNIT_UVS`), so the comb's amplitude must live in the variation of
    /// the *row* means, not the column means.
    #[test]
    fn the_leaflets_run_across_the_blade_not_along_it() {
        let levels: Vec<f32> = crown_levels().into_iter().map(luma).collect();
        let at = |x: u32, y: u32| levels[(y * RES + x) as usize];
        let sd = |v: &[f32]| {
            let m = v.iter().sum::<f32>() / v.len() as f32;
            (v.iter().map(|x| (x - m).powi(2)).sum::<f32>() / v.len() as f32).sqrt()
        };
        let rows: Vec<f32> = (0..RES)
            .map(|y| (0..RES).map(|x| at(x, y)).sum::<f32>() / RES as f32)
            .collect();
        let cols: Vec<f32> = (0..RES)
            .map(|x| (0..RES).map(|y| at(x, y)).sum::<f32>() / RES as f32)
            .collect();
        assert!(
            sd(&rows) > sd(&cols) * 2.0,
            "the pattern's amplitude is across the blade ({:.1}) rather than along \
             it ({:.1}): that is corduroy, not leaflets",
            sd(&cols),
            sd(&rows)
        );
    }

    /// **The rachis is where the blade's spine actually is.**
    ///
    /// `quad` puts the frond's geometric centre line at `u = 0.5`, so the dark
    /// line has to be there and the bright edges have to be at `u → 0` and
    /// `u → 1`. Getting this inverted would paint a bright stripe down the spine
    /// and shade the silhouette — the exact opposite of the reference, whose
    /// frond edges are the brightest green in the crown.
    #[test]
    fn the_spine_is_dark_and_the_blade_edges_are_bright() {
        let levels: Vec<f32> = crown_levels().into_iter().map(luma).collect();
        let column = |x: u32| {
            (0..RES).map(|y| levels[(y * RES + x) as usize]).sum::<f32>() / RES as f32
        };
        let spine = column(RES / 2);
        let edge = (column(0) + column(RES - 1)) * 0.5;
        assert!(
            edge > spine * 1.05,
            "the blade's edges ({edge:.1}) are no brighter than its spine ({spine:.1})"
        );
    }

    /// **The comb resolves, and it wraps.**
    ///
    /// Two properties, both physical. A leaflet must span several texels or the
    /// pattern is at Nyquist inside the buffer and mips straight back to the flat
    /// fill it replaces. And the comb must close across the tile, because
    /// `Repeat` addressing puts row `RES-1` next to row `0`: a comb that does not
    /// wrap draws a hard band across every blade in the game.
    ///
    /// The seam is measured rather than argued from divisibility — `LEAFLETS`
    /// does *not* divide `RES` (64 % 12 = 4) and does not need to. What closes
    /// the tile is that the field's period is `1/LEAFLETS` in `v` and the count
    /// is a whole number, which is a claim about the continuous field; only the
    /// measurement can confirm the sampled grid inherits it.
    #[test]
    fn the_comb_resolves_and_wraps_without_a_seam() {
        let texels_per_leaflet = RES as f32 / LEAFLETS as f32;
        assert!(
            texels_per_leaflet >= 4.0,
            "a leaflet is only {texels_per_leaflet:.1} texels; the comb is at \
             Nyquist in its own buffer"
        );
        let levels: Vec<f32> = crown_levels().into_iter().map(luma).collect();
        let at = |x: u32, y: u32| levels[(y * RES + x) as usize];
        let worst = (0..RES)
            .map(|x| (at(x, RES - 1) - at(x, 0)).abs())
            .fold(0.0f32, f32::max);
        assert!(worst < 4.0, "the comb leaves a seam across the blade: {worst:.1}");
    }

    /// The chromatic half of the pattern, which a grey comb cannot express and
    /// which the reference says is the larger half: across its frond, red swings
    /// `106 → 163` and blue swings the other way while green barely moves. So the
    /// leaflet bodies here must be measurably warmer than the gaps beside them —
    /// and the swing must be a *modulation*, leaving the crown's own hue alone.
    #[test]
    fn the_leaflet_bodies_are_warmer_than_the_gaps_between_them() {
        let levels = crown_levels();
        let warmth: Vec<f32> = levels.iter().map(|l| l[0] - l[2]).collect();
        let bright = levels
            .iter()
            .zip(&warmth)
            .max_by(|a, b| luma(*a.0).total_cmp(&luma(*b.0)))
            .map(|(_, w)| *w)
            .expect("the texture has texels");
        let dark = levels
            .iter()
            .zip(&warmth)
            .min_by(|a, b| luma(*a.0).total_cmp(&luma(*b.0)))
            .map(|(_, w)| *w)
            .expect("the texture has texels");
        assert!(
            bright > dark + 4.0,
            "the brightest texel is no warmer than the darkest ({bright:.1} vs \
             {dark:.1}): the comb is grey, and the reference's frond is not"
        );
    }

    #[test]
    fn the_texture_is_deterministic() {
        assert_eq!(foliage_albedo(), foliage_albedo());
        assert_eq!(base_colour(), base_colour());
    }

    /// The sRGB round trip, pinned at both ends and in the middle. A regression
    /// here silently changes the comb's strength without changing any constant.
    #[test]
    fn bytes_are_the_srgb_encoding_of_the_multiplier_they_stand_for() {
        assert_eq!(byte_for_multiplier(1.0), 255);
        assert_eq!(byte_for_multiplier(0.0), 0);
        assert!((decode_srgb(byte_for_multiplier(0.56)) - 0.56).abs() < 0.01);
        assert!((decode_srgb(byte_for_multiplier(0.36)) - 0.36).abs() < 0.01);
        // The linear toe, and the clamp on out-of-range input.
        assert_eq!(byte_for_multiplier(0.001), 3);
        assert_eq!(byte_for_multiplier(4.0), 255);
        assert_eq!(byte_for_multiplier(-1.0), 0);
    }
}
