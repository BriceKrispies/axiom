//! Colour, and the material handles the whole scene is drawn with.
//!
//! Two rules shape everything here. First, **one material per pooled kind**: the
//! renderer batches by `(mesh, material)`, so a thousand reflector posts sharing
//! one material is one draw call and a thousand posts with per-instance tints is
//! a thousand. Second, **the road must stay dark and the cues must stay bright**:
//! at 300 km/h the only things the eye can actually resolve are the high-contrast
//! ones, so the tarmac is nearly black, the paint is nearly white, and the
//! reflector posts are brighter than anything else in the frame.
//!
//! The zone tints are the one place colour carries information rather than
//! decoration: the verge and the rock/tree colour change with the environment
//! zone, which is what makes the tunnel, the canyon and the coast read as
//! different places rather than the same road repainted.
//!
//! ## Anything that should glow is **emissive**, not fake-bright
//!
//! `Material::with_emissive` used to be a dead knob: it was carried down to
//! `axiom-render`'s `RenderMaterial` and then dropped at the frame packet, so
//! neither backend ever read it. Every lamp in this file was therefore faked by
//! cranking its **base colour** white-hot and hoping the key light hit it.
//!
//! That fake has a fixed cost, and it is the whole reason this scene reads flat.
//! A base colour is a *reflectance*: the shader multiplies it by N·L, by the
//! hemisphere ambient and by the shadow term. Under this app's authored night
//! ambient those factors are small, so a `1.0` red tail lamp arrives on screen as
//! a dull pink slab, and it goes *darker* the moment the car turns away from the
//! sun — the exact opposite of a lamp. It also forces every lamp to be
//! near-white, which is why the reference's two hot red strips came out as one
//! flat orange panel here.
//!
//! Emissive now reaches both backends as its own per-draw term, added after all
//! lighting and before fog (`axiom_host::FrameDrawItem::emissive`). So the rule
//! inverts:
//!
//! * **base colour = the albedo the object actually has** — a tail lamp is a
//!   dark red lens, a reflector post is dim amber plastic, a tunnel lamp is a
//!   grey housing. Dark, so the unlit sides stay dark and the object has shape.
//! * **emissive = the light it emits** — bright, and free to exceed `1.0` in one
//!   channel, because nothing modulates it. This is what makes a lamp separate
//!   from the surface it is mounted in at *any* angle, in shadow, at night.
//!
//! The hemisphere ambient's ground term no longer has to be propped up to keep
//! half of every "bright" object from going black, because the objects that are
//! supposed to be bright now carry their own light.
//!
use axiom::prelude::{Color, Handle, Material, Ratio, RunningApp, TextureSampling};

use crate::track::Zone;

use super::asphalt_texture::{asphalt_albedo, RES as ASPHALT_RES};
use super::chunks::RoadMaterials;
use super::foliage_texture::{base_colour as foliage_base, foliage_albedo, FOLIAGE, RES as FOLIAGE_RES};
use super::verge_texture::{verge_albedo, BASE as VERGE_BASE, RES as VERGE_RES};

/// A colour from linear RGB components.
pub fn rgb(r: f32, g: f32, b: f32) -> Color {
    Color::linear_rgb(ratio(r), ratio(g), ratio(b))
}

/// A `Ratio`, clamped rather than fallible — every value here is authored.
pub fn ratio(v: f32) -> Ratio {
    Ratio::finite_or_zero(v.clamp(0.0, 1.0))
}

/// The **daylight haze at the horizon** — the colour the whole world recedes
/// into, and the single value that decides whether this frame reads as midday or
/// as night.
///
/// Authored in **linear** light, which is not what it looks like: the backend
/// converts to sRGB for display, so this lands on screen at roughly
/// `(43, 177, 208)` before the grade, and — after [`super::GRADE`] runs its
/// exposure, cool white balance, contrast and `1.18` saturation over it — at
/// `(11, 185, 242)` in the finished frame.
///
/// ## Why the old `[0.22, 0.48, 0.80]` painted a milky sky, and this does not
///
/// **This constant, not [`SKY_ZENITH`], is the colour of the sky you can
/// actually see.** The GPU sky shader mixes the two stops on
/// `smoothstep(dir.y)`, and this app's chase camera looks slightly *down*: the
/// top row of the frame sits about 32° up, which is `dir.y ≈ 0.53` and therefore
/// a blend of only **0.57**. The zenith stop is never reached anywhere on
/// screen — every sky pixel in the frame is at least 43% this value, and the
/// band just above the horizon is essentially all of it.
///
/// That is what made the previous pair read as pale cyan milk. Measured on the
/// champion frame against the reference, sampling the clear sky column by
/// column and mapping screen row to blend:
///
/// | | champion sky | reference sky |
/// |---|---|---|
/// | red, horizon → top | 115 → 78 | **1 → 1** |
/// | green, horizon → top | 190 → 150 | 181 → 95 |
/// | blue, horizon → top | **255 → 255** | 242 → 200 |
///
/// Two separate defects, both of them authored here:
///
/// * **Red.** A daylight sky is very nearly a pure blue-green primary — the
///   reference carries a red channel of `0`–`1` across its entire sky. The old
///   horizon red of `0.22` put ~110 display levels of red under every sky pixel
///   in the frame, and red under blue is exactly the definition of a wash. That
///   single number is most of the distance between "coastal noon" and "overcast".
/// * **Blue clipping.** `0.80` linear displays at `231`, and the grade then
///   multiplies it by the white balance's `1.06`, the contrast's `1.1` and pushes
///   it *away* from luma at `1.18` saturation — it leaves the range. So did the
///   old zenith's `0.68`. **Both stops clipped**, which means the sky's blue
///   channel was a flat `255` from the horizon to the top of the frame: the
///   gradient, the sun's halo and the clouds' separation all existed only in red
///   and green, and the one thing a clear sky is made of was a constant.
///
/// So both stops are re-derived *through* the grade rather than authored by eye:
/// the reference's own sky is sampled per row, inverted through
/// [`super::GRADE`]'s exact chain (the saturation step preserves Rec.709 luma, so
/// it inverts exactly), and the two stops are the least-squares fit of the
/// shader's `mix(horizon, zenith, smoothstep(dir.y))` to that curve. Mean squared
/// error over the visible band falls from `12781` to `1340` per sample.
///
/// It stays the frame's **white level** in every other respect — it is still the
/// clear colour and still the horizon stop of the dome. It is simply 18% less
/// bright and no longer red, which is the correction the measurement asks for.
///
/// It is **no longer the depth fog's colour**. That was true when this was
/// authored and it is the reason the red above had to go: a stop fitted to a
/// clear sky is a near-pure blue-green primary, and a haze is neither. See
/// [`HAZE`] for the split and for the horizon seam it costs.
///
/// The thin pale haze strip the reference has hugging the horizon line itself
/// (`~(158, 210, 232)`, below about 5° of elevation) before the sky collapses to
/// blue is **no longer this pair's problem**. It was, and the note that used to
/// stand here said so: `smoothstep` is flattest near zero, so a two-stop dome
/// parameterised on the raw up-component holds its horizon colour over a wide
/// band — exactly the wrong shape for a tight haze layer — and getting it right
/// "would take a third stop or a horizon-hugging exponent in the sky shader,
/// which is an engine change, not a palette one."
///
/// That was the correct diagnosis, and the engine change is the one that landed:
/// the gradient's *shape* is now authored separately from its two colours, as
/// [`SKY_HAZE_HEIGHT`]. This stop is free to go back to being the horizon
/// colour, and [`SKY_ZENITH`] free to go back to being the sky overhead.
///
/// This constant is the **white level of the whole frame**, not just the colour
/// of the empty top of it, and that is why it is authored this far down. It is
/// the clear colour (`set_clear_color`) and the horizon stop of the sky gradient,
/// so it is the tone the top 40% of the frame is made of and the tone the
/// vanishing point is seen *against*. What the receding surfaces themselves
/// converge on is [`HAZE`].
///
/// The previous `[0.0009, 0.0012, 0.0021]` was authored for a moonlit stage: a
/// near-black floor, so that the emissive cues were the only light in shot. The
/// reference is now a **daylight** coastal highway — a blue sky with a sun in it,
/// a turquoise sea and sunlit sand — and against that reference a near-black
/// horizon is not a dark grade, it is the wrong time of day, and no key light or
/// exposure can add daylight back to a frame whose atmosphere is night.
///
/// Held *below* the reference's whitest cloud on purpose: this is haze, not
/// highlight. Pushing it to white would blow the vanishing point out and take the
/// far lane markings with it, and there would be no headroom left for the sun
/// disc ([`SUN`]) or the clouds to read as brighter than the sky they sit in.
pub const SKY: [f32; 3] = [0.024, 0.44, 0.63];

