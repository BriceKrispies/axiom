//! **`?rview=`** — the intermediate-buffer inspector that replaces the whole
//! composite, and the one other query flag `index.js` reads.
//!
//! ```js
//! this.debugView = new URLSearchParams(location.search).get('rview') || null;
//! this._noCascadeCull = /[?&]owNoCascadeCull=1/.test(location.search);
//! ```
//!
//! Two things about that first line survive into the port and are easy to lose.
//! `|| null` is JavaScript falsiness, so `?rview=` with an **empty** value is
//! `null` and the debug arm does not run at all. And `_renderDebug`'s lookup
//! ends `?? map.color`, so an *unrecognised* name is not an error and not a
//! no-op — it selects the finished colour buffer.
//!
//! # The mode number is a table index into the debug shader
//!
//! ```js
//! const map = {
//!   ao:        [this.aoTexture,        0],
//!   normal:    [gb.normalTexture,      1],
//!   velocity:  [gb.velocityTexture,    2],
//!   depth:     [gb.depthTexture,       3],
//!   ssr:       [this.ssr?.texture,     4],
//!   ssrmask:   [this.ssr?.texture,     5],
//!   contact:   [this.contact?.texture, 0],
//!   bloom:     [this.bloom?.texture,   4],
//!   view:      [this.viewRt?.texture,  4],
//!   viewalpha: [this.viewRt?.texture,  5],
//!   color:     [color,                 4],
//! };
//! ```
//!
//! `uMode` selects a channel-unpacking arm in `createDebug()`'s fragment
//! shader, so the integers are a contract with that shader and **not** a
//! renumbering of the view list: three different views share mode `4`, two
//! share `5`, and `contact` shares mode `0` with `ao` because both are a single
//! visibility scalar in `.r`. An "obvious tidy-up" that renumbered them
//! sequentially would silently change how eight of the eleven views decode.
//!
//! # A missing source is not an error
//!
//! `u.tSrc.value = entry[0] ?? color`. Selecting `ssr` on a tier that builds no
//! SSR pass shows the finished frame rather than a black screen — the source's
//! optional chaining (`this.ssr?.texture`) yields `undefined` and the `??`
//! catches it. [`DebugSource::available_in`] is that test, made checkable.

use super::pipeline::FramePipeline;

/// The eleven `?rview=` values, in the source's object-literal order.
///
/// The order is the *declaration* order rather than anything the shader reads —
/// [`DebugView::mode`] is the shader contract — but it is preserved because a
/// reader diffing the two files reads them in this order, and because
/// [`DEBUG_VIEW_NAMES`] and [`DebugView::mode`] are parallel tables indexed by
/// this discriminant.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DebugView {
    /// GTAO visibility.
    Ao = 0,
    /// The G-buffer's oct-encoded view normal.
    Normal = 1,
    /// The G-buffer's screen-space velocity.
    Velocity = 2,
    /// The G-buffer's linear view depth, in metres.
    Depth = 3,
    /// The screen-space reflection colour.
    Ssr = 4,
    /// The same texture, showing its confidence mask instead.
    SsrMask = 5,
    /// The contact-shadow visibility.
    Contact = 6,
    /// The bloom pyramid's finished level.
    Bloom = 7,
    /// The viewmodel target's colour.
    View = 8,
    /// The viewmodel target's alpha, which is its MSAA coverage.
    ViewAlpha = 9,
    /// The frame as the composite would have received it. Also the fallback for
    /// any unrecognised name.
    Color = 10,
}

/// The `?rview=` strings, parallel to the discriminants above.
pub(crate) const DEBUG_VIEW_NAMES: [&str; 11] = [
    "ao",
    "normal",
    "velocity",
    "depth",
    "ssr",
    "ssrmask",
    "contact",
    "bloom",
    "view",
    "viewalpha",
    "color",
];

/// Which buffer a debug view samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DebugSource {
    /// `this.aoTexture` — present only when GTAO ran.
    Ao,
    /// `gb.normalTexture`.
    GBufferNormal,
    /// `gb.velocityTexture`.
    GBufferVelocity,
    /// `gb.depthTexture`.
    GBufferDepth,
    /// `this.ssr?.texture`.
    Ssr,
    /// `this.contact?.texture`.
    Contact,
    /// `this.bloom?.texture`.
    Bloom,
    /// `this.viewRt?.texture`.
    Viewmodel,
    /// The `color` argument — the finished frame, and the universal fallback.
    Color,
}

