/*
 * glyphs.ts — a tiny 5×7 uppercase bitmap font, expressed as GEOMETRY. The
 * engine has no textures and no text primitive (materials are flat baseColor on
 * box/sphere/cylinder), so brand lettering has to be BUILT out of boxes. Each
 * glyph is a 7-row × 5-column mask; a row's run of lit cells is collapsed into
 * one horizontal RUN (run-length encoding) so an unbroken stroke becomes a
 * single box rather than a string of touching cubes — fewer instances, and a
 * cleaner solid stroke than a grid of separate pixels would read as.
 *
 * The output is in an abstract "cell" grid (columns run left→right, rows
 * top→bottom); `label.ts` turns cell runs into welded SceneInstances on a
 * surface. Pure data + pure functions — no engine, no DOM, deterministic.
 */

/** Glyph cell dimensions. Every glyph is GLYPH_W wide × GLYPH_H tall, and
 * glyphs are separated by GLYPH_GAP empty columns. */
export const GLYPH_W = 5;
export const GLYPH_H = 7;
export const GLYPH_GAP = 1;

/**
 * The font. Each entry is 7 strings of 5 chars; `#` is a lit cell, anything
 * else (space / `.`) is empty. Uppercase A–Z, digits 0–9, space, and a small
 * set of punctuation a brand name might carry. Lowercase input is uppercased
 * before lookup; an unknown glyph falls back to `FALLBACK`.
 */
const GLYPHS: Readonly<Record<string, readonly [string, string, string, string, string, string, string]>> = {
  " ": [".....", ".....", ".....", ".....", ".....", ".....", "....."],
  A: [".###.", "#...#", "#...#", "#####", "#...#", "#...#", "#...#"],
  B: ["####.", "#...#", "#...#", "####.", "#...#", "#...#", "####."],
  C: [".####", "#....", "#....", "#....", "#....", "#....", ".####"],
  D: ["####.", "#...#", "#...#", "#...#", "#...#", "#...#", "####."],
  E: ["#####", "#....", "#....", "####.", "#....", "#....", "#####"],
  F: ["#####", "#....", "#....", "####.", "#....", "#....", "#...."],
  G: [".####", "#....", "#....", "#.###", "#...#", "#...#", ".####"],
  H: ["#...#", "#...#", "#...#", "#####", "#...#", "#...#", "#...#"],
  I: ["#####", "..#..", "..#..", "..#..", "..#..", "..#..", "#####"],
  J: ["..###", "...#.", "...#.", "...#.", "#..#.", "#..#.", ".##.."],
  K: ["#...#", "#..#.", "#.#..", "##...", "#.#..", "#..#.", "#...#"],
  L: ["#....", "#....", "#....", "#....", "#....", "#....", "#####"],
  M: ["#...#", "##.##", "#.#.#", "#.#.#", "#...#", "#...#", "#...#"],
  N: ["#...#", "##..#", "#.#.#", "#.#.#", "#.#.#", "#..##", "#...#"],
  O: [".###.", "#...#", "#...#", "#...#", "#...#", "#...#", ".###."],
  P: ["####.", "#...#", "#...#", "####.", "#....", "#....", "#...."],
  Q: [".###.", "#...#", "#...#", "#...#", "#.#.#", "#..#.", ".##.#"],
  R: ["####.", "#...#", "#...#", "####.", "#.#..", "#..#.", "#...#"],
  S: [".####", "#....", "#....", ".###.", "....#", "....#", "####."],
  T: ["#####", "..#..", "..#..", "..#..", "..#..", "..#..", "..#.."],
  U: ["#...#", "#...#", "#...#", "#...#", "#...#", "#...#", ".###."],
  V: ["#...#", "#...#", "#...#", "#...#", "#...#", ".#.#.", "..#.."],
  W: ["#...#", "#...#", "#...#", "#.#.#", "#.#.#", "##.##", "#...#"],
  X: ["#...#", "#...#", ".#.#.", "..#..", ".#.#.", "#...#", "#...#"],
  Y: ["#...#", "#...#", ".#.#.", "..#..", "..#..", "..#..", "..#.."],
  Z: ["#####", "....#", "...#.", "..#..", ".#...", "#....", "#####"],
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

const FALLBACK: readonly [string, string, string, string, string, string, string] = [
  "#####",
  "#...#",
  "#...#",
  "#...#",
  "#...#",
  "#...#",
  "#####",
];

/** One lit horizontal stroke inside a glyph grid: a run of `len` cells starting
 * at (row, col) — row 0 is the TOP row, col 0 the LEFT column. */
export interface CellRun {
  readonly row: number;
  readonly col: number;
  readonly len: number;
}

/** Collapse a 7×5 mask into horizontal runs of lit cells. */
const maskRuns = (mask: readonly string[]): readonly CellRun[] =>
  mask.flatMap((line, row) => {
    // Walk the row, emitting a run each time a lit stretch ends (at a gap or the
    // row's end). A sentinel empty cell appended past the end flushes a trailing run.
    const cells = `${line}.`.split("");
    const acc = cells.reduce<{ readonly runs: readonly CellRun[]; readonly start: number | null }>(
      (state, cell, col) => {
        const lit = cell === "#";
        const opening = lit && state.start === null ? col : state.start;
        const closing = !lit && state.start !== null;
        return {
          runs: closing ? [...state.runs, { col: state.start as number, len: col - (state.start as number), row }] : state.runs,
          start: closing ? null : opening,
        };
      },
      { runs: [], start: null },
    );
    return acc.runs;
  });

/** The lit strokes of one character (uppercased; unknown → the fallback box). */
export const glyphRuns = (char: string): readonly CellRun[] => maskRuns(GLYPHS[char.toUpperCase()] ?? FALLBACK);

/** Total column width of `text` in cells (glyph widths + inter-glyph gaps). A
 * space is a full glyph width of blank. Empty text is zero columns. */
export const textColumns = (text: string): number =>
  text.length === 0 ? 0 : text.length * GLYPH_W + (text.length - 1) * GLYPH_GAP;

/** Every lit stroke of `text`, in whole-string cell coordinates: `col` is the
 * absolute column from the left edge of the first glyph, `row` the shared 0..6
 * row. This is the geometry `label.ts` stamps onto a surface. */
export const textRuns = (text: string): readonly CellRun[] =>
  text.split("").flatMap((char, index) => {
    const originCol = index * (GLYPH_W + GLYPH_GAP);
    return glyphRuns(char).map((run) => ({ col: originCol + run.col, len: run.len, row: run.row }));
  });