/// How solid the ghost car is, `0` invisible … `1` opaque.
///
/// Tuned against the rendered frame rather than derived: the 3D pipeline does
/// not back-face cull, so every translucent box blends twice over itself, and
/// the car's parts overlap on top of that. The value that *looks* like a third
/// opaque is well under a third.
pub const GHOST_OPACITY: f32 = 1.0;

/// The sky directly overhead, and the top of the frame's gradient.
///
/// **Deeper and far more saturated than [`SKY`], not paler.** That is the way a
/// clear daylight sky sits, and it is the same aerial-perspective fact that
/// governed the night version: the band just above the ground is the *lightest*,
/// because that is where the atmosphere is thickest and scatters the most, and
/// the deepest, bluest part is overhead where you are looking through the least
/// air. Getting this the wrong way round is what makes a clear noon read as
/// overcast. [`SKY`] stays the *horizon* colour because that is where the dome
/// is palest, which is the same aerial-perspective fact stated twice.
///
/// Displayed, this is roughly `(7, 48, 111)` before the grade. Note how *little*
/// red it holds — a daylight zenith is nearly a pure blue primary, and the usual
/// mistake is to author it as a light grey-blue, which is a hazy sky, not a clear
/// one.
///
/// **This stop is once again a colour rather than a slope control**, and that is
/// why it moved.
///
/// It used to be neither. The chase camera looks down: the top row of the frame
/// sits ~32° up, `dir.y ≈ 0.53`, and the fixed `smoothstep(dir.y)` gradient the
/// sky shader used to evaluate reached a blend of only **0.545** there. Nothing
/// in shot was ever more than 54% of this value, so this constant was authored as
/// the *overshoot* needed to drag the visible band down — a number chosen for
/// where it lands at 54%, not for what the sky overhead is.
///
/// That approach had a hard ceiling and the champion frame hit it. Matching the
/// reference's top-of-frame green (`85` display, `0.093` linear) through
/// [`super::GRADE`] needs a blend of `0.88`; at the `0.545` the camera supplied,
/// the green here would have had to be **negative**. There was no value that
/// worked. The wall was the gradient's fixed shape, not either colour, which is
/// why the fix is [`SKY_HAZE_HEIGHT`] and not another guess at this stop.
///
/// With the haze band pulled down to `0.234`, the top row now blends at `0.88`
/// and this stop is *nearly all* of what shows there. So it is re-derived as what
/// it says it is — the sky straight overhead — by inverting the reference's own
/// measured top-of-frame colour through the grade and solving the two-stop mix at
/// the blend the camera actually delivers:
///
/// | at 32° up | reference | this pair, before | this pair, now |
/// |---|---|---|---|
/// | red | 0 | 3 | 0 |
/// | green | 85 | 139 | 85 |
/// | blue | 184 | 199 | 185 |
///
/// Every channel rose in *linear* terms while the displayed result got darker,
/// and that is not a contradiction — it is the whole point. The old numbers were
/// overshoots sized for 54% reach; at 88% reach an overshoot renders as a hole,
/// so each one moves back toward the colour it actually names. The blue moves
/// furthest (`0.16` → `0.27`) because it had the furthest to fall. It still
/// clears the clipping check in [`self::tests`] with room to spare — it grades to
/// `0.69`, against the `1.0` that destroyed the `0.68` before it.
pub const SKY_ZENITH: [f32; 3] = [0.0, 0.045, 0.27];

/// **The shape of the sky's gradient**, authored separately from its two colours:
/// the up-component (the sine of the elevation angle) at which the dome stands
/// halfway between [`SKY`] and [`SKY_ZENITH`].
///
/// `0.234` is 13.5° of elevation. The engine's default is `0.5` — 30° — which is
/// the plain `smoothstep(dir.y)` the sky shader evaluated before the parameter
/// existed, and which is wrong twice over for this frame:
///
/// * **It is not what air does.** Optical depth along a ray goes as roughly
///   `1 / sin(elevation)`, so a clear day's haze band is *tight*: the sky
///   collapses to its zenith blue within the first 15–20° and holds it the rest
///   of the way up. The reference shows exactly that — it falls from
///   `(1, 181, 242)` at 5° to `(1, 95, 200)` at 32°, most of that drop happening
///   in the first third of the span.
/// * **It is not what this camera shows.** A midpoint at 30° is a reasonable
///   default for a camera that looks at the sky. This one looks at a road: the
///   entire visible sky is the band from the horizon to 32°, so a 30° midpoint
///   means the frame shows only the flat bottom of the curve and the gradient
///   arrives as a wash however far apart the two stops are authored.
///
/// Pulling the midpoint to 13.5° fixes both at once, and — because the lift is
/// exact at both ends — it moves neither thing that is matched to this gradient:
/// the horizon still returns [`SKY`] exactly, which is what [`super::FrameDepthFog`]
/// fades every distant surface into, so the far road still dissolves into the sky
/// with no seam; and the zenith still returns [`SKY_ZENITH`] exactly.
///
/// Derived, not eyeballed: the reference's top-of-frame green inverted through
/// [`super::GRADE`] needs a blend of `0.88` at `dir.y = 0.53`, which is
/// `smoothstep` of a lifted `0.787`, which is a midpoint of `0.234`.
pub const SKY_HAZE_HEIGHT: f32 = 0.234;

/// The **depth fog's own colour** — the tone every distant surface recedes into,
/// and no longer the same number as [`SKY`].
///
/// ## Why the haze stopped being the sky
///
/// Binding the fog to [`SKY`] is the standard trick and it was the right default
/// while the two agreed. They do not agree here, and the champion frame measures
/// exactly how far apart they are. Sampling the road's centre column at the
/// vanishing point:
///
/// | | champion | reference |
/// |---|---|---|
/// | horizon band | `(87, 119, 129)` | `(157, 204, 210)` |
///
/// Two independent defects sit in that one triple, and **neither can be fixed
/// without the other**:
///
/// * **Not enough of it.** Solving the mix at that band gives a fog fraction of
///   roughly `0.4` where the reference's is `~0.9` — its vanishing point has
///   essentially *become* atmosphere, ours is still more than half tarmac. See
///   the range authored at the [`super::FrameDepthFog`] call site for the pull-in
///   that fixes the strength.
/// * **The wrong colour to have more of.** [`SKY`] displays, through
///   [`super::GRADE`], at `(12, 191, 248)` — a saturated cyan primary with the
///   red driven to nothing, which is correct for the *clear sky* it was
///   least-squares fitted to and wrong for haze. Pushing the density to `0.9`
///   against that colour lands the far road at `(18, 178, 228)`: the green and
///   blue arrive, and the red goes the wrong way, from `87` to `18`, against a
///   reference that wants `157`. The frame's largest single colour deficit is red
///   (the 3D band's mean red is `49` against the reference's `105`), and turning
///   the sky-coloured fog up makes it worse.
///
/// Haze is suspended water and dust lit by the whole sky *and* by sunlit ground.
/// It is pale and very nearly neutral. A clear zenith-to-horizon dome is a
/// near-pure blue-green primary. Those are two different colours and this app was
/// spending one number on both.
///
/// ## Derivation
///
/// Not authored by eye. The reference's own horizon band, `(157, 204, 210)`, is
/// inverted through [`super::GRADE`]'s exact chain — saturation about Rec.709
/// luma, then the contrast S-curve about `0.5`, then exposure × white balance,
/// then the sRGB transfer the backend's render target writes with — which lands
/// on this linear triple. Forward through the same chain it returns
/// `(157, 205, 211)`, within a display level on every channel.
/// [`tests::the_haze_is_the_reference_s_own_horizon_band_and_it_is_not_the_sky`]
/// is that round trip, asserted.
///
/// ## The one thing this costs, and who owns it
///
/// The dome and the fog now meet at the horizon line carrying different colours —
/// a pale warm haze below, a cyan sky above — so there is a step across that row
/// where there used to be none. That step is the **two-stop dome's** limitation,
/// not this constant's: [`SKY`] already documents that `smoothstep` is flattest
/// near zero and therefore cannot hold the thin pale band the reference has
/// hugging its own horizon (`~(158, 210, 232)`, below about 5° of elevation).
/// Closing it takes a third stop or a horizon-hugging exponent in the sky shader,
/// which is an engine change.
///
/// The trade is not close. The seam is one row. The surfaces the fog actually
/// paints — the far road, the receding palm rank, the headland, the skyline — are
/// hundreds of rows, and they are the ones the reference measures against.
pub const HAZE: [f32; 3] = [0.35, 0.53, 0.49];

