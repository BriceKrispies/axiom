//! One neutral, `KIND_*`-tagged 2D draw command — the 2D peer of
//! `axiom-render`'s `RenderCommand`.

use axiom_kernel::{Meters, Radians, Ratio};
use axiom_math::{Mat3, Vec2};

use crate::common2d::{Common2d, Shadow2d};
use crate::fill2d::Fill2d;
use crate::handles::TextureId;
use crate::rect::Rect;
use crate::rgba::Rgba;
use crate::sprite_draw2d::SpriteDraw2d;
use crate::text2d::{GlyphRun, TextDraw2d};

/// Circle geometry payload (private; surfaced as a tuple by `as_circle`).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Circle2d {
    center: Vec2,
    radius: Meters,
}

/// Ellipse geometry payload.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Ellipse2d {
    center: Vec2,
    rx: Meters,
    ry: Meters,
    rotation: Radians,
}

/// Line geometry payload (carries its own colour + width, not a `Fill2d`).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Line2d {
    a: Vec2,
    b: Vec2,
    color: Rgba,
    width: Meters,
}

/// Path geometry payload.
#[derive(Debug, Clone, PartialEq)]
struct Path2d {
    points: Vec<Vec2>,
    closed: bool,
}

/// Sprite payload.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Sprite2d {
    texture: TextureId,
    opts: SpriteDraw2d,
}

/// Text payload.
#[derive(Debug, Clone, PartialEq)]
struct Text2d {
    run: GlyphRun,
    opts: TextDraw2d,
}

/// Particle-quad payload (§10.1): a single presentation-only particle resolved
/// to a centred quad with its faded colour. Carries its own colour (like a
/// [`Line2d`]), not a `Fill2d`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ParticleQuad2d {
    center: Vec2,
    size: Meters,
    color: Rgba,
}

/// One backend-neutral 2D draw command.
///
/// A **tagged struct**, not a data-carrying enum: `kind` selects which payload
/// `Option` is `Some`, and the branchless `as_*` accessors gate on it — so there
/// is no `match` over the command shape anywhere. Every command carries its
/// resolved [`Common2d`] (layer / alpha / shadow) and its baked [`Mat3`]
/// transform; nothing un-resolved reaches a backend. Because the payload types
/// are validated newtypes with no cheap default, each kind's geometry rides in
/// its own `Option` (a `None` needs no fabricated filler value) rather than a
/// shared filler default.
#[derive(Debug, Clone, PartialEq)]
pub struct Draw2dCommand {
    kind: u32,
    submission: u32,
    transform: Mat3,
    common: Common2d,
    fill: Option<Fill2d>,
    rect: Option<Rect>,
    circle: Option<Circle2d>,
    ellipse: Option<Ellipse2d>,
    line: Option<Line2d>,
    path: Option<Path2d>,
    sprite: Option<Sprite2d>,
    text: Option<Text2d>,
    particle: Option<ParticleQuad2d>,
}

impl Draw2dCommand {
    /// Filled convex/area shape.
    pub const KIND_RECT: u32 = 1;
    /// Filled circle.
    pub const KIND_CIRCLE: u32 = 2;
    /// Filled (optionally rotated) ellipse.
    pub const KIND_ELLIPSE: u32 = 3;
    /// Stroked line segment.
    pub const KIND_LINE: u32 = 4;
    /// Filled / stroked polyline.
    pub const KIND_PATH: u32 = 5;
    /// Textured sprite quad.
    pub const KIND_SPRITE: u32 = 6;
    /// A run of glyph quads against a baked font atlas.
    pub const KIND_TEXT_GLYPHS: u32 = 7;
    /// A presentation-only particle quad (§10.1).
    pub const KIND_PARTICLE_QUAD: u32 = 8;

    /// The discriminant code (one of the `KIND_*` constants).
    pub const fn kind_code(&self) -> u32 {
        self.kind
    }

    /// The original submit index, before the `(layer, submission)` sort — the
    /// call order a backend can rely on for equal layers.
    pub const fn submission_index(&self) -> u32 {
        self.submission
    }

    /// The baked 2D transform (the composed transform stack at submit time).
    pub const fn transform(&self) -> Mat3 {
        self.transform
    }

    /// The resolved z-order layer.
    pub const fn layer(&self) -> i32 {
        self.common.layer
    }

