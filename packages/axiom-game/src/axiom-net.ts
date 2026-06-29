/*
 * The netcode binding (SPEC-13 §16): it adapts the `@axiom/client` browser client
 * into `@axiom/game`'s `NetTransport` seam, and owns the deterministic wire codec
 * for the author's flat `Intent` record.
 *
 * ## Why this is a covered SPINE file, not a platform edge
 * The sibling wasm adapters (`wasm-bridge.ts` / `wasm-host.ts`) are coverage-exempt
 * because their correctness IS the live wasm byte layout, only verifiable in a
 * browser. This file is different: it never touches a socket or `@axiom/client`
 * itself. It binds against a structural `AxiomClientLike` interface — the subset of
 * `AxiomClient`'s surface it uses — which a real `AxiomClient` satisfies and a fake
 * satisfies just as well. So the whole adapter is pure, branchless forwarding +
 * byte codec over an injected collaborator: fully testable, held to the 100%
 * coverage + Branchless laws like the rest of the spine. The one genuinely
 * browser-only step — constructing a real `AxiomClient` and calling `connect` — is
 * the `open` callback the APP supplies at boot, outside this SDK (see below). That
 * is also why this file does NOT import `@axiom/client`: the two packages stay
 * decoupled (no workspace path-resolution dependency), and the SDK's net surface is
 * total before any runtime is present (exactly as `net.ts`'s inert transport is).
 *
 * ## What the engine "owns the wire codec" means here
 * `net.ts` states the engine owns the `Intent` wire codec so no hand-written twin
 * lives in author code. The author's `Intent` is a runtime-dynamic
 * `Record<string, boolean | number | string>`, so the codec is the single
 * deterministic, self-describing flat-record format below: a `u32` field count,
 * then per field (in sorted-key order, so insertion order can't change the bytes) a
 * length-prefixed key, a one-byte type tag, and the tagged value. `encodeIntent`
 * produces the payload `AxiomClient.sendIntent` ships; `decodeIntent` is its exact
 * inverse (the round-trip is the codec's proof), the primitive an authority /
 * participants decoder built on this convention reconstructs an `Intent` with. It
 * reuses the same little-endian scalar / `u32`-length-prefixed-UTF-8 convention as
 * the retained-world component codec (`wasm-bridge.ts`).
 *
 * ## Page-boot wiring (in the APP, not here)
 * The app installs the real transport once, where `@axiom/client` resolves:
 *
 *   import { AxiomClient } from "@axiom/client";
 *   import { axiomNetFactory } from "@axiom/game";
 *   bindNetTransport(axiomNetFactory((config) => {
 *     const client = new AxiomClient();
 *     client.connect({ roomId: config.roomId, token: config.token, url: config.url });
 *     return client;
 *   }));
 *
 * ## Deferred: `NetParticipants` from authority snapshots
 * The transport arm (join / intent / status / local-player / rejection) binds in
 * full. The `NetParticipants` arm (per-tick `players`/`inputOf`/`joinedThisTick`/
 * `leftThisTick`) is fed by DECODING the authority's snapshot payload, whose
 * participant schema SPEC-13's high-level authoring layer has not yet defined
 * (`AxiomClient.onSnapshot` delivers the payload as opaque bytes). Building that
 * decoder now would be guessing at an unspecified format; `decodeIntent` here is the
 * codec it will compose once the schema lands. Until then the runtime supplies the
 * single-seat / local `NetParticipants` `net.ts` already documents.
 */

import type { ConnStatus, Intent, JoinConfig, NetTransport, NetTransportFactory } from "./net.ts";
import type { PlayerId, Result } from "./vocabulary.ts";
import { pick } from "./control-flow.ts";

/*
 * The subset of `AxiomClient` (`@axiom/client`) this adapter drives. A real
 * `AxiomClient` satisfies it structurally — its `getStatus(): ClientStatus` returns
 * exactly the `"connected" | "connecting" | "disconnected"` union `ConnStatus` is —
 * and a test fake satisfies it without a socket. `connect` is deliberately absent:
 * the app opens the client in the `open` callback before handing it here.
 */
