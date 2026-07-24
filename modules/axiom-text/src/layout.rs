//! The deterministic text layout engine: spans + styles + fonts + config → a
//! [`LaidOutText`]. Left-to-right, one glyph per Unicode scalar (no complex
//! shaping — see [`crate::TextApi`] docs). Every stage is a pure data transform
//! expressed with iterator combinators, so layout is branchless and replayable.

use axiom_host::Pixels;
use axiom_math::Vec2;

use crate::compiled_font::CompiledFont;
use crate::font_registry::FontHandle;
use crate::laid_out_text::{LaidGlyph, LaidOutText, LineMetrics, TextBounds};
use crate::span::TextSpan;
use crate::style::{StyleOverride, TextStyle};
use crate::text_error::{TextError, TextResult};

/// Horizontal alignment within the text box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    /// Flush left.
    #[default]
    Left,
    /// Centred.
    Center,
    /// Flush right.
    Right,
    /// Justified (approximated as left when no shaping is available).
    Justify,
}

impl Align {
    /// The fraction of the free space placed before the line.
    fn factor(self) -> f32 {
        [0.0, 0.5, 1.0, 0.0][self as usize]
    }
    /// The stable byte discriminant.
    pub const fn raw(self) -> u8 {
        [0u8, 1, 2, 3][self as usize]
    }
    /// Recover from a byte.
    pub fn from_raw(raw: u8) -> Option<Align> {
        [Self::Left, Self::Center, Self::Right, Self::Justify]
            .get(raw as usize)
            .copied()
    }
}

/// Vertical alignment within the text box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VerticalAlign {
    /// Top-aligned.
    #[default]
    Top,
    /// Middle-aligned.
    Middle,
    /// Bottom-aligned.
    Bottom,
}

impl VerticalAlign {
    /// The fraction of the free vertical space placed above the text.
    fn factor(self) -> f32 {
        [0.0, 0.5, 1.0][self as usize]
    }
    /// The stable byte discriminant.
    pub const fn raw(self) -> u8 {
        [0u8, 1, 2][self as usize]
    }
    /// Recover from a byte.
    pub fn from_raw(raw: u8) -> Option<VerticalAlign> {
        [Self::Top, Self::Middle, Self::Bottom]
            .get(raw as usize)
            .copied()
    }
}

/// Line-wrapping mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Wrap {
    /// Never soft-wrap (only explicit newlines break).
    #[default]
    None,
    /// Break at word (whitespace) boundaries.
    Word,
    /// Break at any character.
    Char,
}

impl Wrap {
    /// Whether this mode ever soft-wraps.
    fn is_soft(self) -> bool {
        self != Wrap::None
    }
    /// Whether a break opportunity exists after a glyph with `is_space`.
    fn breaks_after(self, is_space: bool) -> bool {
        (self == Wrap::Char) | ((self == Wrap::Word) & is_space)
    }
    /// The stable byte discriminant.
    pub const fn raw(self) -> u8 {
        [0u8, 1, 2][self as usize]
    }
    /// Recover from a byte.
    pub fn from_raw(raw: u8) -> Option<Wrap> {
        [Self::None, Self::Word, Self::Char]
            .get(raw as usize)
            .copied()
    }
}

/// Overflow policy when text exceeds the box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Overflow {
    /// Draw past the box.
    #[default]
    Visible,
    /// Drop glyphs past the box width.
    Clip,
    /// Like clip; a fuller ellipsis pass is a documented future refinement.
    Ellipsis,
}

impl Overflow {
    /// Whether glyphs past the box width are dropped.
    fn clips(self) -> bool {
        self != Overflow::Visible
    }
    /// The stable byte discriminant.
    pub const fn raw(self) -> u8 {
        [0u8, 1, 2][self as usize]
    }
    /// Recover from a byte.
    pub fn from_raw(raw: u8) -> Option<Overflow> {
        [Self::Visible, Self::Clip, Self::Ellipsis]
            .get(raw as usize)
            .copied()
    }
}

/// The layout configuration for a text object.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutConfig {
    /// Fixed box width in pixels; `None` sizes to content.
    pub width: Option<Pixels>,
    /// Fixed box height in pixels; `None` sizes to content.
    pub height: Option<Pixels>,
    /// Horizontal alignment.
    pub align: Align,
    /// Vertical alignment (needs a fixed `height` to have an effect).
    pub vertical_align: VerticalAlign,
    /// Wrapping mode.
    pub wrap: Wrap,
    /// Overflow policy.
    pub overflow: Overflow,
    /// Maximum lines; extra lines are dropped. `None` = unlimited.
    pub max_lines: Option<u32>,
    /// Tab advance in pixels.
    pub tab_width: Pixels,
}

