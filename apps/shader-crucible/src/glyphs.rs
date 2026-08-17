//! **A 5x7 uppercase bitmap font, as renderable geometry.**
//!
//! ## Why the app carries a font at all
//!
//! The engine has a text module — `modules/axiom-text` — and it cannot be used
//! here. Three facts, each checked against the module rather than assumed:
//!
//! 1. Its product is a `GlyphBatch`: positioned quads plus **atlas sub-rects**.
//!    It holds no GPU handle and no mesh, and **nothing in this repository has
//!    ever turned one into a pixel** — its only Cargo dependent is
//!    `tools/axiom-font-import`, the offline `.axfont` compiler, which never
//!    touches `TextApi` at all.
//! 2. `TextApi` exposes no accessor for a registered font's atlas, and the
//!    built-in fallback font is a private module function. So a caller can get
//!    glyph quads whose UVs point into an atlas **it cannot obtain**, which
//!    means using the module at all requires shipping a `.axfont` asset built
//!    by that tool.
//! 3. `Billboard::Camera` is a byte the module carries and never acts on;
//!    nothing in it reads a camera.
//!
//! Building that bridge — glyph batch to draw list, atlas to texture, billboard
//! to a real orientation — is a **general engine capability**, and adding one
//! inside an app to put twelve captions on twelve spheres is exactly the
//! misplacement this repository's No-Shortcuts rule exists to prevent.
//! `apps/burnt-rubber` reached the same conclusion for its speedometer and
//! wrote a DOM overlay instead (`apps/burnt-rubber/src/web.rs`, `update_hud`).
//!
//! A DOM overlay is not available here either: it would have to project each
//! body through the camera every frame, which lives in `src/web.rs`, and it
//! would stop being world-space the moment the orbit camera moved.
//!
//! ## So: letters are geometry
//!
//! This is the one in-scene 3D-text technique with a working precedent in the
//! repository — `packages/axiom-web-engine/src/glyph-font.ts`, whose header says
//! it in as many words: *"the engine has no texture/text-quad primitive, so an
//! app that wants lettering builds it out of its own meshes."* `apps/casino-games`
//! ships it (`presentation/branding/label.ts`). This is that technique in Rust:
//! a 5x7 cell font, run-length encoded per row into axis-aligned quads, welded
//! into one [`MeshData`] per label.
//!
//! It needs no asset, no new dependency, no atlas, and no engine change.

/// The cell width of one glyph.
pub const GLYPH_W: usize = 5;
/// The cell height of one glyph.
pub const GLYPH_H: usize = 7;
/// The blank cell column between two glyphs.
pub const GLYPH_GAP: usize = 1;

/// One horizontal run of lit cells: `len` cells starting at `(row, col)`.
///
/// Run-length encoding rather than one quad per cell is not a micro-optimisation
/// — a solid 5-cell bar is one quad instead of five, which roughly halves the
/// triangle count of a label and, more importantly, removes the interior seams
/// where two abutting quads meet on a shared edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellRun {
    /// The cell row, `0` at the cap line and `GLYPH_H - 1` at the baseline.
    pub row: usize,
    /// The cell column from the glyph's left edge.
    pub col: usize,
    /// How many cells the run covers.
    pub len: usize,
}

