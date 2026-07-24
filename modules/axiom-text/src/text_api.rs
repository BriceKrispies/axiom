//! [`TextApi`]: the text module's single behavioral facade.
//!
//! One primitive, `Text`, configured many ways. `TextApi` owns the font registry
//! and the retained text objects, and provides every deterministic operation:
//! registering compiled fonts, creating/mutating/removing text, measuring,
//! bounds/hit queries, deterministic snapshots, and backend-neutral glyph
//! batches evaluated at an explicit tick. It never reads a wall clock or a random
//! source; animated behaviour is a pure function of the tick you pass.

use axiom_math::Vec2;

use crate::compiled_font::CompiledFont;
use crate::effect::{evaluate, TextEffect};
use crate::fallback_font::default_font;
use crate::font_registry::{FontHandle, FontRegistry};
use crate::glyph_batch::{GlyphBatch, GlyphInstance};
use crate::laid_out_text::{LaidOutText, LineMetrics, TextBounds};
use crate::layout::{lay_out, LayoutConfig};
use crate::placement::TextPlacement;
use crate::span::TextSpan;
use crate::style::TextStyle;
use crate::text_error::{TextError, TextResult};
use crate::text_limits::TextLimits;
use crate::text_record::{TextHandle, TextRecord};
use crate::text_snapshot::TextSnapshot;

/// One text store slot: a generation counter and an optional record.
#[derive(Debug, Clone, Default)]
struct TextSlot {
    generation: u32,
    record: Option<TextRecord>,
}

/// The stateful text facade: a font registry plus generational text objects.
#[derive(Debug, Clone)]
pub struct TextApi {
    fonts: FontRegistry,
    texts: Vec<TextSlot>,
    limits: TextLimits,
    default_font: FontHandle,
}

impl Default for TextApi {
    fn default() -> Self {
        Self::new()
    }
}

impl TextApi {
    /// Create a text system with default limits and the engine default font
    /// already registered and pinned (it can never be unregistered).
    pub fn new() -> TextApi {
        Self::with_limits(TextLimits::DEFAULT)
    }

    /// Create a text system with explicit safety limits.
    pub fn with_limits(limits: TextLimits) -> TextApi {
        let mut fonts = FontRegistry::default();
        let default_font = fonts
            .register(default_font(), limits.max_fonts.max(1))
            .expect("the default font always registers into a fresh registry");
        fonts.retain(default_font);
        TextApi {
            fonts,
            texts: Vec::new(),
            limits,
            default_font,
        }
    }

    /// The engine default font handle (the vendored bitmap face).
    pub fn default_font(&self) -> FontHandle {
        self.default_font
    }

    // --- fonts -------------------------------------------------------------

    /// Register a compiled `.axfont` from its bytes. Fails with the decode error,
    /// `AtlasPackingOverflow` past the page cap, or `CapacityExceeded` past the
    /// font cap.
    pub fn register_font(&mut self, bytes: &[u8]) -> TextResult<FontHandle> {
        CompiledFont::decode(bytes)
            .and_then(|font| {
                let pages = font
                    .size_layers
                    .iter()
                    .map(|l| l.pages.len() as u32)
                    .fold(0, u32::max);
                (pages <= self.limits.max_atlas_pages)
                    .then_some(font)
                    .ok_or(TextError::AtlasPackingOverflow)
            })
            .and_then(|font| self.fonts.register(font, self.limits.max_fonts))
    }

    /// Unregister a font not currently referenced by live text.
    pub fn unregister_font(&mut self, handle: FontHandle) -> TextResult<()> {
        self.fonts.unregister(handle)
    }

    /// Resolve the first registered font advertising `family`, if any.
    pub fn font_by_family(&self, family: &str) -> Option<FontHandle> {
        self.fonts.find_by_family(family)
    }

    // --- text lifecycle ----------------------------------------------------