    /// The resolved alpha.
    pub const fn alpha(&self) -> Ratio {
        self.common.alpha
    }

    /// The resolved shadow, if any.
    pub const fn shadow(&self) -> Option<Shadow2d> {
        self.common.shadow
    }

    /// The resolved fill/stroke style, present for filled shapes
    /// (rect / circle / ellipse / path); `None` for line / sprite / text.
    pub const fn fill(&self) -> Option<Fill2d> {
        self.fill
    }

    /// The `RECT` destination, or `None`.
    pub const fn as_rect(&self) -> Option<Rect> {
        self.rect
    }

    /// The `CIRCLE` `(center, radius)`, or `None`.
    pub fn as_circle(&self) -> Option<(Vec2, Meters)> {
        self.circle.map(|c| (c.center, c.radius))
    }

    /// The `ELLIPSE` `(center, rx, ry, rotation)`, or `None`.
    pub fn as_ellipse(&self) -> Option<(Vec2, Meters, Meters, Radians)> {
        self.ellipse.map(|e| (e.center, e.rx, e.ry, e.rotation))
    }

    /// The `LINE` `(a, b, color, width)`, or `None`.
    pub fn as_line(&self) -> Option<(Vec2, Vec2, Rgba, Meters)> {
        self.line.map(|l| (l.a, l.b, l.color, l.width))
    }

    /// The `PATH` `(points, closed)`, or `None`.
    pub fn as_path(&self) -> Option<(Vec<Vec2>, bool)> {
        self.path.as_ref().map(|p| (p.points.clone(), p.closed))
    }

    /// The `SPRITE` `(texture, opts)`, or `None`.
    pub fn as_sprite(&self) -> Option<(TextureId, SpriteDraw2d)> {
        self.sprite.map(|s| (s.texture, s.opts))
    }

    /// The `TEXT_GLYPHS` `(run, opts)`, or `None`.
    pub fn as_text(&self) -> Option<(GlyphRun, TextDraw2d)> {
        self.text.as_ref().map(|t| (t.run.clone(), t.opts))
    }

    /// The `PARTICLE_QUAD` `(center, size, color)`, or `None`.
    pub fn as_particle(&self) -> Option<(Vec2, Meters, Rgba)> {
        self.particle.map(|p| (p.center, p.size, p.color))
    }
}

/// The per-draw header every command constructor takes: the submit index, the
/// baked transform, and the resolved [`Common2d`]. Carried as one tuple so the
/// `axiom-draw2d` builder (which lives in another crate and owns the submit
/// counter + transform stack) can stamp a command in one call without the
/// constructors growing 8-argument signatures.
type Draw2dHeader = (u32, Mat3, Common2d);

/// Public constructors used by the `axiom-draw2d` builder to assemble this
/// host-owned contract. External *consumers* never build a command directly;
/// they receive them from a [`Draw2dList`]. Each constructor takes a
/// [`Draw2dHeader`] plus only the public value types its kind needs.
impl Draw2dCommand {
    fn empty(kind: u32, header: Draw2dHeader) -> Self {
        let (submission, transform, common) = header;
        Draw2dCommand {
            kind,
            submission,
            transform,
            common,
            fill: None,
            rect: None,
            circle: None,
            ellipse: None,
            line: None,
            path: None,
            sprite: None,
            text: None,
            particle: None,
        }
    }

    /// A filled/stroked rectangle command.
    pub fn rect(header: Draw2dHeader, r: Rect, fill: Fill2d) -> Self {
        Draw2dCommand {
            rect: Some(r),
            fill: Some(fill),
            ..Self::empty(Self::KIND_RECT, header)
        }
    }

    /// A filled/stroked circle command.
    pub fn circle(header: Draw2dHeader, center: Vec2, radius: Meters, fill: Fill2d) -> Self {
        Draw2dCommand {
            circle: Some(Circle2d { center, radius }),
            fill: Some(fill),
            ..Self::empty(Self::KIND_CIRCLE, header)
        }
    }

