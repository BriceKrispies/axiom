/*
 * text.ts — the pure-TypeScript text authoring surface, the `@axiom/web-engine`
 * counterpart of the Rust `axiom-text` module. `text("Hello, world")` (or
 * `axiom.text(...)`) builds an immutable Text value: plain or rich spans, a
 * style cascade (engine default → text-level → span-level), a layout box
 * (width / align / wrap), and screen/world placement. `glyphs()` lays it out into
 * a backend-neutral list of positioned glyph quads an app draws; `measure()`
 * returns its bounds. Every update method (`setText`, `setStyle`, …) returns a NEW
 * Text, so the value is referentially transparent and trivially testable.
 *
 * Metrics here are a deterministic monospace model (advance = `fontSize ·
 * ADVANCE_RATIO`); true per-glyph metrics come from a compiled `.axfont` via the
 * Rust `axiom-text` runtime. Layout is left-to-right, one quad per code point.
 * Fully branchless (no `if`/`?:`/loops/`&&`/`||`), like the rest of the spine.
 */

import { orElse, pick, presentOf, select } from "./branchless.ts";
import type { Rgba } from "./api.ts";

/** Split a string into Unicode code points (surrogate-safe, keeps newlines). */
const codePoints = (input: string): readonly string[] => Array.from(input);

// ── numeric constants (module-level so no-magic-numbers stays happy) ──────────
const DEFAULT_FONT_SIZE = 16;
const DEFAULT_LINE_HEIGHT = 1.2;
const DEFAULT_WEIGHT = 400;
const ADVANCE_RATIO = 0.6;
const CENTER_FACTOR = 0.5;
const HEX_RADIX = 16;
const BYTE_MAX = 255;
const HEX_PAIR = 2;
const SHORT_HEX_LEN = HEX_PAIR + 1;
const RGB_HEX_LEN = HEX_PAIR * SHORT_HEX_LEN;
const RGBA_HEX_LEN = RGB_HEX_LEN + HEX_PAIR;
const OPAQUE_HEX = "ff";
const OPAQUE_WHITE: Rgba = [1, 1, 1, 1];

/** Branchless AND / OR over booleans (method calls, not the banned operators). */
const all = (flags: readonly boolean[]): boolean => flags.every(Boolean);

// ── public value contract ─────────────────────────────────────────────────────
/** Horizontal alignment within the layout box. */
export type TextAlign = "left" | "center" | "right";
/** Wrapping mode: explicit newlines only, or greedy word wrapping at the width. */
export type TextWrap = "none" | "word";
/** Which space the text is placed in. */
export type TextSpace = "screen" | "world";

/** A sparse per-span or text-level style. `color` accepts `"#rgb"`, `"#rrggbb"`,
 * `"#rrggbbaa"`, or an `Rgba` tuple. */
export interface TextStyleInput {
  readonly fontSize?: number;
  readonly color?: string | Rgba;
  readonly weight?: number;
  readonly italic?: boolean;
  readonly lineHeight?: number;
  readonly letterSpacing?: number;
}

/** The layout box. `width` unset (or non-finite) means content-sized, no wrap. */
export interface TextLayoutInput {
  readonly width?: number;
  readonly align?: TextAlign;
  readonly wrap?: TextWrap;
}

/** Options for `text(...)`. */
export interface TextOptions {
  readonly position?: readonly [number, number];
  readonly style?: TextStyleInput;
  readonly layout?: TextLayoutInput;
  readonly space?: TextSpace;
  readonly visible?: boolean;
}

/** One rich span: text plus an optional style override. */
export interface TextSpanInput {
  readonly text: string;
  readonly style?: TextStyleInput;
}

/** Plain string, or an array of rich spans. */
export type TextContent = string | readonly TextSpanInput[];

/** One positioned glyph quad, top-left in screen pixels (placement folded in). */
export interface TextGlyph {
  readonly char: string;
  readonly x: number;
  readonly y: number;
  readonly width: number;
  readonly height: number;
  readonly color: Rgba;
  readonly fontSize: number;
  readonly line: number;
}

/** Overall laid-out bounds. */
export interface TextBounds {
  readonly width: number;
  readonly height: number;
  readonly lineCount: number;
}