export interface AxiomClientLike {
  /** The current connection status (the authority decides it). */
  readonly getStatus: () => ConnStatus;
  /** The server-assigned client id, `0` until the `Welcome` arrives. */
  readonly getClientId: () => number;
  /** Encode-and-send the local player's intent payload; returns the assigned sequence. */
  readonly sendIntent: (payload: Uint8Array) => number;
  /** Register a connection-status observer. */
  readonly onStatus: (handler: (status: ConnStatus) => void) => void;
  /** Register a rejected-intent observer (the authority's `REASON_*` code). */
  readonly onRejected: (handler: (reasonCode: number) => void) => void;
  /** Send `LeaveRoom` (when connected) and close the connection. */
  readonly disconnect: () => void;
}

/** The byte width of a `u32` field-count / length prefix or an `f32`-sized scalar. */
const U32_BYTES = 4;
/** The byte width of an `f64` numeric field value. */
const F64_BYTES = 8;

/** The one-byte type tags discriminating an `Intent` field's value on the wire. */
const TAG_BOOL = 0;
const TAG_NUMBER = 1;
const TAG_STRING = 2;

/*
 * The `REASON_*` code → text table (mirrors `@axiom/client`'s `messages.ts` and the
 * Rust `axiom-net-protocol` reason codes). An out-of-range code maps to the
 * `unspecified` fallback via a branchless clamp: `min(code, length)` lands on the
 * trailing fallback entry exactly when the code names no known reason.
 */
const REASON_TEXT: readonly string[] = ["unspecified", "malformed", "out-of-order", "not-in-room", "unspecified"];

/** The machine-readable rejection code as the seam's human reason string. */
const reasonText = (code: number): string => pick(REASON_TEXT, Math.min(code, REASON_TEXT.length - 1));

/** A scalar's little-endian bytes, produced by `write` into a fresh `width`-byte view. */
const scalarBytes = (width: number, write: (view: DataView) => void): readonly number[] => {
  const view = new DataView(new ArrayBuffer(width));
  write(view);
  return [...new Uint8Array(view.buffer)];
};

/** A `u32` as its 4 little-endian bytes. */
const u32Bytes = (value: number): readonly number[] =>
  scalarBytes(U32_BYTES, (view): void => {
    view.setUint32(0, value, true);
  });

/** A length-prefixed UTF-8 string: a `u32` byte length then the UTF-8 bytes. */
const stringBytes = (value: string): readonly number[] => {
  const utf8 = [...new TextEncoder().encode(value)];
  return [...u32Bytes(utf8.length), ...utf8];
};

/*
 * The three value encoders, indexed by type tag. Each takes the field's
 * `boolean | number | string` value and coerces to its own type totally — `Number`
 * maps a boolean to `0`/`1` and a number to itself, `String` is identity on a
 * string — so no value is ever down-cast (the `no-unsafe-type-assertion` law).
 */
const VALUE_ENCODERS: readonly ((value: boolean | number | string) => readonly number[])[] = [
  (value): readonly number[] => [TAG_BOOL, Number(value)],
  (value): readonly number[] => [TAG_NUMBER, ...scalarBytes(F64_BYTES, (view): void => {
    view.setFloat64(0, Number(value), true);
  })],
  (value): readonly number[] => [TAG_STRING, ...stringBytes(String(value))],
];

/*
 * The value's encoder index (== its type tag): `boolean -> 0`, `number -> 1`,
 * `string -> 2`. Computed branchlessly — exactly one `typeof` test is true, and the
 * `string` test is weighted by 2 — so it selects without an `if`/`?:`.
 */
const typeIndex = (value: boolean | number | string): number =>
  Number(typeof value === "number") * TAG_NUMBER + Number(typeof value === "string") * TAG_STRING;

/*
 * A stable code-point key comparator (NOT `localeCompare`, which is locale- and
 * thus machine-dependent), expressed branchlessly: `(a > b) - (a < b)` is `-1`/`0`/
 * `+1` by code point. Sorting keys makes the encoded bytes independent of the
 * record's insertion order — the determinism the wire codec requires.
 */
const byKey = (left: readonly [string, unknown], right: readonly [string, unknown]): number =>
  Number(left[0] > right[0]) - Number(left[0] < right[0]);

/** Encode an author `Intent` to its deterministic, self-describing payload bytes. */
export const encodeIntent = (intent: Intent): Uint8Array => {
  const entries = Object.entries(intent).toSorted(byKey);
  const body = entries.flatMap(([key, value]): readonly number[] => [
    ...stringBytes(key),
    ...pick(VALUE_ENCODERS, typeIndex(value))(value),
  ]);
  return Uint8Array.from([...u32Bytes(entries.length), ...body]);
};