    /// A filled/stroked (optionally rotated) ellipse command.
    pub fn ellipse(
        header: Draw2dHeader,
        center: Vec2,
        rx: Meters,
        ry: Meters,
        rotation: Radians,
        fill: Fill2d,
    ) -> Self {
        Draw2dCommand {
            ellipse: Some(Ellipse2d {
                center,
                rx,
                ry,
                rotation,
            }),
            fill: Some(fill),
            ..Self::empty(Self::KIND_ELLIPSE, header)
        }
    }

    /// A straight line-segment command (carries its own colour + width).
    pub fn line(header: Draw2dHeader, a: Vec2, b: Vec2, color: Rgba, width: Meters) -> Self {
        Draw2dCommand {
            line: Some(Line2d { a, b, color, width }),
            ..Self::empty(Self::KIND_LINE, header)
        }
    }

    /// A polyline/polygon command.
    pub fn path(header: Draw2dHeader, points: Vec<Vec2>, fill: Fill2d, closed: bool) -> Self {
        Draw2dCommand {
            path: Some(Path2d { points, closed }),
            fill: Some(fill),
            ..Self::empty(Self::KIND_PATH, header)
        }
    }

    /// A textured sprite command.
    pub fn sprite(header: Draw2dHeader, texture: TextureId, opts: SpriteDraw2d) -> Self {
        Draw2dCommand {
            sprite: Some(Sprite2d { texture, opts }),
            ..Self::empty(Self::KIND_SPRITE, header)
        }
    }

    /// A glyph-run text command.
    pub fn text(header: Draw2dHeader, run: GlyphRun, opts: TextDraw2d) -> Self {
        Draw2dCommand {
            text: Some(Text2d { run, opts }),
            ..Self::empty(Self::KIND_TEXT_GLYPHS, header)
        }
    }