impl Default for LayoutConfig {
    /// The default: content-sized, left/top, no wrapping, visible overflow.
    fn default() -> LayoutConfig {
        LayoutConfig {
            width: None,
            height: None,
            align: Align::Left,
            vertical_align: VerticalAlign::Top,
            wrap: Wrap::None,
            overflow: Overflow::Visible,
            max_lines: None,
            tab_width: Pixels::new(32.0).expect("finite default tab width"),
        }
    }
}

impl LayoutConfig {
    /// Reject negative box dimensions or tab width. (Pixels are finite by
    /// construction; only their sign is checked.)
    pub fn validate(self) -> TextResult<()> {
        let non_negative = |o: Option<Pixels>| o.map_or(true, |v| v.get() >= 0.0);
        (non_negative(self.width) & non_negative(self.height) & (self.tab_width.get() >= 0.0))
            .then_some(())
            .ok_or(TextError::InvalidDimensions)
    }
}

/// One shaped character: its optional drawable glyph plus advance and metrics.
#[derive(Debug, Clone, Copy)]
struct Atom {
    draw: Option<AtomGlyph>,
    advance: f32,
    scale: f32,
    ascent: f32,
    descent: f32,
    line_advance: f32,
    is_space: bool,
    is_newline: bool,
    font: FontHandle,
    glyph_index: u32,
    source_index: u32,
}

/// The drawable part of an atom (absent for spaces/tabs/newlines).
#[derive(Debug, Clone, Copy)]
struct AtomGlyph {
    page: u32,
    uv: [u32; 4],
    bearing: Vec2,
    size: Vec2,
    style: TextStyle,
    overrides: StyleOverride,
}

/// A line under construction: its atoms and their summed advance width.
#[derive(Debug, Default)]
struct LineBuild {
    atoms: Vec<Atom>,
    width: f32,
}

/// The default line metrics for an empty line, from the base style + primary
/// font.
#[derive(Debug, Clone, Copy)]
struct DefaultMetrics {
    ascent: f32,
    descent: f32,
    line_advance: f32,
}

/// Lay out spans into positioned glyphs. `fonts` is the resolved fallback chain
/// (primary first); each glyph records which font supplied it.
pub(crate) fn lay_out(
    spans: &[TextSpan],
    base_style: TextStyle,
    fonts: &[(FontHandle, &CompiledFont)],
    config: &LayoutConfig,
) -> LaidOutText {
    let atoms: Vec<Atom> = spans
        .iter()
        .flat_map(|span| {
            let overrides = span.style;
            let style = overrides.apply(base_style);
            span.text.chars().map(move |ch| (ch, style, overrides))
        })
        .enumerate()
        .map(|(index, (ch, style, overrides))| {
            build_atom(index as u32, ch, style, overrides, fonts, config)
        })
        .collect();

    let defaults = default_metrics(base_style, fonts);
    let lines = break_lines(atoms, config);
    position(lines, config, fonts, defaults)
}

/// Build one atom from a character and its resolved style.
fn build_atom(
    source_index: u32,
    ch: char,
    style: TextStyle,
    overrides: StyleOverride,
    fonts: &[(FontHandle, &CompiledFont)],
    config: &LayoutConfig,
) -> Atom {
    let is_newline = ch == '\n';
    let is_tab = ch == '\t';
    let is_space = ch == ' ';
    let control = is_newline | is_tab;
    let resolved = resolve_glyph(fonts, ch);
    let (font, glyph_index, shaped) = resolved
        .map(|(handle, glyph, font)| {
            (
                handle,
                glyph,
                shape(glyph, style, overrides, font, is_space),
            )
        })
        .unwrap_or((
            FontHandle {
                index: 0,
                generation: 0,
            },
            0,
            Shaped::empty(style),
        ));
    // Newlines/tabs carry no drawable glyph; a tab uses a fixed advance.
    let draw = (!control).then_some(shaped.draw).flatten();
    let advance = [shaped.advance, config.tab_width.get(), 0.0][control_kind(is_newline, is_tab)];
    Atom {
        draw,
        advance,
        scale: shaped.scale,
        ascent: shaped.ascent,
        descent: shaped.descent,
        line_advance: shaped.line_advance,
        is_space,
        is_newline,
        font,
        glyph_index,
        source_index,
    }
}

