//! Scene spatial queries (game-vocabulary Category 2) composed into the bridge:
//! the `overlapCircle` / `overlapBox` / `raycast` queries the TS `HostBridge`
//! scene surface projects, every one forwarding to the engine's Entity-addressed
//! query surface on [`RunningApp`](axiom::prelude::RunningApp) (`overlap_circle`
//! / `overlap_box` / `raycast_hit`). The scene owns the bounds tests and the
//! nearest-hit tie-break; this module only marshals scalars in and the entity-id
//! / hit-record arrays out — no query math is re-implemented here.
//!
//! ## Boundary convention (the established slice / scalar rule)
//! A point or direction crosses as a 3-element `&[f64]` slice (exactly as a math
//! vector does — one slice per vector keeps every method within the engine's
//! argument-count budget); a radius / max-distance as a lone scalar `f64`
//! narrowed to [`Meters`](axiom::prelude::Meters). Results cross as `Vec<f64>`:
//! - `overlapCircle` / `overlapBox` return the matching entity ids ascending;
//! - `raycast` returns `[]` (a miss) or `[entity, hitX, hitY, hitZ]` (the nearest
//!   bounded hit plus its world-space entry point) — the same empty-is-absent
//!   shape the `input` / `world` reads use.

use axiom::prelude::{Meters, Vec3};

use crate::GameBridge;

/// A finite [`Meters`] from a boundary scalar; a non-finite value falls back to a
/// zero radius / reach (an inert query), never a panic.
fn meters(value: f64) -> Meters {
    Meters::new(value as f32).unwrap_or_else(|_| Meters::new(0.0).expect("0.0 is finite"))
}

/// A `Vec3` from a 3-element boundary slice (missing entries read `0`).
fn v3(s: &[f64]) -> Vec3 {
    let [x, y, z]: [f32; 3] = core::array::from_fn(|i| *s.get(i).unwrap_or(&0.0) as f32);
    Vec3::new(x, y, z)
}

impl GameBridge {
    /// Every bounded entity whose world box overlaps the query sphere
    /// (`overlapCircle`), as raw ids in ascending order.
    pub fn overlap_circle(&self, center: &[f64], radius: f64) -> Vec<f64> {
        self.runtime
            .app()
            .overlap_circle(v3(center), meters(radius))
            .into_iter()
            .map(|entity| entity.raw() as f64)
            .collect()
    }

    /// Every bounded entity whose world box overlaps the query box (`overlapBox`),
    /// as raw ids in ascending order.
    pub fn overlap_box(&self, center: &[f64], half_extents: &[f64]) -> Vec<f64> {
        self.runtime
            .app()
            .overlap_box(v3(center), v3(half_extents))
            .into_iter()
            .map(|entity| entity.raw() as f64)
            .collect()
    }

    /// Cast a ray and return the nearest bounded hit (`raycast`): `[]` on a miss,
    /// else `[entity, hitX, hitY, hitZ]` — the entity id plus the world-space
    /// entry point on its box.
    pub fn raycast(&self, origin: &[f64], direction: &[f64], max_distance: f64) -> Vec<f64> {
        self.runtime
            .app()
            .raycast_hit(v3(origin), v3(direction), meters(max_distance))
            .map(|(entity, point)| {
                vec![
                    entity.raw() as f64,
                    f64::from(point.x),
                    f64::from(point.y),
                    f64::from(point.z),
                ]
            })
            .unwrap_or_default()
    }
}

#[cfg(target_arch = "wasm32")]
mod wasm_exports {
    use wasm_bindgen::prelude::*;

    use crate::wasm::WasmGame;

    #[wasm_bindgen]
    impl WasmGame {
        /// Bounded entities overlapping the query sphere (`overlapCircle`).
        #[wasm_bindgen(js_name = overlapCircle)]
        pub fn overlap_circle(&self, center: &[f64], radius: f64) -> Vec<f64> {
            self.bridge.overlap_circle(center, radius)
        }

        /// Bounded entities overlapping the query box (`overlapBox`).
        #[wasm_bindgen(js_name = overlapBox)]
        pub fn overlap_box(&self, center: &[f64], half_extents: &[f64]) -> Vec<f64> {
            self.bridge.overlap_box(center, half_extents)
        }

        /// The nearest bounded ray hit (`raycast`): `[]` or `[entity, x, y, z]`.
        #[wasm_bindgen(js_name = raycast)]
        pub fn raycast(&self, origin: &[f64], direction: &[f64], max_distance: f64) -> Vec<f64> {
            self.bridge.raycast(origin, direction, max_distance)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::GameBridge;
    use axiom::prelude::{
        App, Bounds, Color, DefaultPlugins, Material, Mesh, Renderable, Transform, Vec3, Window,
    };

    const STEP: u64 = 1_000_000;

    /// A bridge over a scene with one bounded cube three units down -Z — the
    /// single hit a query test reasons about.
    fn bridge() -> GameBridge {
        let app = App::new()
            .window(Window::new(64, 64))
            .add_plugins(DefaultPlugins)
            .setup(|world, meshes, materials| {
                let cube = meshes.add(Mesh::cube());
                let material = materials.add(Material::lit(Color::WHITE));
                world.spawn((
                    Transform::from_translation(Vec3::new(0.0, 0.0, -3.0)),
                    Renderable {
                        mesh: cube,
                        material,
                    },
                    Bounds::new(Vec3::new(0.5, 0.5, 0.5)),
                ));
            })
            .build();
        GameBridge::new(app, 0, STEP, 1)
    }

    #[test]
    fn overlap_and_raycast_find_the_bounded_node_and_replay() {
        let b = bridge();
        // The box query at the node's centre returns exactly one entity; the same
        // entity the circle query and the ray (down -Z) report.
        let hits = b.overlap_box(&[0.0, 0.0, -3.0], &[0.2, 0.2, 0.2]);
        assert_eq!(hits.len(), 1);
        assert_eq!(b.overlap_circle(&[0.0, 0.0, -3.0], 1.0), hits);
        // The ray hits the same entity and carries a 4-tuple [entity, x, y, z].
        let ray = b.raycast(&[0.0, 0.0, 0.0], &[0.0, 0.0, -1.0], 100.0);
        assert_eq!(ray.len(), 4);
        assert_eq!(ray[0], hits[0]);
        // The entry point sits on the node's near (-Z) face, ~2.5 out.
        assert!((ray[3] + 2.5).abs() < 1.0e-4);
        // Pure functions of the scene: a second independent bridge agrees byte-wise.
        assert_eq!(hits, bridge().overlap_box(&[0.0, 0.0, -3.0], &[0.2, 0.2, 0.2]));
    }

    #[test]
    fn a_miss_is_the_empty_array() {
        let b = bridge();
        // Nothing at the origin; a ray straight up hits nothing.
        assert!(b.overlap_box(&[0.0, 0.0, 0.0], &[0.2, 0.2, 0.2]).is_empty());
        assert!(b.overlap_circle(&[0.0, 0.0, 0.0], 0.5).is_empty());
        assert!(b.raycast(&[0.0, 0.0, 0.0], &[0.0, 1.0, 0.0], 100.0).is_empty());
        // A non-finite reach degrades to a zero-reach query (an inert miss).
        assert!(b
            .raycast(&[0.0, 0.0, 0.0], &[0.0, 0.0, -1.0], f64::INFINITY)
            .is_empty());
    }
}