    /// A presentation-only particle-quad command (carries its own faded colour).
    pub fn particle_quad(header: Draw2dHeader, center: Vec2, size: Meters, color: Rgba) -> Self {
        Draw2dCommand {
            particle: Some(ParticleQuad2d {
                center,
                size,
                color,
            }),
            ..Self::empty(Self::KIND_PARTICLE_QUAD, header)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handles::FontHandle;
    use crate::text2d::{Glyph2d, TextAlign};

    fn ratio(v: f32) -> Ratio {
        Ratio::new(v).unwrap()
    }

    fn meters(v: f32) -> Meters {
        Meters::new(v).unwrap()
    }

    fn radians(v: f32) -> Radians {
        Radians::new(v).unwrap()
    }

    fn color() -> Rgba {
        Rgba::new(ratio(1.0), ratio(0.0), ratio(0.0), ratio(1.0))
    }

    fn common() -> Common2d {
        Common2d::new(0, ratio(1.0))
    }

    fn fill() -> Fill2d {
        Fill2d::color(color())
    }

    fn rect_geom() -> Rect {
        Rect::new(Vec2::ZERO, Vec2::new(4.0, 3.0))
    }

    fn header(submission: u32) -> (u32, Mat3, Common2d) {
        (submission, Mat3::IDENTITY, common())
    }

    #[test]
    fn rect_command_round_trips_and_reports_none_elsewhere() {
        let c = Draw2dCommand::rect(header(0), rect_geom(), fill());
        assert_eq!(c.kind_code(), Draw2dCommand::KIND_RECT);
        assert_eq!(c.submission_index(), 0);
        assert_eq!(c.transform(), Mat3::IDENTITY);
        assert_eq!(c.layer(), 0);
        assert_eq!(c.alpha(), ratio(1.0));
        assert_eq!(c.shadow(), None);
        assert_eq!(c.fill(), Some(fill()));
        assert_eq!(c.as_rect(), Some(rect_geom()));
        assert_eq!(c.as_circle(), None);
        assert_eq!(c.as_ellipse(), None);
        assert_eq!(c.as_line(), None);
        assert_eq!(c.as_path(), None);
        assert_eq!(c.as_sprite(), None);
        assert_eq!(c.as_text(), None);
        assert_eq!(c.as_particle(), None);
    }

    #[test]
    fn particle_quad_command_round_trips_and_carries_its_own_color() {
        let c = Draw2dCommand::particle_quad(
            header(8),
            Vec2::new(7.0, 8.0),
            meters(0.5),
            color(),
        );
        assert_eq!(c.kind_code(), Draw2dCommand::KIND_PARTICLE_QUAD);
        assert_eq!(c.submission_index(), 8);
        assert_eq!(c.as_particle(), Some((Vec2::new(7.0, 8.0), meters(0.5), color())));
        // A particle carries no Fill2d and is none of the other kinds.
        assert_eq!(c.fill(), None);
        assert_eq!(c.as_rect(), None);
        assert_eq!(c.as_text(), None);
    }

    #[test]
    fn circle_command_round_trips() {
        let c = Draw2dCommand::circle(header(1), Vec2::new(2.0, 3.0), meters(5.0), fill());
        assert_eq!(c.kind_code(), Draw2dCommand::KIND_CIRCLE);
        assert_eq!(c.as_circle(), Some((Vec2::new(2.0, 3.0), meters(5.0))));
        assert_eq!(c.as_rect(), None);
        assert_eq!(c.fill(), Some(fill()));
    }

    #[test]
    fn ellipse_command_round_trips() {
        let c = Draw2dCommand::ellipse(
            header(2),
            Vec2::new(1.0, 1.0),
            meters(4.0),
            meters(2.0),
            radians(0.5),
            fill(),
        );
        assert_eq!(c.kind_code(), Draw2dCommand::KIND_ELLIPSE);
        assert_eq!(
            c.as_ellipse(),
            Some((Vec2::new(1.0, 1.0), meters(4.0), meters(2.0), radians(0.5)))
        );
        assert_eq!(c.as_circle(), None);
    }

    #[test]
    fn line_command_carries_its_own_color_and_has_no_fill() {
        let c = Draw2dCommand::line(
            header(3),
            Vec2::ZERO,
            Vec2::new(10.0, 0.0),
            color(),
            meters(2.0),
        );
        assert_eq!(c.kind_code(), Draw2dCommand::KIND_LINE);
        assert_eq!(
            c.as_line(),
            Some((Vec2::ZERO, Vec2::new(10.0, 0.0), color(), meters(2.0)))
        );
        // A line carries no Fill2d.
        assert_eq!(c.fill(), None);
        assert_eq!(c.as_rect(), None);
    }

    #[test]
    fn path_command_round_trips_points_and_closed() {
        let pts = vec![Vec2::ZERO, Vec2::new(1.0, 0.0), Vec2::new(1.0, 1.0)];
        let c = Draw2dCommand::path(header(4), pts.clone(), fill(), true);
        assert_eq!(c.kind_code(), Draw2dCommand::KIND_PATH);
        assert_eq!(c.as_path(), Some((pts, true)));
        assert_eq!(c.as_circle(), None);
        assert_eq!(c.fill(), Some(fill()));
    }

    #[test]
    fn sprite_command_round_trips() {
        let opts = SpriteDraw2d::new(rect_geom(), Vec2::new(0.5, 0.5), color(), false, true);
        let c = Draw2dCommand::sprite(header(5), TextureId::from_raw(9), opts);
        assert_eq!(c.kind_code(), Draw2dCommand::KIND_SPRITE);
        assert_eq!(c.as_sprite(), Some((TextureId::from_raw(9), opts)));
        // A sprite carries no Fill2d (it tints via its own opts).
        assert_eq!(c.fill(), None);
        assert_eq!(c.as_text(), None);
    }

    #[test]
    fn text_command_round_trips() {
        let run = GlyphRun::new(
            vec![Glyph2d::new(rect_geom(), meters(6.0))],
            meters(12.0),
        );
        let opts = TextDraw2d::new(FontHandle::from_raw(1), color(), TextAlign::LEFT);
        let c = Draw2dCommand::text(header(6), run.clone(), opts);
        assert_eq!(c.kind_code(), Draw2dCommand::KIND_TEXT_GLYPHS);
        assert_eq!(c.as_text(), Some((run, opts)));
        assert_eq!(c.as_sprite(), None);
        assert_eq!(c.fill(), None);
    }

    #[test]
    fn shadow_is_carried_through_common() {
        let s = Shadow2d::new(color(), meters(3.0));
        let c = Draw2dCommand::rect(
            (0, Mat3::IDENTITY, Common2d::with_shadow(2, ratio(0.5), s)),
            rect_geom(),
            fill(),
        );
        assert_eq!(c.layer(), 2);
        assert_eq!(c.alpha(), ratio(0.5));
        assert_eq!(c.shadow(), Some(s));
    }
}