/** An immutable Text value. Update methods return a new Text. */
export interface Text {
  readonly position: readonly [number, number];
  readonly space: TextSpace;
  readonly visible: boolean;
  setText: (content: TextContent) => Text;
  setStyle: (patch: TextStyleInput) => Text;
  setLayout: (patch: TextLayoutInput) => Text;
  setPosition: (x: number, y: number) => Text;
  setVisible: (visible: boolean) => Text;
  measure: () => TextBounds;
  glyphs: () => readonly TextGlyph[];
}

// ── internal shapes ───────────────────────────────────────────────────────────
interface ResolvedStyle {
  readonly fontSize: number;
  readonly color: Rgba;
  readonly weight: number;
  readonly italic: boolean;
  readonly lineHeight: number;
  readonly letterSpacing: number;
}
interface ResolvedBox {
  readonly width: number;
  readonly align: TextAlign;
  readonly wrap: TextWrap;
}
interface Run {
  readonly text: string;
  readonly override: TextStyleInput;
}
interface Cell {
  readonly ch: string;
  readonly style: ResolvedStyle;
  readonly space: boolean;
  readonly newline: boolean;
  readonly advance: number;
}
interface State {
  readonly baseStyle: ResolvedStyle;
  readonly runs: readonly Run[];
  readonly box: ResolvedBox;
  readonly position: readonly [number, number];
  readonly space: TextSpace;
  readonly visible: boolean;
}

const DEFAULT_STYLE: ResolvedStyle = {
  fontSize: DEFAULT_FONT_SIZE,
  color: OPAQUE_WHITE,
  weight: DEFAULT_WEIGHT,
  italic: false,
  lineHeight: DEFAULT_LINE_HEIGHT,
  letterSpacing: 0,
};

const ALIGN_FACTOR: Readonly<Record<TextAlign, number>> = { left: 0, center: CENTER_FACTOR, right: 1 };
const DEFAULT_CELL: Cell = { ch: "", style: DEFAULT_STYLE, space: false, newline: false, advance: 0 };

// ── colour parsing ─────────────────────────────────────────────────────────────
/** Parse one two-hex-digit channel to `0..1`, sanitising a non-finite result. */
const channel = (hex: string, at: number): number => {
  const raw = Number.parseInt(hex.slice(at, at + HEX_PAIR), HEX_RADIX) / BYTE_MAX;
  return select(Number.isFinite(raw), raw, 0);
};

/** Expand a `#rgb` shorthand to `rrggbb` (each nibble doubled). */
const expandShort = (hex: string): string =>
  Array.from(hex)
    .map((nibble): string => nibble + nibble)
    .join("");

/** Normalise a hex string (with/without `#`, 3/6/8 digits) to 8 digits. */
const normaliseHex = (input: string): string => {
  const bare = input.replace("#", "");
  const expanded = select(bare.length === SHORT_HEX_LEN, expandShort(bare), bare);
  return pick([`${expanded}${OPAQUE_HEX}`, expanded], Number(expanded.length === RGBA_HEX_LEN));
};

/** Parse a `"#rrggbbaa"`-style string into an `Rgba`. */
const parseHex = (input: string): Rgba => {
  const full = normaliseHex(input);
  const at = (offset: number): number => channel(full, offset);
  return [at(0), at(HEX_PAIR), at(HEX_PAIR * HEX_PAIR), at(RGB_HEX_LEN)];
};

/** Resolve a colour input (hex string or tuple) to an `Rgba`. A type-guard filter
 * partitions the union without an unsafe assertion. */
const parseColor = (input: string | Rgba): Rgba => {
  const fromHex = [input].filter((value): value is string => typeof value === "string").map((value): Rgba => parseHex(value));
  const fromTuple = [input].filter((value): value is Rgba => typeof value !== "string");
  return pick([...fromTuple, ...fromHex], 0);
};

/** A colour override, present only when the input supplied one. */
const overrideColor = (input: string | Rgba | undefined): Rgba | undefined =>
  presentOf(input).map((value): Rgba => parseColor(value))[0];

