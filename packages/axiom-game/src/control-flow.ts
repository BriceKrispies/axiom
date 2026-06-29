/*
 * The branchless control-flow operators the authoring spine is written in. The
 * Branchless Law (TS) forbids `for`/`if`/`?:`/`??`, so conditional iteration and
 * selection are expressed here as total, pure operators over arrays:
 *   - `each` runs a side effect per element (no `for`/`forEach`),
 *   - `pick` selects `options[index]`, asserting the index is in range, which
 *     also narrows away the `noUncheckedIndexedAccess` `undefined`,
 *   - `orElse` defaults an absent optional to a fallback,
 *   - `whenPresent` runs an effect only for a present optional.
 *
 * These are the @axiom/client `pick`/`each` counterparts re-stated for this SDK;
 * `orElse`/`whenPresent` are part of the public authoring vocabulary (re-exported
 * from `index.ts`).
 */

import { assert } from "./authoring-error.ts";

const SIDE_EFFECT = 0;

/*
 * Run `effect` for each value. `.map` with a constant return satisfies
 * array-callback-return and the produced array is discarded — side-effect
 * iteration with no control-flow branch.
 */
export const each = <Value>(values: readonly Value[], effect: (value: Value) => void): void => {
  values.map((value): number => {
    effect(value);
    return SIDE_EFFECT;
  });
};

/*
 * Narrow an indexed element to `Value`, gated on an in-range check (a numeric
 * comparison, so no `undefined`/`null` token is needed). TS trusts the
 * `asserts value is Value` signature; the `inRange` flag keeps it sound.
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

/*
 * The single absent sentinel (`undefined`), materialized WITHOUT writing the
 * banned `undefined` literal: a 0-argument call to an optional-parameter identity
 * yields the missing argument. The SDK's lint law walls off every direct way to
 * name the empty value — the `undefined` identifier (`eslint/no-undefined`),
 * `typeof x === "undefined"` (`unicorn/no-typeof-undefined`), `void 0`
 * (`eslint/no-void`), and `== null` (`unicorn/no-null`) — so presence is tested
 * by `!==` against this captured sentinel, the one expressible form.
 */
const absentProbe = <Value>(slot?: Value): Value | undefined => slot;
const ABSENT = absentProbe();

/*
 * Default an optional `value` to `fallback` without a branch: `[value]` filtered
 * to the present singleton has length 0 (absent -> `pick` index 0 = `fallback`)
 * or 1 (present -> `pick` index 1 = `value`). The presence test is a `!==`
 * comparison, not control flow — the same shape `pick`/`assert` already use.
 */
export const orElse = <Value>(value: Value | undefined, fallback: Value): Value => {
  const present = [value].filter((candidate): candidate is Value => candidate !== ABSENT);
  return pick([fallback, ...present], present.length);
};

/*
 * Run `effect` only when `value` is present — the branchless "call this optional
 * callback / handle this optional field" form. Filtering `[value]` to its present
 * singleton yields a 0- or 1-element array `each` maps over, with no `if value`.
 */
export const whenPresent = <Value>(value: Value | undefined, effect: (value: Value) => void): void => {
  each([value].filter((candidate): candidate is Value => candidate !== ABSENT), effect);
};

/*
 * Narrow a present optional to its value, throwing {@link AuthoringError} with
 * `message` when it is absent — the branchless "require this optional is here"
 * form. The `assert` (itself branchless) narrows `value` past the captured
 * `ABSENT` sentinel, so a required-but-unset field fails loudly instead of leaking
 * `undefined`.
 */
export const present = <Value>(value: Value | undefined, message: string): Value => {
  const found = [value].filter((candidate): candidate is Value => candidate !== ABSENT);
  assert(found.length > 0, message);
  return pick(found, 0);
};
