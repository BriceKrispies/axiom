# Axiom Kernel — Testing

## Every public concept has direct behavioral tests

Each public thing in the kernel exists to do something, and a test proves *what*
it does — not merely that it compiles or "doesn't panic". Tests live next to the
code as `#[cfg(test)] mod tests` in each module file, so a type and its proof
sit together.

Coverage maps one-to-one onto the public surface:

| Concept | What the tests prove |
|---------|----------------------|
| `SimulationClock` | initial state is zero; one advance moves tick/frame/time; `advance_by(n)` equals `n` single advances; two clocks stay bit-identical; overflow is reported, not wrapped |
| ID types (`EntityId`, `ResourceId`, `AssetId`, `HandleId`, `LayerId`, `MessageId`) | null is invalid and zero; `from_raw` round-trips; ordering/equality are numeric; binary serialization round-trips to exactly 8 bytes |
| `KernelError` | identity is `(scope, code)`; equality ignores the human message; differing scope/code compare unequal |
| `MemoryRange` | `end`, half-open `contains_offset`/`contains_range`, `overlaps`, alignment, and `checked_shift` overflow |
| `Alignment` | powers of two accepted; zero and non-powers rejected; boundary check correct |
| `MessageQueue` | FIFO `pop` order; `peek` does not consume; empty `pop`; `clear` |
| `BinaryWriter`/`BinaryReader` | little-endian layout; all primitives + length-prefixed slices round-trip; **out-of-bounds reads fail via `KernelResult` without advancing**; truncated slices fail |
| `SchemaVersion` | accessors; compatibility by major only; serialization round-trip |
| `LayerManifest` / `LayerImportRule` | kernel has no imports; self-import fails; forward-import fails; valid earlier-layer import succeeds; duplicate dependency/capability rejected |
| `LogRecord` / `InMemoryLogSink` | builder captures level/scope/code/tick/frame/fields; identical inputs produce equal records; sink captures in order, deterministically |
| `TelemetryMetric` / `InMemoryTelemetrySink` | counter and gauge construction; deterministic capture; `counter_total` sums only matching counters |
| `KernelApi` facade | smoke test reaching every capability through the single public entry point |

## Untested public surface is removed

If a public item is not directly tested, it is removed — the kernel keeps no
"might be useful later" surface. While building this layer, a `Default` that
served no tested purpose (`MessageKind`) was dropped rather than left untested,
and accessors that lacked a direct assertion (`MemoryRange::offset`/`length`,
`SimulationClock::step`) had tests added rather than being quietly shipped.

## Deterministic-replay mindset

Tests assert *exact* outcomes, not ranges or "did not crash":

- Clocks are compared for full state equality after the same input sequence.
- Records and metrics are rebuilt from identical inputs and compared with `==`,
  proving the data model is reproducible.
- Serialization asserts exact byte layouts and lengths.

This mirrors how higher layers will rely on the kernel: feed the same inputs,
get the same bytes, logs and telemetry back — every run.

## Architecture tests

`tests/architecture.rs` is an integration test (a separate crate, so it sees
only the public `KernelApi`). It mechanically enforces the hard rules by
scanning `src/` and asserting the absence of:

- browser / JS APIs (`web_sys`, `js_sys`, `wasm_bindgen`, `Math.random`);
- wall-clock time (`std::time`, `SystemTime`, `Instant::now`, `chrono`);
- randomness (`rand::`, `thread_rng`, `random()`);
- console printing (`println!`, `eprintln!`, `print!`, `eprint!`, `dbg!`);
- placeholder macros (`todo!`, `unimplemented!`);
- global mutable state (`static mut`, `lazy_static`);
- any module named `utils`.

It also asserts that `lib.rs` publicly exports exactly one thing. `tests/facade.rs`
proves, from outside the crate, that the single `KernelApi` export can drive
every capability.

## Running the tests

```sh
cargo test --workspace
```

Optional checks used during development:

```sh
cargo clippy --workspace --all-targets   # lint, must be warning-free
cargo build --target wasm32-unknown-unknown -p axiom-kernel   # wasm readiness
```
