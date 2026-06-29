//! The single public facade of the `axiom-draw2d` module.

use axiom_host::{
    Camera2d, Common2d, Draw2dCommand, Draw2dList, Fill2d, FontHandle, GlyphRun, GradientStop,
    PaintId, Rect, RenderTargetId, Rgba, SpriteDraw2d, TextDraw2d, TextMetrics, TextureId,
    TransformDepth,
};
use axiom_kernel::{Meters, Radians, Ratio, Seconds};
use axiom_math::{Mat3, Vec2};

use crate::ids::{EmitterConfig, EmitterId};
use crate::particles::{ParticleField, ParticleQuad};

/// The only public export of `axiom-draw2d`.
///
/// Accumulates a frame's 2D draws onto a host-owned [`Draw2dList`] in progress,
/// then yields the neutral, layer-sorted contract. Shape mirrors `RenderApi`:
/// typed builders in, opaque `KIND_*`-tagged [`Draw2dCommand`]s out, branchless
/// `as_*` accessors for the consumer. It **rasterizes nothing** — turning the
/// list into pixels (and the alpha-blend fix) is the backends' job.
///
/// This module owns only the *builder*; the neutral contract types
/// ([`Draw2dList`], [`Draw2dCommand`], and the value vocabulary) live in the
/// host layer (`axiom_host`), so the render backends that depend on host can
/// name and rasterize them. The builder adds the authoring ergonomics the
/// contract deliberately does not carry: the transform stack and the submit
/// counter. Callers `use axiom_host::{…}` for the value vocabulary they pass in.
///
/// Presentation-class: the only caller is `onRender`. Nothing it produces is
/// authoritative, and there is **no getter that returns draw state into a
/// sim-readable form** — the facade hands out a `Draw2dList` and never reads it
/// back. The particle field (§10.1) is the sharpest case of this rule: it is a
/// private field with no read-back path, so a particle can never be queried by,
/// or feed, sim.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Draw2dApi {
    list: Draw2dList,
    transform_stack: Vec<Mat3>,
    next_submission: u32,
    /// The live, presentation-only particle system (§10.1). Persists across
    /// frames (particles outlive a single `finish`); never exposed.
    particles: ParticleField,
    /// The render target (§10.3) draws currently route into, if any. `None`
    /// routes to the main list.
    active_target: Option<RenderTargetId>,
}

impl Draw2dApi {
    /// A fresh, empty draw surface.
    pub fn new() -> Self {
        Draw2dApi::default()
    }

    /// The composed transform currently on top of the stack (identity when the
    /// stack is empty).
    fn current_transform(&self) -> Mat3 {
        self.transform_stack
            .last()
            .copied()
            .unwrap_or(Mat3::IDENTITY)
    }

    /// Build the per-draw header tuple (submit index + baked transform + the
    /// caller's resolved common attributes) and advance the submission counter.
    fn next_header(&mut self, common: Common2d) -> (u32, Mat3, Common2d) {
        let header = (self.next_submission, self.current_transform(), common);
        self.next_submission += 1;
        header
    }

    /// Append a built command, routing it into the active render target (§10.3)
    /// when one is open, else the main list. Branchless — the host owns the sink
    /// selection.
    fn route(&mut self, cmd: Draw2dCommand) {
        self.list.push_command_routed(self.active_target, cmd);
    }

    // --- Camera + transform stack ---

    /// Set the 2D camera for this frame (centre + zoom).
    pub fn set_camera2d(&mut self, center: Vec2, zoom: Ratio) {
        self.list.set_camera(Camera2d::new(center, zoom));
    }

    /// Push `m` onto the transform stack, composing it onto the current top
    /// (`current * m`). Returns the depth to restore with [`Self::pop_transform`].
    pub fn push_transform(&mut self, m: Mat3) -> TransformDepth {
        let depth = TransformDepth::from_raw(self.transform_stack.len());
        let composed = self.current_transform().multiply(m);
        self.transform_stack.push(composed);
        depth
    }

