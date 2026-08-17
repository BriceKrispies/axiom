//! **The diagnostic levers**, and what each one actually removes from a frame.
//!
//! This app is ~30 fps on a real phone at a fixed resolution and nobody knows
//! why. "The phone is slow" is not a finding; it is a symptom. The only way to
//! turn one into the other is a controlled A/B — change exactly one input, hold
//! everything else, read the panel — and the only way an A/B gets *run* on a
//! phone is if it is a button under the canvas rather than a query string
//! somebody has to type into a mobile address bar.
//!
//! So this module is the lever set, as data: what each lever is, what the
//! shipping value of it is, and — crucially — **which of them can move without a
//! reload**. Everything here is browser-free and therefore driven by the native
//! tests at the bottom; `crate::web` owns the wasm exports the page's buttons
//! call, and `web/index.html` owns the buttons.
//!
//! ## The levers, and the mechanism each one uses
//!
//! | Lever | What it removes | Reload? |
//! |---|---|---|
//! | captions | 12 of the frame's 25 draws | no |
//! | shadows | the shadow draws and the PCF result | no |
//! | surfaces | the generated program on *n* draws | no |
//! | solo | every body but one, at a fixed framing | no |
//! | half res | 3/4 of the fragments | no |
//! | adaptive | nothing — it *adds* a controller | no |
//! | force | nothing — it *adds* the frames the idle loop skips | no |
//! | device pixels | the backbuffer↔screen match | **yes** |
//!
//! ### Why there is a lever whose whole job is to make the app do more work
//!
//! `force` is the odd one out: every other lever removes something so a difference
//! between two readings can be attributed to it, and this one removes nothing at
//! all. It exists because [`crate::redraw`] stopped the app redrawing an image that
//! had not changed — which is the correct behaviour and also, on a still camera
//! looking at a static station, exactly the configuration in which the panel has no
//! frames to measure. The steady-state cost of a station is a real question, and an
//! instrument that can no longer answer it has been made worse, not better.
//!
//! So the loop can be held open on demand. `FORCE` changes no pixel and no draw —
//! `tests::force_changes_how_often_a_frame_is_drawn_and_nothing_about_it` pins
//! that — it changes only *how often* the identical frame is submitted, which is
//! precisely what a per-frame cost is measured over.
//!
//! ### Why "shadows off" is two mechanisms and not one
//!
//! There is no app-reachable switch that stops the shadow pass. What there is:
//!
//! 1. **The packet's `light_view_proj`.** `shadow_cull::light_volume` builds the
//!    light's frustum from it and keeps only the batches that cast into it. Hand
//!    it the identity and that frustum is the world cube `[-1, 1]³`, so every
//!    body in this scene — the nearest stands 1.9 units out — is culled and the
//!    shadow pass submits **no draws**. The same identity puts every fragment's
//!    shadow lookup outside the map, where `shadow_factor` returns a hard 1.0.
//! 2. **The capability profile.** `GpuBackendApi::set_capability_profile` clears
//!    `RenderCapability::Shadows`, which zeroes the `CAP_SHADOWS` bit the
//!    fragment stage tests before it *uses* the PCF result.
//!
//! Both are app-side and both move at runtime. What neither can do is stop
//! `begin_render_pass("axiom-shadow-pass")` and its full-size depth clear, which
//! is unconditional, or stop the 25 `textureSampleCompare` taps themselves —
//! the shader selects on the result of `shadow_factor`, and `select` evaluates
//! both arms. Those two are engine changes; they are named in the app's README
//! and in [`crate::export`]'s output rather than guessed at here.
//!
//! ### Why "solo" also moves the camera
//!
//! Isolating a station is only a *measurement* if the isolated frames are
//! comparable to each other. A station's cost is per-pixel, so a body that
//! happens to stand at the edge of the shot and one that fills it are not two
//! readings of the same experiment. [`solo_camera`] therefore frames every solo
//! from the same offset relative to its own body: same standoff, same lens, same
//! screen coverage, one body at a time. The only thing that differs between two
//! solo runs is *which shader is on the pixels*, which is the question.

