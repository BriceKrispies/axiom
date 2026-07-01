//! # Axiom Netplay (browser) — live server-authoritative multiplayer
//!
//! Two browsers each control a cube. The **authoritative server** owns the
//! simulation — and in the `examples/axiom-netplay-dotnet` setup it runs the
//! *real Axiom engine* in-process (natively, via FFI). Each browser sends
//! *intents* (arrow-key move deltas) and receives *snapshots* that are the
//! engine's actual rendered frame: per-cube `[mvp(16), colour(4)]` instance
//! floats.
//!
//! This browser crate runs its **own** engine instance for rendering, and the
//! page tells it where to draw the two cubes each frame via [`web::set_positions`]
//! — positions the page computes with **client-side prediction** (its own cube)
//! and **interpolation** (the other cube) from the authoritative snapshots. The
//! engine integrates a per-tick *delta*, so [`inputs_to_targets`] turns "draw at
//! this absolute position" into the delta that lands the cube there. The
//! networking + prediction + interpolation live in the page's JavaScript over the
//! `@axiom/client` SDK; the server stays authoritative.

use axiom::prelude::*;

/// The presentation canvas element id (must match `web/index.html`).
pub const CANVAS_ID: &str = "axiom-netplay-canvas";

/// The cubes' spawn positions `[p0x, p0y, p1x, p1y]` — matches the scene and the
/// server's authoritative initial state, so the first frame already lines up.
pub const INITIAL_POSITIONS: [f32; 4] = [-1.5, 0.0, 1.5, 0.0];

/// Build the [`PlayerInput`]s that move each cube from its `current` rendered
/// position to the `target` (delta = target - current); after the tick each cube
/// sits exactly at `target`. A zero delta (target == current) holds it still.
pub fn inputs_to_targets(current: [f32; 4], target: [f32; 4]) -> [PlayerInput; 2] {
    [
        PlayerInput::new(
            0,
            Vec3::new(target[0] - current[0], target[1] - current[1], 0.0),
        ),
        PlayerInput::new(
            1,
            Vec3::new(target[2] - current[2], target[3] - current[3], 0.0),
        ),
    ]
}

/// A linear colour channel from an authored literal.
fn ch(value: f32) -> Ratio {
    Ratio::new(value).expect("authored colour channel is finite")
}

/// Build the netplay scene: two player cubes (player 0 red on the left, player 1
/// blue on the right), a pulled-back camera, and a directional light. The browser
/// uses this for the cube geometry, the instance count, and an initial frame; the
/// authoritative per-frame transforms come from the server's engine. Authored
/// identically to the server scene (`apps/axiom-netplay-ffi`) so the instance
/// floats line up with this vertex buffer.
pub fn build_netplay_app() -> RunningApp {
    App::new()
        .window(
            Window::new(800, 600)
                .with_surface_id(CANVAS_ID)
                .with_clear_color(Color::linear_rgb(ch(0.04), ch(0.05), ch(0.08))),
        )
        .add_plugins(DefaultPlugins)
        .setup(|world, meshes, materials| {
            let cube = meshes.add(Mesh::cube());
            let red = materials.add(Material::lit(Color::linear_rgb(
                ch(0.90),
                ch(0.27),
                ch(0.27),
            )));
            let blue = materials.add(Material::lit(Color::linear_rgb(
                ch(0.30),
                ch(0.45),
                ch(0.95),
            )));
            world.spawn((
                Transform::from_translation(Vec3::new(-1.5, 0.0, 0.0)),
                Renderable {
                    mesh: cube,
                    material: red,
                },
                Player::new(0),
            ));
            world.spawn((
                Transform::from_translation(Vec3::new(1.5, 0.0, 0.0)),
                Renderable {
                    mesh: cube,
                    material: blue,
                },
                Player::new(1),
            ));
            world.spawn((
                Transform::from_translation(Vec3::new(0.0, 0.0, 9.0)),
                Camera::perspective(PerspectiveProjection {
                    fov_y: Angle::degrees(55.0),
                    near: Meters::new(0.1).expect("near plane is finite"),
                    far: Meters::new(100.0).expect("far plane is finite"),
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

#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scene_draws_two_player_cubes() {
        let mut app = build_netplay_app();
        assert_eq!(app.tick(0).draws().len(), 2);
    }

    #[test]
    fn inputs_to_targets_are_the_per_player_deltas() {
        let inputs = inputs_to_targets(INITIAL_POSITIONS, [-1.0, 0.5, 2.0, -0.5]);
        assert_eq!(inputs[0].delta, Vec3::new(0.5, 0.5, 0.0));
        assert_eq!(inputs[1].delta, Vec3::new(0.5, -0.5, 0.0));
        let still = inputs_to_targets(INITIAL_POSITIONS, INITIAL_POSITIONS);
        assert_eq!(still[0].delta, Vec3::ZERO);
        assert_eq!(still[1].delta, Vec3::ZERO);
    }

    #[test]
    fn a_player_move_changes_the_rendered_frame() {
        // Same path the server's engine takes; proves a move is visible.
        let mut app = build_netplay_app();
        let still = app.tick_with(0, &[]).instance_floats();
        let moved = app
            .tick_with(1, &[PlayerInput::new(0, Vec3::new(0.5, 0.0, 0.0))])
            .instance_floats();
        assert_ne!(still, moved);
    }
}