    /// Restore the transform stack to `depth` (the value a matching
    /// [`Self::push_transform`] returned).
    pub fn pop_transform(&mut self, depth: TransformDepth) {
        self.transform_stack.truncate(depth.raw());
    }

    // --- Shapes ---

    /// Draw a filled/stroked rectangle.
    pub fn rect(&mut self, r: Rect, style: Fill2d, common: Common2d) {
        let cmd = Draw2dCommand::rect(self.next_header(common), r, style);
        self.route(cmd);
    }

    /// Draw a filled/stroked circle.
    pub fn circle(&mut self, center: Vec2, radius: Meters, style: Fill2d, common: Common2d) {
        let cmd = Draw2dCommand::circle(self.next_header(common), center, radius, style);
        self.route(cmd);
    }

    /// Draw a filled/stroked (optionally rotated) ellipse.
    pub fn ellipse(
        &mut self,
        center: Vec2,
        rx: Meters,
        ry: Meters,
        rotation: Radians,
        style: Fill2d,
        common: Common2d,
    ) {
        let cmd = Draw2dCommand::ellipse(self.next_header(common), center, rx, ry, rotation, style);
        self.route(cmd);
    }

    /// Draw a straight line segment with its own colour and width.
    pub fn line(&mut self, a: Vec2, b: Vec2, color: Rgba, width: Meters, common: Common2d) {
        let cmd = Draw2dCommand::line(self.next_header(common), a, b, color, width);
        self.route(cmd);
    }

    /// Draw a polyline / polygon through `points` (closed when `closed`).
    pub fn path(&mut self, points: &[Vec2], style: Fill2d, common: Common2d, closed: bool) {
        let cmd = Draw2dCommand::path(self.next_header(common), points.to_vec(), style, closed);
        self.route(cmd);
    }

    // --- Sprites + text ---

    /// Draw a textured sprite (source sub-rect / anchor / tint / flips ride on
    /// `opts`; placement on the current transform).
    pub fn sprite(&mut self, texture: TextureId, opts: SpriteDraw2d, common: Common2d) {
        let cmd = Draw2dCommand::sprite(self.next_header(common), texture, opts);
        self.route(cmd);
    }

    /// Draw a glyph run as `KIND_TEXT_GLYPHS` — glyph sub-rects against a baked
    /// font atlas, the same shape as a sprite draw.
    pub fn text(&mut self, run: GlyphRun, opts: TextDraw2d, common: Common2d) {
        let cmd = Draw2dCommand::text(self.next_header(common), run, opts);
        self.route(cmd);
    }

    /// Measure a glyph run against `font` (width = sum of advances, height =
    /// line height). Pure: no font registry, no rasterization.
    pub fn measure_text(&self, run: &GlyphRun, font: FontHandle) -> TextMetrics {
        run.measure(font)
    }

    // --- Paints ---

    /// Register a linear gradient, returning its [`PaintId`]. A command's
    /// [`Fill2d`] references the paint by id; it never inlines stops.
    pub fn linear_gradient(&mut self, from: Vec2, to: Vec2, stops: &[GradientStop]) -> PaintId {
        self.list.register_linear(from, to, stops.to_vec())
    }

    /// Register a radial gradient, returning its [`PaintId`].
    pub fn radial_gradient(
        &mut self,
        center: Vec2,
        radius: Meters,
        stops: &[GradientStop],
    ) -> PaintId {
        self.list.register_radial(center, radius, stops.to_vec())
    }

    // --- Particles (§10.1, presentation-only) ---

    /// Register a particle emitter, returning its [`EmitterId`]. The emitter is a
    /// recipe; nothing is spawned until [`Self::emit`].
    pub fn create_emitter(&mut self, config: EmitterConfig) -> EmitterId {
        self.particles.create_emitter(config)
    }

