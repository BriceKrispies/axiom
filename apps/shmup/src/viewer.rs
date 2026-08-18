//! **The parts viewer** — the first pixels the ported weapon geometry has ever
//! produced.
//!
//! `weapons::parts` is 27 builders ported from Claude-of-Duty's `parts.js` and
//! pinned, bucket by bucket, against goldens captured from the original
//! JavaScript. That proves the *numbers* match. It does not prove the geometry
//! looks like a gun — a triangle soup can agree with a golden to 1e-5 and still
//! be inside-out, sub-pixel, or hollow. This module puts four visually
//! distinctive parts on screen so a human can look at them.
//!
//! ## What it draws
//!
//! Four parts, each built through the real builder at its real
//! `buildRifle()`-call-site dimensions, laid out side by side on a slow
//! turntable:
//!
//! | slot | builder | source call site |
//! |------|---------|------------------|
//! | 0 | [`add_upper_receiver`] | `models/rifle.js`, the AR flat-top upper |
//! | 1 | [`build_magazine`]     | the 30-round curved STANAG |
//! | 2 | [`build_optic`]        | the T2-pattern tube red dot |
//! | 3 | [`add_muzzle_device`]  | the compensator |
//!
//! Each builder fills an [`Assembly`], whose `build()` returns one merged
//! [`Geo`] per material bucket. Every bucket becomes one engine mesh
//! ([`MeshData`]) and one node, so what you see on screen is exactly the
//! geometry the golden tests compare — no re-derivation, no simplification.
//!
//! ## Scale, and why the camera is so close
//!
//! The parts are authored in **metres at real scale**: an AR upper receiver is
//! 0.198 m end to end and the magazine body is 0.0255 m across. A camera placed
//! at the engine-demo-usual 8 m would render the whole rack about four pixels
//! wide. The turntable therefore orbits at 0.52 m, and each part is recentred on
//! its own bounding box so the rack composes around the origin rather than
//! around the rifle's bore axis (`y = 0.075`, `z = -0.04`) the builders author
//! against.
//!
//! ## The engine path
//!
//! This is an ordinary [`App`]: `setup` authors the turntable camera and the
//! light rig, `install` registers the built geometry and spawns it, and `run`
//! drives `axiom-windowing`'s presentation loop. `install` exists because the
//! author-geometry registration surface (`RunningApp::add_mesh_data`) lives on
//! the *realized* app — before it, an app with its own geometry had to abandon
//! `App::run` and drive windowing by hand, which is why every author-geometry
//! app in this repository has its own loop.

use std::collections::BTreeMap;

use axiom::prelude::*;

use crate::weapons::geometry::{Assembly, Geo};
use crate::weapons::parts::barrel::{add_muzzle_device, MuzzleKind};
use crate::weapons::parts::magazine::{build_magazine, MagazineOpts};
use crate::weapons::parts::optics::{build_optic, OpticOpts};
use crate::weapons::parts::receiver::{add_upper_receiver, UpperReceiverOpts};

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

/// The canvas the page hands the engine.
const SURFACE_ID: &str = "shmup-parts";

/// Metres between two adjacent parts on the rack. The widest part here (the
/// optic, across its clamp bar) is under 0.05 m, so this leaves daylight between
/// every silhouette even at the quarter-turn where the rack is end-on to the
/// camera and the parts line up in depth.
const SLOT_PITCH: f32 = 0.105;

/// Metres the turntable camera orbits at, and how high it rides. The rack is a
/// line along X, so a camera at rack height would stack all four parts on top of
/// one another twice per revolution, when the line points at the lens. Riding
/// high enough to look down on it (~30 degrees) keeps every part separated in
/// frame at every phase, and the radius then follows from fitting the 0.32 m
/// rack into the 45 degree frame broadside.
const ORBIT_RADIUS: f32 = 0.52;
const ORBIT_HEIGHT: f32 = 0.30;

/// Ticks per turntable revolution. `App::run` advances one tick per presented
/// frame, so at 60 Hz this is a twelve-second turn — slow enough to read a
/// silhouette, fast enough that a screenshot pair differs.
const TURNTABLE_PERIOD: u32 = 720;

/// A linear colour channel from a known-finite authored literal.
pub(crate) fn ch(value: f32) -> Ratio {
    Ratio::new(value).expect("authored colour channel is finite")
}

/// The linear albedo for one of the kit's material bucket names. The buckets are
/// the strings the builders pass to `Assembly::add` — `"alu"`, `"steel"`,
/// `"polymer"`, the optic's glass stack — and this is the viewer's own reading
/// of them, not the game's material system (`materials/` owns that, and it
/// speaks in shader graphs this viewer has no way to bind yet).
pub(crate) fn bucket_color(bucket: &str) -> Color {
    let [r, g, b] = match bucket {
        "alu" => [0.52, 0.53, 0.56],
        "steel" => [0.34, 0.36, 0.40],
        "optic_tube" => [0.17, 0.18, 0.20],
        "lens_ring" => [0.44, 0.45, 0.47],
        "lens_vig" => [0.03, 0.03, 0.035],
        "glass" => [0.14, 0.34, 0.44],
        "polymer" => [0.10, 0.105, 0.115],
        "rubber" => [0.05, 0.05, 0.055],
        "cavity" => [0.02, 0.02, 0.022],
        _ => [0.45, 0.45, 0.47],
    };
    Color::linear_rgb(ch(r), ch(g), ch(b))
}

