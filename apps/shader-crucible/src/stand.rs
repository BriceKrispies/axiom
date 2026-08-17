//! **Standing the twelve bodies up.** One per station subject, each carrying the
//! surface of the station it demonstrates.
//!
//! Registration happens here, after `App::build()`, rather than in the `setup`
//! closure, because two of the bodies need registrations that live on
//! `RunningApp` and not on `Assets<Mesh>`: station 3's baked tile goes in through
//! `add_texture_data` and station 7's marched body through `add_mesh_data`.
//!
//! ## Why a sphere for nearly everything
//!
//! A sphere shows a lighting model (station 6 is unreadable on a flat quad),
//! shows a displacement (station 5's silhouette is the point), and shows an
//! object-space pattern wrapping a solid rather than a decal. The exceptions are
//! the ones whose subject is not shading: station 3 is a **quad**, because a
//! baked texture is a picture and wants to be seen flat; station 5's wind body is
//! a **cube**, because a displaced silhouette reads best against an outline the
//! eye already knows; station 7 is the marched implicit body, which is its own
//! subject.
//!
//! ## The meshes are deliberately not tessellated
//!
//! Station 1's scratches are finer than a triangle of the built-in sphere, so
//! they vanish on the software rasterizer's one-sample-per-triangle path. That is
//! limitation 3 and it is left visible. Subdividing until the software arm
//! resolved them would be measuring a mesh instead of a backend.

use axiom::prelude::*;

use crate::layout::{ch, slot_position, GROUND_Y};
use crate::stations::{displacement, implicit, layered, lighting, live, patterns, retune};

/// Register the meshes, materials and bodies of every station.
pub fn populate(app: &mut RunningApp) {
    let sphere = app.add_mesh(Mesh::cube());
    let ball = app.add_mesh(Mesh::sphere());
    let quad = app.add_mesh(Mesh::plane());

    // The ground: an ordinary lit material, so a station's own colour is never
    // confused with the floor's.
    // A neutral checker rather than the `UvGrid`'s red/green ramp: every station
    // is a *colour*, and a floor with a hue gradient across it would be read as
    // part of the thing under test.
    let ground_material = app.add_material(
        Material::lit(Color::linear_rgb(ch(0.115), ch(0.125), ch(0.148)))
            .with_texture(Texture::Checker),
    );
    app.spawn(Spawn::new(
        Transform::combine(
            Transform::from_translation(Vec3::new(0.0, GROUND_Y, 0.0)),
            Transform::from_scale(Vec3::new(34.0, 1.0, 22.0)),
        ),
        quad,
        ground_material,
    ));

    let slot = &mut 0_usize;

    // 1 — the layered material, on a ball so the paint wraps a solid.
    place(app, slot, ball, layered::layered_material(), 1.05);
    // 2 — the live procedural surface. A ball, so `Uv` wraps it.
    place(app, slot, ball, live::live_surface(), 1.05);
    // 3 — the SAME graph, baked. Registered as a raw texture on an ordinary lit
    // material: this body deliberately carries NO surface program, because the
    // whole point is that its pixels came from a bake.
    let baked = live::baked_albedo()
        .and_then(|pixels| {
            app.add_texture_data(live::BAKE_RES, live::BAKE_RES, pixels)
                .ok()
        })
        .map(|handle| {
            app.add_material(Material::lit(Color::WHITE).with_custom_texture(handle.id()))
        });
    baked.into_iter().for_each(|material| {
        app.spawn(Spawn::new(
            Transform::combine(
                Transform::from_translation(slot_position(*slot)),
                Transform::from_scale(Vec3::new(1.9, 1.0, 1.9)),
            ),
            quad,
            material,
        ));
        *slot += 1;
    });
    // 4 — the parameter retune.
    place(app, slot, ball, retune::retune_surface(), 1.05);
    // 5 — the two time-varying displacements. The wind rides a CUBE, because a
    // displaced silhouette is easiest to read against a shape whose undisplaced
    // outline the eye already knows — which is also what makes the undisplaced
    // shadow beside it legible.
    place(app, slot, sphere, displacement::wind_surface(), 0.95);
    place(app, slot, ball, displacement::ripple_surface(), 1.15);
    // 6 — the three lighting models, side by side, in `LightingModel::ALL` order.
    lighting::lighting_surfaces()
        .into_iter()
        .for_each(|surface| place(app, slot, ball, surface, 1.0));
    // 7 — the marched implicit body. Its own geometry, so its scale is 1.
    implicit::implicit_body()
        .and_then(|mesh| {
            app.add_mesh_data(MeshData::new(
                mesh.positions().to_vec(),
                mesh.normals().to_vec(),
                mesh.uvs().to_vec(),
                mesh.indices().to_vec(),
            ))
            .ok()
        })
        .into_iter()
        .for_each(|handle| place(app, slot, handle, implicit::implicit_surface(), 0.85));
    // 8 — marble and wood.
    patterns::pattern_surfaces()
        .into_iter()
        .for_each(|surface| place(app, slot, ball, surface, 1.05));
}

/// Stand one station's body in the next slot of the row, carrying `surface` as
/// its material — so the draw names that surface's own digest as its
/// `surface_program`.
fn place(
    app: &mut RunningApp,
    slot: &mut usize,
    mesh: Handle<Mesh>,
    surface: Surface,
    scale: f32,
) {
    let material = app.add_material(Material::from_surface(surface));
    app.spawn(Spawn::new(
        Transform::combine(
            Transform::from_translation(slot_position(*slot)),
            Transform::from_scale(Vec3::new(scale, scale, scale)),
        ),
        mesh,
        material,
    ));
    *slot += 1;
}

