/*
 * Coerce a string-or-bytes value to bytes. Branchless: exactly one of the two
 * filters matches, and concatBytes keeps the single surviving chunk.
 */

import { concatBytes } from "./byte-writer.ts";

const encodeText = (text: string): Uint8Array => new TextEncoder().encode(text);

/** Coerce a string (UTF-8 encoded) or Uint8Array to bytes. */
export const toBytes = (value: string | Uint8Array): Uint8Array =>
  concatBytes([
    ...[value].filter((item): item is Uint8Array => item instanceof Uint8Array),
    ...[value].filter((item): item is string => typeof item === "string").map((text): Uint8Array => encodeText(text)),
  ]);