/// Every glyph this font can draw, and the seven 5-bit rows of each.
///
/// The bits are read most-significant-first across the five cells, so the
/// literal `0b10001` is a cell lit at each edge — the table is legible as
/// pixel art in the source, which is the only reason a hand-authored font is
/// maintainable at all.
const GLYPHS: [(char, [u8; GLYPH_H]); 46] = [
    (' ', [0, 0, 0, 0, 0, 0, 0]),
    ('A', [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001]),
    ('B', [0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110]),
    ('C', [0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110]),
    ('D', [0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110]),
    ('E', [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111]),
    ('F', [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000]),
    ('G', [0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111]),
    ('H', [0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001]),
    ('I', [0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110]),
    ('J', [0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100]),
    ('K', [0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001]),
    ('L', [0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111]),
    ('M', [0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001]),
    ('N', [0b10001, 0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001]),
    ('O', [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110]),
    ('P', [0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000]),
    ('Q', [0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101]),
    ('R', [0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001]),
    ('S', [0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110]),
    ('T', [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100]),
    ('U', [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110]),
    ('V', [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100]),
    ('W', [0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001]),
    ('X', [0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001]),
    ('Y', [0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100]),
    ('Z', [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111]),
    ('0', [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110]),
    ('1', [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110]),
    ('2', [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111]),
    ('3', [0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110]),
    ('4', [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010]),
    ('5', [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110]),
    ('6', [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110]),
    ('7', [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000]),
    ('8', [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110]),
    ('9', [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100]),
    // The separator the labels read as "·": a single centred cell.
    ('.', [0, 0, 0, 0b00100, 0, 0, 0]),
    ('+', [0, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0]),
    ('-', [0, 0, 0, 0b11111, 0, 0, 0]),
    ('/', [0b00001, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b10000]),
    ('(', [0b00010, 0b00100, 0b01000, 0b01000, 0b01000, 0b00100, 0b00010]),
    (')', [0b01000, 0b00100, 0b00010, 0b00010, 0b00010, 0b00100, 0b01000]),
    (':', [0, 0b00100, 0, 0, 0, 0b00100, 0]),
    ('%', [0b11001, 0b11010, 0b00010, 0b00100, 0b01000, 0b01011, 0b10011]),
    ('*', [0, 0b10101, 0b01110, 0b11111, 0b01110, 0b10101, 0]),
];

/// The seven rows of `character`, or the seven rows of a space when this font
/// has no glyph for it.
///
/// Falling back to a blank rather than to a "missing glyph" box is deliberate:
/// a label is a caption, and a caption with a tofu box in it is *less* readable
/// than one with a gap. [`crate::label::LINES`] is pinned by a test to the
/// characters this table actually has, so the fallback is a safety net rather
/// than a silent degradation.
pub fn glyph_rows(character: char) -> [u8; GLYPH_H] {
    let upper = character.to_ascii_uppercase();
    GLYPHS
        .iter()
        .find(|(candidate, _)| *candidate == upper)
        .map(|(_, rows)| *rows)
        .unwrap_or([0; GLYPH_H])
}

/// Whether this font has a glyph for `character` (case-insensitively).
pub fn has_glyph(character: char) -> bool {
    let upper = character.to_ascii_uppercase();
    GLYPHS.iter().any(|(candidate, _)| *candidate == upper)
}

/// Every lit run of one glyph, in row-major order.
pub fn glyph_runs(character: char) -> Vec<CellRun> {
    let rows = glyph_rows(character);
    let mut runs = Vec::new();
    for (row, bits) in rows.iter().enumerate() {
        let mut col = 0_usize;
        while col < GLYPH_W {
            let lit = |c: usize| bits & (1 << (GLYPH_W - 1 - c)) != 0;
            if lit(col) {
                let start = col;
                while col < GLYPH_W && lit(col) {
                    col += 1;
                }
                runs.push(CellRun {
                    row,
                    col: start,
                    len: col - start,
                });
            } else {
                col += 1;
            }
        }
    }
    runs
}

/// How many cell columns `text` occupies, gaps included — the advance of the
/// whole run, with no trailing gap.
pub fn text_columns(text: &str) -> usize {
    let count = text.chars().count();
    count
        .checked_sub(1)
        .map(|gaps| count * GLYPH_W + gaps * GLYPH_GAP)
        .unwrap_or(0)
}