/// One built part: its merged-per-material buckets and the centre of the
/// bounding box over all of them.
struct Part {
    buckets: BTreeMap<String, Geo>,
    center: Vec3,
}

impl Part {
    /// Build one part by running `build` against a fresh [`Assembly`], then
    /// measure the bounding box the whole part occupies so the viewer can seat
    /// it on the rack rather than at the rifle's bore.
    fn build(name: &str, build: impl FnOnce(&mut Assembly)) -> Part {
        let mut asm = Assembly::new(name);
        build(&mut asm);
        let buckets = asm.build();
        Part {
            center: center_of(&buckets),
            buckets,
        }
    }
}

/// The centre of the axis-aligned bounding box over every bucket's positions.
/// An empty part (impossible here, but the arithmetic has to say something)
/// centres on the origin.
fn center_of(buckets: &BTreeMap<String, Geo>) -> Vec3 {
    let mut lo = [f32::INFINITY; 3];
    let mut hi = [f32::NEG_INFINITY; 3];
    for geo in buckets.values() {
        for p in geo.pos.chunks_exact(3) {
            for axis in 0..3 {
                lo[axis] = lo[axis].min(p[axis]);
                hi[axis] = hi[axis].max(p[axis]);
            }
        }
    }
    let mid = |axis: usize| {
        let m = (lo[axis] + hi[axis]) * 0.5;
        if m.is_finite() {
            m
        } else {
            0.0
        }
    };
    Vec3::new(mid(0), mid(1), mid(2))
}

/// One material bucket as engine-registerable geometry.
///
/// The kit's [`Geo`] is Three.js-shaped — flat `f32` triples plus an optional
/// index — and [`MeshData`] is Axiom-shaped, so this is a pure re-layout. Two
/// details are not cosmetic: a bucket that reached `Assembly::build` without an
/// index is a triangle *soup*, so the identity index is synthesised for it; and
/// `normalize_attributes` fills the UV/normal a merged bucket may be missing,
/// which `MeshData` validation requires one of per vertex.
pub(crate) fn to_mesh_data(geo: &Geo) -> MeshData {
    let mut geo = geo.clone();
    geo.normalize_attributes();
    let positions: Vec<Vec3> = geo
        .pos
        .chunks_exact(3)
        .map(|c| Vec3::new(c[0], c[1], c[2]))
        .collect();
    let normals: Vec<Vec3> = geo
        .normal
        .chunks_exact(3)
        .map(|c| Vec3::new(c[0], c[1], c[2]))
        .collect();
    let uvs: Vec<Vec2> = geo.uv.chunks_exact(2).map(|c| Vec2::new(c[0], c[1])).collect();
    let indices = if geo.index.is_empty() {
        (0..positions.len() as u32).collect()
    } else {
        geo.index.clone()
    };
    MeshData::new(positions, normals, uvs, indices)
}

/// The four parts, in rack order. Each is built at the dimensions its real
/// `buildRifle()` call site uses, so what is on screen is the rifle's own
/// geometry and not a synthetic test case.
fn rack() -> Vec<Part> {
    vec![
        Part::build("upper", |asm| {
            add_upper_receiver(
                asm,
                "alu",
                "steel",
                "cavity",
                UpperReceiverOpts {
                    z_rear: 0.055,
                    z_front: -0.143,
                    bore: 0.075,
                    r: 0.0192,
                    port_z: -0.052,
                    rail_top: 0.1036,
                },
            );
        }),
        Part::build("magazine", |asm| {
            build_magazine(
                asm,
                (),
                MagazineOpts {
                    w: 0.0255,
                    d: 0.0655,
                    len: 0.212,
                    curve: 0.03,
                    segs: 8,
                    witness: 4,
                    poly: "polymer",
                    ..Default::default()
                },
            );
        }),
        Part::build("optic", |asm| {
            build_optic(
                asm,
                OpticOpts {
                    rail_top: -0.02,
                    ..Default::default()
                },
            );
        }),
        Part::build("muzzle", |asm| {
            add_muzzle_device(asm, "steel", "cavity", MuzzleKind::Comp, 0.03, 0.0072, 0.0);
        }),
    ]
}

