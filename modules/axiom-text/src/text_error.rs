//! The text module's deterministic error type.
//!
//! An error's *identity* is its variant, exactly like the kernel's `(scope,
//! code)` model: the enum derives `PartialEq`/`Eq`, so two errors of the same
//! kind compare equal and there is no human-readable string participating in
//! equality. This keeps text failures deterministic and replayable. Text is not
//! a kernel concern, so — like every other module's error (`FigureError`,
//! `MathError`) — the vocabulary lives here rather than being wedged into the
//! closed `KernelErrorScope`.

/// Why a text operation failed. `Copy` and small: every failure is a checked,
/// deterministic identity a test can assert on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextError {
    /// A compiled `.axfont` byte stream could not be decoded (bad magic, a
    /// truncated section, or bytes that do not form a valid asset).
    MalformedFont,
    /// A compiled font declared a format version this runtime does not know.
    UnsupportedFontVersion,
    /// A font's face metrics are impossible (zero units-per-em, or an ascent not
    /// strictly above its descent).
    InvalidFontMetrics,
    /// An atlas page declared dimensions that do not match its pixel payload, or
    /// a zero dimension.
    InvalidAtlasDimensions,
    /// A glyph raster referenced an atlas page index that does not exist, or a UV
    /// rectangle outside its page.
    InvalidAtlasPage,
    /// A glyph metrics table listed the same glyph index twice, or was not sorted
    /// strictly ascending.
    DuplicateGlyph,
    /// A codepoint map listed the same codepoint twice, or was not sorted
    /// strictly ascending.
    DuplicateCodepoint,
    /// A font declared a replacement codepoint that its own codepoint map does
    /// not resolve to a glyph.
    MissingReplacementGlyph,
    /// An operation named a font family/handle that is not registered.
    MissingFont,
    /// A glyph was requested for a codepoint no font in the fallback chain
    /// covers, and the font carries no replacement glyph.
    MissingGlyph,
    /// A font's family or face name bytes were not valid UTF-8.
    InvalidFontMetadataUtf8,
    /// Text used a script whose shaping this runtime does not implement; the
    /// engine records this rather than silently producing broken glyph order.
    UnsupportedShaping,
    /// A font size was not a finite value strictly greater than zero, or fell
    /// outside a style's declared fit range.
    InvalidFontSize,
    /// A line-height multiple was not a finite value greater than zero.
    InvalidLineHeight,
    /// An opacity was not a finite value within `0.0..=1.0`.
    InvalidOpacity,
    /// A width/height constraint was negative or non-finite, or a min exceeded a
    /// max.
    InvalidDimensions,
    /// A world placement carried a non-finite transform component.
    InvalidTransform,
    /// An effect parameter was non-finite or outside its allowed range.
    InvalidEffectParams,
    /// A text handle referred to a slot whose generation has moved on (the text
    /// was removed and its slot possibly reused).
    StaleTextHandle,
    /// A font handle referred to a slot whose generation has moved on.
    StaleFontHandle,
    /// A configured capacity was exceeded: the maximum number of live text
    /// objects or fonts, the per-object character/span/line cap, or the
    /// per-snapshot glyph cap. All caps collapse to one identity because a caller
    /// handles every one the same way — reduce the input — and distinguishing
    /// "too many spans" from "too many lines" adds no decision the caller can act
    /// on. The engine still tells you *which* limit via the [`crate::TextLimits`]
    /// it was checked against.
    CapacityExceeded,
    /// A font declared more atlas pages than the configured per-font cap.
    AtlasPackingOverflow,
    /// A font that is still referenced by live text cannot be unregistered.
    FontStillReferenced,
}

/// The result of a fallible text operation.
pub type TextResult<T> = Result<T, TextError>;