/// Every lit run of `text`, with each glyph's runs advanced into the run's own
/// cell space (so `col` is measured from the left edge of the whole string).
pub fn text_runs(text: &str) -> Vec<CellRun> {
    text.chars()
        .enumerate()
        .flat_map(|(index, character)| {
            let advance = index * (GLYPH_W + GLYPH_GAP);
            glyph_runs(character).into_iter().map(move |run| CellRun {
                row: run.row,
                col: run.col + advance,
                len: run.len,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The table has no duplicate character, which a `find` would silently
    /// resolve in favour of whichever came first.
    #[test]
    fn every_glyph_in_the_table_is_distinct() {
        let mut seen: Vec<char> = GLYPHS.iter().map(|(c, _)| *c).collect();
        seen.sort_unstable();
        let count = seen.len();
        seen.dedup();
        assert_eq!(seen.len(), count);
    }

    /// Lower case reaches the upper-case glyph, and an unknown character is a
    /// blank rather than a panic.
    #[test]
    fn lookup_folds_case_and_falls_back_to_blank() {
        assert_eq!(glyph_rows('a'), glyph_rows('A'));
        assert_eq!(glyph_rows('~'), [0; GLYPH_H]);
        assert!(has_glyph('z') && !has_glyph('~'));
    }

    /// **`H` is two full-height stems and one bar.** A run decoder that read the
    /// bits in the wrong order would produce a mirrored glyph and every label
    /// would be unreadable in a way no count-based test would catch, so this
    /// asserts the actual geometry of one asymmetric-free letter and one
    /// deliberately asymmetric one.
    #[test]
    fn the_runs_of_h_are_two_stems_and_a_bar() {
        let runs = glyph_runs('H');
        // Six rows of two 1-cell stems, one row of a single 5-cell bar.
        assert_eq!(runs.iter().filter(|r| r.len == 5).count(), 1);
        assert_eq!(runs.iter().filter(|r| r.len == 1).count(), 12);
        assert!(runs.iter().all(|r| r.len == 1 || r.len == 5));
    }

    /// **`F` is lit on the left, not the right** — the asymmetry that pins the
    /// bit order. Row 6 (the baseline) of `F` is a single cell at column 0.
    #[test]
    fn the_bit_order_is_most_significant_bit_leftmost() {
        let runs = glyph_runs('F');
        let baseline: Vec<&CellRun> = runs.iter().filter(|r| r.row == GLYPH_H - 1).collect();
        assert_eq!(baseline.len(), 1);
        assert_eq!(baseline[0].col, 0);
        assert_eq!(baseline[0].len, 1);
    }

    /// A blank glyph contributes no geometry at all.
    #[test]
    fn a_space_lights_nothing() {
        assert!(glyph_runs(' ').is_empty());
        assert_eq!(text_runs("   ").len(), 0);
    }

    /// The advance of a string is glyphs plus the gaps *between* them, and an
    /// empty string is zero columns wide rather than a subtraction overflow.
    #[test]
    fn the_advance_counts_gaps_between_glyphs_only() {
        assert_eq!(text_columns(""), 0);
        assert_eq!(text_columns("A"), GLYPH_W);
        assert_eq!(text_columns("AB"), GLYPH_W * 2 + GLYPH_GAP);
        assert_eq!(text_columns("ABC"), GLYPH_W * 3 + GLYPH_GAP * 2);
    }

    /// A string's runs are its glyphs' runs, advanced — the second glyph starts
    /// exactly one glyph-plus-gap to the right of the first.
    #[test]
    fn a_strings_runs_are_its_glyphs_runs_advanced() {
        let single = glyph_runs('E');
        let pair = text_runs("EE");
        assert_eq!(pair.len(), single.len() * 2);
        let shifted = pair[single.len()];
        assert_eq!(shifted.col, single[0].col + GLYPH_W + GLYPH_GAP);
        assert_eq!(shifted.row, single[0].row);
    }

    /// Every run stays inside its glyph's five cells.
    #[test]
    fn no_run_leaves_its_glyph_cell() {
        GLYPHS.iter().for_each(|(character, _)| {
            glyph_runs(*character).iter().for_each(|run| {
                assert!(run.col + run.len <= GLYPH_W, "{character} overflows");
                assert!(run.row < GLYPH_H, "{character} overflows");
                assert!(run.len > 0);
            });
        });
    }
}
