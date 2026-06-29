/*
 * Error type and branchless validation primitive for the authoring surface — the
 * @axiom/game counterpart of @axiom/client's `protocol-error.ts`.
 *
 * `assert` is a TypeScript assertion function: it narrows the caller's type and
 * throws on failure, with no control-flow branch in its own body (it selects the
 * failing arm by slicing a one-element array to length 0 or 1). This is how the
 * authoring surface validates and narrows without `if`/`?:`/`&&` under the
 * Branchless Law.
 */

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

/** Branchlessly assert a condition, throwing {@link AuthoringError} when it is false. */
export const assert: (condition: boolean, message: string) => asserts condition = (
  condition,
  message,
): void => {
  // Slice to length 0 (condition true -> no throw) or 1 (false -> map calls fail).
  [message].slice(Number(condition)).map((reason): never => fail(reason));
};
