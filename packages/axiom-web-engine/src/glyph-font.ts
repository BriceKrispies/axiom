/*
 * glyph-font.ts — the engine's built-in 5×7 uppercase bitmap font, exposed as
 * renderable GEOMETRY. The engine has no texture/text-quad primitive, so an app
 * that wants lettering BUILDS it out of its own meshes; this module owns the
 * letterforms (the TypeScript twin of the Rust `axiom-text` fallback face) and
 * hands back the lit strokes, so no app has to vendor its own font.
 *
 * Each glyph is a 7-row × 5-column mask; a row's run of lit cells is collapsed
 * into one horizontal RUN (run-length encoding) so an unbroken stroke becomes a
 * single box rather than a string of touching cubes. Output is an abstract cell
 * grid (columns left→right, rows top→bottom); the caller turns runs into whatever
 * geometry it draws. Pure data + pure functions, fully branchless.
 */

import { absentProbe, isPresent, orElse, pick, select } from "./branchless.ts";

/** A typed "no open run" sentinel (an absent column index). */
const NO_START = absentProbe<number>();

/** Glyph cell dimensions: every glyph is `GLYPH_W` wide × `GLYPH_H` tall, and
 * glyphs are separated by `GLYPH_GAP` empty columns. */
export const GLYPH_W = 5;
export const GLYPH_H = 7;
export const GLYPH_GAP = 1;

const LIT = "#";
const GAP = ".";
type Mask = readonly [string, string, string, string, string, string, string];

/** The font: uppercase A–Z, digits, space, and a small punctuation set. `#` is a
 * lit cell; anything else is empty. Lowercase is uppercased before lookup; an
 * unknown character falls back to `FALLBACK`. */
const GLYPHS: Readonly<Record<string, Mask>> = {
  " ": [".....", ".....", ".....", ".....", ".....", ".....", "....."],
  "A": [".###.", "#...#", "#...#", "#####", "#...#", "#...#", "#...#"],
  "B": ["####.", "#...#", "#...#", "####.", "#...#", "#...#", "####."],
  "C": [".####", "#....", "#....", "#....", "#....", "#....", ".####"],
  "D": ["####.", "#...#", "#...#", "#...#", "#...#", "#...#", "####."],
  "E": ["#####", "#....", "#....", "####.", "#....", "#....", "#####"],
  "F": ["#####", "#....", "#....", "####.", "#....", "#....", "#...."],
  "G": [".####", "#....", "#....", "#.###", "#...#", "#...#", ".####"],
  "H": ["#...#", "#...#", "#...#", "#####", "#...#", "#...#", "#...#"],
  "I": ["#####", "..#..", "..#..", "..#..", "..#..", "..#..", "#####"],
  "J": ["..###", "...#.", "...#.", "...#.", "#..#.", "#..#.", ".##.."],
  "K": ["#...#", "#..#.", "#.#..", "##...", "#.#..", "#..#.", "#...#"],
  "L": ["#....", "#....", "#....", "#....", "#....", "#....", "#####"],
  "M": ["#...#", "##.##", "#.#.#", "#.#.#", "#...#", "#...#", "#...#"],
  "N": ["#...#", "##..#", "#.#.#", "#.#.#", "#.#.#", "#..##", "#...#"],
  "O": [".###.", "#...#", "#...#", "#...#", "#...#", "#...#", ".###."],
  "P": ["####.", "#...#", "#...#", "####.", "#....", "#....", "#...."],
  "Q": [".###.", "#...#", "#...#", "#...#", "#.#.#", "#..#.", ".##.#"],
  "R": ["####.", "#...#", "#...#", "####.", "#.#..", "#..#.", "#...#"],
  "S": [".####", "#....", "#....", ".###.", "....#", "....#", "####."],
  "T": ["#####", "..#..", "..#..", "..#..", "..#..", "..#..", "..#.."],
  "U": ["#...#", "#...#", "#...#", "#...#", "#...#", "#...#", ".###."],
  "V": ["#...#", "#...#", "#...#", "#...#", "#...#", ".#.#.", "..#.."],
  "W": ["#...#", "#...#", "#...#", "#.#.#", "#.#.#", "##.##", "#...#"],
  "X": ["#...#", "#...#", ".#.#.", "..#..", ".#.#.", "#...#", "#...#"],
  "Y": ["#...#", "#...#", ".#.#.", "..#..", "..#..", "..#..", "..#.."],
  "Z": ["#####", "....#", "...#.", "..#..", ".#...", "#....", "#####"],
  "0": [".###.", "#...#", "#..##", "#.#.#", "##..#", "#...#", ".###."],
  "1": ["..#..", ".##..", "..#..", "..#..", "..#..", "..#..", ".###."],
  "2": [".###.", "#...#", "....#", "..##.", ".#...", "#....", "#####"],
  "3": ["####.", "....#", "....#", ".###.", "....#", "....#", "####."],
  "4": ["...#.", "..##.", ".#.#.", "#..#.", "#####", "...#.", "...#."],
  "5": ["#####", "#....", "####.", "....#", "....#", "#...#", ".###."],
  "6": [".###.", "#....", "#....", "####.", "#...#", "#...#", ".###."],
  "7": ["#####", "....#", "...#.", "..#..", ".#...", ".#...", ".#..."],
  "8": [".###.", "#...#", "#...#", ".###.", "#...#", "#...#", ".###."],
  "9": [".###.", "#...#", "#...#", ".####", "....#", "....#", ".###."],
  "&": [".##..", "#..#.", "#.#..", ".#...", "#.#.#", "#..#.", ".##.#"],
  ".": [".....", ".....", ".....", ".....", ".....", ".##..", ".##.."],
  "-": [".....", ".....", ".....", ".####", ".....", ".....", "....."],
  "'": ["..#..", "..#..", ".#...", ".....", ".....", ".....", "....."],
  "!": ["..#..", "..#..", "..#..", "..#..", "..#..", ".....", "..#.."],
  "?": [".###.", "#...#", "....#", "..##.", "..#..", ".....", "..#.."],
} as const;