/// The sun's disc colour — **deliberately far above `1.0`**.
///
/// Every other colour in this file is a reflectance and belongs in `0..1`. This
/// one is a radiance: it is the brightest thing in the frame by a wide margin,
/// and authoring it at white would make it a flat white circle. The surplus over
/// white is what the frame's bloom spends on the flare around it, which is what
/// makes it read as a light source rather than a sticker.
///
/// **Warm, and brightest in red** — the inversion of the cool, blue-brightest
/// disc this replaced. A midday sun is not neutral white against its own sky:
/// the sky took the blue out of it (that is *why* the sky is blue), so the disc
/// and the flare around it run warm while the shadows they leave stay cool. That
/// split — warm light, cool shade — is most of what makes a frame read as
/// sunlit rather than merely bright, and it cannot be graded in afterwards: the
/// three grade knobs are global, so a warmth added there warms the shadows too.
///
/// How far above white barely matters, and that is worth knowing before tuning
/// it: the render target is 8-bit, so every value at or above `1.0` is already
/// clamped to white before the bloom's bright pass samples it. What decides how
/// much the sun glows is therefore the *area* of above-threshold pixels — the
/// disc plus its halo — not this number. Reach for `MOON_HALO_FALLOFF` when the
/// glow is wrong; this only has to clear `1.0` to say "radiance, not paint".
pub const SUN: [f32; 3] = [2.4, 2.25, 1.95];

/// The tarmac's own colour — the largest surface in any frame, and therefore the
/// one that decides the **colour temperature of the whole shot**.
///
/// It was `[0.085, 0.088, 0.105]`: a deliberate blue tilt, blue a quarter above
/// red. That reads as a reasonable choice in isolation and is wrong in context,
/// because it is the *third* blue-weighted term stacked on the same pixels. The
/// hemisphere ambient's sky colour is blue by 1.9× red, the depth fog fades the
/// far road toward a cool [`HAZE`], and the key light is cool by authorship.
/// Multiply a blue albedo by a blue light under a blue fog and the road does not
/// read as *lit coolly* — it reads as **navy**, which is what the champion's
/// lower half is and what the reference's is not. Measured against the reference,
/// the champion's near tarmac carries a blue excess of roughly a third over red
/// where the reference's is neutral-to-warm.
///
/// The physical fact the old value contradicted: bitumen is a warm near-black.
/// Real asphalt is neutral with a brown tilt, and every cool cast a night road
/// carries is the *moon's*, not the road's. Putting the cool in the light and
/// keeping it out of the surface is exactly what gives the reference its
/// warm-neutral near lane under a cold sky, instead of one flat blue wash — the
/// near road is where the car's own warm pool light wins, and the far road, lit
/// by nothing but ambient and fog, stays cold. A blue albedo erases that split.
///
/// So this is a **pure hue rotation, not an exposure change**: red and blue swap
/// roles at the same magnitude (blue was 1.24× red; red is now 1.19× blue), and
/// the Rec.709 luminance is held at `0.0889` against the old `0.0886` — a 0.3%
/// difference, below a display level. Nothing here lifts or crushes the frame,
/// so it cannot disturb the black point the grade is spending on the floor, and
/// the software arm — which sees the same albedo through a different shader —
/// changes colour without changing brightness.
///
/// Named once because it was written out four times (two material arms and two
/// tests) and a colour authored in four places is a colour that drifts.
pub const TARMAC: [f32; 3] = [0.095, 0.088, 0.080];

/// ## The gloss set, and why every value in it moved with the era
///
/// `roughness` (`0` mirror-smooth … `1` matte) is the one authored material
/// property that is **not** self-contained: the backend spends it as
/// `spec = pow(N·H, 48) * (1 - roughness)` and then adds
/// `light_colour * light_intensity * spec` on top of the diffuse term
/// (`axiom-gpu-backend/src/scene_renderer.rs`). A gloss value is therefore only
/// meaningful *against a key intensity*, and the peak radiance a surface can
/// throw is exactly `(1 - roughness) * KEY_INTENSITY`.
///
/// Every number below was authored against the **moonlit** key, which ran at
/// `0.85`. The era-C rig is a midday sun at [`super::KEY_INTENSITY`] `2.6` —
/// **3.06×** — and nothing rescaled the gloss with it, so every specular peak in
/// the frame was multiplied by three and clipped. That is not a subtle shift; it
/// is the champion frame's single largest defect and it lands on the two things
/// the eye goes to first:
///
/// * the **tarmac** at `0.68` threw `0.32 × 2.6 = 0.83` linear — an order of
///   magnitude over its own diffuse value (`0.09 × 2.6 × N·L ≈ 0.08`) — so the
///   broad `48`-power lobe painted a blown white sheet from the bumper to the
///   vanishing point. It erased the asphalt grain [`super::asphalt_texture`]
///   exists to provide, it erased the lane markings' contrast against the road,
///   and it read as a lens defect rather than as a surface.
/// * the **car**: paint at `0.30` threw `1.82` and glass at `0.12` threw `2.29`,
///   both far past the `1.0` the 8-bit target can hold. The bonnet is very nearly
///   a road-parallel plane, so it caught the same lobe the road did and went
///   white — taking the twin stripes, which are the model's whole read, with it.
///
/// The reference decides the direction, and it is unambiguous: a **dry** road at
/// noon under a high sun is matte. There is no sheen anywhere in that frame — the
/// tarmac is flat charcoal carrying palm shadows, and the only blown pixels in
/// the shot belong to the sun itself. The wet-look streak was a night-stage
/// device (it existed to put the moon's mark on the largest surface in a frame
/// that had almost no light in it), and daylight retires it: by day the tarmac
/// gets its tonal range from the sun's diffuse, from cast shadows and from
/// atmosphere, none of which a night stage had.
///
/// So the set is re-sized **by the budget, not by eye**: each value is chosen so
/// that `(1 - roughness) * KEY_INTENSITY` lands where that surface belongs, and
/// the ordering glass > paint > tarmac — the physical fact that survives the era
/// change — is preserved exactly.
/// [`tests::the_gloss_set_is_sized_against_the_key_it_is_lit_by`] is the
/// assertion that keeps the two in step, and it is the one this module was
/// missing: it fires the next time the key moves without the gloss moving.
///
/// **The key has been re-exposed `1.84 → 5.9`, and this set moves with it —
/// holding every peak byte-identical.** [`super::KEY_INTENSITY`] was solved
/// against the wrong statistic (the warm *median* of a road that is 39% palm
/// shadow, i.e. the penumbra) and is now solved against the reference's sunlit
/// mode. A re-exposure that does not carry the gloss with it is the exact defect
/// this whole comment was written about, one era later and three times worse: at
/// the old `(1 - roughness)` the tarmac would throw `0.06 × 5.9 = 0.354` linear —
/// past the `0.25` wall below, the blown white sheet again.
///
/// So every `(1 - roughness)` is scaled by `1.84 / 5.9 = 0.3119`, which leaves
/// each surface's peak radiance exactly where the paragraphs above put it. The
/// products are unchanged to three places, the ordering is unchanged, and no
/// pixel of specular in the frame moves. **This move is specular-neutral by
/// construction** — the diffuse exposure rises, the highlights do not, which is
/// the only shape a re-exposure of a Lambert-plus-lobe rig may take.
///
/// Tarmac: `0.0187 × 5.9 = 0.110` linear at the exact mirror point (was
/// `0.06 × 1.84 = 0.110`) over a diffuse base that has itself risen to ~0.22. A
/// whisper of directionality down the sun line, so the road is not one flat fill
/// across its length — and nothing the eye reads as a highlight; against the
/// brighter diffuse it is now *less* of a highlight than it was, which is what
/// the reference's matte noon tarmac wants.
pub const TARMAC_ROUGHNESS: f32 = 0.9813;

/// Automotive clear-coat. `0.0499 × 5.9 = 0.294` linear (was `0.16 × 1.84 =
/// 0.294`) — a real, visible highlight riding the crown of the bonnet and the
/// shoulder line, about a stop under clipping, so the stripes and the panel
/// breaks stay legible underneath it. See [`TARMAC_ROUGHNESS`] for the budget
/// this is drawn from and for why the number moved while the product did not.
pub const CAR_PAINT_ROUGHNESS: f32 = 0.9501;

/// Glazing — still the glossiest surface in the frame, as it must be.
/// `0.0998 × 5.9 = 0.589` linear (was `0.32 × 1.84 = 0.589`): the windscreen
/// holds a hot sun glint at the mirror point and stays a dark near-black
/// trapezoid everywhere else, which is what a raked screen at noon actually
/// looks like. See [`TARMAC_ROUGHNESS`].
pub const CAR_GLASS_ROUGHNESS: f32 = 0.9002;