/** A `DataView` over `bytes`' exact backing region (respecting any sub-array offset). */
const viewOf = (bytes: Uint8Array): DataView => new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);

/** Read a length-prefixed UTF-8 string at `offset`, returning `[value, nextOffset]`. */
const readString = (bytes: Uint8Array, offset: number): readonly [string, number] => {
  const start = offset + U32_BYTES;
  const end = start + viewOf(bytes).getUint32(offset, true);
  return [new TextDecoder().decode(bytes.subarray(start, end)), end];
};

/*
 * The three value decoders, indexed by the field's type tag, each returning
 * `[value, nextOffset]`. The inverse of `VALUE_ENCODERS`.
 */
const VALUE_DECODERS: readonly ((bytes: Uint8Array, offset: number) => readonly [boolean | number | string, number])[] =
  [
    (bytes, offset): readonly [boolean | number | string, number] => [viewOf(bytes).getUint8(offset) !== 0, offset + 1],
    (bytes, offset): readonly [boolean | number | string, number] => [
      viewOf(bytes).getFloat64(offset, true),
      offset + F64_BYTES,
    ],
    (bytes, offset): readonly [boolean | number | string, number] => readString(bytes, offset),
  ];

/*
 * Decode payload bytes back to an `Intent` — the exact inverse of `encodeIntent`. A
 * single mutable `cursor` walks the buffer as `Array.from`'s map visits each of the
 * `count` fields (the field count was encoded up front, so iteration is bounded and
 * needs no `while`): read the key, the type tag, then the tag's value.
 */
export const decodeIntent = (bytes: Uint8Array): Intent => {
  const view = viewOf(bytes);
  const count = view.getUint32(0, true);
  const cursor = { offset: U32_BYTES };
  const pairs = Array.from({ length: count }, (): readonly [string, boolean | number | string] => {
    const [key, afterKey] = readString(bytes, cursor.offset);
    const tag = view.getUint8(afterKey);
    const [value, next] = pick(VALUE_DECODERS, tag)(bytes, afterKey + 1);
    cursor.offset = next;
    return [key, value];
  });
  return Object.fromEntries(pairs);
};

/** No seats — the empty list whose missing first element is the absent player value. */
const NO_SEATS: readonly PlayerId[] = [];

/** The absent local-player value (an un-admitted client), without the banned `undefined` literal. */
const absentPlayer = (): Result<PlayerId> => NO_SEATS.at(0);

/*
 * The local player's seat from the client id: `0` is "not yet admitted" (the absent
 * value), any other id is the seat. Branchless `pick` over two thunks so the present
 * arm never runs while un-admitted.
 */
const localPlayerOf = (clientId: number): Result<PlayerId> =>
  pick<() => Result<PlayerId>>(
    [(): Result<PlayerId> => absentPlayer(), (): Result<PlayerId> => clientId],
    Number(clientId !== 0),
  )();

/** Build a `NetTransport` over an opened `AxiomClientLike` — pure forwarding + the intent codec. */
export const netTransportFromClient = (client: AxiomClientLike): NetTransport => ({
  leave: (): void => {
    client.disconnect();
  },
  localPlayer: (): Result<PlayerId> => localPlayerOf(client.getClientId()),
  onRejected: (callback: (reason: string) => void): void => {
    client.onRejected((code: number): void => {
      callback(reasonText(code));
    });
  },
  onStatus: (callback: (status: ConnStatus) => void): void => {
    client.onStatus(callback);
  },
  sendIntent: (intent: Intent): void => {
    client.sendIntent(encodeIntent(intent));
  },
  status: (): ConnStatus => client.getStatus(),
});

/*
 * The runtime's `NetTransportFactory` over `@axiom/client`: each `joinRoom` opens a
 * client for the config (the `open` callback the app supplies — the one browser-only
 * step) and wraps it as a `NetTransport`. Pass this to `bindNetTransport`.
 */
export const axiomNetFactory =
  (open: (config: JoinConfig) => AxiomClientLike): NetTransportFactory =>
  (config: JoinConfig): NetTransport =>
    netTransportFromClient(open(config));