use axiom::prelude::*;

use crate::label::COUNT as CAPTION_COUNT;
use crate::layout::slot_position;

/// How many surfaces the crucible authors — the ceiling of the `surfaces`
/// lever. Pinned against `stations::all_surfaces()` by
/// `tests::the_surface_ceiling_is_the_authored_set`.
pub const SURFACE_COUNT: usize = 11;

/// How many bodies stand on the stand — the range of the `solo` lever.
pub const BODY_COUNT: usize = CAPTION_COUNT;

/// **Which authored surface each body wears**, in slot order.
///
/// Body 3 is the deliberate exception: it is station 2's graph *baked* into an
/// ordinary texture, so it carries no surface program at all. `crate::export`'s
/// `tests::the_body_to_surface_map_is_the_frames_own` checks the whole table
/// against the `surface_program` the scene really puts on each draw, so a body
/// added or reordered fails a test rather than mislabelling a cost.
pub const BODY_SURFACE: [Option<usize>; BODY_COUNT] = [
    Some(0),
    Some(1),
    None,
    Some(2),
    Some(3),
    Some(4),
    Some(5),
    Some(6),
    Some(7),
    Some(8),
    Some(9),
    Some(10),
];

/// The `surfaces` lever's stops, in cycle order. `None` is the whole authored
/// set; the rest present a prefix of it, so a draw whose surface is missing
/// takes the constant fallback and every other thing about the frame is
/// unchanged.
const SURFACE_STOPS: [Option<usize>; 4] = [None, Some(0), Some(3), Some(6)];

/// **How far the solo camera stands off its body**, in world units.
///
/// The bodies are unit primitives scaled to 0.85..1.15, so a half-extent of
/// about 0.53. At the scene's 58° vertical field of view the frame's half-height
/// at distance `d` is `0.554 · d`, so this standoff puts a body across ~76% of
/// the canvas's height. Large enough that the fragment stage is doing real work
/// — the thing being measured — and small enough that the near plane (0.1) is
/// nowhere near the surface.
const SOLO_STANDOFF: f32 = 1.25;

/// **Every diagnostic lever, and its current position.**
///
/// [`Levers::SHIPPING`] is the configuration the app actually ships in, and the
/// one the page's `RESET` button returns to. A field's shipping value is its
/// value there and nowhere else, so "is anything switched on?" is one equality.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Levers {
    /// Whether the twelve caption meshes are drawn. 12 of the frame's 25 draws.
    pub captions: bool,
    /// Whether the frame carries a real light-space projection and the backend
    /// keeps its `Shadows` capability.
    pub shadows: bool,
    /// How many authored surfaces are handed to the present — `None` is all of
    /// them.
    pub surfaces: Option<usize>,
    /// Which single body is drawn, if any, framed by [`solo_camera`].
    pub solo: Option<usize>,
    /// Whether the backend renders at the render-scale ladder's floor (0.50
    /// linear, a quarter of the fragments) instead of full.
    pub half_res: bool,
    /// Whether the adaptive render-scale controller is driving the resolution.
    pub adaptive: bool,
    /// **Whether the frame loop is held open**, redrawing the identical frame
    /// every callback instead of idling once nothing changes.
    ///
    /// The one lever that adds work rather than removing it, and the one the panel
    /// needs pulled to measure a steady-state frame cost on a configuration that
    /// would otherwise stop drawing. See [`crate::redraw`] for the rule it
    /// overrides and the module docs above for why it exists.
    pub force: bool,
    /// Whether the backbuffer matches the device's own pixels (the shipping
    /// behaviour) or is pinned at the authored 1280x640. **A reload lever**: the
    /// backbuffer is fixed when the surface is configured.
    pub device_pixels: bool,
    /// An explicitly pinned backbuffer, from `?back=WxH`. The other reload
    /// lever, and the only way to ask for a size neither the device nor the
    /// authoring resolution offers.
    pub back: Option<(u32, u32)>,
}

