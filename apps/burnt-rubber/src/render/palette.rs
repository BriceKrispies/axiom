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

/// A colour from linear RGB components.
pub fn rgb(r: f32, g: f32, b: f32) -> Color {
    Color::linear_rgb(ratio(r), ratio(g), ratio(b))
}

/// A `Ratio`, clamped rather than fallible — every value here is authored.
pub fn ratio(v: f32) -> Ratio {
    Ratio::finite_or_zero(v.clamp(0.0, 1.0))
}

/// The night-adjacent sky the whole demo is lit against. Dark enough that the
/// paint, the reflectors and the boost read as genuinely bright.
///
/// Authored in **linear** light, which is not what it looks like: the backend
/// converts to sRGB for display, so a linear `0.055` lands on screen at roughly
/// `0.25` - a mid slate, not a night sky. These values are chosen for the
/// displayed result, which is why they look implausibly dark written down.
///
/// This constant is the **black level of the whole frame**, not just the colour
/// of the empty top of it, and that is why it is authored this far down. It is
/// three things at once: the clear colour (`set_clear_color`), the horizon stop
/// of the sky gradient, and — decisively — the colour [`super::FrameDepthFog`]
/// fades every distant surface into. Whatever value sits here is therefore the
/// darkest tone the frame can contain, and every receding thing is dragged up to
/// meet it.
///
/// The previous `[0.011, 0.015, 0.026]` was a *displayed* sRGB `(27, 33, 45)` —
/// measured on the rendered frame, the sky read a flat, milky navy across the
/// whole upper half, and the road dissolved into that navy rather than into the
/// night. A real moonlit night stage has no such floor: the sky off-axis from the
/// moon is essentially black, and the reference frame measures `(1.5, 2.4, 4.3)`
/// there — roughly a **seventeenth** of the linear radiance this constant was
/// carrying. Authoring the floor at that level is what lets the emissive cues
/// (paint, reflector posts, tail lamps, the moon) be the only light in shot,
/// which is the entire look; a lifted floor is contrast subtracted uniformly from
/// every pixel, and no amount of key light adds it back.
///
/// Held a whisker above the reference's own reading, and blue-weighted rather
/// than neutral, on purpose: a literal `0` floor would flatten the sky gradient
/// and the fog's far end into a single dead black, and the moon would hang in a
/// void with no atmosphere around it to sit in.
pub const SKY: [f32; 3] = [0.0009, 0.0012, 0.0021];

/// How solid the ghost car is, `0` invisible … `1` opaque.
///
/// Tuned against the rendered frame rather than derived: the 3D pipeline does
/// not back-face cull, so every translucent box blends twice over itself, and
/// the car's parts overlap on top of that. The value that *looks* like a third
/// opaque is well under a third.
pub const GHOST_OPACITY: f32 = 1.0;

/// The sky directly overhead, and the top of the frame's gradient.
///
/// **Darker than [`SKY`], not brighter.** That is the way a real night sky sits:
/// the deepest part is overhead, and the band just above the ground is the
/// lightest, because that is where the atmosphere is thickest and scatters the
/// most. Getting this the wrong way round is what makes a night sky read as an
/// overcast day. [`SKY`] stays the *horizon* colour precisely because it is also
/// the colour the depth fog fades into — so the far road dissolves into the sky
/// it is standing under, with no seam between the two.
///
/// Lowered with [`SKY`] and by the same reasoning, keeping the ratio between the
/// two — the zenith stays roughly a third of the horizon's radiance, so the
/// gradient still falls off upward instead of inverting.
pub const SKY_ZENITH: [f32; 3] = [0.0003, 0.0006, 0.0012];

/// The moon's disc colour — **deliberately far above `1.0`**.
///
/// Every other colour in this file is a reflectance and belongs in `0..1`. This
/// one is a radiance: it is the brightest thing in the frame by a wide margin,
/// and authoring it at white would make it a flat white circle. The surplus over
/// white is what the frame's bloom spends on the halo around it, which is what
/// makes it read as a light source rather than a sticker. Cool, like the key
/// light it is the source of, and brightest in blue.
///
/// How far above white barely matters, and that is worth knowing before tuning
/// it: the render target is 8-bit, so every value at or above `1.0` is already
/// clamped to white before the bloom's bright pass samples it. What decides how
/// much the moon glows is therefore the *area* of above-threshold pixels — the
/// disc plus its halo — not this number. Reach for `MOON_HALO_FALLOFF` when the
/// glow is wrong; this only has to clear `1.0` to say "radiance, not paint".
pub const MOON: [f32; 3] = [1.25, 1.32, 1.5];