/// The viewer app: a turntable camera and a light rig authored in `setup`, the
/// ported geometry registered and spawned in `install`.
///
/// The turntable is the *camera* orbiting a static rack, not the rack spinning
/// under a static camera. That is deliberate: a spin has to be authored as a
/// `Spin` component in `setup`, and the part nodes do not exist until `install`
/// has registered their geometry — so the one node that can carry the spin is
/// the camera's parent, and orbiting the camera is what that means.
pub fn parts_viewer_app() -> App {
    App::new()
        .window(
            Window::new(1280, 720)
                .with_surface_id(SURFACE_ID)
                .with_clear_color(Color::linear_rgb(ch(0.016), ch(0.019), ch(0.024))),
        )
        .add_plugins(DefaultPlugins)
        .setup(|world, _meshes, _materials| {
            // The turntable: a spinning root whose child is the camera, aimed at
            // the rack's centre. The parent's rotation carries the camera's
            // orientation with it, so it stays aimed all the way round.
            world
                .spawn((
                    Transform::IDENTITY,
                    Spin::around(Vec3::UNIT_Y).period(TURNTABLE_PERIOD),
                ))
                .with_child((
                    Transform::from_translation(Vec3::new(0.0, ORBIT_HEIGHT, ORBIT_RADIUS))
                        .looking_at(Vec3::ZERO, Vec3::UNIT_Y)
                        .expect("the orbit position is not on top of its target"),
                    Camera::perspective(PerspectiveProjection {
                        fov_y: Angle::degrees(45.0),
                        // Centimetre-scale near plane: the nearest bevel is
                        // ~0.4 m away and the whole rack is 0.3 m deep, so the
                        // engine-usual 0.1/100 range would waste the entire
                        // depth buffer on empty space.
                        near: Meters::new(0.05).expect("authored near plane is finite"),
                        far: Meters::new(20.0).expect("authored far plane is finite"),
                    }),
                ));
            // A key from high front-left and a dimmer fill from the opposite
            // side. Machined aluminium reads almost entirely off its highlight
            // rolloff, so a single light leaves half of every part black.
            world.spawn((
                Transform::IDENTITY,
                DirectionalLight {
                    direction: Vec3::new(-0.45, -0.85, -0.35),
                    color: Color::linear_rgb(ch(1.0), ch(0.97), ch(0.92)),
                    intensity: ch(1.15),
                },
            ));
            world.spawn((
                Transform::from_translation(Vec3::new(0.35, 0.30, -0.45)),
                PointLight {
                    color: Color::linear_rgb(ch(0.55), ch(0.68), ch(1.0)),
                    intensity: ch(0.9),
                },
            ));
            world.spawn((
                Transform::from_translation(Vec3::new(-0.40, 0.18, 0.40)),
                PointLight {
                    color: Color::linear_rgb(ch(1.0), ch(0.80), ch(0.55)),
                    intensity: ch(0.7),
                },
            ));
        })
        .install(|running| install_rack(running, rack()))
}

/// Register every bucket of every part and spawn it in its rack slot. Each
/// bucket is one mesh and one node; the node transform seats the part on the
/// rack (slot offset, minus the part's own bounding-box centre).
fn install_rack(running: &mut RunningApp, parts: Vec<Part>) {
    let count = parts.len() as f32;
    for (slot, part) in parts.into_iter().enumerate() {
        let x = (slot as f32 - (count - 1.0) * 0.5) * SLOT_PITCH;
        let seat = Vec3::new(x - part.center.x, -part.center.y, -part.center.z);
        for (bucket, geo) in &part.buckets {
            let mesh = running
                .add_mesh_data(to_mesh_data(geo))
                .expect("a golden-pinned part bucket is valid renderable geometry");
            let material = running.add_material(Material::lit(bucket_color(bucket)));
            running.spawn(Spawn::new(Transform::from_translation(seat), mesh, material));
        }
    }
}

/// Browser entry: author the rack and drive the engine's presentation loop.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn shmup_parts_start() {
    console_error_panic_hook::set_once();
    parts_viewer_app().run();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_rack_part_builds_non_empty_indexed_geometry() {
        let parts = rack();
        assert_eq!(parts.len(), 4);
        for part in &parts {
            assert!(!part.buckets.is_empty(), "a part built no material bucket");
            for (bucket, geo) in &part.buckets {
                let data = to_mesh_data(geo);
                assert!(!data.positions().is_empty(), "{bucket}: no positions");
                assert_eq!(
                    data.normals().len(),
                    data.positions().len(),
                    "{bucket}: one normal per vertex"
                );
                assert_eq!(data.indices().len() % 3, 0, "{bucket}: a triangle list");
                assert!(
                    data.indices().iter().all(|i| (*i as usize) < data.positions().len()),
                    "{bucket}: every index is in range"
                );
            }
            // Real-scale metres: no part on this rack is bigger than a rifle.
            assert!(part.center.x.abs() < 0.5 && part.center.z.abs() < 0.5);
        }
    }

    #[test]
    fn the_viewer_draws_every_bucket_and_turns() {
        let mut app = parts_viewer_app().build();
        let buckets: usize = rack().iter().map(|p| p.buckets.len()).sum();
        assert_eq!(app.renderable_count(), buckets);
        let early = app.tick(0);
        assert_eq!(early.draws().len(), buckets);
        // Three lights: the key plus two fills.
        assert_eq!(early.lights().len(), 3);
        // The turntable has moved a quarter turn by tick 180, so every draw's
        // MVP differs — the camera is genuinely orbiting.
        let mut later = early.clone();
        for tick in 1..=180 {
            later = app.tick(tick);
        }
        assert_ne!(early.draws()[0].mvp(), later.draws()[0].mvp());
    }
}