impl Levers {
    /// **The shipping configuration.** Everything on, nothing forced, adaptive
    /// off — see `crate::web::drive` for why an instrument does not adapt.
    pub const SHIPPING: Levers = Levers {
        captions: true,
        shadows: true,
        surfaces: None,
        solo: None,
        half_res: false,
        adaptive: false,
        // The app ships idling: it draws when the image would change and not
        // otherwise. Holding the loop open is something you ask for to take a
        // measurement, never something the app does to you.
        force: false,
        device_pixels: true,
        back: None,
    };

    /// The levers a page load asked for, read from a `location.search` string
    /// (with or without its leading `?`).
    ///
    /// The query form is kept because it is how a *link* carries a
    /// configuration — a run someone else should reproduce is a URL, not a
    /// sequence of taps. The buttons and the query string set the same fields.
    pub fn from_query(query: &str) -> Levers {
        let value = |key: &str| {
            query
                .trim_start_matches('?')
                .split('&')
                .filter_map(|pair| pair.split_once('='))
                .find(|(name, _)| *name == key)
                .map(|(_, value)| value.to_string())
        };
        let flag = |key: &str, shipped: bool| {
            value(key)
                .map(|raw| raw != "0")
                .unwrap_or(shipped)
        };
        Levers {
            captions: flag("captions", true),
            shadows: flag("shadows", true),
            surfaces: value("surfaces").and_then(|raw| raw.parse().ok()),
            solo: value("solo")
                .and_then(|raw| raw.parse::<usize>().ok())
                .filter(|body| *body >= 1 && *body <= BODY_COUNT)
                .map(|body| body - 1),
            half_res: value("half").as_deref() == Some("1"),
            adaptive: value("adapt").as_deref() == Some("1"),
            force: value("force").as_deref() == Some("1"),
            device_pixels: value("dpr").as_deref() != Some("0"),
            back: value("back").and_then(|raw| {
                raw.split_once('x')
                    .and_then(|(w, h)| w.parse::<u32>().ok().zip(h.parse::<u32>().ok()))
            }),
        }
    }

    /// Whether every lever is where the app ships it.
    pub fn is_shipping(&self) -> bool {
        *self == Levers::SHIPPING
    }

    /// **Whether returning to the shipping configuration needs a page reload.**
    ///
    /// The backbuffer is decided once, when the surface is configured, so the
    /// two levers that move it cannot move at runtime. The page's `RESET` reads
    /// this and reloads itself when it is true, which is why nobody has to type
    /// a URL to get back to the shipping configuration.
    pub fn reload_required(&self) -> bool {
        !self.device_pixels | self.back.is_some()
    }

    /// The `surfaces` lever's next stop.
    pub fn cycled_surfaces(self) -> Levers {
        let position = SURFACE_STOPS
            .iter()
            .position(|stop| *stop == self.surfaces)
            .unwrap_or(0);
        Levers {
            surfaces: SURFACE_STOPS[(position + 1) % SURFACE_STOPS.len()],
            ..self
        }
    }

    /// The `solo` lever stepped by `delta` through `ALL, body 1 .. body 12`,
    /// wrapping at both ends.
    pub fn stepped_solo(self, delta: i32) -> Levers {
        let states = BODY_COUNT as i32 + 1;
        let current = self.solo.map_or(0, |body| body as i32 + 1);
        let next = (current + delta).rem_euclid(states);
        Levers {
            solo: (next > 0).then(|| (next - 1) as usize),
            ..self
        }
    }

    /// How many surfaces this configuration presents.
    pub fn presented_surfaces(&self) -> usize {
        self.surfaces.unwrap_or(SURFACE_COUNT).min(SURFACE_COUNT)
    }

    /// How the frame's draw list should be cut for these levers.
    pub fn plan(&self) -> PacketPlan {
        PacketPlan {
            captions: self.captions & self.solo.is_none(),
            ground: self.solo.is_none(),
            solo: self.solo,
            shadows: self.shadows,
            surfaces: self.presented_surfaces(),
        }
    }

