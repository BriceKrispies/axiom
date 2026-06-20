/*
 * A little-endian byte writer built on DataView, so every integer is serialized
 * without bitwise operators or hand-rolled loops. Each scalar becomes a small
 * byte chunk; chunks are flattened once at `finish`.
 */

const SIZE_U8 = 1;
const SIZE_U16 = 2;
const SIZE_U32 = 4;
const SIZE_U64 = 8;
const AT_START = 0;
const LITTLE_ENDIAN = true;

/** A little-endian byte writer. */
export interface ByteWriter {
  readonly byteSlice: (data: Uint8Array) => void;
  readonly finish: () => Uint8Array;
  readonly u16: (value: number) => void;
  readonly u32: (value: number) => void;
  readonly u64: (value: number) => void;
  readonly u8: (value: number) => void;
}

const u8Bytes = (value: number): Uint8Array => {
  const bytes = new Uint8Array(SIZE_U8);
  new DataView(bytes.buffer).setUint8(AT_START, value);
  return bytes;
};

const u16Bytes = (value: number): Uint8Array => {
  const bytes = new Uint8Array(SIZE_U16);
  new DataView(bytes.buffer).setUint16(AT_START, value, LITTLE_ENDIAN);
  return bytes;
};

const u32Bytes = (value: number): Uint8Array => {
  const bytes = new Uint8Array(SIZE_U32);
  new DataView(bytes.buffer).setUint32(AT_START, value, LITTLE_ENDIAN);
  return bytes;
};

const u64Bytes = (value: number): Uint8Array => {
  const bytes = new Uint8Array(SIZE_U64);
  new DataView(bytes.buffer).setBigUint64(AT_START, BigInt(value), LITTLE_ENDIAN);
  return bytes;
};

/** Concatenate byte chunks into one Uint8Array. */
export const concatBytes = (chunks: readonly Uint8Array[]): Uint8Array =>
  Uint8Array.from(chunks.flatMap((chunk): readonly number[] => [...chunk]));

/** Create a fresh little-endian byte writer. */
export const byteWriter = (): ByteWriter => {
  const chunks: Uint8Array[] = [];
  const push = (bytes: Uint8Array): void => {
    chunks.push(bytes);
  };
  return {
    byteSlice: (data): void => {
      push(u32Bytes(data.length));
      push(data);
    },
    finish: (): Uint8Array => concatBytes(chunks),
    u16: (value): void => {
      push(u16Bytes(value));
    },
    u32: (value): void => {
      push(u32Bytes(value));
    },
    u64: (value): void => {
      push(u64Bytes(value));
    },
    u8: (value): void => {
      push(u8Bytes(value));
    },
  };
};
