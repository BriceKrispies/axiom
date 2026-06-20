/*
 * A little-endian byte reader over a DataView. Every read first asserts that the
 * frame still has enough bytes, so a truncated frame throws a ProtocolError
 * instead of reading past the end.
 */

import { assert } from "./protocol-error.ts";

const SIZE_U8 = 1;
const SIZE_U16 = 2;
const SIZE_U32 = 4;
const SIZE_U64 = 8;
const AT_START = 0;
const LITTLE_ENDIAN = true;

/** A little-endian byte reader. */
export interface ByteReader {
  readonly byteSlice: () => Uint8Array;
  readonly u16: () => number;
  readonly u32: () => number;
  readonly u64: () => number;
  readonly u8: () => number;
}

/** Create a byte reader positioned at the start of `data`. */
export const byteReader = (data: Uint8Array): ByteReader => {
  const view = new DataView(data.buffer, data.byteOffset, data.byteLength);
  let position = AT_START;
  const need = (count: number): void => {
    assert(position + count <= data.length, "frame ended before a value could be read");
  };
  const read = <Value,>(size: number, get: (offset: number) => Value): Value => {
    need(size);
    const value = get(position);
    position += size;
    return value;
  };
  const u32 = (): number => read(SIZE_U32, (offset): number => view.getUint32(offset, LITTLE_ENDIAN));
  return {
    byteSlice: (): Uint8Array => {
      const length = u32();
      need(length);
      const out = data.slice(position, position + length);
      position += length;
      return out;
    },
    u16: (): number => read(SIZE_U16, (offset): number => view.getUint16(offset, LITTLE_ENDIAN)),
    u32,
    u64: (): number =>
      read(SIZE_U64, (offset): number => Number(view.getBigUint64(offset, LITTLE_ENDIAN))),
    u8: (): number => read(SIZE_U8, (offset): number => view.getUint8(offset)),
  };
};