/// `0` = normal advance, `1` = tab advance, `2` = newline (zero).
fn control_kind(is_newline: bool, is_tab: bool) -> usize {
    (usize::from(is_tab) * 1) + (usize::from(is_newline) * 2)
}

/// The shaped metrics of one glyph at a style.
#[derive(Debug, Clone, Copy)]
struct Shaped {
    draw: Option<AtomGlyph>,
    advance: f32,
    scale: f32,
    ascent: f32,
    descent: f32,
    line_advance: f32,
}

impl Shaped {
    fn empty(style: TextStyle) -> Shaped {
        Shaped {
            draw: None,
            advance: 0.0,
            scale: style.font_size.get() / 16.0,
            ascent: style.font_size.get(),
            descent: style.font_size.get() * 0.25,
            line_advance: style.font_size.get() * style.line_height.get(),
        }
    }
}

/// Compute a glyph's shaped metrics and its atlas rectangle at `style`.
fn shape(
    glyph: u32,
    style: TextStyle,
    overrides: StyleOverride,
    font: &CompiledFont,
    is_space: bool,
) -> Shaped {
    let upem = f32::from(font.metrics.units_per_em);
    let scale = style.font_size.get() / upem;
    // `glyph` was resolved from this font's cmap or replacement, both of which a
    // validated font guarantees have metrics — so this never misses.
    let metric = font
        .metric(glyph)
        .expect("a resolved glyph always has metrics in its font");
    let word = style.word_spacing.get() * f32::from(u8::from(is_space));
    let raster = font
        .nearest_layer(style.font_size.get() as u32)
        .and_then(|layer| layer.raster(glyph));
    let uv = raster.map(|r| [r.x, r.y, r.w, r.h]).unwrap_or([0, 0, 0, 0]);
    let page = raster.map(|r| r.page).unwrap_or(0);
    Shaped {
        draw: Some(AtomGlyph {
            page,
            uv,
            bearing: Vec2::new(
                metric.bearing_x as f32 * scale,
                metric.bearing_y as f32 * scale,
            ),
            size: Vec2::new(metric.width as f32 * scale, metric.height as f32 * scale),
            style,
            overrides,
        }),
        advance: metric.advance as f32 * scale + style.letter_spacing.get() + word,
        scale,
        ascent: font.metrics.ascent as f32 * scale,
        descent: -font.metrics.descent as f32 * scale,
        line_advance: font.metrics.line_advance() as f32 * scale * style.line_height.get(),
    }
}

/// Resolve a character through the fallback chain, then the primary's
/// replacement.
fn resolve_glyph<'a>(
    fonts: &'a [(FontHandle, &'a CompiledFont)],
    ch: char,
) -> Option<(FontHandle, u32, &'a CompiledFont)> {
    fonts
        .iter()
        .find_map(|(handle, font)| {
            font.glyph_for_codepoint(ch as u32)
                .map(|g| (*handle, g, *font))
        })
        .or_else(|| {
            fonts
                .first()
                .and_then(|(handle, font)| font.replacement_glyph().map(|g| (*handle, g, *font)))
        })
}

/// Default line metrics for empty lines, from the base style and primary font.
fn default_metrics(base: TextStyle, fonts: &[(FontHandle, &CompiledFont)]) -> DefaultMetrics {
    fonts
        .first()
        .map(|(_, font)| {
            let scale = base.font_size.get() / f32::from(font.metrics.units_per_em);
            DefaultMetrics {
                ascent: font.metrics.ascent as f32 * scale,
                descent: -font.metrics.descent as f32 * scale,
                line_advance: font.metrics.line_advance() as f32 * scale * base.line_height.get(),
            }
        })
        .unwrap_or(DefaultMetrics {
            ascent: base.font_size.get(),
            descent: base.font_size.get() * 0.25,
            line_advance: base.font_size.get() * base.line_height.get(),
        })
}