impl DebugSource {
    /// Whether this pipeline built the pass that owns the buffer.
    ///
    /// `false` is the source's `undefined` reaching `?? color`: the view is
    /// still shown, sampling the finished frame instead.
    pub(crate) fn available_in(self, pipeline: &FramePipeline) -> bool {
        [
            pipeline.runs_gtao(),      // Ao
            pipeline.runs_prepass(),   // GBufferNormal
            pipeline.runs_prepass(),   // GBufferVelocity
            pipeline.runs_prepass(),   // GBufferDepth
            pipeline.runs_ssr(false),  // Ssr
            pipeline.runs_contact(),   // Contact
            pipeline.bloom_levels().is_some(), // Bloom
            true,                      // Viewmodel — always allocated
            true,                      // Color
        ][self as usize]
    }
}

impl DebugView {
    /// The `?rview=` string.
    pub(crate) const fn name(self) -> &'static str {
        DEBUG_VIEW_NAMES[self as usize]
    }

    /// `uMode` — the unpacking arm in `createDebug()`'s fragment shader. Three
    /// views share `4` and two share `5`; see the module docs.
    pub(crate) const fn mode(self) -> u32 {
        [0, 1, 2, 3, 4, 5, 0, 4, 4, 5, 4][self as usize]
    }

    /// Which buffer the view samples.
    pub(crate) const fn source(self) -> DebugSource {
        [
            DebugSource::Ao,
            DebugSource::GBufferNormal,
            DebugSource::GBufferVelocity,
            DebugSource::GBufferDepth,
            DebugSource::Ssr,
            DebugSource::Ssr,
            DebugSource::Contact,
            DebugSource::Bloom,
            DebugSource::Viewmodel,
            DebugSource::Viewmodel,
            DebugSource::Color,
        ][self as usize]
    }

    /// The buffer actually sampled on this pipeline — `entry[0] ?? color`.
    pub(crate) fn resolved_source(self, pipeline: &FramePipeline) -> DebugSource {
        let wanted = self.source();
        [DebugSource::Color, wanted][usize::from(wanted.available_in(pipeline))]
    }
}

/// `new URLSearchParams(location.search).get('rview') || null`, then
/// `map[debugView] ?? map.color`.
///
/// - a query with no `rview` at all, or `?rview=`, yields `None` — the frame
///   runs its normal composite;
/// - a recognised name yields that view;
/// - **anything else yields [`DebugView::Color`]**, which is the `?? map.color`
///   fallback and not an error.
pub(crate) fn parse_rview(query: &str) -> Option<DebugView> {
    query_value(query, "rview")
        .filter(|value| !value.is_empty())
        .map(|value| {
            DEBUG_VIEW_NAMES
                .iter()
                .position(|name| *name == value)
                .map_or(DebugView::Color, |index| VIEWS[index])
        })
}

/// Every view, in discriminant order.
const VIEWS: [DebugView; 11] = [
    DebugView::Ao,
    DebugView::Normal,
    DebugView::Velocity,
    DebugView::Depth,
    DebugView::Ssr,
    DebugView::SsrMask,
    DebugView::Contact,
    DebugView::Bloom,
    DebugView::View,
    DebugView::ViewAlpha,
    DebugView::Color,
];

/// `/[?&]owNoCascadeCull=1/.test(location.search)` — puts every caster back
/// into every cascade.
///
/// The source keeps it as "the A/B switch the pixel gate was run through, and
/// the escape hatch if a subsystem ever ships geometry whose bounds lie about
/// where it is". It disables [`crate::shadow_cull`]'s equivalent, not the
/// cascades themselves.
pub(crate) fn no_cascade_cull(query: &str) -> bool {
    query.contains("?owNoCascadeCull=1") | query.contains("&owNoCascadeCull=1")
}

/// The first value of `key` in a `?a=1&b=2` query string.
///
/// A deliberately small stand-in for `URLSearchParams`: `index.js` reads
/// exactly one key through it, with no percent-encoding, no repeated keys and
/// no bare flags in any documented usage.
fn query_value<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    query
        .trim_start_matches('?')
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(name, _)| *name == key)
        .map(|(_, value)| value)
}

#[cfg(test)]
mod tests {
    use super::{
        no_cascade_cull, parse_rview, DebugSource, DebugView, DEBUG_VIEW_NAMES, VIEWS,
    };
    use crate::frame_graph::pipeline::FramePipeline;
    use crate::frame_graph::quality::QualityTier;
    use axiom_host::BackendCapabilityProfile;

