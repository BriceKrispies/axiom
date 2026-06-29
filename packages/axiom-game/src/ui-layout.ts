/*
 * `solveLayout` (SPEC-09 §4.2) — the author-facing projection of the engine's
 * responsive flex solver (`axiom_layout::solve`, reached through the Wave-2
 * `uiSolveLayout` export). The author hands a `LayoutNode` tree + a viewport and
 * gets back each node's solved screen rect, keyed by id — the placement the HUD
 * draws into. No flex math runs in TS: this file only TRANSLATES the tree into the
 * flat node table the native solver consumes and reshapes the flat rect result
 * back, exactly the contract-boundary translation an app does between two module
 * facades (SPEC-14).
 *
 * The flat table the export consumes is `NODE_STRIDE`-wide records
 * `[parent, directionIdx, justifyIdx, alignIdx, gap, basis, grow]` (see
 * `apps/axiom-game-runtime/src/ui.rs`). A preorder flatten assigns each node a dense
 * index — parent always before child, which the native builder requires — so the
 * `parent` column is the parent's index (or a root's negative sentinel). The
 * contract's `aspect` has no column in the Wave-2 export and is dropped here (a
 * documented partial; the seven columns are all the native record carries).
 */

import { each, orElse, pick } from "./control-flow.ts";
import type { Rect } from "./vocabulary.ts";
import { boundHost } from "./host-binding.ts";

/** A responsive flex node (SPEC-09 §4.2 `LayoutNode`): an id, optional stacking direction, and flex sizing. */
export interface LayoutNode {
  /** The stable id the solved rect is keyed by in the result. */
  readonly id: string;
  /** The stacking direction of this node's children (default: `row`). */
  readonly direction?: "row" | "column";
  /** The flex grow factor — share of the free main-axis space (default: 0). */
  readonly grow?: number;
  /** The flex basis — the node's base main-axis size before grow (default: 0). */
  readonly basis?: number;
  /** The width/height ratio to hold (dropped: the Wave-2 export carries no aspect column). */
  readonly aspect?: number;
  /** This node's children, laid out along `direction`. */
  readonly children?: readonly LayoutNode[];
}

/** One node flattened to its id + the flex columns the native record needs. */
interface FlatNode {
  readonly id: string;
  readonly parent: number;
  readonly direction: number;
  readonly basis: number;
  readonly grow: number;
}

/** A root node's `parent` column — a negative index the native solver reads as a root. */
const ROOT_PARENT = -1;
/** The dense stacking-direction indices the native `style_from` selects (0=row, 1=column). */
const DIRECTIONS: readonly LayoutNode["direction"][] = ["row", "column"];
/** The dense direction indices, indexed by `DIRECTIONS.indexOf(direction)`. */
const DIRECTION_INDICES: readonly number[] = [0, 1];
/** The default flex basis / grow / gap when a node omits them. */
const DEFAULT_SIZE = 0;
/** `Justify::Start` / `Align::Start` — the mobile-first defaults `LayoutNode` carries no field for. */
const START = 0;
/** The four scalars per solved rect (`[x, y, w, h]`) the export returns per node. */
const RECT_STRIDE = 4;
/** The width / height offsets within one solved `[x, y, w, h]` rect block. */
const Y_OFFSET = 1;
const W_OFFSET = 2;
const H_OFFSET = 3;

/*
 * Flatten the tree to a preorder list, assigning each node its dense index and its
 * parent's index. Recursion (not a loop) walks the children — branchless: `each`
 * over `orElse(children, [])` runs nothing at a leaf, so there is no base-case
 * `if`. Parent is pushed before its children, so every `parent` column is already
 * assigned when a child records it.
 */
const flatten = (root: LayoutNode): readonly FlatNode[] => {
  const acc: FlatNode[] = [];
  const visit = (node: LayoutNode, parent: number): void => {
    const self = acc.length;
    acc.push({
      basis: orElse(node.basis, DEFAULT_SIZE),
      direction: pick(DIRECTION_INDICES, DIRECTIONS.indexOf(orElse(node.direction, "row"))),
      grow: orElse(node.grow, DEFAULT_SIZE),
      id: node.id,
      parent,
    });
    each(orElse(node.children, []), (child): void => {
      visit(child, self);
    });
  };
  visit(root, ROOT_PARENT);
  return acc;
};

/** The seven flat columns of one node: `[parent, directionIdx, justifyIdx, alignIdx, gap, basis, grow]`. */
const columnsOf = (node: FlatNode): readonly number[] => [
  node.parent,
  node.direction,
  START,
  START,
  DEFAULT_SIZE,
  node.basis,
  node.grow,
];

/** The solved rect for the node at dense `index`, read from the flat `[x, y, w, h]…` result. */
const rectAt = (solved: readonly number[], index: number): Rect => ({
  height: pick(solved, index * RECT_STRIDE + H_OFFSET),
  width: pick(solved, index * RECT_STRIDE + W_OFFSET),
  x: pick(solved, index * RECT_STRIDE),
  y: pick(solved, index * RECT_STRIDE + Y_OFFSET),
});

/*
 * Solve `root` against `viewport` and return each node's screen rect keyed by id.
 * The `viewport` is a `Rect` (the contract shape); only its `width`/`height` cross
 * to the solver (screen space is top-left origin, so the viewport's `x`/`y` are
 * dropped). Forwards to `axiom_layout::solve` through the bound host — no TS math.
 */
export const solveLayout = (root: LayoutNode, viewport: Rect): Record<string, Rect> => {
  const nodes = flatten(root);
  const solved = boundHost().uiSolveLayout(
    { height: viewport.height, width: viewport.width },
    nodes.flatMap((node): readonly number[] => columnsOf(node)),
  );
  return Object.fromEntries(
    nodes.map((node, index): readonly [string, Rect] => [node.id, rectAt(solved, index)]),
  );
};