/// Greedy line breaking: split atoms into atomic tokens (per wrap mode), then
/// pack tokens into lines, breaking before a token that would overflow.
fn break_lines(atoms: Vec<Atom>, config: &LayoutConfig) -> Vec<LineBuild> {
    let max = config.width.map(|p| p.get()).unwrap_or(f32::INFINITY);
    let wrap = config.wrap;
    // Pack atoms directly, tracking the last break opportunity within the line.
    let (mut lines, last) = atoms.into_iter().fold(
        (Vec::<LineBuild>::new(), LineBuild::default()),
        |(mut lines, mut cur), atom| {
            let overflow = wrap.is_soft()
                & (!cur.atoms.is_empty())
                & wrap.breaks_after(cur.atoms.last().map_or(false, |a| a.is_space))
                & (cur.width + atom.advance > max);
            overflow.then(|| lines.push(core::mem::take(&mut cur)));
            let newline = atom.is_newline;
            let advance = atom.advance;
            (!newline).then(|| {
                cur.atoms.push(atom);
                cur.width += advance;
            });
            newline.then(|| lines.push(core::mem::take(&mut cur)));
            (lines, cur)
        },
    );
    (!last.atoms.is_empty()).then(|| lines.push(last));
    truncate_lines(lines, config.max_lines)
}

/// Drop lines beyond `max_lines` when a cap is set.
fn truncate_lines(lines: Vec<LineBuild>, max_lines: Option<u32>) -> Vec<LineBuild> {
    let keep = max_lines.map_or(lines.len(), |m| (m as usize).min(lines.len()));
    lines.into_iter().take(keep).collect()
}

/// Position every line's atoms into glyphs, computing per-line and overall
/// metrics.
fn position(
    lines: Vec<LineBuild>,
    config: &LayoutConfig,
    fonts: &[(FontHandle, &CompiledFont)],
    defaults: DefaultMetrics,
) -> LaidOutText {
    let total_advance: f32 = lines.iter().map(|l| line_metrics_of(l, defaults).2).sum();
    let vy = config
        .height
        .map(|h| (h.get() - total_advance).max(0.0) * config.vertical_align.factor())
        .unwrap_or(0.0);
    let acc = lines.iter().enumerate().fold(
        Placed {
            cursor_y: vy,
            glyphs: Vec::new(),
            lines: Vec::new(),
            width: 0.0,
        },
        |acc, (line_index, line)| place_line(acc, line, line_index as u32, config, fonts, defaults),
    );
    let first = acc.lines.first().copied();
    let baseline = |l: LineMetrics| l.baseline_y().get();
    let top = |l: LineMetrics| l.top_y().get();
    let height = |l: LineMetrics| l.height().get();
    LaidOutText {
        bounds: TextBounds::new(
            acc.width,
            (acc.cursor_y - vy).max(0.0),
            first.map_or(defaults.ascent, |l| baseline(l) - top(l)),
            first.map_or(defaults.descent, |l| top(l) + height(l) - baseline(l)),
            first.map_or(defaults.ascent, baseline),
            acc.lines.len() as u32,
            acc.glyphs.len() as u32,
        ),
        glyphs: acc.glyphs,
        lines: acc.lines,
    }
}

/// The accumulator threaded through line positioning.
#[derive(Debug)]
struct Placed {
    cursor_y: f32,
    glyphs: Vec<LaidGlyph>,
    lines: Vec<LineMetrics>,
    width: f32,
}

/// The `(ascent, descent, line_advance)` of a line, defaulting for empty lines.
fn line_metrics_of(line: &LineBuild, defaults: DefaultMetrics) -> (f32, f32, f32) {
    let fold_max =
        |pick: fn(&Atom) -> f32, base: f32| line.atoms.iter().map(pick).fold(base, f32::max);
    (
        fold_max(|a| a.ascent, defaults.ascent),
        fold_max(|a| a.descent, defaults.descent),
        fold_max(|a| a.line_advance, defaults.line_advance),
    )
}

