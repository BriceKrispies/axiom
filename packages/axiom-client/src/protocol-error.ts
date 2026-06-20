/*
 * Error type and branchless validation primitives for the wire codec.
 *
 * `assert` is a TypeScript assertion function: it narrows the caller's type and
 * throws on failure, with no control-flow branch in its own body (it selects the
 * failing arm by slicing a one-element array to length 0 or 1). This is how the
 * codec validates and narrows without `if`/`?:`/`&&` under the Branchless Law.
 */

/** Thrown when a frame is malformed, truncated, out of bounds, or invalid. */
export class ProtocolError extends Error {
  public constructor(message: string) {
    super(message);
    this.name = "ProtocolError";
  }
}

/** Always throw a {@link ProtocolError}; typed `never` so it composes in expressions. */
export const fail = (message: string): never => {
  throw new ProtocolError(message);
};

/** Branchlessly assert a condition, throwing {@link ProtocolError} when it is false. */
export const assert: (condition: boolean, message: string) => asserts condition = (
  condition,
  message,
): void => {
  // Slice to length 0 (condition true -> no throw) or 1 (false -> map calls fail).
  [message].slice(Number(condition)).map((reason): never => fail(reason));
};