/// Register the four road materials.
///
/// The tarmac is the one material here that carries a **texture**: it is the
/// largest surface in any frame, and a flat fill renders it identically at eight
/// metres and at sixty, which is the one thing real asphalt never does. See
/// [`super::asphalt_texture`] for the grain and for why it is deliberately quiet.
/// A malformed buffer would be a bug in that module rather than a condition to
/// handle at runtime, so an unexpected rejection simply leaves the tarmac
/// untextured — exactly what it was before, never a missing road.
pub fn road_materials(app: &mut RunningApp) -> RoadMaterials {
    let asphalt = app
        .add_texture_data(ASPHALT_RES, ASPHALT_RES, asphalt_albedo())
        .ok();
    let ground = app
        .add_texture_data(VERGE_RES, VERGE_RES, verge_albedo())
        .ok();
    RoadMaterials {
        // Asphalt: dark, and neutral-warm — see [`TARMAC`] for why the hue is
        // the road's single most consequential number and why it is no longer
        // blue. It still separates from the verge, which is green and half a
        // stop brighter. The grain multiplies this base colour (the shader
        // computes `albedo * colour`), so the hue lives here and only the
        // shading is sampled.
        // `roughness` is [`TARMAC_ROUGHNESS`], and that constant carries the
        // whole argument: the value here was sized against a moonlit key three
        // times dimmer than the era-C sun, and left unchanged it turns the
        // largest surface in the frame into a blown white sheet that erases the
        // grain sampled just above it. A dry road at noon is matte.
        surface: app.add_material(
            asphalt
                .map(|t| {
                    Material::lit(rgb(TARMAC[0], TARMAC[1], TARMAC[2]))
                        .with_custom_texture(t.id())
                        .with_roughness(ratio(TARMAC_ROUGHNESS))
                        // The tarmac is the one surface in this game that runs
                        // from under the front wheels to the vanishing point, so
                        // it is the one that needs anisotropic sampling. At the
                        // camera's grazing angle a screen pixel a few hundred
                        // metres out covers a metre or more *along* the road
                        // while still covering only centimetres *across* it. A
                        // trilinear sampler picks its mip level from the larger
                        // of those two, so it would blur the road laterally by
                        // that same ratio — the grain, and with it any sense of
                        // surface, would wash out to flat grey a short way past
                        // the car. Anisotropic filtering averages along the long
                        // axis only, which is exactly the shape of the problem.
                        .with_texture_sampling(TextureSampling::Anisotropic)
                })
                .unwrap_or_else(|| {
                    Material::lit(rgb(TARMAC[0], TARMAC[1], TARMAC[2]))
                        .with_roughness(ratio(TARMAC_ROUGHNESS))
                }),
        ),
        // Paint: a real white pigment plus a low emissive floor, so the lane
        // markings hold their brightness in shadow, inside the tunnel, and
        // against the night ambient — the reference's markings are the second
        // brightest thing in the frame after the tail lamps.
        paint: app.add_material(
            Material::lit(rgb(0.72, 0.73, 0.70)).with_emissive(rgb(0.30, 0.30, 0.28)),
        ),
        // Guardrail and tunnel wall: mid grey, matte.
        rail: app.add_material(Material::lit(rgb(0.38, 0.40, 0.44))),
        // The verge — the roadside ground, and until now the frame's last
        // genuine flat fill. It is the second-largest ground plane in any shot
        // (the quad reaches `VERGE_REACH` past the barrier, forty-odd metres
        // either side, from the bumper to the horizon) and it rendered *exactly*
        // one RGB triple across all of it: sampling the champion frame's verge
        // returns a standard deviation of `0.00` over thousands of pixels, where
        // the reference's cleanest roadside still measures ~1-2 levels and reads
        // as dry earth breaking through low cover.
        //
        // [`super::verge_texture`] authors that mix. The base colour here is not
        // a new colour — it is the channel-wise maximum of the texture's two
        // targets, because a custom albedo can only darken, and the module holds
        // the *textured mean* on the flat fill's own luminance so adding a
        // surface is not also a grade. The colour still spans zone boundaries
        // neutrally, which is why one mesh can carry it.
        //
        // Sampled `Crisp`, unlike the tarmac: `Crisp` still minifies linearly
        // across a real mip chain, and the verge has no lane-width lateral
        // feature that has to survive at the vanishing point, so it does not
        // need — or want — the tarmac's 16x anisotropy.
        verge: app.add_material(
            ground
                .map(|t| {
                    Material::lit(rgb(VERGE_BASE[0], VERGE_BASE[1], VERGE_BASE[2]))
                        .with_custom_texture(t.id())
                })
                .unwrap_or_else(|| {
                    Material::lit(rgb(VERGE_BASE[0], VERGE_BASE[1], VERGE_BASE[2]))
                }),
        ),
    }
}

/// The tint a zone's large scenery takes.
pub const fn zone_tint(zone: Zone) -> [f32; 3] {
    match zone {
        Zone::Meadow => [0.20, 0.34, 0.17],
        Zone::Coast => [0.24, 0.32, 0.30],
        Zone::Forest => [0.11, 0.26, 0.14],
        Zone::Tunnel => [0.24, 0.24, 0.27],
        Zone::Industrial => [0.30, 0.29, 0.26],
        Zone::Canyon => [0.36, 0.24, 0.17],
    }
}

/// The six materials one car body is built from.
///
/// A livery is what makes the *same* car model render as two different cars.
/// The player's is opaque paint; the ghost's is the translucent set below. The
/// alternative — a second `PlayerCar::install` that reaches into the palette for
/// different fields — would duplicate the model's material choices in two
/// places and let them drift.
#[derive(Debug, Clone, Copy)]
pub struct CarLivery {
    /// Painted bodywork.
    pub body: Handle<Material>,
    /// Glazing.
    pub glass: Handle<Material>,
    /// Tyres, and the near-black valance.
    pub tyre: Handle<Material>,
    /// Twin bonnet stripes and the number plate — the pale trim that stops the
    /// paint reading as one unbroken coloured shell.
    pub trim: Handle<Material>,
    /// Tail lamps.
    pub brake_light: Handle<Material>,
    /// The boost plume.
    pub exhaust: Handle<Material>,
}

/// Every material the scene uses, registered once at install.
#[derive(Debug, Clone, Copy)]
pub struct ScenePalette {
    pub road: RoadMaterials,
    /// Reflector posts — the brightest thing in the frame, on purpose.
    pub post: Handle<Material>,
    /// Tree trunks and utility poles.
    pub timber: Handle<Material>,
    /// Tree crowns.
    pub foliage: Handle<Material>,
    /// Rock and distant hills.
    pub stone: Handle<Material>,
    /// Sign boards.
    pub sign: Handle<Material>,
    /// Tunnel ceiling lights.
    pub lamp: Handle<Material>,
    /// Low industrial buildings.
    pub building: Handle<Material>,
    /// The player's body.
    pub car_body: Handle<Material>,
    /// The player's glass.
    pub car_glass: Handle<Material>,
    /// Wheels, on every car.
    pub tyre: Handle<Material>,
    /// The player's stripes and number plate.
    pub car_trim: Handle<Material>,
    /// Brake lights.
    pub brake_light: Handle<Material>,
    /// Boost exhaust.
    pub boost_flame: Handle<Material>,
    /// Traffic bodies, one per variant.
    pub traffic: [Handle<Material>; 4],
    /// Traffic tail lights.
    pub traffic_light: Handle<Material>,
    /// Wind/speed streaks.
    pub streak: Handle<Material>,
    /// Impact sparks.
    pub spark: Handle<Material>,
    /// The finish arch.
    pub finish: Handle<Material>,
    /// The ghost car's translucent livery.
    pub ghost: CarLivery,
}

impl ScenePalette {
    /// The player's own livery — opaque paint, glass, rubber and lamps.
    pub const fn player_livery(&self) -> CarLivery {
        CarLivery {
            body: self.car_body,
            glass: self.car_glass,
            tyre: self.tyre,
            trim: self.car_trim,
            brake_light: self.brake_light,
            exhaust: self.boost_flame,
        }
    }
}