    /// Spawn a burst from emitter `id` at `at`, flying along `direction`. The
    /// particles live in the presentation-only field; an unknown id is a no-op.
    pub fn emit(&mut self, id: EmitterId, at: Vec2, direction: Vec2) {
        self.particles.emit(id, at, direction);
    }

    /// Step the live particles by the **presentation** delta `dt` (real
    /// frame-delta, never a sim tick) and append each survivor as a
    /// `KIND_PARTICLE_QUAD` command into the list (routed like any other draw),
    /// before [`Self::finish`]'s layer sort. Particle alpha rides on the faded
    /// quad colour, so each quad's [`Common2d`] alpha is full.
    pub fn advance_particles(&mut self, dt: Seconds) {
        self.particles.advance(dt);
        self.particles.quads().into_iter().for_each(|q| {
            let ParticleQuad {
                center,
                size,
                color,
                layer,
            } = q;
            let header = self.next_header(Common2d::new(layer, Ratio::finite_or_zero(1.0)));
            self.route(Draw2dCommand::particle_quad(header, center, size, color));
        });
    }

    // --- Render targets (§10.3) ---

    /// Create an off-screen render target of `width`×`height` pixels, returning
    /// its [`RenderTargetId`]. A render target is a named nested list; the backend
    /// owns the actual surface.
    pub fn create_render_target(&mut self, width: u32, height: u32) -> RenderTargetId {
        self.list.create_render_target(width, height)
    }

    /// Route subsequent draws into `target` until the matching [`Self::end_target`].
    pub fn begin_target(&mut self, target: RenderTargetId) {
        self.active_target = Some(target);
    }

    /// Stop routing into a render target; subsequent draws return to the main list.
    pub fn end_target(&mut self) {
        self.active_target = None;
    }

    /// The [`TextureId`] naming `target`'s off-screen surface — the handle a later
    /// draw binds to sample the rendered target.
    pub fn target_texture(&self, target: RenderTargetId) -> TextureId {
        self.list.target_texture(target)
    }

    // --- Finalize ---