/// The tarmac's own colour — the largest surface in any frame, and therefore the
/// one that decides the **colour temperature of the whole shot**.
///
/// It was `[0.085, 0.088, 0.105]`: a deliberate blue tilt, blue a quarter above
/// red. That reads as a reasonable choice in isolation and is wrong in context,
/// because it is the *third* blue-weighted term stacked on the same pixels. The
/// hemisphere ambient's sky colour is blue by 1.9× red, the depth fog fades into
/// a [`SKY`] blue by 2.3×, and the moon and its key light are cool by authorship.
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
    RoadMaterials {
        // Asphalt: dark, and neutral-warm — see [`TARMAC`] for why the hue is
        // the road's single most consequential number and why it is no longer
        // blue. It still separates from the verge, which is green and half a
        // stop brighter. The grain multiplies this base colour (the shader
        // computes `albedo * colour`), so the hue lives here and only the
        // shading is sampled.
        // `roughness` is what decides how much of the moon the tarmac throws
        // back. It is not decoration on a night stage: a matte road reflects the
        // moon nowhere, so the brightest object in the sky leaves no mark on the
        // largest surface in the frame, and the two read as unrelated. At 0.6 the
        // road catches a broad, low sheen down the line to the moon — the look of
        // asphalt that is damp rather than polished, which is also the only way a
        // near-black surface gets any tonal variation across its length at all.
        //
        // 0.68 rather than lower because the streak is brightest in the near
        // corner, where the reflection geometry is most favourable: any glossier
        // and that corner saturates to flat white and the sheen stops reading as
        // a surface and starts reading as a blown highlight.
        surface: app.add_material(
            asphalt
                .map(|t| {
                    Material::lit(rgb(TARMAC[0], TARMAC[1], TARMAC[2]))
                        .with_custom_texture(t.id())
                        .with_roughness(ratio(0.68))
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
                    Material::lit(rgb(TARMAC[0], TARMAC[1], TARMAC[2])).with_roughness(ratio(0.68))
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
        // The verge: a neutral base, since one mesh spans zone boundaries.
        verge: app.add_material(Material::lit(rgb(0.115, 0.145, 0.105))),
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
    /// Tyre smoke.
    pub smoke: Handle<Material>,
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
            foliage: lit(app, [0.13, 0.27, 0.15]),
            stone: lit(app, [0.28, 0.26, 0.24]),
            sign: glowing(app, [0.62, 0.64, 0.60], [0.22, 0.22, 0.20]),
            lamp: glowing(app, [0.30, 0.28, 0.24], [1.0, 0.86, 0.52]),
            building: lit(app, [0.22, 0.22, 0.25]),
            // Automotive clear-coat — the glossiest surface in the frame, and
            // the one always closest to the camera. Without it the car is a flat
            // orange cut-out against a lit road; with it the moon rides along its
            // upper edges and the body finally reads as a curved metal shell.
            car_body: glossy(app, [0.86, 0.16, 0.07], 0.30),
            // Glass is glossier still, and nearly black in albedo — so almost
            // everything it shows is reflection, which is exactly what a
            // windscreen at night looks like.
            car_glass: glossy(app, [0.07, 0.09, 0.13], 0.12),
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
            // Dark, because it is opaque: bright smoke would punch a light
            // grey hole in the road behind the car.
            smoke: lit(app, [0.30, 0.30, 0.33]),
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
            p.smoke,
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
        // The sky is authored in linear light; after the backend's sRGB
        // conversion this lands around 0.13 on screen, which is the night it
        // looks like rather than the black it reads as here.
        assert!(luminance(SKY) < 0.03, "and the sky is dark enough to see them against");
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

    /// The tarmac carries the aggregate grain, and **nothing else does**.
    ///
    /// Both halves matter. Without a texture the largest surface in the frame is
    /// a flat fill that renders identically at eight metres and at sixty. With
    /// the texture on the *paint*, the lane markings — the app's whole speed cue,
    /// and the brightest thing on the road — would come out mottled instead of
    /// solid white. `material_textures` is the same resolution the backends read,
    /// so this asserts what actually reaches the GPU rather than what was
    /// authored.
    #[test]
    fn only_the_tarmac_carries_the_asphalt_grain() {
        use super::super::asphalt_texture::RES;

        let mut app = app();
        let m = road_materials(&mut app);
        let textures = app.material_textures();
        let of = |h: Handle<Material>| {
            textures
                .iter()
                .find(|t| t.material_id() == h.id())
                .expect("every road material resolves a texture entry")
        };

        let tarmac = of(m.surface);
        let (w, h, pixels) = (tarmac.width(), tarmac.height(), tarmac.pixels().to_vec());
        assert_eq!((w, h), (RES, RES), "the tarmac samples the authored grain");
        assert_eq!(pixels.len(), (RES * RES * 4) as usize);
        assert!(
            pixels.chunks(4).map(|t| t[0]).collect::<Vec<_>>().windows(2).any(|p| p[0] != p[1]),
            "a grain that is one flat value is not a texture"
        );

        // The 1x1 opaque-white fallback: an untextured material, unchanged.
        for other in [m.paint, m.rail, m.verge] {
            let entry = of(other);
            assert_eq!(
                (entry.width(), entry.height(), entry.pixels()),
                (1, 1, [255, 255, 255, 255].as_slice()),
                "only the tarmac is textured"
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