    /// Eight of the eleven views share a mode number with another view. The
    /// integers are the debug shader's unpacking arms, not a view index.
    #[test]
    fn the_mode_numbers_are_the_shaders_arms_and_are_deliberately_not_unique() {
        let modes: Vec<u32> = VIEWS.iter().map(|v| v.mode()).collect();
        assert_eq!(modes, vec![0, 1, 2, 3, 4, 5, 0, 4, 4, 5, 4]);
        // `contact` reuses `ao`'s arm: both are one visibility scalar in `.r`.
        assert_eq!(DebugView::Contact.mode(), DebugView::Ao.mode());
        // `bloom`, `view` and `color` all reuse the plain-RGB arm.
        assert_eq!(DebugView::Bloom.mode(), 4);
        assert_eq!(DebugView::View.mode(), 4);
        assert_eq!(DebugView::Color.mode(), 4);
        // `ssrmask` and `viewalpha` share the alpha arm.
        assert_eq!(DebugView::SsrMask.mode(), DebugView::ViewAlpha.mode());
        // Only five distinct arms exist for eleven views.
        let mut distinct = modes.clone();
        distinct.sort_unstable();
        distinct.dedup();
        assert_eq!(distinct, vec![0, 1, 2, 3, 4, 5]);
    }

    /// The parallel name/discriminant tables cannot drift.
    #[test]
    fn every_view_names_itself_at_its_own_index() {
        VIEWS.iter().enumerate().for_each(|(i, v)| {
            assert_eq!(*v as usize, i);
            assert_eq!(v.name(), DEBUG_VIEW_NAMES[i]);
            assert_eq!(parse_rview(&format!("?rview={}", v.name())), Some(*v));
        });
    }

    /// Absent, empty and unrecognised are three different answers, and only the
    /// first two switch the debug arm off.
    #[test]
    fn an_unrecognised_view_name_shows_the_finished_frame_rather_than_failing() {
        assert_eq!(parse_rview(""), None);
        assert_eq!(parse_rview("?"), None);
        assert_eq!(parse_rview("?quality=low"), None);
        assert_eq!(parse_rview("?rview="), None, "`|| null` catches the empty string");
        assert_eq!(parse_rview("?rview=ao"), Some(DebugView::Ao));
        assert_eq!(parse_rview("?quality=low&rview=depth"), Some(DebugView::Depth));
        assert_eq!(
            parse_rview("?rview=nonsense"),
            Some(DebugView::Color),
            "`?? map.color` is a fallback, not an error"
        );
        // A bare flag has no `=` and is skipped rather than read as a key.
        assert_eq!(parse_rview("?rview"), None);
    }

    /// Selecting a buffer the tier never built falls back to the finished
    /// frame; on a tier that built it, the view samples what it names.
    #[test]
    fn a_view_of_a_pass_that_was_never_built_falls_back_to_the_colour_buffer() {
        let profile = BackendCapabilityProfile::all();
        let low = FramePipeline::resolve(QualityTier::Low, profile, 16);
        let ultra = FramePipeline::resolve(QualityTier::Ultra, profile, 16);

        // `low` builds no GTAO, no SSR and no contact shadows.
        assert_eq!(DebugView::Ao.resolved_source(&low), DebugSource::Color);
        assert_eq!(DebugView::Ssr.resolved_source(&low), DebugSource::Color);
        assert_eq!(DebugView::SsrMask.resolved_source(&low), DebugSource::Color);
        assert_eq!(DebugView::Contact.resolved_source(&low), DebugSource::Color);
        // ...but it does build the prepass and the bloom pyramid.
        assert_eq!(
            DebugView::Normal.resolved_source(&low),
            DebugSource::GBufferNormal
        );
        assert_eq!(DebugView::Bloom.resolved_source(&low), DebugSource::Bloom);

        // `ultra` builds all of them.
        assert_eq!(DebugView::Ao.resolved_source(&ultra), DebugSource::Ao);
        assert_eq!(DebugView::Ssr.resolved_source(&ultra), DebugSource::Ssr);
        assert_eq!(
            DebugView::Contact.resolved_source(&ultra),
            DebugSource::Contact
        );
        assert_eq!(
            DebugView::Velocity.resolved_source(&ultra),
            DebugSource::GBufferVelocity
        );
        assert_eq!(
            DebugView::Depth.resolved_source(&ultra),
            DebugSource::GBufferDepth
        );
        // The viewmodel target and the colour buffer always exist.
        assert_eq!(
            DebugView::View.resolved_source(&low),
            DebugSource::Viewmodel
        );
        assert_eq!(
            DebugView::ViewAlpha.resolved_source(&ultra),
            DebugSource::Viewmodel
        );
        assert_eq!(DebugView::Color.resolved_source(&low), DebugSource::Color);
    }

    /// The cascade-cull escape hatch matches only its own exact flag.
    #[test]
    fn the_cascade_cull_escape_hatch_matches_the_source_regex() {
        assert!(no_cascade_cull("?owNoCascadeCull=1"));
        assert!(no_cascade_cull("?quality=low&owNoCascadeCull=1"));
        assert!(!no_cascade_cull("?owNoCascadeCull=0"));
        assert!(!no_cascade_cull("?xowNoCascadeCull=1"));
        assert!(!no_cascade_cull(""));
    }
}