    /// Create text from a plain UTF-8 string, using the default font.
    pub fn text(&mut self, content: &str) -> TextResult<TextHandle> {
        self.create(vec![TextSpan::plain(content)])
    }

    /// Create text from structured rich spans, using the default font.
    pub fn text_rich(&mut self, spans: Vec<TextSpan>) -> TextResult<TextHandle> {
        self.create(spans)
    }

    /// Remove a text object, releasing its font references.
    pub fn remove(&mut self, handle: TextHandle) -> TextResult<()> {
        self.texts
            .get_mut(handle.index as usize)
            .filter(|slot| (slot.generation == handle.generation) & slot.record.is_some())
            .ok_or(TextError::StaleTextHandle)
            .map(|slot| {
                let fonts = slot
                    .record
                    .take()
                    .map(|record| record.fonts)
                    .expect("slot was just filtered to Some");
                slot.generation += 1;
                fonts
            })
            .map(|fonts| {
                fonts
                    .into_iter()
                    .for_each(|handle| self.fonts.release(handle))
            })
    }
}

/// Mutation operations (split into its own `impl` to keep each block small).
impl TextApi {
    /// Replace the text content (plain).
    pub fn set_text(&mut self, handle: TextHandle, content: &str) -> TextResult<()> {
        self.set_spans(handle, vec![TextSpan::plain(content)])
    }

    /// Replace the text content with rich spans.
    pub fn set_spans(&mut self, handle: TextHandle, spans: Vec<TextSpan>) -> TextResult<()> {
        self.check_content(&spans)
            .and_then(|()| self.with_record_mut(handle, |record| record.set_spans(spans)))
    }

    /// Replace the layout-affecting base style.
    pub fn set_style(&mut self, handle: TextHandle, style: TextStyle) -> TextResult<()> {
        style
            .validate()
            .and_then(|()| self.with_record_mut(handle, |record| record.set_base_style(style)))
    }

    /// Change only the fill colour (does not re-lay-out).
    pub fn set_color(&mut self, handle: TextHandle, color: crate::color::Rgba) -> TextResult<()> {
        color
            .validate()
            .and_then(|()| self.with_record_mut(handle, |record| record.set_color(color)))
    }

    /// Change only opacity (does not re-lay-out).
    pub fn set_opacity(
        &mut self,
        handle: TextHandle,
        opacity: axiom_kernel::Ratio,
    ) -> TextResult<()> {
        ((opacity.get() >= 0.0) & (opacity.get() <= 1.0))
            .then_some(())
            .ok_or(TextError::InvalidOpacity)
            .and_then(|()| self.with_record_mut(handle, |record| record.set_opacity(opacity)))
    }

    /// Replace the layout configuration.
    pub fn set_layout(&mut self, handle: TextHandle, config: LayoutConfig) -> TextResult<()> {
        config
            .validate()
            .and_then(|()| self.with_record_mut(handle, |record| record.set_config(config)))
    }

    /// Replace the placement (screen or world).
    pub fn set_placement(
        &mut self,
        handle: TextHandle,
        placement: TextPlacement,
    ) -> TextResult<()> {
        placement
            .validate()
            .and_then(|()| self.with_record_mut(handle, |record| record.set_placement(placement)))
    }

    /// Show or hide the text.
    pub fn set_visible(&mut self, handle: TextHandle, visible: bool) -> TextResult<()> {
        self.with_record_mut(handle, |record| record.set_visible(visible))
    }

    /// Set the static/dynamic update-mode hint.
    pub fn set_update_mode(
        &mut self,
        handle: TextHandle,
        mode: crate::text_record::UpdateMode,
    ) -> TextResult<()> {
        self.with_record_mut(handle, |record| record.set_update_mode(mode))
    }

    /// The current update-mode hint.
    pub fn update_mode(&self, handle: TextHandle) -> TextResult<crate::text_record::UpdateMode> {
        self.with_record(handle, |record| record.update_mode)
    }

