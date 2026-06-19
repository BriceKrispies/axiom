# Axiom Net Protocol — Testing

All tests are inline `#[cfg(test)]` modules next to the code they cover. Run
them with:

```sh
cargo test -p axiom-net-protocol
```

The module is part of the engine spine, so it is held to the Coverage Law (100%
regions/lines/functions) and the Branchless Law (zero control flow in non-test
code). The workspace coverage gate covers it:

```sh
scripts/coverage.ps1        # Windows
bash scripts/coverage.sh    # Linux / CI
```

## What the tests prove

### Field validators (one file per field)
- `protocol_version` — nonzero accepted, zero rejected (`Message`/`InvalidId`),
  serialization round-trips, decode re-rejects zero and truncation.
- `client_id` — nonzero accepted, zero rejected, round-trips, decode re-rejects
  zero and truncation.
- `room_id` — non-empty + bounded accepted (incl. exact max), empty and
  over-long rejected (`Message`/`OutOfBounds`), round-trips, decode re-rejects
  empty and truncation.
- `opaque_payload` — empty/bounded/exact-max accepted, over-max rejected,
  round-trips, decode re-rejects over-max and truncation.

### Frame envelope (`frame`)
- Header round-trips to the written kind; every known kind (`0..=6`) peeks back.
- `read_expected_kind` accepts the matching kind and rejects a mismatched or
  unknown kind (`InvalidDiscriminant`).
- `peek_kind` rejects an unknown kind and a truncated header.
- An incompatible wire major is rejected (`SchemaVersionMismatch`).

### Every message type (one file each)
For `JoinRoom`, `LeaveRoom`, `ClientIntent`, `Welcome`, `ServerSnapshot`,
`ServerEvent`, `RejectedIntent`:
- accessors return the constructed fields;
- **round-trip**: `decode(encode(m)) == m`;
- construction surfaces each field's validation failure;
- decoding a *different* message's frame fails on the kind discriminant;
- **every truncated prefix** of an encoded frame fails to decode (walks the
  error arm of every field read), while the full frame decodes.
- `Welcome` additionally proves a zero `fixed_step_ns` is rejected on both
  construct and decode.

### Facade (`net_protocol_api`)
- Each of the seven messages round-trips end-to-end through
  `encode_*` → `message_kind` → `decode_*` as plain primitives.
- Encoders surface validation failures.
- Decoding a frame as the wrong message is rejected (`InvalidDiscriminant`).
- `message_kind` rejects an unknown/empty frame.

## Determinism

Encoding is little-endian and field-ordered with no map iteration, no randomness,
and no clock, so a given message always produces the same bytes — the round-trip
and prefix tests assert exactly that.