// ── style + box resolution ──────────────────────────────────────────────────────
const resolveStyle = (base: ResolvedStyle, patch: TextStyleInput): ResolvedStyle => ({
  fontSize: orElse(patch.fontSize, base.fontSize),
  color: orElse(overrideColor(patch.color), base.color),
  weight: orElse(patch.weight, base.weight),
  italic: orElse(patch.italic, base.italic),
  lineHeight: orElse(patch.lineHeight, base.lineHeight),
  letterSpacing: orElse(patch.letterSpacing, base.letterSpacing),
});

const resolveBox = (input: TextLayoutInput): ResolvedBox => ({
  width: orElse(input.width, Number.POSITIVE_INFINITY),
  align: orElse(input.align, "left"),
  wrap: orElse(input.wrap, "none"),
});

const EMPTY_STYLE: TextStyleInput = {};

/** Normalise plain/rich content into runs (each carrying its raw override). A
 * type-guard filter partitions string vs span items without an assertion. */
const toRuns = (content: TextContent): readonly Run[] =>
  [content].flat().map((item): Run => {
    const texts = [item].filter((value): value is string => typeof value === "string");
    const spans = [item].filter((value): value is TextSpanInput => typeof value !== "string");
    return {
      text: pick([...spans.map((span): string => orElse(span.text, "")), ...texts], 0),
      override: pick([...spans.map((span): TextStyleInput => orElse(span.style, EMPTY_STYLE)), ...texts.map((): TextStyleInput => EMPTY_STYLE)], 0),
    };
  });

// ── layout ──────────────────────────────────────────────────────────────────────
/** Flatten runs to per-code-point cells with resolved style + advance. */
const toCells = (state: State): readonly Cell[] =>
  state.runs.flatMap((run): readonly Cell[] => {
    const style = resolveStyle(state.baseStyle, run.override);
    const advance = style.fontSize * ADVANCE_RATIO + style.letterSpacing;
    return codePoints(run.text).map((ch): Cell => ({
      ch,
      style,
      space: ch === " ",
      newline: ch === "\n",
      advance,
    }));
  });

/** Split a cell stream into lines at explicit newlines (the newline cell is
 * dropped; it only terminates its line). */
const splitNewlines = (cells: readonly Cell[]): readonly Cell[][] =>
  cells.reduce<Cell[][]>(
    (lines, cell) => {
      const rest = lines.slice(0, -1);
      const current = pick(lines, lines.length - 1);
      const next: Cell[][] = select(cell.newline, [current, [] as Cell[]], [[...current, cell]]);
      return [...rest, ...next];
    },
    [[]],
  );

/** Group a line into alternating word / whitespace runs (by `space`). */
const groupRuns = (line: readonly Cell[]): readonly Cell[][] =>
  line.reduce<Cell[][]>((runs, cell) => {
    const rest = runs.slice(0, -1);
    const current = orElse(runs.at(-1), [] as Cell[]);
    const head = orElse(current.at(0), cell);
    const same = all([current.length > 0, head.space === cell.space]);
    return select(same, [...rest, [...current, cell]], [...runs, [cell]]);
  }, []);

const runWidth = (run: readonly Cell[]): number => run.reduce((sum, cell) => sum + cell.advance, 0);

interface PackAcc {
  readonly lines: Cell[][];
  readonly current: Cell[];
  readonly width: number;
}

/** Greedily pack word/space runs into sub-lines that fit `maxWidth`. */
const packRuns = (runs: readonly Cell[][], maxWidth: number): readonly Cell[][] => {
  const seed: PackAcc = { lines: [], current: [], width: 0 };
  const packed = runs.reduce<PackAcc>((acc, run) => {
    const rw = runWidth(run);
    const isSpace = orElse(run.at(0), DEFAULT_CELL).space;
    const overflow = all([acc.current.length > 0, acc.width + rw > maxWidth, !isSpace]);
    return select(
      overflow,
      { lines: [...acc.lines, acc.current], current: [...run], width: rw },
      { lines: acc.lines, current: [...acc.current, ...run], width: acc.width + rw },
    );
  }, seed);
  return [...packed.lines, packed.current];
};

/** Apply word wrapping to a line when enabled and the width is finite. */
const wrapLine = (line: readonly Cell[], box: ResolvedBox): readonly Cell[][] => {
  const enabled = all([box.wrap === "word", Number.isFinite(box.width)]);
  return select(enabled, packRuns(groupRuns(line), box.width), [[...line]]);
};

