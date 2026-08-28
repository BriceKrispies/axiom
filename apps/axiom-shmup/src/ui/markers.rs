//! Everything anchored to a world position: objective markers with distance,
//! grenade danger indicators, and floating damage numbers.
//!
//! Ported from `C:/dev/Claude-of-Duty/src/ui/markers.js:1-263`.
//!
//! Off-screen targets are clamped to a rectangular ring inside the safe area
//! and their glyph swaps to a chevron pointing at the target — the same
//! behaviour as a CoD objective you have turned away from.
//!
//! ## The camera boundary
//!
//! The source projects with `THREE.Camera` (`_v.project(camera)`, which
//! multiplies by `projectionMatrix * matrixWorldInverse` and divides by `w`).
//! No camera/render subsystem has landed in this port yet (see the crate root
//! docs), so [`project`] is written against the narrow [`ScreenProjector`]
//! trait instead of a concrete camera type: anything that can hand back a
//! view-projection [`Mat4`](axiom_math::Mat4) and an eye position can drive
//! it today, and the real camera binding implements the trait when it lands
//! without this module changing at all.

use axiom_math::{Mat4, Vec4};

use super::util::{clamp01, ease, metres, Pool};

/// The narrow camera contract [`project`] needs: a view-projection matrix
/// (clip-space = `view_projection() * world_homogeneous`) and the eye
/// position used for the distance readout. Mirrors exactly what
/// `Vector3.project(camera)` needs from a `THREE.Camera` — nothing more.
pub trait ScreenProjector {
    fn eye(&self) -> [f32; 3];
    fn view_projection(&self) -> Mat4;
}

/// A fixed view-projection + eye, for tests and for any caller that already
/// has the matrix (an app's per-frame camera state) without wanting to define
/// a whole type for it.
#[derive(Debug, Clone, Copy)]
pub struct FixedCamera {
    pub eye: [f32; 3],
    pub view_projection: Mat4,
}

impl ScreenProjector for FixedCamera {
    fn eye(&self) -> [f32; 3] {
        self.eye
    }

    fn view_projection(&self) -> Mat4 {
        self.view_projection
    }
}

/// The result of projecting one world point — the source's shared `_proj`
/// scratch object (`markers.js:10`), returned by value here since Rust has no
/// need for the source's allocation-avoidance trick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Projection {
    pub x: f64,
    pub y: f64,
    pub dist: f64,
    pub behind: bool,
    pub offscreen: bool,
    pub angle_deg: f64,
}

/// Projects a world point into HUD pixels. Ported from `markers.js:11-43`.
pub fn project(pos: [f32; 3], camera: &dyn ScreenProjector, w: f64, h: f64, margin: f64) -> Projection {
    let eye = camera.eye();
    let dist = ((pos[0] - eye[0]) as f64).hypot((pos[1] - eye[1]) as f64).hypot((pos[2] - eye[2]) as f64);

    let clip = camera.view_projection().transform_vec4(Vec4::new(pos[0], pos[1], pos[2], 1.0));
    let wdiv = if clip.w == 0.0 { 1.0 } else { clip.w };
    let ndc_x = (clip.x / wdiv) as f64;
    let ndc_y = (clip.y / wdiv) as f64;
    let ndc_z = (clip.z / wdiv) as f64;

    let behind = ndc_z > 1.0;
    let (mut x, mut y) = ((ndc_x * 0.5 + 0.5) * w, (-ndc_y * 0.5 + 0.5) * h);
    if behind {
        // mirror through the centre so the edge arrow points the correct way
        x = w - x;
        y = h - y;
    }

    let cx = w * 0.5;
    let cy = h * 0.5;
    let mut dx = x - cx;
    let mut dy = y - cy;
    let mx = w * 0.5 - margin;
    let my = h * 0.5 - margin;
    let mut off = behind;
    if dx.abs() > mx || dy.abs() > my {
        off = true;
        let s = (mx / if dx.abs() == 0.0 { 1e-4 } else { dx.abs() }).min(my / if dy.abs() == 0.0 { 1e-4 } else { dy.abs() });
        dx *= s;
        dy *= s;
    }

    Projection {
        x: cx + dx,
        y: cy + dy,
        dist,
        behind,
        offscreen: off,
        angle_deg: dy.atan2(dx).to_degrees() + 90.0,
    }
}