/// Position one line's atoms, appending glyphs and a [`LineMetrics`].
fn place_line(
    mut acc: Placed,
    line: &LineBuild,
    line_index: u32,
    config: &LayoutConfig,
    fonts: &[(FontHandle, &CompiledFont)],
    defaults: DefaultMetrics,
) -> Placed {
    let (ascent, _descent, line_advance) = line_metrics_of(line, defaults);
    let baseline = acc.cursor_y + ascent;
    let align_off = config
        .width
        .map(|w| (w.get() - line.width) * config.align.factor())
        .unwrap_or(0.0);
    let start_glyph = acc.glyphs.len() as u32;
    let pen = line.atoms.iter().fold(
        Pen {
            x: align_off,
            prev: None,
            column: 0,
        },
        |pen, atom| {
            advance_atom(
                pen,
                atom,
                baseline,
                line_index,
                config,
                fonts,
                &mut acc.glyphs,
            )
        },
    );
    acc.lines.push(LineMetrics::new(
        start_glyph,
        acc.glyphs.len() as u32 - start_glyph,
        baseline,
        acc.cursor_y,
        line_advance,
        pen.x - align_off,
    ));
    acc.width = acc.width.max(pen.x - align_off);
    acc.cursor_y += line_advance;
    acc
}

/// The pen state threaded across a line's atoms.
#[derive(Debug, Clone, Copy)]
struct Pen {
    x: f32,
    prev: Option<(FontHandle, u32, f32)>,
    column: u32,
}

/// Place one atom's glyph (if any) and advance the pen, applying kerning.
fn advance_atom(
    pen: Pen,
    atom: &Atom,
    baseline: f32,
    line_index: u32,
    config: &LayoutConfig,
    fonts: &[(FontHandle, &CompiledFont)],
    glyphs: &mut Vec<LaidGlyph>,
) -> Pen {
    let kern = pen
        .prev
        .filter(|(pf, _, _)| *pf == atom.font)
        .and_then(|(_, pg, _)| {
            fonts
                .iter()
                .find(|(h, _)| *h == atom.font)
                .map(|(_, f)| f.kern(pg, atom.glyph_index) as f32 * atom.scale)
        })
        .unwrap_or(0.0);
    let x = pen.x + kern;
    let box_w = config.width.map(|p| p.get()).unwrap_or(f32::INFINITY);
    atom.draw.iter().for_each(|g| {
        let right = x + g.bearing.x + g.size.x;
        let visible = (!config.overflow.clips()) | (right <= box_w);
        visible.then(|| {
            glyphs.push(LaidGlyph {
                font: atom.font,
                glyph: atom.glyph_index,
                page: g.page,
                uv: g.uv,
                position: Vec2::new(x + g.bearing.x, baseline - g.bearing.y),
                size: g.size,
                advance: atom.advance,
                style: g.style,
                overrides: g.overrides,
                source_index: atom.source_index,
                line: line_index,
                column: pen.column,
            });
        });
    });
    Pen {
        x: x + atom.advance,
        prev: Some((atom.font, atom.glyph_index, atom.scale)),
        column: pen.column + 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alignment_discriminants_round_trip() {
        [Align::Left, Align::Center, Align::Right, Align::Justify]
            .into_iter()
            .for_each(|a| assert_eq!(Align::from_raw(a.raw()), Some(a)));
        assert_eq!(Align::from_raw(9), None);
        [
            VerticalAlign::Top,
            VerticalAlign::Middle,
            VerticalAlign::Bottom,
        ]
        .into_iter()
        .for_each(|v| assert_eq!(VerticalAlign::from_raw(v.raw()), Some(v)));
        assert_eq!(VerticalAlign::from_raw(9), None);
    }

    #[test]
    fn lay_out_with_no_fonts_falls_back_to_default_metrics() {
        // Exercises the empty-font-chain arms (no glyph resolves; empty-line
        // default metrics) that the facade never hits because it always supplies
        // the default font.
        let laid = lay_out(
            &[TextSpan::plain("A B")],
            TextStyle::default(),
            &[],
            &LayoutConfig::default(),
        );
        assert_eq!(laid.glyph_count(), 0, "no fonts → no drawable glyphs");
        assert_eq!(laid.lines().len(), 1, "still one line");
        assert!(
            laid.bounds().height().get() > 0.0,
            "default metrics give height"
        );
    }

    #[test]
    fn wrap_and_overflow_discriminants_round_trip() {
        [Wrap::None, Wrap::Word, Wrap::Char]
            .into_iter()
            .for_each(|w| assert_eq!(Wrap::from_raw(w.raw()), Some(w)));
        assert_eq!(Wrap::from_raw(9), None);
        [Overflow::Visible, Overflow::Clip, Overflow::Ellipsis]
            .into_iter()
            .for_each(|o| assert_eq!(Overflow::from_raw(o.raw()), Some(o)));
        assert_eq!(Overflow::from_raw(9), None);
    }
}
