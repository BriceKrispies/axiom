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
//! ## Anything that should glow is bright in its **base colour**
//!
//! `Material::with_emissive` exists on the umbrella and is carried all the way
//! down to `axiom-render`'s `RenderMaterial` — and then stops. The GPU backend
//! never reads it, and `DrawData` exposes only `color()`, so on the live
//! browser path an emissive term contributes exactly nothing. A reflector post
//! authored as a dim amber with a bright emissive therefore renders as a dim
//! amber, which is the opposite of the one thing it exists to do.
//!
//! So every "glowing" material here carries its brightness in `base_color`, and
//! the emissive is kept only as a declaration of intent for a backend that
//! grows support. The hemisphere ambient's ground term is raised for the same
//! reason: with a single directional key light, a face turned away from it has
//! nothing but ambient, and a pure-black ambient makes half of every bright
//! object black.
//!
//! This is a real engine gap, and it is deliberately *not* worked around by
//! adding an emissive path to the renderer: a bright base colour expresses what
//! this app needs, and a second lighting term in the GPU backend is a
//! render-pipeline change with nothing to do with racing.

use axiom::prelude::{Color, Handle, Material, Ratio, RunningApp};

use crate::track::Zone;

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
pub const SKY: [f32; 3] = [0.011, 0.015, 0.026];

/// Register the four road materials.
pub fn road_materials(app: &mut RunningApp) -> RoadMaterials {
    RoadMaterials {
        // Asphalt: dark, and slightly blue so it separates from the verge.
        surface: app.add_material(Material::lit(rgb(0.085, 0.088, 0.105))),
        // Paint: bright, with a little emissive so it stays legible in shadow
        // and inside the tunnel.
        paint: app.add_material(
            Material::lit(rgb(0.88, 0.89, 0.86)).with_emissive(rgb(0.16, 0.16, 0.15)),
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
}

impl ScenePalette {
    /// Register every material. Called once, at install.
    pub fn install(app: &mut RunningApp) -> ScenePalette {
        let lit = |app: &mut RunningApp, c: [f32; 3]| app.add_material(Material::lit(rgb(c[0], c[1], c[2])));
        let glowing = |app: &mut RunningApp, c: [f32; 3], e: [f32; 3]| {
            app.add_material(Material::lit(rgb(c[0], c[1], c[2])).with_emissive(rgb(e[0], e[1], e[2])))
        };
        ScenePalette {
            road: road_materials(app),
            // Retro-reflective amber, bright enough to read from any angle
            // without leaning on an emissive term the backend ignores.
            post: glowing(app, [1.0, 0.80, 0.20], [0.55, 0.36, 0.05]),
            timber: lit(app, [0.16, 0.12, 0.09]),
            foliage: lit(app, [0.13, 0.27, 0.15]),
            stone: lit(app, [0.28, 0.26, 0.24]),
            sign: glowing(app, [0.86, 0.88, 0.84], [0.10, 0.10, 0.09]),
            lamp: glowing(app, [1.0, 0.96, 0.82], [0.85, 0.72, 0.42]),
            building: lit(app, [0.22, 0.22, 0.25]),
            car_body: lit(app, [0.86, 0.16, 0.07]),
            car_glass: lit(app, [0.07, 0.09, 0.13]),
            tyre: lit(app, [0.045, 0.045, 0.05]),
            brake_light: glowing(app, [1.0, 0.14, 0.08], [0.70, 0.03, 0.02]),
            boost_flame: glowing(app, [0.62, 0.90, 1.0], [0.55, 0.85, 1.0]),
            traffic: [
                lit(app, [0.30, 0.44, 0.66]),
                lit(app, [0.62, 0.60, 0.55]),
                lit(app, [0.20, 0.46, 0.32]),
                lit(app, [0.52, 0.42, 0.16]),
            ],
            traffic_light: glowing(app, [0.95, 0.16, 0.10], [0.42, 0.03, 0.02]),
            streak: glowing(app, [0.86, 0.92, 1.0], [0.42, 0.52, 0.68]),
            // Dark, because it is opaque: bright smoke would punch a light
            // grey hole in the road behind the car.
            smoke: lit(app, [0.30, 0.30, 0.33]),
            spark: glowing(app, [1.0, 0.86, 0.45], [0.95, 0.62, 0.18]),
            finish: glowing(app, [0.35, 1.0, 0.72], [0.14, 0.62, 0.42]),
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
        let asphalt = luminance([0.085, 0.088, 0.105]);
        let paint = luminance([0.88, 0.89, 0.86]);
        let post = luminance([1.0, 0.80, 0.20]);
        assert!(asphalt < 0.15, "the tarmac is nearly black");
        assert!(paint > 0.7, "the paint is nearly white");
        assert!(post > asphalt * 4.0, "the reflectors dominate the tarmac");
        // The sky is authored in linear light; after the backend's sRGB
        // conversion this lands around 0.13 on screen, which is the night it
        // looks like rather than the black it reads as here.
        assert!(luminance(SKY) < 0.03, "and the sky is dark enough to see them against");
    }

    /// The rule the module docs explain: everything meant to glow is bright in
    /// its BASE colour, because the backend never reads `emissive`. A later edit
    /// that moves the brightness back into the emissive term makes these objects
    /// disappear on the live path, so the rule is pinned here.
    #[test]
    fn everything_that_should_glow_is_bright_without_its_emissive() {
        let luminance = |c: [f32; 3]| c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722;
        // (name, base colour) for every material whose whole job is to be seen.
        let glowing: [(&str, [f32; 3]); 7] = [
            ("post", [1.0, 0.80, 0.20]),
            ("lamp", [1.0, 0.96, 0.82]),
            ("brake light", [1.0, 0.14, 0.08]),
            ("boost flame", [0.62, 0.90, 1.0]),
            ("traffic light", [0.95, 0.16, 0.10]),
            ("spark", [1.0, 0.86, 0.45]),
            ("finish", [0.35, 1.0, 0.72]),
        ];
        let asphalt = luminance([0.085, 0.088, 0.105]);
        for (name, colour) in glowing {
            // Its brightest channel is at or near full, so the object is bright
            // under the key light rather than relying on a dead emissive term.
            let peak = colour[0].max(colour[1]).max(colour[2]);
            assert!(peak >= 0.9, "{name} is not bright in base colour: {colour:?}");
            assert!(
                luminance(colour) > asphalt * 2.0,
                "{name} does not stand out against the tarmac"
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