/// One objective's `updateObjectives` input (`markers.js:137`).
#[derive(Debug, Clone)]
pub struct Objective {
    pub position: [f32; 3],
    pub label: String,
    pub name: String,
}

/// One objective marker's resolved on-screen state.
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectiveFrame {
    pub translate_x_px: f64,
    pub translate_y_px: f64,
    pub width_px: f64,
    pub label: String,
    pub dist_text: String,
    pub name: String,
    /// `true` -> chevron pointing at the target; `false` -> diamond.
    pub edge: bool,
    pub chevron_rotation_deg: f64,
    pub opacity: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GrenadeFrame {
    pub translate_x_px: f64,
    pub translate_y_px: f64,
    pub danger_close: bool,
    pub ring_scale: f64,
    pub ring_opacity: f64,
    pub opacity: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DamageNumberFrame {
    pub translate_x_px: f64,
    pub translate_y_px: f64,
    pub scale: f64,
    pub opacity: f64,
}

/// `markers.js:85-263`'s `WorldMarkers` class, minus its DOM handles. `N` is
/// the wasm view's per-slot node payload for each of the three pools.
pub struct WorldMarkers<N> {
    obj_pool: Pool<N>,
    nade_pool: Pool<[f32; 3]>,
    dn_pool: Pool<[f32; 3]>,
    rng: crate::rng::Rng,
}

impl<N> WorldMarkers<N> {
    pub fn new(obj_nodes: Vec<N>, nade_count: usize, dn_count: usize, rng: crate::rng::Rng) -> Self {
        WorldMarkers {
            obj_pool: Pool::new(obj_nodes),
            nade_pool: Pool::new(vec![[0.0; 3]; nade_count]),
            dn_pool: Pool::new(vec![[0.0; 3]; dn_count]),
            rng,
        }
    }

    /// Returns `(slot index, frame)` for every objective still shown this
    /// frame, in list order — up to the pool's capacity.
    pub fn update_objectives(
        &self,
        list: &[Objective],
        camera: &dyn ScreenProjector,
        w: f64,
        h: f64,
        k: f64,
    ) -> Vec<(usize, ObjectiveFrame)> {
        let margin = 74.0 * k;
        list.iter()
            .take(self.obj_pool.count())
            .enumerate()
            .map(|(i, o)| {
                let p = project(o.position, camera, w, h, margin);
                let fade = clamp01(1.15 - p.dist / 260.0) * if p.offscreen { 0.72 } else { 1.0 };
                (
                    i,
                    ObjectiveFrame {
                        translate_x_px: p.x - 20.0 * k,
                        translate_y_px: p.y - 12.0 * k,
                        width_px: 40.0 * k,
                        label: o.label.clone(),
                        dist_text: metres(p.dist),
                        name: o.name.clone(),
                        edge: p.offscreen,
                        chevron_rotation_deg: p.angle_deg,
                        opacity: fade,
                    },
                )
            })
            .collect()
    }

    pub fn spawn_grenade(&mut self, position: [f32; 3], fuse: f64) -> usize {
        let i = self.nade_pool.acquire();
        self.nade_pool.slots_mut()[i].life = fuse;
        *self.nade_pool.node_mut(i) = position;
        i
    }

    pub fn update_grenades(&mut self, dt: f64, camera: &dyn ScreenProjector, w: f64, h: f64, k: f64) -> Vec<(usize, GrenadeFrame)> {
        let margin = 56.0 * k;
        let mut out = Vec::new();
        for i in 0..self.nade_pool.count() {
            let mut slot = self.nade_pool.slot(i);
            if !slot.alive {
                continue;
            }
            slot.t += dt;
            if slot.t >= slot.life {
                self.nade_pool.release(i);
                continue;
            }
            self.nade_pool.slots_mut()[i] = slot;

            let p = project(*self.nade_pool.node(i), camera, w, h, margin);
            let close = p.dist < 9.0;
            let remain = 1.0 - slot.t / slot.life;
            let rate = 2.2 + (1.0 - remain) * 5.0;
            let ph = (slot.t * rate) % 1.0;
            let rs = 0.7 + 0.9 * ease::out_cubic(ph);
            out.push((
                i,
                GrenadeFrame {
                    translate_x_px: p.x,
                    translate_y_px: p.y,
                    danger_close: close,
                    ring_scale: rs,
                    ring_opacity: 0.9 * (1.0 - ph),
                    opacity: clamp01(remain * 4.0),
                },
            ));
        }
        out
    }

    /// Life is 1.25s for a kill, 0.95s otherwise (`markers.js:214`). The
    /// lateral-drift (`a`) and rise-rate (`b`) scratch values are drawn from
    /// `self.rng` right here, exactly once per spawn, and kept in the pool
    /// slot itself — the same `it.a`/`it.b` the source stashes off its
    /// pool record. Returns the acquired slot index.
    pub fn spawn_damage(&mut self, position: [f32; 3], is_kill: bool) -> usize {
        let i = self.dn_pool.acquire();
        let a = self.rng.signed() * 16.0;
        let b = 0.9 + self.rng.float() * 0.25;
        {
            let slot = &mut self.dn_pool.slots_mut()[i];
            slot.life = if is_kill { 1.25 } else { 0.95 };
            slot.a = a;
            slot.b = b;
        }
        *self.dn_pool.node_mut(i) = position;
        i
    }

    pub fn update_damage(&mut self, dt: f64, camera: &dyn ScreenProjector, w: f64, h: f64, k: f64) -> Vec<(usize, DamageNumberFrame)> {
        let mut out = Vec::new();
        for i in 0..self.dn_pool.count() {
            let mut slot = self.dn_pool.slot(i);
            if !slot.alive {
                continue;
            }
            slot.t += dt;
            let u = slot.t / slot.life;
            if u >= 1.0 {
                self.dn_pool.release(i);
                continue;
            }
            self.dn_pool.slots_mut()[i] = slot;

            let p = project(*self.dn_pool.node(i), camera, w, h, 0.0);
            if p.behind {
                out.push((i, DamageNumberFrame { translate_x_px: p.x, translate_y_px: p.y, scale: 1.0, opacity: 0.0 }));
                continue;
            }
            let rise = ease::out_cubic(clamp01(u * 1.15)) * 42.0 * k * slot.b;
            let drift = slot.a * k * ease::out_quad(u);
            let pop = 1.0 + 0.35 * (1.0 - ease::out_quint(clamp01(u / 0.12)));
            let a = if u < 0.55 { 1.0 } else { 1.0 - ease::in_quad((u - 0.55) / 0.45) };
            out.push((
                i,
                DamageNumberFrame {
                    translate_x_px: p.x + drift,
                    translate_y_px: p.y - rise,
                    scale: pop,
                    opacity: a * clamp01(2.6 - p.dist / 90.0),
                },
            ));
        }
        out
    }

    pub fn clear(&mut self) {
        self.nade_pool.release_all();
        self.dn_pool.release_all();
    }

    pub fn obj_node(&self, i: usize) -> &N {
        self.obj_pool.node(i)
    }

    pub fn obj_count(&self) -> usize {
        self.obj_pool.count()
    }
}

#[cfg(target_arch = "wasm32")]
pub mod view {
    use web_sys::Element;

    use super::super::util::dom;
    use super::{Objective, ObjectiveFrame, ScreenProjector, WorldMarkers};

    fn diamond(parent: &Element) -> Element {
        let s = dom::svg("svg", Some(parent));
        dom::set_attr(&s, "viewBox", "0 0 16 16");
        let r = dom::svg("rect", Some(&s));
        for (k, v) in [("x", "3.2"), ("y", "3.2"), ("width", "9.6"), ("height", "9.6"), ("transform", "rotate(45 8 8)")] {
            dom::set_attr(&r, k, v);
        }
        dom::set_attr(&r, "fill", "rgba(121,210,255,.9)");
        dom::set_attr(&r, "stroke", "rgba(6,20,28,.75)");
        dom::set_attr(&r, "stroke-width", "1");
        s
    }

    fn chevron(parent: &Element) -> Element {
        let s = dom::svg("svg", Some(parent));
        dom::set_attr(&s, "viewBox", "0 0 16 16");
        let p = dom::svg("path", Some(&s));
        dom::set_attr(&p, "d", "M8 1.5 14.4 13H1.6z");
        dom::set_attr(&p, "fill", "rgba(121,210,255,.95)");
        dom::set_attr(&p, "stroke", "rgba(6,20,28,.7)");
        dom::set_attr(&p, "stroke-width", "1");
        dom::set_display(&s, "none");
        s
    }

    fn nade_glyph(parent: &Element) {
        let s = dom::svg("svg", Some(parent));
        dom::set_attr(&s, "viewBox", "0 0 16 16");
        let c = dom::svg("circle", Some(&s));
        for (k, v) in [("cx", "8"), ("cy", "8"), ("r", "5.4")] {
            dom::set_attr(&c, k, v);
        }
        dom::set_attr(&c, "fill", "rgba(255,63,49,.95)");
        dom::set_attr(&c, "stroke", "rgba(0,0,0,.5)");
        dom::set_attr(&c, "stroke-width", "1");
        let r = dom::svg("rect", Some(&s));
        for (k, v) in [("x", "7.2"), ("y", "0.8"), ("width", "1.6"), ("height", "3.2")] {
            dom::set_attr(&r, k, v);
        }
        dom::set_attr(&r, "fill", "rgba(255,63,49,.95)");
    }

    pub struct ObjNode {
        root: Element,
        diamond: Element,
        chevron: Element,
        letter: Element,
        dist: Element,
        name: Element,
    }

    fn build_objective(parent: &Element) -> ObjNode {
        let root = dom::el("div", Some("ow-mk"), Some(parent));
        let glyph = dom::el("div", Some("ow-mk-glyph"), Some(&root));
        let dia = diamond(&glyph);
        let chev = chevron(&glyph);
        let letter = dom::el("div", Some("ow-mk-letter"), Some(&glyph));
        let dist = dom::el("div", Some("ow-mk-dist"), Some(&root));
        let name = dom::el("div", Some("ow-mk-name"), Some(&root));
        dom::set_display(&root, "none");
        ObjNode { root, diamond: dia, chevron: chev, letter, dist, name }
    }

    pub struct NadeNode {
        root: Element,
        ring: Element,
        label: Element,
    }

    fn build_nade(parent: &Element) -> NadeNode {
        let root = dom::el("div", Some("ow-nade"), Some(parent));
        let ring = dom::el("div", Some("ow-nade-ring"), Some(&root));
        let core = dom::el("div", Some("ow-nade-core"), Some(&root));
        nade_glyph(&core);
        let label = dom::el("div", Some("ow-nade-label"), Some(&root));
        dom::set_text(&label, "GRENADE");
        dom::set_display(&root, "none");
        NadeNode { root, ring, label }
    }

    /// `WorldMarkers`'s DOM binding. Grenade and damage-number *positions*
    /// live in the pure core's own pools (`spawn_grenade`/`spawn_damage`
    /// store them — see `markers.js`'s `node._pos`, kept as pool data here
    /// rather than DOM-node data since nothing DOM-specific about a `Vec3`);
    /// this view only ever owns the elements those pools' frames get painted
    /// onto, indexed 1:1 with the pool's stable slot indices.
    pub struct WorldMarkersView {
        core: WorldMarkers<ObjNode>,
        root: Element,
        nade_nodes: Vec<NadeNode>,
        dn_nodes: Vec<Element>,
    }

    impl WorldMarkersView {
        pub fn new(parent: &Element, rng: crate::rng::Rng) -> Self {
            let root = dom::el("div", Some("ow-layer"), Some(parent));
            let obj_nodes: Vec<ObjNode> = (0..6).map(|_| build_objective(&root)).collect();
            let nade_nodes: Vec<NadeNode> = (0..4).map(|_| build_nade(&root)).collect();
            let dn_nodes: Vec<Element> = (0..16).map(|_| dom::el("div", Some("ow-dn"), Some(&root))).collect();
            WorldMarkersView { core: WorldMarkers::new(obj_nodes, 4, 16, rng), root, nade_nodes, dn_nodes }
        }

        pub fn update_objectives(&mut self, list: &[Objective], camera: &dyn ScreenProjector, w: f64, h: f64, k: f64) {
            let frames = self.core.update_objectives(list, camera, w, h, k);
            let shown = frames.len();
            for (i, frame) in &frames {
                Self::apply_objective(self.core.obj_node(*i), frame);
            }
            for i in shown..self.core.obj_count() {
                dom::set_display(&self.core.obj_node(i).root, "none");
            }
        }

        fn apply_objective(node: &ObjNode, frame: &ObjectiveFrame) {
            dom::set_display(&node.root, "");
            dom::set_style(
                &node.root,
                "transform",
                &format!("translate({:.1}px,{:.1}px)", frame.translate_x_px, frame.translate_y_px),
            );
            dom::set_style(&node.root, "width", &format!("{:.1}px", frame.width_px));
            dom::set_text(&node.letter, &frame.label);
            dom::set_text(&node.dist, &frame.dist_text);
            dom::set_text(&node.name, &frame.name);
            dom::set_display(&node.diamond, if frame.edge { "none" } else { "" });
            dom::set_display(&node.chevron, if frame.edge { "" } else { "none" });
            dom::set_style(&node.letter, "opacity", if frame.edge { "0" } else { "1" });
            if frame.edge {
                dom::set_attr(&node.chevron, "transform", &format!("rotate({:.1}deg)", frame.chevron_rotation_deg));
            }
            dom::set_style(&node.root, "opacity", &format!("{:.3}", frame.opacity));
        }

        pub fn spawn_grenade(&mut self, position: [f32; 3], fuse: f64) {
            let i = self.core.spawn_grenade(position, fuse);
            dom::set_display(&self.nade_nodes[i].root, "");
        }

        pub fn update_grenades(&mut self, dt: f64, camera: &dyn ScreenProjector, w: f64, h: f64, k: f64) {
            for (i, frame) in self.core.update_grenades(dt, camera, w, h, k) {
                let node = &self.nade_nodes[i];
                dom::set_style(
                    &node.root,
                    "transform",
                    &format!("translate({:.1}px,{:.1}px)", frame.translate_x_px, frame.translate_y_px),
                );
                dom::set_text(&node.label, if frame.danger_close { "DANGER CLOSE" } else { "GRENADE" });
                dom::set_style(&node.ring, "transform", &format!("scale({:.3})", frame.ring_scale));
                dom::set_style(&node.ring, "opacity", &format!("{:.3}", frame.ring_opacity));
                dom::set_style(&node.root, "opacity", &format!("{:.3}", frame.opacity));
            }
        }

        pub fn spawn_damage(&mut self, position: [f32; 3], amount: f64, kind_class: &str) {
            let is_kill = kind_class == "kill";
            let i = self.core.spawn_damage(position, is_kill);
            let node = &self.dn_nodes[i];
            dom::set_display(node, "");
            dom::set_text(node, &(amount.round() as i64).to_string());
            dom::set_class(node, "hs", kind_class == "hs");
            dom::set_class(node, "kill", kind_class == "kill");
            dom::set_class(node, "armour", kind_class == "armour");
        }

        pub fn update_damage(&mut self, dt: f64, camera: &dyn ScreenProjector, w: f64, h: f64, k: f64) {
            for (i, frame) in self.core.update_damage(dt, camera, w, h, k) {
                let node = &self.dn_nodes[i];
                dom::set_style(
                    node,
                    "transform",
                    &format!(
                        "translate({:.1}px,{:.1}px) translate(-50%,-50%) scale({:.3})",
                        frame.translate_x_px, frame.translate_y_px, frame.scale
                    ),
                );
                dom::set_style(node, "opacity", &format!("{:.3}", frame.opacity));
            }
        }

        pub fn clear(&mut self) {
            self.core.clear();
        }

        pub fn dispose(&self) {
            dom::remove(&self.root);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_math::Vec3;

    /// A camera looking down +Z from the origin, `w`x`h` = 200x100 — enough
    /// to make the projection math legible by hand.
    fn test_camera() -> FixedCamera {
        let view = Mat4::look_at(Vec3::new(0.0, 0.0, -1.0), Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0))
            .expect("look_at");
        let proj = Mat4::perspective(90f32.to_radians(), 2.0, 0.1, 100.0).expect("perspective");
        FixedCamera { eye: [0.0, 0.0, -1.0], view_projection: proj.multiply(view) }
    }

    #[test]
    fn a_point_dead_ahead_projects_to_screen_centre() {
        let cam = test_camera();
        let p = project([0.0, 0.0, 5.0], &cam, 200.0, 100.0, 0.0);
        assert!((p.x - 100.0).abs() < 1e-3);
        assert!((p.y - 50.0).abs() < 1e-3);
        assert!(!p.behind);
        assert!(!p.offscreen);
    }

    #[test]
    fn a_point_behind_the_camera_mirrors_through_the_centre() {
        let cam = test_camera();
        // a point up and to the right, but *behind* the camera (z < eye.z):
        // the raw NDC projection would put it on one side, and the source's
        // mirror-through-centre step (`markers.js:18-22`) flips it so an
        // edge arrow still points at the real target.
        let behind = project([2.0, 1.0, -5.0], &cam, 200.0, 100.0, 0.0);
        assert!(behind.behind);
        assert!(behind.offscreen);
    }

    #[test]
    fn offscreen_points_clamp_into_the_margin_rectangle() {
        let cam = test_camera();
        // far off to one side: NDC x saturates near +-1, well past the visible width.
        let p = project([50.0, 0.0, 5.0], &cam, 200.0, 100.0, 10.0);
        assert!(p.offscreen);
        assert!(p.x <= 100.0 + (100.0 - 10.0) + 1e-6);
        assert!(p.x >= 0.0 - 1e-6);
    }

    #[test]
    fn distance_is_euclidean_from_the_eye() {
        let cam = test_camera();
        let p = project([3.0, 4.0, -1.0], &cam, 200.0, 100.0, 0.0); // eye at (0,0,-1)
        assert!((p.dist - 5.0).abs() < 1e-4);
    }

    #[test]
    fn objectives_beyond_pool_capacity_are_dropped_not_wrapped() {
        let cam = test_camera();
        let markers: WorldMarkers<()> = WorldMarkers::new((0..2).map(|_| ()).collect(), 4, 16, crate::rng::Rng::new(1));
        let list: Vec<Objective> = (0..5)
            .map(|i| Objective { position: [i as f32, 0.0, 5.0], label: i.to_string(), name: String::new() })
            .collect();
        let frames = markers.update_objectives(&list, &cam, 200.0, 100.0, 1.0);
        assert_eq!(frames.len(), 2);
    }

    #[test]
    fn grenade_pulse_rate_increases_as_the_fuse_burns_down() {
        let cam = test_camera(); // eye at (0,0,-1)
        let mut markers: WorldMarkers<()> = WorldMarkers::new(vec![], 2, 2, crate::rng::Rng::new(1));
        markers.spawn_grenade([0.0, 0.0, 20.0], 2.0); // dist = 21 >= 9: not danger close
        let early = markers.update_grenades(0.05, &cam, 200.0, 100.0, 1.0);
        assert_eq!(early.len(), 1);
        assert!(!early[0].1.danger_close);
    }

    #[test]
    fn grenade_close_range_reports_danger_close() {
        let cam = test_camera(); // eye at (0,0,-1)
        let mut markers: WorldMarkers<()> = WorldMarkers::new(vec![], 2, 2, crate::rng::Rng::new(1));
        markers.spawn_grenade([0.0, 0.0, 3.0], 2.0); // dist = 4 < 9: danger close
        let frames = markers.update_grenades(0.01, &cam, 200.0, 100.0, 1.0);
        assert_eq!(frames.len(), 1);
        assert!(frames[0].1.danger_close);
    }

    #[test]
    fn damage_number_behind_camera_is_fully_transparent() {
        let cam = test_camera();
        let mut markers: WorldMarkers<()> = WorldMarkers::new(vec![], 2, 2, crate::rng::Rng::new(1));
        markers.spawn_damage([0.0, 0.0, -5.0], false); // behind the -Z-looking camera
        let frames = markers.update_damage(0.01, &cam, 200.0, 100.0, 1.0);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].1.opacity, 0.0);
    }

    #[test]
    fn kill_damage_numbers_live_longer_than_regular_hits() {
        let mut markers: WorldMarkers<()> = WorldMarkers::new(vec![], 2, 2, crate::rng::Rng::new(1));
        let hit = markers.spawn_damage([0.0, 0.0, 5.0], false);
        let kill = markers.spawn_damage([0.0, 0.0, 5.0], true);
        assert_eq!(markers.dn_pool.slot(hit).life, 0.95);
        assert_eq!(markers.dn_pool.slot(kill).life, 1.25);
    }

    #[test]
    fn clear_releases_grenades_and_damage_numbers() {
        let mut markers: WorldMarkers<()> = WorldMarkers::new(vec![], 2, 2, crate::rng::Rng::new(1));
        markers.spawn_grenade([0.0, 0.0, 5.0], 2.0);
        markers.spawn_damage([0.0, 0.0, 5.0], false);
        markers.clear();
        assert!(markers.update_grenades(0.01, &test_camera(), 200.0, 100.0, 1.0).is_empty());
        assert!(markers.update_damage(0.01, &test_camera(), 200.0, 100.0, 1.0).is_empty());
    }
}