interface Positioned {
  readonly glyphs: TextGlyph[];
  readonly cursorY: number;
  readonly maxWidth: number;
}

const lineWidth = (line: readonly Cell[]): number => line.reduce((sum, cell) => sum + cell.advance, 0);
const lineFontSize = (line: readonly Cell[]): number =>
  line.reduce((peak, cell) => Math.max(peak, cell.style.fontSize), DEFAULT_FONT_SIZE);
const lineHeightFactor = (line: readonly Cell[]): number =>
  line.reduce((peak, cell) => Math.max(peak, cell.style.lineHeight), DEFAULT_LINE_HEIGHT);

interface LineEntry {
  readonly line: readonly Cell[];
  readonly index: number;
}

/** Position one line's drawable cells into glyph quads. */
const placeLine = (acc: Positioned, entry: LineEntry, state: State): Positioned => {
  const { line, index } = entry;
  const fontSize = lineFontSize(line);
  const height = fontSize * lineHeightFactor(line);
  const width = lineWidth(line);
  const free = Math.max(0, state.box.width - width);
  const offsetX = select(Number.isFinite(free), free, 0) * ALIGN_FACTOR[state.box.align];
  const startX = state.position[0] + offsetX;
  const topY = state.position[1] + acc.cursorY;
  const placed = line.reduce<{ glyphs: TextGlyph[]; penX: number }>(
    (run, cell) => {
      const drawn = [cell]
        .filter((candidate): boolean => !candidate.space)
        .map((visible): TextGlyph => ({
          char: visible.ch,
          x: run.penX,
          y: topY,
          width: visible.advance,
          height: visible.style.fontSize,
          color: visible.style.color,
          fontSize: visible.style.fontSize,
          line: index,
        }));
      return { glyphs: [...run.glyphs, ...drawn], penX: run.penX + cell.advance };
    },
    { glyphs: [], penX: startX },
  );
  return {
    glyphs: [...acc.glyphs, ...placed.glyphs],
    cursorY: acc.cursorY + height,
    maxWidth: Math.max(acc.maxWidth, width),
  };
};

/** Lay a state out into positioned glyphs and its overall bounds. */
const layout = (state: State): { readonly glyphs: readonly TextGlyph[]; readonly bounds: TextBounds } => {
  const lines = splitNewlines(toCells(state)).flatMap((line): readonly Cell[][] => wrapLine(line, state.box));
  const positioned = lines.reduce<Positioned>((acc, line, index) => placeLine(acc, { line, index }, state), {
    glyphs: [],
    cursorY: 0,
    maxWidth: 0,
  });
  return {
    glyphs: positioned.glyphs,
    bounds: { width: positioned.maxWidth, height: positioned.cursorY, lineCount: lines.length },
  };
};

// ── the immutable Text value ────────────────────────────────────────────────────
const make = (state: State): Text => ({
  position: state.position,
  space: state.space,
  visible: state.visible,
  setText: (content: TextContent): Text => make({ ...state, runs: toRuns(content) }),
  setStyle: (patch: TextStyleInput): Text => make({ ...state, baseStyle: resolveStyle(state.baseStyle, patch) }),
  setLayout: (patch: TextLayoutInput): Text => make({ ...state, box: { ...state.box, ...resolveBox(patch) } }),
  setPosition: (x: number, y: number): Text => make({ ...state, position: [x, y] }),
  setVisible: (visible: boolean): Text => make({ ...state, visible }),
  measure: (): TextBounds => layout(state).bounds,
  glyphs: (): readonly TextGlyph[] => pick([[], layout(state).glyphs], Number(state.visible)),
});

/** Create a Text value from plain or rich content. */
export const text = (content: TextContent, options: TextOptions = {}): Text =>
  make({
    baseStyle: resolveStyle(DEFAULT_STYLE, orElse(options.style, {})),
    runs: toRuns(content),
    box: resolveBox(orElse(options.layout, {})),
    position: orElse(options.position, [0, 0]),
    space: orElse(options.space, "screen"),
    visible: orElse(options.visible, true),
  });

/** The spec-shaped grouping so `axiom.text("Hello, world")` reads verbatim. */
export const axiom = { text } as const;