    /// Finish the frame: take the accumulated list, **stable-sort it by
    /// `(layer, submission)`** so equal layers keep call order, and yield the
    /// neutral host-owned [`Draw2dList`]. Resets the per-frame surface (transform
    /// stack, submit counter, open render target) for the next frame. The live
    /// particle field persists — particles outlive a single frame.
    pub fn finish(&mut self) -> Draw2dList {
        let mut out = std::mem::take(&mut self.list);
        out.sort_commands();
        self.transform_stack.clear();
        self.next_submission = 0;
        self.active_target = None;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_host::{Glyph2d, Shadow2d, Stroke2d, TextAlign};

    fn ratio(v: f32) -> Ratio {
        Ratio::new(v).unwrap()
    }

    fn meters(v: f32) -> Meters {
        Meters::new(v).unwrap()
    }

    fn radians(v: f32) -> Radians {
        Radians::new(v).unwrap()
    }

    fn red() -> Rgba {
        Rgba::new(ratio(1.0), ratio(0.0), ratio(0.0), ratio(1.0))
    }

    fn common(layer: i32) -> Common2d {
        Common2d::new(layer, ratio(1.0))
    }

    fn seconds(v: f32) -> Seconds {
        Seconds::new(v).unwrap()
    }

    fn clear() -> Rgba {
        Rgba::new(ratio(0.0), ratio(0.0), ratio(0.0), ratio(0.0))
    }

    fn emitter(count: u32, layer: i32) -> EmitterConfig {
        EmitterConfig {
            count,
            lifetime: seconds(2.0),
            speed: meters(10.0),
            spread: ratio(0.25),
            gravity: Vec2::new(0.0, -4.0),
            size: meters(0.5),
            color_start: red(),
            color_end: clear(),
            layer,
        }
    }

    fn unit_rect() -> Rect {
        Rect::new(Vec2::ZERO, Vec2::ONE)
    }

    #[test]
    fn new_equals_default() {
        assert_eq!(Draw2dApi::new(), Draw2dApi::default());
    }

    #[test]
    fn empty_finish_is_empty_list() {
        let mut api = Draw2dApi::new();
        let list = api.finish();
        assert!(list.is_empty());
        assert_eq!(list.camera(), None);
        assert_eq!(list.paint_count(), 0);
    }

    #[test]
    fn rect_records_one_command_with_identity_transform() {
        let mut api = Draw2dApi::new();
        api.rect(unit_rect(), Fill2d::color(red()), common(0));
        let list = api.finish();
        assert_eq!(list.len(), 1);
        let c = list.at(0).unwrap();
        assert_eq!(c.kind_code(), Draw2dCommand::KIND_RECT);
        assert_eq!(c.as_rect(), Some(unit_rect()));
        assert_eq!(c.transform(), Mat3::IDENTITY);
        assert_eq!(c.submission_index(), 0);
    }

    #[test]
    fn every_shape_builder_records_its_kind() {
        let mut api = Draw2dApi::new();
        api.rect(unit_rect(), Fill2d::color(red()), common(0));
        api.circle(Vec2::ZERO, meters(1.0), Fill2d::color(red()), common(0));
        api.ellipse(
            Vec2::ZERO,
            meters(2.0),
            meters(1.0),
            radians(0.0),
            Fill2d::color(red()),
            common(0),
        );
        api.line(Vec2::ZERO, Vec2::ONE, red(), meters(1.0), common(0));
        api.path(
            &[Vec2::ZERO, Vec2::ONE],
            Fill2d::stroked(Stroke2d::new(red(), meters(1.0))),
            common(0),
            false,
        );
        api.sprite(
            TextureId::from_raw(1),
            SpriteDraw2d::new(unit_rect(), Vec2::ZERO, red(), false, false),
            common(0),
        );
        api.text(
            GlyphRun::new(vec![Glyph2d::new(unit_rect(), meters(3.0))], meters(6.0)),
            TextDraw2d::new(FontHandle::from_raw(1), red(), TextAlign::LEFT),
            common(0),
        );
        let list = api.finish();
        let kinds: Vec<u32> = list.commands().iter().map(Draw2dCommand::kind_code).collect();
        assert_eq!(
            kinds,
            vec![
                Draw2dCommand::KIND_RECT,
                Draw2dCommand::KIND_CIRCLE,
                Draw2dCommand::KIND_ELLIPSE,
                Draw2dCommand::KIND_LINE,
                Draw2dCommand::KIND_PATH,
                Draw2dCommand::KIND_SPRITE,
                Draw2dCommand::KIND_TEXT_GLYPHS,
            ]
        );
    }

    #[test]
    fn push_transform_composes_onto_current_and_bakes_onto_draws() {
        let mut api = Draw2dApi::new();
        let depth = api.push_transform(Mat3::translation(Vec2::new(10.0, 0.0)));
        api.push_transform(Mat3::scale(Vec2::new(2.0, 2.0)));
        // Inside both transforms: a draw bakes translation * scale.
        api.rect(unit_rect(), Fill2d::color(red()), common(0));
        let baked = api.finish();
        let expected = Mat3::translation(Vec2::new(10.0, 0.0)).multiply(Mat3::scale(Vec2::new(2.0, 2.0)));
        assert_eq!(baked.at(0).unwrap().transform(), expected);
        // The returned depth marks the empty stack to restore to.
        assert_eq!(depth, TransformDepth::from_raw(0));
    }

    #[test]
    fn pop_transform_restores_to_depth() {
        let mut api = Draw2dApi::new();
        let depth = api.push_transform(Mat3::translation(Vec2::new(5.0, 0.0)));
        api.push_transform(Mat3::scale(Vec2::new(3.0, 3.0)));
        api.pop_transform(depth);
        // Back to identity (stack truncated to 0): the draw bakes identity.
        api.rect(unit_rect(), Fill2d::color(red()), common(0));
        let list = api.finish();
        assert_eq!(list.at(0).unwrap().transform(), Mat3::IDENTITY);
    }

    #[test]
    fn set_camera2d_resolves_onto_the_list() {
        let mut api = Draw2dApi::new();
        api.set_camera2d(Vec2::new(3.0, 4.0), ratio(2.0));
        let list = api.finish();
        assert_eq!(list.camera(), Some(Camera2d::new(Vec2::new(3.0, 4.0), ratio(2.0))));
    }

    #[test]
    fn gradients_register_and_resolve_through_the_list() {
        let mut api = Draw2dApi::new();
        let stops = [
            GradientStop::new(ratio(0.0), red()),
            GradientStop::new(ratio(1.0), red()),
        ];
        let lin = api.linear_gradient(Vec2::ZERO, Vec2::new(1.0, 0.0), &stops);
        let rad = api.radial_gradient(Vec2::ONE, meters(4.0), &stops);
        // A rect fills via the linear paint id (referenced, not inlined).
        api.rect(unit_rect(), Fill2d::paint(lin), common(0));
        let list = api.finish();
        assert_eq!(list.paint_count(), 2);
        assert_eq!(list.paint_linear(lin), Some((Vec2::ZERO, Vec2::new(1.0, 0.0))));
        assert_eq!(list.paint_radial(rad), Some((Vec2::ONE, meters(4.0))));
        assert_eq!(list.paint_stops(lin).map(|s| s.len()), Some(2));
        // The rect's fill references the paint id, carrying no stops itself.
        assert_eq!(list.at(0).unwrap().fill(), Some(Fill2d::paint(lin)));
    }

    #[test]
    fn shadow_rides_through_common_onto_the_command() {
        let mut api = Draw2dApi::new();
        let shadow = Shadow2d::new(red(), meters(2.0));
        api.rect(
            unit_rect(),
            Fill2d::color(red()),
            Common2d::with_shadow(1, ratio(0.5), shadow),
        );
        let list = api.finish();
        let c = list.at(0).unwrap();
        assert_eq!(c.shadow(), Some(shadow));
        assert_eq!(c.alpha(), ratio(0.5));
    }

    #[test]
    fn measure_text_delegates_to_the_run() {
        let api = Draw2dApi::new();
        let run = GlyphRun::new(
            vec![
                Glyph2d::new(unit_rect(), meters(3.0)),
                Glyph2d::new(unit_rect(), meters(4.0)),
            ],
            meters(10.0),
        );
        let m = api.measure_text(&run, FontHandle::from_raw(2));
        assert_eq!(m.width, meters(7.0));
        assert_eq!(m.height, meters(10.0));
    }

    #[test]
    fn finish_resets_surface_for_next_frame() {
        let mut api = Draw2dApi::new();
        api.set_camera2d(Vec2::ONE, ratio(1.0));
        api.linear_gradient(Vec2::ZERO, Vec2::ONE, &[GradientStop::new(ratio(0.0), red())]);
        api.rect(unit_rect(), Fill2d::color(red()), common(0));
        let first = api.finish();
        assert_eq!(first.len(), 1);
        // Second frame starts empty: no carried commands, paints, or camera, and
        // submission counter reset to 0.
        api.rect(unit_rect(), Fill2d::color(red()), common(0));
        let second = api.finish();
        assert_eq!(second.len(), 1);
        assert_eq!(second.camera(), None);
        assert_eq!(second.paint_count(), 0);
        assert_eq!(second.at(0).unwrap().submission_index(), 0);
    }

    #[test]
    fn advance_particles_appends_quads_that_evolve_and_fade() {
        let mut api = Draw2dApi::new();
        let id = api.create_emitter(emitter(3, 5));
        api.emit(id, Vec2::ZERO, Vec2::new(1.0, 0.0));
        api.advance_particles(seconds(0.5));
        let list = api.finish();
        // One KIND_PARTICLE_QUAD per emitted particle.
        assert_eq!(list.len(), 3);
        let (center, size, color) = list.at(0).unwrap().as_particle().unwrap();
        assert_eq!(list.at(0).unwrap().kind_code(), Draw2dCommand::KIND_PARTICLE_QUAD);
        // Real work: the particle moved along +x and its colour faded toward the
        // (transparent) end colour, so alpha dropped below the start's 1.0.
        assert!(center.x > 0.0, "particle integrated along the emit direction");
        assert_eq!(size, meters(0.5));
        assert!(color.a.get() < 1.0, "colour faded toward color_end");
    }

    #[test]
    fn particles_are_deterministic_across_identical_runs() {
        let run = || {
            let mut api = Draw2dApi::new();
            let id = api.create_emitter(emitter(6, 2));
            api.emit(id, Vec2::new(1.0, 2.0), Vec2::new(0.0, 1.0));
            api.advance_particles(seconds(0.25));
            api.advance_particles(seconds(0.25));
            api.finish()
        };
        // Same facade calls + same dt stream ⇒ byte-identical draw lists.
        assert_eq!(run(), run());
    }

    #[test]
    fn layer_sort_still_holds_with_particle_quads() {
        let mut api = Draw2dApi::new();
        // A background rect on layer 0, particles on layer 5.
        api.rect(unit_rect(), Fill2d::color(red()), common(0));
        let id = api.create_emitter(emitter(2, 5));
        api.emit(id, Vec2::ZERO, Vec2::new(1.0, 0.0));
        api.advance_particles(seconds(0.5));
        let list = api.finish();
        let kinds: Vec<u32> = list.commands().iter().map(Draw2dCommand::kind_code).collect();
        // The low-layer rect sorts before the higher-layer particle quads.
        assert_eq!(
            kinds,
            vec![
                Draw2dCommand::KIND_RECT,
                Draw2dCommand::KIND_PARTICLE_QUAD,
                Draw2dCommand::KIND_PARTICLE_QUAD,
            ]
        );
    }

    #[test]
    fn render_target_routes_draws_into_a_nested_list() {
        let mut api = Draw2dApi::new();
        let target = api.create_render_target(64, 32);
        // While the target is open, draws route into it, not the main list.
        api.begin_target(target);
        api.rect(unit_rect(), Fill2d::color(red()), common(0));
        api.end_target();
        // After end_target, draws return to the main list.
        api.circle(Vec2::ZERO, meters(1.0), Fill2d::color(red()), common(0));
        let list = api.finish();
        // The main list holds only the circle; the target holds the rect.
        assert_eq!(list.len(), 1);
        assert_eq!(list.at(0).unwrap().kind_code(), Draw2dCommand::KIND_CIRCLE);
        let routed = list.target_commands(target).unwrap();
        assert_eq!(routed.len(), 1);
        assert_eq!(routed[0].kind_code(), Draw2dCommand::KIND_RECT);
    }

    #[test]
    fn target_texture_names_the_targets_surface_and_lists_are_byte_stable() {
        let build = || {
            let mut api = Draw2dApi::new();
            let target = api.create_render_target(16, 16);
            api.begin_target(target);
            api.rect(unit_rect(), Fill2d::color(red()), common(1));
            api.end_target();
            (api.target_texture(target), api.finish())
        };
        let (tex_a, list_a) = build();
        let (tex_b, list_b) = build();
        // The handle naming the off-screen surface is the target's slot texture.
        assert_eq!(tex_a, TextureId::from_raw(0));
        assert_eq!(tex_a, tex_b);
        // Byte-stable across identical runs.
        assert_eq!(list_a, list_b);
    }
}
