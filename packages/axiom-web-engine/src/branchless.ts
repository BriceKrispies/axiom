/*
 * branchless.ts — the spine's shared BRANCHLESS selection + Option primitives.
 * These are the exact recipes from docs/unbranching.md, extracted once so the
 * store, the reconciler, and the input snapshot express "conditional value",
 * "Option flow", and "index selection" without an `if`/`?:`/`??` and without each
 * file re-deriving the same helpers.
 *
 * This is NOT a junk drawer: its charter is precisely the branchless value-
 * selection and absent-sentinel toolkit and nothing else. `absentProbe`/`ABSENT`
 * materialize the missing value without writing the banned `undefined` literal;
 * `fail`/`assert` throw through a value transform; `pick`/`select` index a table;
 * `presentOf`/`isPresent`/`demand`/`orElse`/`orCompute` model `Option` flow.
 */

/** The absent value, materialized WITHOUT the banned `undefined` literal: a
 * 0-argument call to an optional-parameter identity yields the missing argument.
 * Presence is tested by `!==` against this captured value. */
export const absentProbe = <Value>(slot?: Value): Value | undefined => slot;
export const ABSENT = absentProbe();

/** Throw `message` as a value-producing expression (usable inside a `.map`). */
export const fail = (message: string): never => {
  throw new Error(message);
};

/** Assert `condition`, branchlessly: true → `slice(1)` → `[]` (nothing runs);
 * false → `slice(0)` → `[message]` whose `.map` calls `fail`. */
export const assert: (condition: boolean, message: string) => asserts condition = (condition, message): void => {
  [message].slice(Number(condition)).map((reason): never => fail(reason));
};

const assertInRange: <Value>(value: Value | undefined, inRange: boolean) => asserts value is Value = (
  _value,
  inRange,
): void => {
  assert(inRange, "branchless: selection index out of range");
};

/** Index `options` at `index`, asserting the slot exists (coerces the
 * `noUncheckedIndexedAccess` `| undefined` away without a non-null assertion). */
export const pick = <Value>(options: readonly Value[], index: number): Value => {
  const chosen = options[index];
  assertInRange(chosen, index < options.length);
  return chosen;
};

/** `whenTrue` if `condition` else `whenFalse`, via a two-slot table index. */
export const select = <Value>(condition: boolean, whenTrue: Value, whenFalse: Value): Value =>
  pick([whenFalse, whenTrue], Number(condition));

/** `[value]` when present, `[]` when absent — a guard as a filterable singleton. */
export const presentOf = <Value>(value: Value | undefined): Value[] =>
  [value].filter((candidate): candidate is Value => candidate !== ABSENT);

/** True when `value` is present (not the absent sentinel). */
export const isPresent = (value: unknown): boolean => value !== ABSENT;

/** `value` if present, else throw `message` (the `Option::expect` analogue). */
export const demand = <Value>(value: Value | undefined, message: string): Value => {
  const found = presentOf(value);
  assert(found.length > 0, message);
  return pick(found, 0);
};

/** `value` if present, else `fallback` (the `Option::unwrap_or` analogue). */
export const orElse = <Value>(value: Value | undefined, fallback: Value): Value => {
  const found = presentOf(value);
  return pick([fallback, ...found], found.length);
};

/** `value` if present, else `compute()` — the lazy `Option::unwrap_or_else`. */
export const orCompute = <Value>(value: Value | undefined, compute: () => Value): Value => {
  const found = presentOf(value);
  const thunks: (() => Value)[] = [compute, ...found.map((candidate): (() => Value) => (): Value => candidate)];
  return pick(thunks, found.length)();
};