    /// The label the page prints on the `solo` control.
    pub fn solo_label(&self) -> String {
        self.solo
            .map(|body| format!("BODY {}", body + 1))
            .unwrap_or_else(|| "ALL".to_string())
    }

    /// **The lever state, as the page reads it back.** One JSON object; the page
    /// renders every button's caption and pressed-state from exactly this, so
    /// Rust owns the state and the markup owns nothing but its shape.
    pub fn state_json(&self) -> String {
        format!(
            "{{\"captions\":{},\"shadows\":{},\"surfaces\":{},\"surfaces_total\":{},\
             \"solo\":{},\"solo_label\":\"{}\",\"half_res\":{},\"adaptive\":{},\
             \"force\":{},\"device_pixels\":{},\"back\":{},\"reload_required\":{},\
             \"shipping\":{}}}",
            self.captions,
            self.shadows,
            self.presented_surfaces(),
            SURFACE_COUNT,
            self.solo
                .map(|body| (body + 1).to_string())
                .unwrap_or_else(|| "null".to_string()),
            self.solo_label(),
            self.half_res,
            self.adaptive,
            self.force,
            self.device_pixels,
            self.back
                .map(|(w, h)| format!("\"{w}x{h}\""))
                .unwrap_or_else(|| "null".to_string()),
            self.reload_required(),
            self.is_shipping(),
        )
    }
}

/// **How a frame's draw list is cut.** The value [`crate::frame::packet_of_plan`]
/// takes, and the only thing that stands between the scene's draw list and the
/// packet the backend receives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PacketPlan {
    /// Whether the trailing caption draws survive.
    pub captions: bool,
    /// Whether the ground plane survives.
    pub ground: bool,
    /// The one body that survives, if the frame is isolating one.
    pub solo: Option<usize>,
    /// Whether the packet carries the scene's real light projection.
    pub shadows: bool,
    /// **How many bodies keep their generated program.** A body past this takes
    /// the constant fallback pipeline instead — see [`PacketPlan::program_of`].
    pub surfaces: usize,
}

impl PacketPlan {
    /// The whole frame, exactly as the app ships it.
    pub const EVERYTHING: PacketPlan = PacketPlan {
        captions: true,
        ground: true,
        solo: None,
        shadows: true,
        surfaces: SURFACE_COUNT,
    };

    /// Whether draw `index` of a `total`-draw frame survives this plan.
    ///
    /// The scene's draw list is, in order: the ground, one draw per body, then
    /// one caption per body. `crate::scene`'s tests pin that partition against
    /// the registration order, so the arithmetic below is checked rather than
    /// assumed.
    pub fn keeps(&self, index: usize, total: usize) -> bool {
        let bodies = total.saturating_sub(CAPTION_COUNT);
        let is_ground = index == 0;
        let is_caption = index >= bodies;
        let body = index.saturating_sub(1);
        self.solo
            .map(|only| !is_ground & !is_caption & (body == only))
            .unwrap_or(
                (is_ground & self.ground)
                    | (is_caption & self.captions)
                    | (!is_ground & !is_caption),
            )
    }

    /// **The `surface_program` draw `index` should carry.**
    ///
    /// A body whose surface sits past [`Self::surfaces`] is handed `0`, which is
    /// the constant fallback pipeline — so the frame draws the same geometry, at
    /// the same resolution, with the same batching, and the *only* difference is
    /// how many draws run a generated shader.
    ///
    /// This is a correction, and the correction is itself the finding. The lever
    /// used to work by narrowing the `Surface` slice handed to
    /// `present_packet_with_surfaces`, on the belief that a draw whose surface
    /// was missing would take the fallback. It does not: the startup barrier has
    /// already bound all eleven programs to the device, and a draw finds its
    /// program by digest from that cache whatever slice the present is given.
    /// Withholding surfaces changed no pixel and no measurement — verified on
    /// screen, with every body still wearing its own shader at `SURFACES 0/11`.
    /// The lane the backend really keys on is the draw's own, so that is the one
    /// this cuts.
    pub fn program_of(&self, index: usize, total: usize, authored: u64) -> u64 {
        let bodies = total.saturating_sub(CAPTION_COUNT);
        let body = index.saturating_sub(1);
        let surfaced = (index >= 1)
            & (index < bodies)
            & BODY_SURFACE
                .get(body)
                .copied()
                .flatten()
                .is_some_and(|surface| surface < self.surfaces);
        authored * u64::from(surfaced)
    }
}

