//! SPEC-11 §7 "nova-roll" slice: renders a cube + emissive cylinder scene on
//! both the GPU off-screen backend and the software Canvas 2D backend, and
//! asserts they agree on object placement (centroid + coverage), not pixels —
//! the Canvas 2D fallback is a low-res flat-shaded degrade by design. Emissive
//! is asserted as carried-but-not-yet-shaded (round-trips on the material; no
//! backend threads it into a pixel yet).
//!
//! Requires the native GPU adapter the sandbox provides (the off-screen arm)
//! for the parity test; the pure material test runs in the default build.
#![allow(dead_code)]

mod common;

use axiom::prelude::*;

/// Authoring window / GPU render size (16:9, matching the canvas quality tiers so
/// the two backends share a projection aspect).
const W: u32 = 480;
const H: u32 = 270;

fn ch(v: f32) -> Ratio {
    Ratio::new(v).expect("finite colour channel")
}

fn cylinder_emissive() -> Color {
    Color::linear_rgb(ch(0.6), ch(0.5), ch(0.1))
}

#[cfg(feature = "offscreen")]
fn nova_roll() -> RunningApp {
    App::new()
        .window(
            Window::new(W, H)
                .with_clear_color(Color::linear_rgb(ch(0.02), ch(0.02), ch(0.05))),
        )
        .add_plugins(DefaultPlugins)
        .setup(|world, meshes, materials| {
            let cube = meshes.add(Mesh::cube());
            let cylinder = meshes.add(Mesh::cylinder());
            let cube_mat = materials.add(Material::lit(Color::linear_rgb(
                ch(0.85),
                ch(0.30),
                ch(0.25),
            )));
            let cyl_mat = materials.add(
                Material::lit(Color::linear_rgb(ch(0.25), ch(0.55), ch(0.90)))
                    .with_emissive(cylinder_emissive()),
            );
            world.spawn((
                Transform::from_translation(Vec3::new(-1.6, 0.0, 0.0)),
                Renderable {
                    mesh: cube,
                    material: cube_mat,
                },
            ));
            world.spawn((
                Transform::from_translation(Vec3::new(1.6, 0.0, 0.0)),
                Renderable {
                    mesh: cylinder,
                    material: cyl_mat,
                },
            ));
            world.spawn((
                Transform::from_translation(Vec3::new(0.0, 0.0, 6.0)),
                Camera::perspective(PerspectiveProjection {
                    fov_y: Angle::degrees(55.0),
                    near: Meters::new(0.1).expect("near finite"),
                    far: Meters::new(100.0).expect("far finite"),
                }),
            ));
            world.spawn((
                Transform::IDENTITY,
                DirectionalLight {
                    direction: Vec3::new(0.3, -1.0, 0.4),
                    color: Color::WHITE,
                    intensity: ch(1.0),
                },
            ));
        })
        .build()
}

// The GPU↔canvas2d comparison needs the native off-screen arm (`offscreen`
// feature); the pure material test below always runs.
#[cfg(feature = "offscreen")]
#[test]
fn nova_roll_renders_one_frame_on_both_backends_in_agreement() {
    let mut app = nova_roll();
    let meshes = app.mesh_set();
    let materials = app.material_textures();
    let outcome = app.tick(0);

    let (gpu_px, gw, gh) = common::render_gpu(&meshes, &materials, &outcome, W, H);
    // Quality 2 → a 320×180 internal framebuffer (16:9, same aspect as W×H).
    let (sw_px, sw_w, sw_h) = common::render_canvas2d(&meshes, &outcome, 2, W, H);

    assert_eq!((gw, gh), (W, H));
    assert!(sw_w > 0 && sw_h > 0);
    assert_eq!(gpu_px.len() as u32, gw * gh * 4);
    assert_eq!(sw_px.len() as u32, sw_w * sw_h * 4);

    let (gcx, gcy, gcov) = common::region_stats(&gpu_px, gw, gh, 24);
    let (scx, scy, scov) = common::region_stats(&sw_px, sw_w, sw_h, 24);

    assert!(gcov > 0.02 && gcov < 0.7, "gpu coverage {gcov:.3} out of range");
    assert!(scov > 0.02 && scov < 0.7, "canvas2d coverage {scov:.3} out of range");

    let dx = (gcx - scx).abs();
    let dy = (gcy - scy).abs();
    assert!(
        dx < 0.10 && dy < 0.10,
        "centroid disagreement gpu=({gcx:.3},{gcy:.3}) canvas2d=({scx:.3},{scy:.3})"
    );

    let (gl, gr) = common::half_coverage(&gpu_px, gw, gh, 24);
    let (sl, sr) = common::half_coverage(&sw_px, sw_w, sw_h, 24);
    assert!(gl > 0.01 && gr > 0.01, "gpu missing an object in a half: l={gl:.3} r={gr:.3}");
    assert!(sl > 0.01 && sr > 0.01, "canvas2d missing an object in a half: l={sl:.3} r={sr:.3}");
}

#[test]
fn cylinder_material_carries_emissive() {
    let e = cylinder_emissive();
    let m = Material::lit(Color::linear_rgb(ch(0.25), ch(0.55), ch(0.90))).with_emissive(e);
    assert_eq!(m.emissive(), e);
    assert_ne!(Material::lit(Color::WHITE).emissive(), e);
}