impl ScenePalette {
    /// Register every material. Called once, at install.
    pub fn install(app: &mut RunningApp) -> ScenePalette {
        let lit = |app: &mut RunningApp, c: [f32; 3]| app.add_material(Material::lit(rgb(c[0], c[1], c[2])));
        // A lit material with an authored surface roughness (`0` mirror-smooth …
        // `1` matte), which the backends turn into a specular highlight strength.
        let glossy = |app: &mut RunningApp, c: [f32; 3], roughness: f32| {
            app.add_material(
                Material::lit(rgb(c[0], c[1], c[2])).with_roughness(ratio(roughness)),
            )
        };
        let glowing = |app: &mut RunningApp, c: [f32; 3], e: [f32; 3]| {
            app.add_material(Material::lit(rgb(c[0], c[1], c[2])).with_emissive(rgb(e[0], e[1], e[2])))
        };
        // Lit, faintly self-luminous, and *translucent* — the ghost set. The
        // emissive term is what keeps it readable at night: a purely diffuse
        // surface at a third opacity all but vanishes against a dark road.
        let ghostly = |app: &mut RunningApp, c: [f32; 3], e: [f32; 3], opacity: f32| {
            app.add_material(
                Material::lit(rgb(c[0], c[1], c[2]))
                    .with_emissive(rgb(e[0], e[1], e[2]))
                    .with_opacity(ratio(opacity)),
            )
        };
        ScenePalette {
            road: road_materials(app),
            // Retro-reflective amber: dim plastic that throws back a lot of
            // light. The albedo is what the post looks like switched off.
            post: glowing(app, [0.34, 0.26, 0.06], [1.0, 0.66, 0.10]),
            timber: lit(app, [0.16, 0.12, 0.09]),
            // Every leaf surface in the game — the palm crowns that line the
            // coast, the shrub clumps on the verge, the conifer cones inland —
            // and until now the frame's largest remaining flat fill after the two
            // ground planes. A palm crown rendered as exactly two RGB triples:
            // one on the up-facing blades, one on the down-facing ones. The
            // reference's crowns measure a **13.2%** median within-frond
            // variation, entirely inside one lit blade, and most of it is
            // chromatic — its green channel barely moves while red and blue swing
            // in opposition across every leaflet.
            //
            // [`super::foliage_texture`] authors that comb. `quad` stretches the
            // texture once across each blade with `u` across its width and `v`
            // along its length, so the leaflets, the rachis at `u = 0.5` and the
            // bright silhouette edges are all the real parts of the leaf rather
            // than a pattern laid over it.
            //
            // The base colour is `foliage_base()`, not [`FOLIAGE`]: a custom
            // albedo can only darken, this pattern's mean multiplier is ~0.56,
            // and the module divides that back out per channel so the textured
            // crown displays the same colour the flat fill did. Adding a surface
            // is not also a grade. The untextured arm keeps [`FOLIAGE`] itself,
            // because without the texture there is nothing to compensate for.
            foliage: {
                let base = foliage_base();
                let leaf = app
                    .add_texture_data(FOLIAGE_RES, FOLIAGE_RES, foliage_albedo())
                    .ok();
                app.add_material(
                    leaf.map(|t| {
                        Material::lit(rgb(base[0], base[1], base[2]))
                            .with_custom_texture(t.id())
                    })
                    .unwrap_or_else(|| {
                        Material::lit(rgb(FOLIAGE[0], FOLIAGE[1], FOLIAGE[2]))
                    }),
                )
            },
            stone: lit(app, [0.28, 0.26, 0.24]),
            sign: glowing(app, [0.62, 0.64, 0.60], [0.22, 0.22, 0.20]),
            lamp: glowing(app, [0.30, 0.28, 0.24], [1.0, 0.86, 0.52]),
            building: lit(app, [0.22, 0.22, 0.25]),
            // Automotive clear-coat — glossier than anything else on the ground,
            // and the surface always closest to the camera. Without it the car is
            // a flat orange cut-out against a lit road; with it the sun rides
            // along its upper edges and the body reads as a curved metal shell.
            // Re-sized to [`CAR_PAINT_ROUGHNESS`] against the daylight key: the
            // bonnet is very nearly a road-parallel plane, so at the old value it
            // caught the same blown lobe the tarmac did and took the twin stripes
            // — the model's whole read — with it.
            car_body: glossy(app, [0.86, 0.16, 0.07], CAR_PAINT_ROUGHNESS),
            // Glass is glossier still, and nearly black in albedo — so almost
            // everything it shows is reflection, which is exactly what a raked
            // windscreen looks like. [`CAR_GLASS_ROUGHNESS`] keeps it the frame's
            // glossiest surface while holding the glint to the mirror point
            // instead of flooding the whole screen white.
            car_glass: glossy(app, [0.07, 0.09, 0.13], CAR_GLASS_ROUGHNESS),
            tyre: lit(app, [0.045, 0.045, 0.05]),
            // Stripe and plate trim: pale, slightly warm, and with a whisper of
            // self-luminance. It is the only *light* value on the car, so it has
            // to survive a night key that leaves the paint at a quarter value —
            // a purely diffuse pale grey goes the same dark as the bodywork and
            // the stripes disappear, which is the whole point of having them.
            car_trim: glowing(app, [0.72, 0.70, 0.64], [0.10, 0.09, 0.08]),
            // The player's tail lamps sit *inside* a red body. Their albedo is a
            // dark red lens — DARKER than the paint around it, which is what a
            // lens actually is — and every bit of their separation comes from the
            // emissive, so they read as two hot strips at any angle, in shadow,
            // and with the sun behind the car.
            brake_light: glowing(app, [0.20, 0.02, 0.01], [1.0, 0.18, 0.10]),
            boost_flame: glowing(app, [0.10, 0.16, 0.22], [0.70, 0.95, 1.0]),
            traffic: [
                lit(app, [0.30, 0.44, 0.66]),
                lit(app, [0.62, 0.60, 0.55]),
                lit(app, [0.20, 0.46, 0.32]),
                lit(app, [0.52, 0.42, 0.16]),
            ],
            // Traffic tail lamps: the same dark-lens rule. They no longer need to
            // out-luminance their host body in albedo, because the emissive does
            // the separating — so they can be the deep red a tail lamp is.
            traffic_light: glowing(app, [0.16, 0.02, 0.01], [1.0, 0.14, 0.06]),
            streak: glowing(app, [0.14, 0.16, 0.20], [0.55, 0.66, 0.85]),
            spark: glowing(app, [0.24, 0.18, 0.06], [1.0, 0.78, 0.28]),
            finish: glowing(app, [0.08, 0.22, 0.16], [0.26, 1.0, 0.66]),
            // The ghost. Cold cyan-white against the player's hot orange, so at a
            // glance you always know which car is yours, and translucent through
            // the engine's real alpha path (`Material::with_opacity` — folded
            // into the per-draw alpha, blended with `ALPHA_BLENDING`, and sorted
            // back-to-front by `axiom-render`).
            //
            // The authored numbers are deliberately *lower* than the opacity you
            // want to see. The 3D pipeline draws with `cull_mode: None`, so a
            // translucent box blends its far faces and then its near faces over
            // them, and the car's parts overlap each other as well — a ghost
            // authored at 0.5 reads nearly solid. These are tuned by eye against
            // the rendered frame, not by arithmetic.
            ghost: CarLivery {
                body: ghostly(app, [0.30, 0.70, 0.95], [0.05, 0.22, 0.34], GHOST_OPACITY),
                glass: ghostly(app, [0.12, 0.26, 0.38], [0.02, 0.08, 0.14], GHOST_OPACITY * 0.8),
                tyre: ghostly(app, [0.06, 0.10, 0.14], [0.0, 0.0, 0.0], GHOST_OPACITY),
                trim: ghostly(app, [0.40, 0.62, 0.78], [0.10, 0.26, 0.36], GHOST_OPACITY),
                // A ghost's lamps are a hint, not a warning — dimmer than the
                // player's, so they never read as *your* brake lights.
                brake_light: ghostly(app, [0.10, 0.18, 0.24], [0.20, 0.55, 0.75], GHOST_OPACITY),
                exhaust: ghostly(app, [0.10, 0.20, 0.28], [0.35, 0.70, 0.95], GHOST_OPACITY),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom::prelude::{App, DefaultPlugins, Window};

    fn app() -> RunningApp {
        App::new()
            .window(Window::new(64, 64))
            .add_plugins(DefaultPlugins)
            .setup(|_, _, _| {})
            .build()
    }

    #[test]
    fn colours_clamp_rather_than_producing_an_invalid_ratio() {
        let c = rgb(-1.0, 0.5, 4.0);
        assert_eq!(c.to_array()[0], 0.0);
        assert!((c.to_array()[1] - 0.5).abs() < 1.0e-6);
        assert_eq!(c.to_array()[2], 1.0);
        assert_eq!(ratio(f32::NAN).get(), 0.0);
    }

    #[test]
    fn every_material_registers_and_they_are_distinct() {
        let mut app = app();
        let p = ScenePalette::install(&mut app);
        let handles = [
            p.road.surface,
            p.road.paint,
            p.road.rail,
            p.road.verge,
            p.post,
            p.timber,
            p.foliage,
            p.stone,
            p.sign,
            p.lamp,
            p.building,
            p.car_body,
            p.car_glass,
            p.tyre,
            p.car_trim,
            p.brake_light,
            p.boost_flame,
            p.traffic_light,
            p.streak,
            p.spark,
            p.finish,
        ];
        // Distinct handles: a duplicate would silently merge two draw groups and
        // paint something the wrong colour.
        for (i, a) in handles.iter().enumerate() {
            for b in handles.iter().skip(i + 1) {
                assert_ne!(a, b, "material {i} is shared");
            }
        }
        for variant in p.traffic {
            assert!(!handles.contains(&variant), "traffic variants are their own");
        }
    }

    /// The contrast rule, asserted: tarmac dark, paint bright, posts brighter.
    #[test]
    fn the_road_is_dark_and_the_speed_cues_are_bright() {
        let luminance = |c: [f32; 3]| c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722;
        let asphalt = luminance(TARMAC);
        // Paint reads as its pigment PLUS its emissive floor — the second term is
        // what keeps the markings white inside the tunnel and under the night sky.
        let paint = luminance([0.72, 0.73, 0.70]) + luminance([0.30, 0.30, 0.28]);
        // A reflector post is dim plastic that emits; its emitted light is the
        // number that has to dominate the road.
        let post = luminance([1.0, 0.66, 0.10]);
        assert!(asphalt < 0.15, "the tarmac is nearly black");
        assert!(paint > 0.7, "the paint is nearly white");
        assert!(post > asphalt * 4.0, "the reflectors dominate the tarmac");
        // ...and the road still reads as a dark ribbon laid across a bright
        // world, which is what the daylight reference shows: the tarmac is a
        // fraction of the sky it sits under, so the frame's contrast comes from
        // the road being dark rather than from the sky being dim.
        assert!(
            luminance(SKY) > asphalt * 4.0,
            "the road no longer separates from the sky above it"
        );
    }

    /// The **time of day**, pinned where it is actually decided.
    ///
    /// [`SKY`] is not just the empty top of the frame: it is the clear colour and
    /// the horizon stop of the gradient, and it sets the level [`HAZE`] is
    /// authored against. A frame whose atmosphere is authored at night is a
    /// night frame however it is lit and however it is graded — the fog adds its
    /// colour after the lighting, and the three grade knobs (exposure, contrast,
    /// saturation) are global multiplies that cannot put daylight into a black
    /// horizon. So the daylight the reference is shot in lives here, and the two
    /// properties that make it read as a *clear* day rather than an overcast one
    /// are asserted rather than left to a comment.
    #[test]
    fn the_atmosphere_is_a_clear_daylight_sky_and_the_zenith_is_the_deep_end() {
        let luminance = |c: [f32; 3]| c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722;
        assert!(
            luminance(SKY) > 0.3,
            "the horizon haze is not daylight: {:?}",
            luminance(SKY)
        );
        // Aerial perspective, in both directions: the horizon is the *paler*
        // stop (most air, most scattering) and the zenith is the deeper, more
        // saturated one. Inverting this is what makes a clear noon look overcast.
        assert!(
            luminance(SKY_ZENITH) < luminance(SKY),
            "the sky gets lighter overhead — that is an overcast lid, not a clear day"
        );
        let saturation = |c: [f32; 3]| c[2] / c[0].max(1.0e-6);
        assert!(
            saturation(SKY_ZENITH) > saturation(SKY) * 2.0,
            "the zenith is no more saturated than the haze: the gradient has no blue in it"
        );
        // And the sun is a radiance, warm, and over the top of the range so the
        // bloom has something above threshold to spread.
        assert!(SUN.iter().all(|c| *c > 1.0), "the sun is paint, not light: {SUN:?}");
        assert!(SUN[0] > SUN[2], "a midday sun is warm; its sky took the blue out of it");
    }

    /// **A sky colour is only meaningful through the grade it is displayed
    /// under**, and the champion frame's sky proved it: both stops were authored
    /// as sensible-looking linear blues and both left the range once
    /// [`super::super::GRADE`] had run, so the finished frame's blue channel was a
    /// flat `255` from the horizon to the top of the image. A gradient whose
    /// dominant channel is a constant is not a gradient — the sun's halo, the
    /// clouds' separation and the dome's own depth all disappeared into it, and no
    /// exposure, bloom or saturation move could bring them back, because the
    /// information was destroyed at the clamp.
    ///
    /// This is the assertion the module was missing. It mirrors the host's grade
    /// chain (`axiom_host::frame_postprocess::grade_pixel`) over each authored
    /// stop and fails the moment either one clips, or the sky picks up a red cast,
    /// or the two stops stop producing a visible slope across the band the chase
    /// camera actually shows.
    #[test]
    fn the_sky_gradient_survives_the_grade_without_clipping_or_going_grey() {
        let grade_cfg = super::super::GRADE;
        let (exposure, wb, contrast, saturation) = (
            grade_cfg.exposure().get(),
            grade_cfg.white_balance(),
            grade_cfg.contrast().get(),
            grade_cfg.saturation().get(),
        );
        // The mirror below omits the black-point floor removal, which this preset
        // does not use. If that ever changes, the mirror is wrong, not the sky.
        assert_eq!(
            grade_cfg.black_point().get(),
            0.0,
            "the grade grew a black point; this mirror no longer models it"
        );
        // Linear -> sRGB, the transfer the backend's render target writes with.
        let encode = |v: f32| {
            if v <= 0.003_130_8 {
                12.92 * v
            } else {
                1.055 * v.powf(1.0 / 2.4) - 0.055
            }
        };
        // (exposure x white balance) -> contrast S-curve about 0.5 -> saturation
        // about Rec.709 luma. Returned unclamped on purpose: the whole point is to
        // see whether a channel leaves `0..1` before the hardware clamps it.
        let graded = |lin: [f32; 3]| {
            let curve = |i: usize| ((encode(lin[i]) * exposure * wb[i]) - 0.5) * contrast + 0.5;
            let v = [curve(0), curve(1), curve(2)];
            let luma = 0.2126 * v[0] + 0.7152 * v[1] + 0.0722 * v[2];
            [
                luma + (v[0] - luma) * saturation,
                luma + (v[1] - luma) * saturation,
                luma + (v[2] - luma) * saturation,
            ]
        };

        for (name, stop) in [("horizon", SKY), ("zenith", SKY_ZENITH)] {
            let out = graded(stop);
            assert!(
                out[2] < 1.0,
                "the {name} stop's blue clips at {:.3} after the grade: the sky's \
                 own colour becomes a constant and the dome goes flat",
                out[2]
            );
            // A clear daylight sky is very nearly a pure blue-green primary. The
            // reference's carries a red channel of 0..1 across its whole span; red
            // under blue is the definition of a milky sky, and it is authored
            // here, never graded in.
            assert!(
                stop[0] < stop[2] * 0.08,
                "the {name} stop carries a red cast ({stop:?}) — that is haze, not \
                 a clear noon sky"
            );
        }

        // ...and the two stops produce a real slope across the band the camera
        // shows. The chase camera looks *down*, so the top row of the frame is
        // only ~32° up (`dir.y = 0.53`) — a pair that agrees over that range
        // renders as one flat wash however different the two numbers look.
        //
        // The blend it reaches there is no longer a fixed `smoothstep(dir.y)`:
        // it is `smoothstep` of the haze lift, mirroring
        // `axiom_host::frame_sky::haze_lift`, so this test tracks
        // [`SKY_HAZE_HEIGHT`] rather than pinning a number that silently goes
        // stale the moment the gradient's shape is re-authored. At the default
        // height of `0.5` the lift is the identity and this is the old `0.545`;
        // at the authored `0.234` it is `0.88`.
        const VISIBLE_TOP_UP: f32 = 0.53;
        let k = SKY_HAZE_HEIGHT / (1.0 - SKY_HAZE_HEIGHT);
        let lifted = VISIBLE_TOP_UP / (VISIBLE_TOP_UP + (1.0 - VISIBLE_TOP_UP) * k);
        let visible_top_blend = lifted * lifted * (3.0 - 2.0 * lifted);
        assert!(
            visible_top_blend > 0.8,
            "the top of the frame reaches only {visible_top_blend:.2} of the zenith \
             stop: the haze band is too wide for the band this camera shows"
        );
        let top = graded([
            SKY[0] * (1.0 - visible_top_blend) + SKY_ZENITH[0] * visible_top_blend,
            SKY[1] * (1.0 - visible_top_blend) + SKY_ZENITH[1] * visible_top_blend,
            SKY[2] * (1.0 - visible_top_blend) + SKY_ZENITH[2] * visible_top_blend,
        ]);
        let horizon = graded(SKY);
        let span = (horizon[2] - top[2]) * 255.0;
        assert!(
            span > 30.0,
            "the visible sky spans only {span:.0} display levels of blue between \
             its horizon and the top of the frame: that is a wash, not a dome"
        );
    }

    /// **The haze is not the sky**, and it is the reference's own horizon band.
    ///
    /// Both halves are the assertion. A fog colour bound to a clear-sky stop is
    /// the default every renderer reaches for, and it is why the champion frame's
    /// vanishing point measured `(87, 119, 129)` against the reference's
    /// `(157, 204, 210)`: turning that fog *up* — which the range at the
    /// [`super::FrameDepthFog`] call site now does — drives the far road's red
    /// from `87` toward the sky stop's `12` when the reference wants `157`. So
    /// the round trip is pinned here. If [`super::GRADE`] moves, or if someone
    /// re-binds the fog to [`SKY`], this fires.
    #[test]
    fn the_haze_is_the_reference_s_own_horizon_band_and_it_is_not_the_sky() {
        let grade_cfg = super::super::GRADE;
        let (exposure, wb, contrast, saturation) = (
            grade_cfg.exposure().get(),
            grade_cfg.white_balance(),
            grade_cfg.contrast().get(),
            grade_cfg.saturation().get(),
        );
        let encode = |v: f32| {
            if v <= 0.003_130_8 {
                12.92 * v
            } else {
                1.055 * v.powf(1.0 / 2.4) - 0.055
            }
        };
        let graded = |lin: [f32; 3]| {
            let curve = |i: usize| ((encode(lin[i]) * exposure * wb[i]) - 0.5) * contrast + 0.5;
            let v = [curve(0), curve(1), curve(2)];
            let luma = 0.2126 * v[0] + 0.7152 * v[1] + 0.0722 * v[2];
            [0, 1, 2].map(|i| ((luma + (v[i] - luma) * saturation) * 255.0).clamp(0.0, 255.0))
        };

        // The reference's measured horizon band, which is what this constant was
        // inverted out of. Two display levels of tolerance: the round trip is
        // through a power transfer, and the constant is authored to two decimals.
        const REFERENCE_HORIZON_BAND: [f32; 3] = [157.0, 204.0, 210.0];
        let haze = graded(HAZE);
        for (i, channel) in ["red", "green", "blue"].iter().enumerate() {
            assert!(
                (haze[i] - REFERENCE_HORIZON_BAND[i]).abs() <= 2.0,
                "the haze's {channel} lands at {:.0}, not the reference's {:.0}",
                haze[i],
                REFERENCE_HORIZON_BAND[i]
            );
        }

        // ...and it is a haze, not the dome. The clear sky is a near-pure
        // blue-green primary by assertion two tests up; suspended water and dust
        // lit by the whole sky plus sunlit ground is pale and very nearly
        // neutral, and the whole point of splitting the two constants is that the
        // frame's red survives the fog being turned up.
        let sky = graded(SKY);
        assert!(
            haze[0] > sky[0] + 100.0,
            "the haze carries no more red than the clear sky ({:.0} vs {:.0}): it \
             is the sky again under another name, and every distant surface will \
             recede into a cyan primary",
            haze[0],
            sky[0]
        );
        let spread = |c: [f32; 3]| {
            (c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])) / c[0].max(c[1]).max(c[2]).max(1.0)
        };
        assert!(
            spread(haze) < spread(sky) * 0.5,
            "the haze is as saturated as the dome ({:.2} vs {:.2}) — haze is pale",
            spread(haze),
            spread(sky)
        );
        // Pale, but still below the whitest thing in shot: this is atmosphere,
        // not a highlight, and a haze at white erases the vanishing point.
        assert!(
            haze.iter().all(|c| *c < 235.0),
            "the haze blows out the vanishing point: {haze:?}"
        );
    }

    /// The colour-temperature rule, pinned: **the cool on this stage belongs to
    /// the light, not to the road.**
    ///
    /// Three terms already tint the tarmac blue — the hemisphere ambient's sky
    /// colour, the depth fog's [`SKY`], and the moon key. A blue albedo under all
    /// three is what turns a moonlit road into a navy wash, and it is the one of
    /// the four that is a *surface* property and therefore simply wrong: bitumen
    /// is a warm near-black. So the tarmac is asserted warm-side-of-neutral, and
    /// asserted to have been rotated in hue *without* being re-exposed — a future
    /// edit that "fixes" the road by brightening it is not this rule.
    #[test]
    fn the_tarmac_is_warm_neutral_and_the_night_gets_its_cool_from_the_light() {
        let luminance = |c: [f32; 3]| c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722;
        assert!(
            TARMAC[0] > TARMAC[2],
            "the road is bitumen, not water: it may not be authored blue ({TARMAC:?})"
        );
        // Warm, but asphalt — not terracotta. A red/blue ratio past ~1.4 stops
        // reading as a road surface and starts reading as a tinted one.
        assert!(TARMAC[0] / TARMAC[2] < 1.4, "the tarmac is warm-neutral, not orange");
        // A hue rotation, not an exposure change: the grade's black point is
        // spending its whole budget on the frame's floor, and a brighter road
        // would take that back. Held within 2% of the value this replaced.
        assert!(
            (luminance(TARMAC) - 0.0886).abs() < 0.002,
            "the tarmac changed brightness, not just hue: {:?}",
            luminance(TARMAC)
        );
        // And the cool the frame does carry is still the light's: the ambient
        // that lands on this surface is blue-weighted by a wide margin.
        let ambient_sky = [0.19_f32, 0.25, 0.36];
        assert!(
            ambient_sky[2] > ambient_sky[0] * 1.5,
            "the sky fill is no longer the cool in the frame"
        );
    }

    /// The rule the module docs explain, inverted now that emissive is real:
    /// everything meant to glow carries its brightness in the **emissive** term,
    /// and its base colour is the plausible albedo of the thing switched off. An
    /// edit that pushes the brightness back into the base colour re-introduces
    /// the flat-slab bug — a lamp that dims when it turns away from the sun — so
    /// the rule is pinned here.
    #[test]
    fn everything_that_should_glow_glows_from_its_emissive_not_its_albedo() {
        let luminance = |c: [f32; 3]| c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722;
        // (name, base albedo, emissive) for every material whose job is to be seen.
        let glowing: [(&str, [f32; 3], [f32; 3]); 7] = [
            ("post", [0.34, 0.26, 0.06], [1.0, 0.66, 0.10]),
            ("lamp", [0.30, 0.28, 0.24], [1.0, 0.86, 0.52]),
            ("brake light", [0.20, 0.02, 0.01], [1.0, 0.18, 0.10]),
            ("boost flame", [0.10, 0.16, 0.22], [0.70, 0.95, 1.0]),
            ("traffic light", [0.16, 0.02, 0.01], [1.0, 0.14, 0.06]),
            ("spark", [0.24, 0.18, 0.06], [1.0, 0.78, 0.28]),
            ("finish", [0.08, 0.22, 0.16], [0.26, 1.0, 0.66]),
        ];
        let asphalt = luminance(TARMAC);
        for (name, albedo, emissive) in glowing {
            // The emitted light is what makes it bright, and it is bright: its
            // peak channel is at or near full and nothing in the shader scales it.
            let peak = emissive[0].max(emissive[1]).max(emissive[2]);
            assert!(peak >= 0.6, "{name} emits nothing worth seeing: {emissive:?}");
            assert!(
                luminance(emissive) > luminance(albedo) * 2.0,
                "{name} is still faking its glow in albedo: {albedo:?} vs {emissive:?}"
            );
            // Switched off, it is a dark object with shape — not a white cut-out.
            assert!(
                luminance(albedo) < 0.35,
                "{name}'s unlit albedo is too hot to read as an object: {albedo:?}"
            );
            // And lit, it still dominates the road it is seen against.
            assert!(
                luminance(emissive) > asphalt * 2.0,
                "{name} does not stand out against the tarmac"
            );
        }
    }

    /// The rule the tarmac comparison above cannot express: a lamp is only a lamp
    /// if it separates from the surface it is **mounted in**. That separation used
    /// to be albedo alone, which forced a red tail lamp inside a red body to run
    /// near-white and still lose (a `(1.0, 0.14, 0.08)` lamp displayed within a
    /// few sRGB steps of the `(0.86, 0.16, 0.07)` paint around it — one flat
    /// orange slab, no visible lights).
    ///
    /// The comparison has to be made **per channel, in the hue the lamp emits**,
    /// not in luminance: red is a low-luminance hue, so a deep red lamp can never
    /// out-*luminance* a large red-orange panel however hot it is — which is
    /// precisely why the old luminance rule kept pushing the lamps toward white
    /// and away from the reference. With a real emissive term the lamp wins on the
    /// only axis that matters: the body can never reflect more red than its own
    /// albedo times the light that reaches it, while the lamp adds its emissive on
    /// top of its own shading with nothing scaling it down.
    #[test]
    fn a_light_reads_against_the_body_it_is_mounted_on() {
        // Player: a dark lens in red paint. Albedo alone LOSES here on purpose —
        // the lamp is darker than the body it sits in, exactly like the real part.
        let car_body = [0.86, 0.16, 0.07];
        let brake_albedo = [0.20, 0.02, 0.01];
        let brake_emissive = [1.0, 0.18, 0.10];
        assert!(
            brake_albedo[0] < car_body[0],
            "the tail lens should be darker than the paint when it is switched off"
        );
        // Over-exposed in its own channel: full scale, which is the hottest
        // `rgb()` can author, and the look the night reference actually has.
        assert!(
            brake_emissive[0] >= 1.0,
            "the tail lamp is not running over-exposed: {brake_emissive:?}"
        );
        assert!(
            brake_emissive[0] > car_body[0],
            "the tail lamp emits less red than the paint can reflect: {:.2} vs {:.2}",
            brake_emissive[0],
            car_body[0]
        );
        // …and it stays a LAMP, not a white blob: the off-hue channels stay low,
        // so the strip reads red rather than blowing out to the body's orange.
        assert!(
            brake_emissive[1] < 0.30 && brake_emissive[2] < 0.30,
            "the tail lamp has washed out to white: {brake_emissive:?}"
        );

        // Traffic lamps sit on bodies of four different hues; the emitted red has
        // to beat the reddest of them, not just the average.
        let traffic_emissive = [1.0, 0.14, 0.06];
        let traffic: [[f32; 3]; 4] = [
            [0.30, 0.44, 0.66],
            [0.62, 0.60, 0.55],
            [0.20, 0.46, 0.32],
            [0.52, 0.42, 0.16],
        ];
        for body in traffic {
            assert!(
                traffic_emissive[0] > body[0],
                "the traffic lamp emits less red than its own car reflects: {body:?}"
            );
        }
    }

    /// **A gloss value is only meaningful against the key it is lit by.**
    ///
    /// This is the assertion the module was missing, and the champion frame's
    /// largest defect lived in the gap. The backend spends roughness as
    /// `spec = pow(N·H, 48) * (1 - roughness)` and adds
    /// `light_colour * light_intensity * spec` after the diffuse term, so a
    /// surface's peak specular radiance is exactly `(1 - roughness) * key`. Every
    /// other test in this file measures a colour, and a colour cannot see that
    /// product: the whole set stayed byte-identical while the era-C rig tripled
    /// `KEY_INTENSITY` (`0.85` → `2.6`) underneath it, and three surfaces
    /// silently went from "shiny" to "clipped white".
    ///
    /// The render target is 8-bit, so `1.0` is the whole budget. What each
    /// surface may spend of it is the physical claim:
    ///
    /// * **tarmac** — a dry road at noon is matte, and the reference has no sheen
    ///   on it at all. Anything approaching its own diffuse value stops being a
    ///   surface property and becomes a white sheet laid over the largest object
    ///   in the frame, erasing the asphalt grain and the lane markings with it.
    /// * **car paint** — a real highlight, and deliberately under `1.0`: past
    ///   that the bonnet clips and the twin stripes go with it.
    /// * **glass** — allowed to be the hottest, because a windscreen glint is the
    ///   one thing in shot that legitimately approaches the sun's own value. Not
    ///   past it.
    ///
    /// And the ordering, which is the part that must survive any future re-tune:
    /// glass is glossier than paint is glossier than tarmac. That is physics, not
    /// art direction, and no exposure change may invert it.
    #[test]
    fn the_gloss_set_is_sized_against_the_key_it_is_lit_by() {
        let key = super::super::KEY_INTENSITY;
        let peak = |roughness: f32| (1.0 - roughness) * key;

        // The road. Its own diffuse under this key is about
        // `TARMAC_g * key * N·L ≈ 0.088 * 5.9 * 0.40 ≈ 0.21`; a specular peak
        // that runs away from that is the blown streak, not a damp sheen.
        let tarmac = peak(TARMAC_ROUGHNESS);
        assert!(
            tarmac < 0.25,
            "the tarmac's specular peak is {tarmac:.2} linear against a key of \
             {key}; past ~0.25 the sun's lobe paints a blown white sheet from the \
             bumper to the vanishing point and the asphalt grain stops existing"
        );
        // ...and not zero: a road with no directionality at all is one flat fill
        // down its whole length, which is the defect the texture also fights.
        assert!(tarmac > 0.05, "the road has lost its sun line entirely: {tarmac:.3}");

        let paint = peak(CAR_PAINT_ROUGHNESS);
        assert!(
            (0.2..0.7).contains(&paint),
            "the car's clear-coat peaks at {paint:.2} linear; under ~0.2 it is a \
             flat cut-out and over ~0.7 the bonnet clips and the stripes vanish"
        );

        let glass = peak(CAR_GLASS_ROUGHNESS);
        assert!(
            glass < 1.0,
            "the windscreen peaks at {glass:.2} linear — past 1.0 the glint is not \
             a glint, it is a clipped region the bloom then spreads"
        );

        assert!(
            glass > paint && paint > tarmac,
            "glass > paint > tarmac is physics, not art direction: {glass:.2} / \
             {paint:.2} / {tarmac:.2}"
        );
    }

    #[test]
    fn every_zone_has_its_own_tint() {
        let tints: Vec<[f32; 3]> = Zone::ALL.iter().map(|z| zone_tint(*z)).collect();
        for (i, a) in tints.iter().enumerate() {
            for b in tints.iter().skip(i + 1) {
                assert_ne!(a, b, "zone {i} shares a tint");
            }
            assert!(a.iter().all(|c| (0.0..=1.0).contains(c)));
        }
    }

    /// **The two ground planes carry a surface; the paint and the rail do not.**
    ///
    /// Every half matters. Without a texture the tarmac — the largest surface in
    /// the frame — is a flat fill that renders identically at eight metres and at
    /// sixty, and the verge, the second largest, was measurably worse than that:
    /// exactly one RGB triple across thousands of pixels of the champion frame.
    /// With a texture on the *paint*, the lane markings — the app's whole speed
    /// cue, and the brightest thing on the road — would come out mottled instead
    /// of solid white, and the rail is a metre-tall strip seen edge-on that has
    /// nothing to gain. `material_textures` is the same resolution the backends
    /// read, so this asserts what actually reaches the GPU rather than what was
    /// authored.
    #[test]
    fn the_ground_planes_carry_a_surface_and_the_paint_and_rail_do_not() {
        use super::super::asphalt_texture::RES;
        use super::super::verge_texture::RES as VERGE;

        let mut app = app();
        let m = road_materials(&mut app);
        let textures = app.material_textures();
        let of = |h: Handle<Material>| {
            textures
                .iter()
                .find(|t| t.material_id() == h.id())
                .expect("every road material resolves a texture entry")
        };

        for (name, handle, res) in [("tarmac", m.surface, RES), ("verge", m.verge, VERGE)] {
            let entry = of(handle);
            let (w, h, pixels) = (entry.width(), entry.height(), entry.pixels().to_vec());
            assert_eq!((w, h), (res, res), "the {name} samples its authored texture");
            assert_eq!(pixels.len(), (res * res * 4) as usize);
            assert!(
                pixels.chunks(4).map(|t| t[0]).collect::<Vec<_>>().windows(2).any(|p| p[0] != p[1]),
                "a {name} texture that is one flat value is not a texture"
            );
        }

        // The 1x1 opaque-white fallback: an untextured material, unchanged.
        for other in [m.paint, m.rail] {
            let entry = of(other);
            assert_eq!(
                (entry.width(), entry.height(), entry.pixels()),
                (1, 1, [255, 255, 255, 255].as_slice()),
                "only the ground planes are textured"
            );
        }
    }

    /// The tarmac — and **only** the tarmac — is sampled anisotropically.
    ///
    /// Both halves are load-bearing. Without it the road's grain is blurred away
    /// laterally by the ratio between its along-view and across-view footprints,
    /// which at this camera's grazing angle is tens to one; the surface goes flat
    /// grey a short way past the car. With it on anything else, that material
    /// loses the hard magnified texels that are the engine's whole look, for a
    /// surface that never recedes far enough to need the trade.
    #[test]
    fn only_the_tarmac_is_sampled_anisotropically() {
        let mut app = app();
        let m = road_materials(&mut app);
        let textures = app.material_textures();
        let sampling = |h: Handle<Material>| {
            textures
                .iter()
                .find(|t| t.material_id() == h.id())
                .expect("the material resolves a texture entry")
                .sampling()
        };
        assert_eq!(
            sampling(m.surface),
            TextureSampling::Anisotropic,
            "the road runs to the horizon and must be filtered for it"
        );
        for other in [m.paint, m.rail, m.verge] {
            assert_eq!(
                sampling(other),
                TextureSampling::Crisp,
                "nothing but the tarmac should give up crisp magnification"
            );
        }
    }

    #[test]
    fn the_road_materials_are_four_distinct_handles() {
        let mut app = app();
        let m = road_materials(&mut app);
        let all = [m.surface, m.paint, m.rail, m.verge];
        for (i, a) in all.iter().enumerate() {
            for b in all.iter().skip(i + 1) {
                assert_ne!(a, b, "road material {i} is shared");
            }
        }
    }
}
