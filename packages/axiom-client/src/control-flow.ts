/*
 * The branchless control-flow operators the client spine is written in. The
 * Branchless Law (TS) forbids `for`/`if`/`?:`, so conditional iteration and
 * selection are expressed here as total, pure operators over arrays:
 *   - `pick` indexes an options array (asserting the index is in range, which
 *     also narrows away the `noUncheckedIndexedAccess` `undefined`),
 *   - `each` runs a side effect per element (no `for`/`forEach`/`reduce`).
 */

import { assert } from "./protocol-error.ts";

/*
 * Narrow an indexed element to Value, gated on an in-range check (a numeric
 * comparison, so no `undefined`/typeof/null token is needed). TS trusts the
 * `asserts value is Value` signature; `inRange` keeps it sound.
 */
const assertPresent: <Value>(value: Value | undefined, inRange: boolean) => asserts value is Value = (
  _value,
  inRange,
): void => {
  assert(inRange, "branchless selection index out of range");
};

/** Branchlessly select `options[index]`, asserting the index is in range. */
export const pick = <Value>(options: readonly Value[], index: number): Value => {
  const chosen = options[index];
  assertPresent(chosen, index < options.length);
  return chosen;
};

const SIDE_EFFECT = 0;

/*
 * Run `effect` for each value. Side-effect iteration without `for...of` (branch
 * ban), `.forEach` (no-array-forEach), or `.reduce` (no-array-reduce): `.map`
 * with a constant return satisfies array-callback-return; the result is unused.
 */
export const each = <Value>(values: readonly Value[], effect: (value: Value) => void): void => {
  values.map((value): number => {
    effect(value);
    return SIDE_EFFECT;
  });
};