    /// Replace the effect stack. (Effect parameters are typed finite values, so
    /// there is nothing further to validate.)
    pub fn set_effects(&mut self, handle: TextHandle, effects: Vec<TextEffect>) -> TextResult<()> {
        self.with_record_mut(handle, |record| record.set_effects(effects))
    }

    /// Point the text at a font chain (primary first); the default font is always
    /// appended as a final fallback.
    pub fn set_fonts(&mut self, handle: TextHandle, fonts: Vec<FontHandle>) -> TextResult<()> {
        fonts
            .iter()
            .try_for_each(|handle| self.fonts.require(*handle))
            .and_then(|()| {
                let previous = self.with_record(handle, |record| record.fonts.clone());
                previous.map(|previous| {
                    fonts.iter().for_each(|handle| self.fonts.retain(*handle));
                    previous
                        .into_iter()
                        .for_each(|handle| self.fonts.release(handle));
                    let _ = self.with_record_mut(handle, |record| record.set_fonts(fonts));
                })
            })
    }
}

/// Query + snapshot operations, plus internals.
impl TextApi {
    /// Measure text without creating a persistent object.
    pub fn measure(
        &self,
        spans: &[TextSpan],
        style: TextStyle,
        config: LayoutConfig,
    ) -> TextResult<TextBounds> {
        style.validate().and_then(|()| config.validate()).map(|()| {
            let font = self
                .fonts
                .get(self.default_font)
                .expect("the default font is always registered and pinned");
            let chain = [(self.default_font, font)];
            lay_out(spans, style, &chain, &config).bounds()
        })
    }

    /// The overall bounds of a laid-out text.
    pub fn bounds(&mut self, handle: TextHandle) -> TextResult<TextBounds> {
        self.read_layout(handle, LaidOutText::bounds)
    }

    /// The per-line metrics of a laid-out text.
    pub fn line_metrics(&mut self, handle: TextHandle) -> TextResult<Vec<LineMetrics>> {
        self.read_layout(handle, |laid| laid.lines().to_vec())
    }

    /// The bounding rectangle `(x, y, w, h)` of one glyph, in pixels.
    pub fn glyph_bounds(
        &mut self,
        handle: TextHandle,
        glyph: usize,
    ) -> TextResult<
        Option<(
            axiom_host::Pixels,
            axiom_host::Pixels,
            axiom_host::Pixels,
            axiom_host::Pixels,
        )>,
    > {
        self.read_layout(handle, |laid| laid.glyph_bounds(glyph))
    }

    /// Hit test: the source `char` index nearest a text-local point.
    pub fn hit(&mut self, handle: TextHandle, point: Vec2) -> TextResult<Option<u32>> {
        self.read_layout(handle, |laid| laid.source_at(point))
    }

    /// The backend-neutral glyph batch, with effects evaluated at `tick`.
    pub fn batch(&mut self, handle: TextHandle, tick: u64) -> TextResult<GlyphBatch> {
        self.ensure_layout(handle)
            .and_then(|()| self.with_record(handle, |record| build_batch(record, tick)))
            .and_then(|batch| {
                (batch.glyphs.len() as u32 <= self.limits.max_glyphs)
                    .then_some(batch)
                    .ok_or(TextError::CapacityExceeded)
            })
    }

    /// A deterministic snapshot of the text at `tick`.
    pub fn snapshot(&mut self, handle: TextHandle, tick: u64) -> TextResult<TextSnapshot> {
        self.batch(handle, tick)
            .map(|batch| TextSnapshot::of(&batch, tick))
    }

    /// Drop all cached layouts, forcing re-layout on next query.
    pub fn clear_caches(&mut self) {
        self.texts.iter_mut().for_each(|slot| {
            slot.record.iter_mut().for_each(|record| {
                record.cache = None;
                record.dirty = crate::text_record::DirtyFlags::FRESH;
            });
        });
    }

    // --- internals ---------------------------------------------------------

    fn live_texts(&self) -> u32 {
        self.texts
            .iter()
            .filter(|slot| slot.record.is_some())
            .count() as u32
    }