const FALLBACK: Mask = ["#####", "#...#", "#...#", "#...#", "#...#", "#...#", "#####"];

/** One lit horizontal stroke inside a glyph grid: a run of `len` cells starting
 * at (row, col) — row 0 is the TOP row, col 0 the LEFT column. */
export interface CellRun {
  readonly row: number;
  readonly col: number;
  readonly len: number;
}

interface RunState {
  readonly runs: readonly CellRun[];
  readonly start: number | undefined;
}

/** Collapse a 7×5 mask into horizontal runs of lit cells. A `GAP` sentinel is
 * appended to each row so a run touching the right edge is flushed. */
const maskRuns = (mask: readonly string[]): readonly CellRun[] =>
  mask.flatMap((line, row): readonly CellRun[] => {
    const cells = `${line}${GAP}`.split("");
    const acc = cells.reduce<RunState>(
      (state, cell, col) => {
        const lit = cell === LIT;
        const open = isPresent(state.start);
        const startNew = select(lit, !open, false);
        const closing = select(open, !lit, false);
        const at = orElse(state.start, col);
        return {
          runs: pick([state.runs, [...state.runs, { row, col: at, len: col - at }]], Number(closing)),
          start: pick([select(startNew, col, state.start), NO_START], Number(closing)),
        };
      },
      { runs: [], start: NO_START },
    );
    return acc.runs;
  });

/** The lit strokes of one character (uppercased; unknown → the fallback box). */
export const glyphRuns = (char: string): readonly CellRun[] => maskRuns(orElse(GLYPHS[char.toUpperCase()], FALLBACK));

/** Total column width of `text` in cells (glyph widths + inter-glyph gaps). Empty
 * text is zero columns. */
export const textColumns = (text: string): number =>
  select(text.length === 0, 0, text.length * GLYPH_W + (text.length - 1) * GLYPH_GAP);

/** Every lit stroke of `text`, in whole-string cell coordinates: `col` is the
 * absolute column from the left edge of the first glyph, `row` the shared 0..6
 * row. This is the geometry a caller stamps onto a surface. */
export const textRuns = (text: string): readonly CellRun[] =>
  Array.from(text).flatMap((char, index): readonly CellRun[] => {
    const originCol = index * (GLYPH_W + GLYPH_GAP);
    return glyphRuns(char).map((run): CellRun => ({ row: run.row, col: originCol + run.col, len: run.len }));
  });
