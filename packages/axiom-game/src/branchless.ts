/*
 * Branchless primitives for the authoring spine — the @axiom/client `each`/`pick`
 * counterparts re-stated for this SDK. They replace the `for`/`if`/`?:` the
 * Branchless Law (TS) forbids with iterator and table-selection forms:
 *   - `each` runs a side effect per element (no `for`/`forEach`),
 *   - `assert` throws on a false condition with no `if` (it slices a one-element
 *     array to length 0 or 1),
 *   - `pick` selects `options[index]`, asserting the index is in range, which
 *     also narrows away the `noUncheckedIndexedAccess` `undefined`.
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

/** Thrown when an authoring call is given an out-of-range index or count. */
export class AuthoringError extends Error {
  public constructor(message: string) {
    super(message);
    this.name = "AuthoringError";
  }
}

/** Always throw an {@link AuthoringError}; typed `never` so it composes in expressions. */
export const fail = (message: string): never => {
  throw new AuthoringError(message);
};

/*
 * Branchlessly assert a condition, throwing {@link AuthoringError} when it is
 * false. Slicing `[message]` to length `Number(condition)` yields `[]` (true ->
 * no throw) or `[message]` (false -> `map` calls `fail`).
 */
export const assert: (condition: boolean, message: string) => asserts condition = (
  condition,
  message,
): void => {
  [message].slice(Number(condition)).map((reason): never => fail(reason));
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