    fn check_content(&self, spans: &[TextSpan]) -> TextResult<()> {
        let chars: usize = spans.iter().map(TextSpan::char_count).sum();
        ((chars as u32 <= self.limits.max_chars_per_text)
            & (spans.len() as u32 <= self.limits.max_spans))
            .then_some(())
            .ok_or(TextError::CapacityExceeded)
    }

    fn create(&mut self, spans: Vec<TextSpan>) -> TextResult<TextHandle> {
        self.check_content(&spans)
            .and_then(|()| {
                (self.live_texts() < self.limits.max_text_objects)
                    .then_some(())
                    .ok_or(TextError::CapacityExceeded)
            })
            .map(|()| {
                let record = TextRecord::new(vec![self.default_font], spans);
                self.fonts.retain(self.default_font);
                let free = self.texts.iter().position(|slot| slot.record.is_none());
                let index = free.unwrap_or(self.texts.len());
                free.is_none().then(|| self.texts.push(TextSlot::default()));
                let slot = &mut self.texts[index];
                slot.record = Some(record);
                TextHandle {
                    index: index as u32,
                    generation: slot.generation,
                }
            })
    }

    fn with_record<R>(
        &self,
        handle: TextHandle,
        f: impl FnOnce(&TextRecord) -> R,
    ) -> TextResult<R> {
        self.texts
            .get(handle.index as usize)
            .filter(|slot| slot.generation == handle.generation)
            .and_then(|slot| slot.record.as_ref())
            .map(f)
            .ok_or(TextError::StaleTextHandle)
    }

    fn with_record_mut<R>(
        &mut self,
        handle: TextHandle,
        f: impl FnOnce(&mut TextRecord) -> R,
    ) -> TextResult<R> {
        self.texts
            .get_mut(handle.index as usize)
            .filter(|slot| slot.generation == handle.generation)
            .and_then(|slot| slot.record.as_mut())
            .map(f)
            .ok_or(TextError::StaleTextHandle)
    }

    fn require_text(&self, handle: TextHandle) -> TextResult<()> {
        self.with_record(handle, |_| ())
    }

    /// Ensure the layout is current, then read it. The cache is always populated
    /// by [`Self::ensure_layout`], so the read never misses.
    fn read_layout<R>(
        &mut self,
        handle: TextHandle,
        f: impl FnOnce(&LaidOutText) -> R,
    ) -> TextResult<R> {
        self.ensure_layout(handle).and_then(|()| {
            self.with_record(handle, |record| {
                f(record
                    .cache
                    .as_ref()
                    .expect("ensure_layout populated the cache"))
            })
        })
    }

    fn ensure_layout(&mut self, handle: TextHandle) -> TextResult<()> {
        self.require_text(handle).map(|()| {
            let fonts = &self.fonts;
            let default = self.default_font;
            let slot = &mut self.texts[handle.index as usize];
            slot.record.iter_mut().for_each(|record| {
                let recompute = record.dirty.needs_layout() | record.cache.is_none();
                recompute.then(|| {
                    let mut chain: Vec<(FontHandle, &CompiledFont)> = record
                        .fonts
                        .iter()
                        .filter_map(|handle| fonts.get(*handle).map(|font| (*handle, font)))
                        .collect();
                    (!record.fonts.contains(&default)).then(|| {
                        fonts.get(default).map(|font| chain.push((default, font)));
                    });
                    let laid = lay_out(&record.spans, record.base_style, &chain, &record.config);
                    record.cache = Some(laid);
                    record.dirty.content = false;
                    record.dirty.font = false;
                    record.dirty.glyph = false;
                    record.dirty.line = false;
                });
            });
        })
    }
}

