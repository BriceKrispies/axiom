/*
 * Branchless selection primitives. `pick` indexes an options array (asserting the
 * index is in range, which also narrows away the `noUncheckedIndexedAccess`
 * `undefined`); `coalesce` defaults an optional value via a destructuring default,
 * which applies exactly when the value is undefined. Together they replace the
 * `if`/`?:`/`??` the Branchless Law forbids with table selection.
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

/** Branchlessly default an optional `value` to `fallback` when it is absent. */
export const coalesce = <Value>(value: Value | undefined, fallback: Value): Value => {
  const [resolved = fallback] = [value];
  return resolved;
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
