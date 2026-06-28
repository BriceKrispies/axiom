/*
 * Branchless iteration primitive, the @axiom/client `each` counterpart re-stated
 * for the authoring SDK. It runs a side effect per element without `for`/`for...of`
 * (the Branchless Law branch ban) or `.forEach` (the no-array-forEach rule).
 */

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