/// **The camera that frames a solo'd body**, identically for every body.
///
/// Straight in front of the body at [`SOLO_STANDOFF`], level, looking at its
/// centre. The offset is the *same* for all twelve, so two solo runs differ in
/// which shader is on the pixels and in nothing else — which is the entire point
/// of isolating one.
pub fn solo_camera(body: usize) -> Transform {
    let center = slot_position(body.min(BODY_COUNT - 1));
    let eye = center.add(Vec3::new(0.0, 0.0, SOLO_STANDOFF));
    Transform::from_translation(eye)
        .looking_at(center, Vec3::UNIT_Y)
        // Unreachable for an authored offset straight down `-z`; a live page
        // keeps the un-rotated eye rather than panicking if it ever were.
        .unwrap_or_else(|_| Transform::from_translation(eye))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The lever ceiling is the authored surface set, not a number that can
    /// drift away from it.
    #[test]
    fn the_surface_ceiling_is_the_authored_set() {
        assert_eq!(SURFACE_COUNT, crate::stations::all_surfaces().len());
        assert_eq!(BODY_COUNT, crate::layout::SLOT_COUNT);
    }

    /// An untouched page load is the shipping configuration — the buttons start
    /// where the app ships, and `RESET` has somewhere to return to.
    #[test]
    fn an_empty_query_is_the_shipping_configuration() {
        assert_eq!(Levers::from_query(""), Levers::SHIPPING);
        assert_eq!(Levers::from_query("?"), Levers::SHIPPING);
        assert!(Levers::SHIPPING.is_shipping());
        assert!(!Levers::SHIPPING.adaptive, "an instrument does not adapt");
    }

    /// Every lever is reachable from a URL, so a run can be handed to somebody
    /// else as a link rather than as a sequence of taps.
    #[test]
    fn every_lever_is_reachable_from_the_query_string() {
        let levers = Levers::from_query(
            "?captions=0&shadows=0&surfaces=3&solo=5&half=1&adapt=1&force=1&dpr=0&back=640x320",
        );
        assert_eq!(
            levers,
            Levers {
                captions: false,
                shadows: false,
                surfaces: Some(3),
                solo: Some(4),
                half_res: true,
                adaptive: true,
                force: true,
                device_pixels: false,
                back: Some((640, 320)),
            }
        );
        assert!(!levers.is_shipping());
        assert!(levers.reload_required());
    }

    /// **The two backbuffer levers are the only ones that need a reload**, and
    /// the page knows which they are without being told twice.
    #[test]
    fn only_the_backbuffer_levers_require_a_reload() {
        assert!(!Levers::SHIPPING.reload_required());
        assert!(Levers::from_query("?dpr=0").reload_required());
        assert!(Levers::from_query("?back=640x320").reload_required());
        assert!(
            !Levers::from_query("?captions=0&solo=1&half=1&adapt=1&force=1&surfaces=0")
                .reload_required()
        );
        assert_eq!(Levers::from_query("?back=nonsense").back, None);
    }

    /// A `solo` outside the stand is ignored rather than isolating a body that
    /// does not exist.
    #[test]
    fn an_out_of_range_solo_is_ignored() {
        assert_eq!(Levers::from_query("?solo=0").solo, None);
        assert_eq!(Levers::from_query("?solo=13").solo, None);
        assert_eq!(Levers::from_query("?solo=x").solo, None);
        assert_eq!(Levers::from_query("?solo=12").solo, Some(11));
    }

    /// The surfaces cycle visits every stop and returns to "all".
    #[test]
    fn the_surface_cycle_returns_to_the_whole_set() {
        let mut levers = Levers::SHIPPING;
        let stops: Vec<usize> = (0..SURFACE_STOPS.len())
            .map(|_| {
                levers = levers.cycled_surfaces();
                levers.presented_surfaces()
            })
            .collect();
        assert_eq!(stops, vec![0, 3, 6, SURFACE_COUNT]);
        assert_eq!(levers, Levers::SHIPPING);
    }

    /// The solo control walks the whole stand in both directions and wraps.
    #[test]
    fn the_solo_control_walks_the_stand_and_wraps() {
        let mut levers = Levers::SHIPPING;
        levers = levers.stepped_solo(1);
        assert_eq!(levers.solo, Some(0));
        assert_eq!(levers.solo_label(), "BODY 1");
        (0..BODY_COUNT).for_each(|_| levers = levers.stepped_solo(1));
        assert_eq!(levers.solo, None, "a full walk returns to ALL");
        assert_eq!(levers.stepped_solo(-1).solo, Some(BODY_COUNT - 1));
    }

    /// **The captions lever removes exactly the captions.** 12 of 25 draws, and
    /// nothing else moves.
    #[test]
    fn the_captions_lever_removes_exactly_the_caption_draws() {
        let plan = Levers {
            captions: false,
            ..Levers::SHIPPING
        }
        .plan();
        let kept: Vec<usize> = (0..25).filter(|index| plan.keeps(*index, 25)).collect();
        assert_eq!(kept.len(), 13);
        assert_eq!(kept, (0..13).collect::<Vec<usize>>());
    }

    /// **Solo keeps one draw.** No ground, no caption, one body — so the frame's
    /// cost is one body's cost and there is nothing else in it to attribute to.
    #[test]
    fn solo_keeps_exactly_one_body_and_nothing_else() {
        (0..BODY_COUNT).for_each(|body| {
            let plan = Levers {
                solo: Some(body),
                ..Levers::SHIPPING
            }
            .plan();
            let kept: Vec<usize> = (0..25).filter(|index| plan.keeps(*index, 25)).collect();
            assert_eq!(kept, vec![body + 1], "solo {body} kept the wrong draws");
        });
    }

    /// The whole frame survives the shipping plan — the lever set is off by
    /// default and cuts nothing.
    #[test]
    fn the_shipping_plan_keeps_every_draw_and_every_program() {
        assert_eq!(Levers::SHIPPING.plan(), PacketPlan::EVERYTHING);
        assert!((0..25).all(|index| PacketPlan::EVERYTHING.keeps(index, 25)));
        // Every authored program survives untouched, and the ground and the
        // captions keep the `0` they were authored with.
        (0..25).for_each(|index| {
            assert_eq!(PacketPlan::EVERYTHING.program_of(index, 25, 77), 77 * u64::from((1..13).contains(&index) & (index != 3)));
        });
    }

    /// **The surfaces lever cuts the program lane, body by body.**
    ///
    /// `SURFACES 3` keeps a generated shader on the three bodies wearing the
    /// first three authored surfaces and hands every other body the constant
    /// fallback. The lever used to narrow the `Surface` slice given to the
    /// present instead, which changed nothing at all — the barrier had already
    /// bound every program to the device.
    #[test]
    fn the_surfaces_lever_cuts_the_program_lane_body_by_body() {
        let plan = Levers {
            surfaces: Some(3),
            ..Levers::SHIPPING
        }
        .plan();
        let surfaced: Vec<usize> = (0..25)
            .filter(|index| plan.program_of(*index, 25, 99) != 0)
            .collect();
        // Bodies 1, 2 and 4 wear authored surfaces 0, 1 and 2; body 3 is the
        // baked tile and wears none.
        assert_eq!(surfaced, vec![1, 2, 4]);
        // None at all is a frame of pure fallback pipelines.
        let none = Levers {
            surfaces: Some(0),
            ..Levers::SHIPPING
        }
        .plan();
        assert!((0..25).all(|index| none.program_of(index, 25, 99) == 0));
    }

    /// **Every solo frames its body identically.** The eye is the same offset
    /// from every body's centre, which is what makes two solo readings
    /// comparable: same screen coverage, different shader.
    #[test]
    fn every_solo_frames_its_body_from_the_same_offset() {
        (0..BODY_COUNT).for_each(|body| {
            let camera = solo_camera(body);
            let center = slot_position(body);
            let offset = camera.translation.subtract(center);
            assert!((offset.x).abs() < 1.0e-6, "{offset:?}");
            assert!((offset.y).abs() < 1.0e-6, "{offset:?}");
            assert!((offset.z - SOLO_STANDOFF).abs() < 1.0e-6, "{offset:?}");
            // Level and square on, exactly like the authored shot: the solo
            // framing must not introduce a rotation that changes coverage.
            let r = camera.rotation;
            assert!(r.x.abs() + r.y.abs() + r.z.abs() < 1.0e-4, "{r:?}");
        });
        // An out-of-range body is clamped onto the stand rather than framing a
        // slot that does not exist.
        assert_eq!(solo_camera(99).translation, solo_camera(BODY_COUNT - 1).translation);
    }

    /// **The solo standoff really does fill the frame.** The measurement is only
    /// worth taking if the isolated body is what the fragment stage is spending
    /// its time on, so this pins the coverage rather than trusting the constant.
    #[test]
    fn a_solo_body_covers_most_of_the_frames_height() {
        let half_height =
            SOLO_STANDOFF * (crate::scene::CAMERA_FOV_DEGREES.to_radians() * 0.5).tan();
        // The largest body is a unit primitive at scale 1.15, so a half-extent
        // of 0.575.
        let coverage = 0.575 / half_height;
        assert!(coverage > 0.6 && coverage <= 1.0, "coverage {coverage}");
        // ...and the eye is still clear of the surface by an order of magnitude
        // more than the 0.1 near plane.
        assert!(SOLO_STANDOFF - 0.575 > 0.5);
    }

    /// The state the page reads back is legal JSON carrying every lever.
    #[test]
    fn the_state_json_carries_every_lever() {
        let json = Levers::SHIPPING.state_json();
        assert!(json.starts_with('{') && json.ends_with('}'));
        ["captions", "shadows", "surfaces", "solo_label", "half_res", "adaptive",
         "force", "device_pixels", "back", "reload_required", "shipping"]
            .iter()
            .for_each(|key| assert!(json.contains(&format!("\"{key}\":")), "{json}"));
        assert!(json.contains("\"shipping\":true"));
        assert!(Levers {
            captions: false,
            ..Levers::SHIPPING
        }
        .state_json()
        .contains("\"shipping\":false"));
    }

    /// **`FORCE` is not a rendering lever.** It holds the frame loop open so the
    /// panel has frames to measure; it must not change one thing about the frame
    /// that is then measured, or the reading would be of a different app.
    ///
    /// The plan a forced frame is cut with is byte-identical to the plan an idle
    /// one would have been cut with — same draws, same programs, same light
    /// projection — and only [`Levers::is_shipping`] can tell the two apart, which
    /// is how the page knows to light the button up.
    #[test]
    fn force_changes_how_often_a_frame_is_drawn_and_nothing_about_it() {
        let forced = Levers {
            force: true,
            ..Levers::SHIPPING
        };
        assert_eq!(forced.plan(), Levers::SHIPPING.plan());
        assert_eq!(forced.presented_surfaces(), Levers::SHIPPING.presented_surfaces());
        assert!(!forced.is_shipping(), "the page must show FORCE as pulled");
        assert!(!forced.reload_required(), "FORCE moves at runtime");
        assert!(forced.state_json().contains("\"force\":true"));
    }
}