/// Build the neutral glyph batch from a record's cached layout, applying effects
/// (evaluated at `tick`) and visibility.
fn build_batch(record: &TextRecord, tick: u64) -> GlyphBatch {
    let total = record.char_count() as u32;
    let base = record.base_style;
    let laid = record
        .cache
        .as_ref()
        .expect("ensure_layout populated the cache");
    let glyphs = record
        .visible
        .then(|| {
            laid.glyphs
                .iter()
                .filter_map(|g| {
                    let m = evaluate(&record.effects, g.source_index, total, tick);
                    // Visual fields resolve against the CURRENT base style + the
                    // span override, so colour/opacity changes need no re-layout.
                    let color = g.overrides.color.unwrap_or(base.color);
                    let opacity = g.overrides.opacity.unwrap_or(base.opacity);
                    m.visible.then(|| GlyphInstance {
                        font: g.font,
                        page: g.page,
                        glyph: g.glyph,
                        source_start: g.source_index,
                        source_len: 1,
                        position: Vec2::new(g.position.x + m.offset.x, g.position.y + m.offset.y),
                        size: g.size,
                        uv_x: g.uv[0],
                        uv_y: g.uv[1],
                        uv_w: g.uv[2],
                        uv_h: g.uv[3],
                        color: color.with_opacity(axiom_kernel::Ratio::finite_or_zero(
                            opacity.get() * m.alpha,
                        )),
                        outline_width: g.overrides.outline_width.unwrap_or(base.outline_width),
                        outline_color: g.overrides.outline_color.unwrap_or(base.outline_color),
                        shadow_offset: g.overrides.shadow_offset.unwrap_or(base.shadow_offset),
                        shadow_color: g.overrides.shadow_color.unwrap_or(base.shadow_color),
                        order: (u64::from(g.line) << 20) | u64::from(g.column),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    GlyphBatch {
        placement: record.placement,
        glyphs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Rgba;
    use crate::effect::EffectKind;
    use crate::layout::{Align, VerticalAlign, Wrap};
    use crate::placement::Space;
    use crate::style::StyleOverride;
    use crate::text_record::UpdateMode;
    use axiom_host::Pixels;
    use axiom_kernel::Ratio;
    use axiom_math::{Transform, Vec2};

    fn px(v: f32) -> Pixels {
        Pixels::new(v).unwrap()
    }
    fn rat(v: f32) -> Ratio {
        Ratio::finite_or_zero(v)
    }
    fn shake(amplitude: f32, seed: u32) -> TextEffect {
        TextEffect {
            kind: EffectKind::Shake,
            start_tick: 0,
            duration: 0,
            speed: rat(0.0),
            amplitude: px(amplitude),
            seed,
        }
    }

    #[test]
    fn hello_world_renders_with_the_default_font() {
        let mut api = TextApi::new();
        let h = api.text("HELLO, WORLD").unwrap();
        let batch = api.batch(h, 0).unwrap();
        assert_eq!(batch.glyphs.len(), 12, "one glyph per char");
        assert_eq!(
            batch.placement.space,
            Space::Screen,
            "default is screen space"
        );
        // Default fill is opaque white; glyphs carry real atlas rects.
        assert_eq!(batch.glyphs[0].color, Rgba::WHITE);
        assert!(batch.glyphs.iter().all(|g| g.size.x > 0.0));
        assert!(batch.glyphs.iter().any(|g| g.uv_w > 0));
    }

    #[test]
    fn rich_spans_carry_per_span_colour() {
        let mut api = TextApi::new();
        let h = api
            .text_rich(vec![
                TextSpan::plain("SCORE "),
                TextSpan::styled(
                    "1250",
                    StyleOverride {
                        color: Some(Rgba::from_rgba8(255, 212, 74, 255)),
                        ..Default::default()
                    },
                ),
            ])
            .unwrap();
        let batch = api.batch(h, 0).unwrap();
        // First glyph (of "SCORE ") is white; the digits are gold.
        assert_eq!(batch.glyphs[0].color, Rgba::WHITE);
        assert_eq!(
            batch.glyphs.last().unwrap().color,
            Rgba::from_rgba8(255, 212, 74, 255)
        );
    }

    #[test]
    fn set_color_recolours_without_relayout() {
        let mut api = TextApi::new();
        let h = api.text("HI").unwrap();
        let before = api.bounds(h).unwrap();
        api.set_color(h, Rgba::BLACK).unwrap();
        let after = api.bounds(h).unwrap();
        assert_eq!(before, after, "colour change does not move glyphs");
        assert_eq!(api.batch(h, 0).unwrap().glyphs[0].color, Rgba::BLACK);
    }

    #[test]
    fn measure_needs_no_object() {
        let api = TextApi::new();
        let bounds = api
            .measure(
                &[TextSpan::plain("HI")],
                TextStyle::default(),
                LayoutConfig::default(),
            )
            .unwrap();
        assert!(bounds.width().get() > 0.0);
        assert_eq!(bounds.glyph_count(), 2);
    }

    #[test]
    fn word_wrap_produces_multiple_lines() {
        let mut api = TextApi::new();
        let h = api.text("AAAA BBBB CCCC").unwrap();
        api.set_layout(
            h,
            LayoutConfig {
                width: Some(px(60.0)),
                wrap: Wrap::Word,
                ..LayoutConfig::default()
            },
        )
        .unwrap();
        let lines = api.line_metrics(h).unwrap();
        assert!(lines.len() > 1, "narrow box wraps into multiple lines");
    }

    #[test]
    fn newline_breaks_lines_and_alignment_shifts_right() {
        let mut api = TextApi::new();
        let h = api.text("A\nBBBB").unwrap();
        assert_eq!(api.line_metrics(h).unwrap().len(), 2);
        api.set_layout(
            h,
            LayoutConfig {
                width: Some(px(200.0)),
                align: Align::Right,
                ..LayoutConfig::default()
            },
        )
        .unwrap();
        let (x, _, _, _) = api.glyph_bounds(h, 0).unwrap().unwrap();
        assert!(x.get() > 0.0, "right alignment pushes the first glyph in");
    }

    #[test]
    fn snapshots_are_deterministic_and_effects_are_tick_driven() {
        let mut api = TextApi::new();
        let h = api.text("READY").unwrap();
        assert_eq!(api.snapshot(h, 0).unwrap(), api.snapshot(h, 0).unwrap());
        api.set_effects(h, vec![TextEffect::reveal(rat(1.0))])
            .unwrap();
        // Reveal shows fewer glyphs early on.
        assert!(api.batch(h, 1).unwrap().glyphs.len() < api.batch(h, 100).unwrap().glyphs.len());
        assert_ne!(api.snapshot(h, 1).unwrap(), api.snapshot(h, 100).unwrap());
    }

    #[test]
    fn shake_effect_is_replayable() {
        let mut api = TextApi::new();
        let h = api.text("HI").unwrap();
        api.set_effects(h, vec![shake(3.0, 5)]).unwrap();
        assert_eq!(api.snapshot(h, 7).unwrap(), api.snapshot(h, 7).unwrap());
    }

    #[test]
    fn visibility_hides_the_batch() {
        let mut api = TextApi::new();
        let h = api.text("HI").unwrap();
        api.set_visible(h, false).unwrap();
        assert!(api.batch(h, 0).unwrap().glyphs.is_empty());
    }

    #[test]
    fn world_placement_round_trips_through_the_record() {
        let mut api = TextApi::new();
        let h = api.text("HI").unwrap();
        api.set_placement(h, TextPlacement::at_world(Transform::IDENTITY))
            .unwrap();
        assert_eq!(api.batch(h, 0).unwrap().placement.space, Space::World);
    }

    #[test]
    fn hit_testing_maps_a_point_to_a_char() {
        let mut api = TextApi::new();
        let h = api.text("HI").unwrap();
        assert_eq!(api.hit(h, Vec2::new(1000.0, 0.0)).unwrap(), Some(1));
    }

    #[test]
    fn remove_invalidates_the_handle() {
        let mut api = TextApi::new();
        let h = api.text("HI").unwrap();
        api.remove(h).unwrap();
        assert_eq!(api.bounds(h), Err(TextError::StaleTextHandle));
        assert_eq!(api.remove(h), Err(TextError::StaleTextHandle));
    }

    #[test]
    fn object_capacity_is_enforced() {
        let mut api = TextApi::with_limits(TextLimits {
            max_text_objects: 1,
            ..TextLimits::DEFAULT
        });
        api.text("A").unwrap();
        assert_eq!(api.text("B"), Err(TextError::CapacityExceeded));
    }

    #[test]
    fn content_length_cap_is_enforced() {
        let mut api = TextApi::with_limits(TextLimits {
            max_chars_per_text: 2,
            ..TextLimits::DEFAULT
        });
        assert_eq!(api.text("TOOLONG"), Err(TextError::CapacityExceeded));
    }

    #[test]
    fn register_and_use_a_custom_font_then_unregister() {
        let mut api = TextApi::new();
        let bytes = default_font().encode();
        let handle = api.register_font(&bytes).unwrap();
        assert_eq!(
            api.font_by_family("Axiom Default"),
            Some(api.default_font())
        );
        let h = api.text("HI").unwrap();
        api.set_fonts(h, vec![handle]).unwrap();
        assert!(!api.batch(h, 0).unwrap().glyphs.is_empty());
        // Still referenced by the text → cannot unregister until re-pointed.
        assert_eq!(
            api.unregister_font(handle),
            Err(TextError::FontStillReferenced)
        );
        api.set_fonts(h, vec![api.default_font()]).unwrap();
        assert_eq!(api.unregister_font(handle), Ok(()));
    }

    #[test]
    fn malformed_font_bytes_are_rejected() {
        let mut api = TextApi::new();
        assert_eq!(api.register_font(&[0, 1, 2]), Err(TextError::MalformedFont));
    }

    #[test]
    fn invalid_style_and_placement_are_rejected() {
        let mut api = TextApi::new();
        let h = api.text("HI").unwrap();
        assert_eq!(
            api.set_style(
                h,
                TextStyle {
                    font_size: px(-1.0),
                    ..TextStyle::default()
                }
            ),
            Err(TextError::InvalidFontSize)
        );
        assert_eq!(api.set_opacity(h, rat(2.0)), Err(TextError::InvalidOpacity));
        let mut bad = TextPlacement::default();
        bad.screen.scale = rat(0.0);
        assert_eq!(api.set_placement(h, bad), Err(TextError::InvalidDimensions));
    }

    #[test]
    fn update_mode_and_clear_caches_work() {
        let mut api = TextApi::new();
        let h = api.text("HI").unwrap();
        api.set_update_mode(h, UpdateMode::Dynamic).unwrap();
        assert_eq!(api.update_mode(h).unwrap(), UpdateMode::Dynamic);
        let before = api.bounds(h).unwrap();
        api.clear_caches();
        assert_eq!(
            api.bounds(h).unwrap(),
            before,
            "re-layout after cache clear is identical"
        );
    }

    #[test]
    fn glyph_count_cap_is_enforced() {
        let mut api = TextApi::with_limits(TextLimits {
            max_glyphs: 1,
            ..TextLimits::DEFAULT
        });
        let h = api.text("HI").unwrap();
        assert_eq!(api.batch(h, 0), Err(TextError::CapacityExceeded));
    }

    #[test]
    fn vertical_align_offsets_within_a_fixed_height() {
        let mut api = TextApi::new();
        let h = api.text("HI").unwrap();
        api.set_layout(
            h,
            LayoutConfig {
                height: Some(px(200.0)),
                vertical_align: VerticalAlign::Bottom,
                ..LayoutConfig::default()
            },
        )
        .unwrap();
        // Bottom alignment pushes the single line's glyphs well down the box.
        let (_, y, _, _) = api.glyph_bounds(h, 0).unwrap().unwrap();
        assert!(y.get() > 100.0, "bottom valign moves the line down");
    }

    #[test]
    fn default_constructor_and_set_text_replace_content() {
        let mut api = TextApi::default();
        let h = api.text("A").unwrap();
        assert_eq!(api.batch(h, 0).unwrap().glyphs.len(), 1);
        api.set_text(h, "BCD").unwrap();
        assert_eq!(api.batch(h, 0).unwrap().glyphs.len(), 3);
    }

    #[test]
    fn max_lines_truncates() {
        let mut api = TextApi::new();
        let h = api.text("A\nB\nC\nD").unwrap();
        assert_eq!(api.line_metrics(h).unwrap().len(), 4);
        api.set_layout(
            h,
            LayoutConfig {
                max_lines: Some(2),
                ..LayoutConfig::default()
            },
        )
        .unwrap();
        assert_eq!(api.line_metrics(h).unwrap().len(), 2, "capped to max_lines");
    }

    #[test]
    fn valid_opacity_dims_the_fill() {
        let mut api = TextApi::new();
        let h = api.text("HI").unwrap();
        api.set_opacity(h, rat(0.5)).unwrap();
        assert_eq!(api.batch(h, 0).unwrap().glyphs[0].color.alpha().get(), 0.5);
    }

    #[test]
    fn a_removed_slot_is_reused_by_the_next_text() {
        let mut api = TextApi::new();
        let first = api.text("A").unwrap();
        api.remove(first).unwrap();
        // The freed slot is scanned and reused (with a bumped generation).
        let second = api.text("B").unwrap();
        assert_eq!(second.index, first.index, "slot index reused");
        assert_ne!(second.generation, first.generation, "generation advanced");
        assert_eq!(api.bounds(first), Err(TextError::StaleTextHandle));
        assert_eq!(api.batch(second, 0).unwrap().glyphs.len(), 1);
    }

    /// Composite the real glyph batch against the real atlas into an ASCII raster
    /// — a concrete proof the pipeline produces *visible* text, not just data.
    #[test]
    fn hello_world_composites_to_visible_ink() {
        let mut api = TextApi::new();
        let h = api.text("HELLO, WORLD").unwrap();
        // Size 8 = native bitmap scale (1 atlas px per glyph px), so the raster is
        // the font's true shape.
        api.set_style(
            h,
            TextStyle {
                font_size: px(8.0),
                ..TextStyle::default()
            },
        )
        .unwrap();
        let batch = api.batch(h, 0).unwrap();
        let font = default_font();
        let page = &font.size_layers[0].pages[0];

        let width = (batch
            .glyphs
            .iter()
            .map(|g| (g.position.x + g.size.x) as usize)
            .max()
            .unwrap_or(0))
            + 1;
        let mut grid = vec![b' '; width * 9];
        batch.glyphs.iter().for_each(|g| {
            (0..g.uv_h).for_each(|dy| {
                (0..g.uv_w).for_each(|dx| {
                    let cov = page.pixels[((g.uv_y + dy) * page.width + g.uv_x + dx) as usize];
                    let px = g.position.x as usize + dx as usize;
                    let py = g.position.y as usize + dy as usize;
                    ((cov > 127) & (px < width) & (py < 9)).then(|| grid[py * width + px] = b'#');
                });
            });
        });
        let art: String = grid
            .chunks(width)
            .map(|row| String::from_utf8_lossy(row).into_owned())
            .collect::<Vec<_>>()
            .join("\n");
        // The 'H' opens with two verticals bridged by a bar — proof of real,
        // legible glyph shapes (not just non-zero ink).
        assert!(art.contains("#   #"), "H/W verticals present in the raster");
        assert!(art.contains("#####"), "a full bar (E/L/bridge) is rendered");
        let ink = grid.iter().filter(|&&c| c == b'#').count();
        assert!(ink > 60, "expected legible ink, got {ink} lit pixels");
    }
}
