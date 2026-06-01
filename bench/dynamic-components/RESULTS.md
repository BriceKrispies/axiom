# Dynamic-component access: benchmark results

Run: `cargo run --release --manifest-path bench/dynamic-components/Cargo.toml`
(numbers are machine-dependent; rerun locally — these are one representative run).

Question: for **app-blind / type-erased** dynamic components, what does typed
access cost three ways?

| read-sum hot loop (1 field of `Transform`, 100k entities) | ns/access | µs/frame | vs downcast |
|---|---|---|---|
| `downcast` — safe `Any::downcast_ref` → `&T`            | 3.1  | 311  | 1.0× |
| `unsafe`   — `TypeId`-checked pointer cast → `&T`       | 2.8  | 281  | 0.9× |
| `bytes`    — `Reflect`-serialized, deserialize to owned `T` | 19.5 | 1954 | 6.3× |

Build/insert 100k: typed (move) ~5.5 ms · bytes (serialize) ~31.8 ms (~5.8×).

## What it means

- **No disk anywhere.** The "bytes" path serializes/deserializes to an in-memory
  `Vec<u8>`. The cost is CPU (re-parsing) + losing direct memory access, not I/O.
- **`downcast` ≈ `unsafe`.** Borrowed `&T` access is ~3 ns either way; the unsafe
  buys ~nothing in speed. Its only value is being *coverable* (no dead branch).
  So the real fork for the fast path is *which law you bend*: a coverage
  carve-out for the provably-dead downcast arm (stays safe) **or** lifting the
  `unsafe` ban (stays 100%). Same ~3 ns.
- **The real gap is reference vs reconstruction.** Fast paths hand out a pointer
  to a `Transform` that already exists in memory (~free). The bytes path
  *rebuilds* a `Transform` from its encoded form on every read (parse 10 bounds-
  checked floats) → ~6×.
- **Worst-case framing:** reads one field but reconstructs the whole 40-byte
  component every frame. Reading more fields, or deserializing a column once per
  frame into a scratch buffer, shrinks the gap.
- **The hot path needs none of this.** The static `World<S>` already gives free
  borrowed `&T` with no law bent. This tax applies only to *app-blind* dynamic
  components (mod/plugin/agent-authored types the app never named) — and only
  hurts if those are read in a hot per-frame loop.

## Takeaway

Reserve the law-bending fast path for a *measured* hot loop over dynamic
components. For the cold uses an app-blind store is actually for (save/load,
agent queries, tooling), the safe bytes path is fine and bends no law.
