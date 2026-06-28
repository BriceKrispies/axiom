# World → bytes / restore: full-session snapshot spec

**Status:** **implemented** (the §5 checklist landed; deferred items noted in §6).
**Audience:** an agent implementing deterministic full-session snapshot+restore
for Axiom-embedded games.

This documents exactly what Axiom needs to implement so a running game session
can be serialized to an opaque byte buffer and restored byte-faithfully — the
capability an authoritative host (e.g. a `.NET` game server embedding Axiom via
the FFI) needs for persistence, room rewind, crash recovery, and out-of-process
room workers.

The headline: **most of this already exists, is layered correctly, and is
covered by truncation/schema/round-trip tests.** This is an extension job, not a
from-scratch serialization system.

> **Implementation note (landed).** The five gaps below are implemented:
> - **A** — `DeterministicRng::state()` / `from_state()` + `Reflect`
>   (`crates/axiom-kernel/src/deterministic_rng.rs`), with round-trip and
>   sequence-continuation tests.
> - **B** — `RunningApp::snapshot_session(&DeterministicRng)` /
>   `restore_session(&[u8]) -> KernelResult<DeterministicRng>`
>   (`modules/axiom/src/app.rs`): the host owns the RNG; the engine bundles it
>   beside the durable sim state in one versioned blob.
> - **C** — `Reflect` for `i32` / `i64` / `u8` and the branchless tagged-union
>   read-dispatch helper `BinaryReader::read_tagged` (§3), using the existing
>   `InvalidDiscriminant` error code (`crates/axiom-kernel/src/binary_reader.rs`;
>   `write_i64`/`read_i64` added to the binary primitives).
> - **D** — `axiom_session_snapshot_len` / `axiom_session_snapshot_write` /
>   `axiom_session_restore` over the C ABI (`apps/axiom-netplay-ffi/src/ffi.rs`);
>   the `Session` now owns a seeded RNG carried inside the blob.
> - **E** — `World::write_snapshot`/`read_snapshot` serialize the `startup_done`
>   bit faithfully (the *restore-implies-started* rule, carried by the data rather
>   than assumed): a mid-session snapshot restores as started, so a restored scene
>   never re-runs its authoring startup systems, while a pre-startup snapshot still
>   runs them (`crates/axiom-ecs/src/world.rs`).
>
> Deferred (see §6): a `#[derive(Reflect)]` macro, `Reflect` for
> `Option`/`Vec`/`String`, and the sim-core enum codecs.

---

## 0. What already exists

| Piece | Location |
|---|---|
| `World<S>::write_snapshot` / `read_snapshot` — entity identity (live slots, **generations, free list, next-slot**) + every component column | `crates/axiom-ecs/src/world.rs:189,201` |
| Generic column codec for any `T: Reflect` (no per-column code) | `crates/axiom-ecs/src/erased_column.rs:33` |
| `EntityRegistry` serialize/deserialize — **future-spawn determinism survives restore** | proven by `world.rs` `snapshot_restore_preserves_future_spawn_determinism` |
| `Reflect` trait + composite support (compose field reflects) | `crates/axiom-kernel/src/reflect.rs:18`; composite example `modules/axiom-scene/src/scene_storage.rs:67` (`ControllerState`) |
| `RunningApp::snapshot_sim() -> Vec<u8>` / `restore_sim(&[u8]) -> KernelResult<()>` | `modules/axiom/src/app.rs:520,528`; round-trip-into-a-fresh-app test at `app.rs:719` |
| Versioned wire format (`SchemaVersion` header, rejects incompatible major with a deterministic error, not a panic) | `world.rs:17,202` |

`World::write_snapshot` does exactly what its name says today. The work below is
about everything *around* the World that a general game needs to round-trip but
the existing cube/controller demos did not exercise.

---

## 1. The complete determinism surface

For a restored session to produce byte-identical future ticks, the snapshot must
capture **everything mutable that feeds a future tick** — and *only* that.

| State | Needed? | Exists? |
|---|---|---|
| Entity registry (slots / generations / free list / next) | yes | ✅ `World::write_snapshot` |
| Component columns (gameplay state held as components) | yes | ✅ via `ColumnSet` + `Reflect` |
| Non-column persistent maps (e.g. scene's `players`, `controllers`) | yes | ✅ serialized alongside (`scene_storage.rs:40,48,96`) |
| Transient per-tick queues (`pending_moves`, `pending_controls`) | no — drained each tick, empty at a tick boundary | ✅ correctly excluded (`scene_storage.rs:44,53`) |
| Per-frame engine machinery (runtime, driver, frame builder) | no — tick continues forward, the caller owns it | ✅ deliberately excluded (`app.rs:514-519`) |
| **RNG state** | **yes — if the game uses randomness** | ❌ **missing** (§2A) |
| Current tick | yes | ⚠️ caller-owned (host carries it; fine) |
| **`World.startup_done` / startup-phase position** | **maybe** | ❌ not serialized (§2E) |
| **Game-specific component value codecs** | yes | ⚠️ partial — narrow leaf coverage (§2C) |

That yields the gap list: **C (value codecs), A (RNG), E (startup), D (FFI),
B (compose).** Four small, one moderate.

---

## 2. The gaps

### A. RNG state capture/restore — *missing; small; mandatory for any game with randomness*

`DeterministicRng` is a single `u64` of state (`crates/axiom-kernel/src/deterministic_rng.rs:14`)
but exposes **no accessor and no `Reflect` impl** — only `seeded(seed)` and
`next_*`. The existing demos snapshot cleanly because they use no RNG. Any real
game (loot, spawns, shuffles, crits) will **diverge on restore** without this.

Implement in the kernel (where `DeterministicRng` lives — root layer):

```rust
impl DeterministicRng {
    pub const fn from_state(state: u64) -> Self { DeterministicRng { state } }
    pub const fn state(&self) -> u64 { self.state }
}

impl Reflect for DeterministicRng {
    const SCHEMA: TypeSchema =
        TypeSchema::new("DeterministicRng", &[FieldSchema::new("state", "u64")]);
    fn reflect_write(&self, w: &mut BinaryWriter) { self.state.reflect_write(w); }
    fn reflect_read(r: &mut BinaryReader<'_>) -> KernelResult<Self> {
        u64::reflect_read(r).map(DeterministicRng::from_state)
    }
}
```

Ships with its tests (Coverage Law): round-trip, and "a restored RNG continues
the **identical** sequence" (snapshot mid-stream, restore, assert the next N
draws match the original's continuation). This is the single most important
missing piece — it is what makes replay/rewind/recovery correct for a game that
rolls dice.

### B. A `SessionSnapshot` aggregate — *missing; small; mostly composition*

`snapshot_sim` today covers only the **scene** world. A game that holds gameplay
state in its own `World<GameStorage>` (the recommended data-driven shape) gets
its columns serialized for free by `World::write_snapshot` — **provided every
column type impls `Reflect`** (§C). What's needed is one aggregate bundling, into
a single opaque, versioned buffer:

```
[schema version][world snapshot bytes][rng state][game-extra state, if any]
```

For the embedding boundary, keep it **one opaque blob** — do not split RNG out.
The host stores the whole buffer and hands it back verbatim on restore. One
buffer in, one buffer out is the cleanest possible cross-language contract.

### C. `Reflect` value-codec breadth — *the only moderate piece*

This is the recurring per-game cost and the part already flagged as "a
substantial, self-contained serialization pass" in
`modules/axiom-sim-core/PHASE_2_DEFERRED.md`. Today `Reflect`'s **leaf coverage
is only `u32 / u64 / f32 / bool / EntityId`** (`reflect.rs:45-60`). Composites
work but are **hand-written** (no derive macro) — see `ControllerState`
(`scene_storage.rs:67`). There is no support for `i32 / i64 / u8`, `Option`,
`Vec<T>`, `String`, or **enums / tagged unions**.

Two ways to pay this, by integration shape:

- **Data-driven engine (recommended):** the game's state is *one* generic
  component representation interpreted from a definition. You write `Reflect` for
  that **once**, and every data-defined game rides it. The codec surface is
  `O(1)`, not `O(games)` — which is why the data-driven path also de-risks
  serialization.
- **Hand-coded games:** each component struct needs a hand-written branchless
  `Reflect`, and any enum component needs a branchless tag codec. Painful at
  scale.

The genuinely valuable kernel additions (in priority order):

1. `Reflect` for `i32 / i64 / u8` (two's-complement encoders; `i64` is explicitly
   called out as missing in the sim-core deferred note).
2. A **sanctioned branchless tagged-union codec** so enum components round-trip
   (see §3).
3. Optionally a `#[derive(Reflect)]` proc-macro to remove composite boilerplate —
   Axiom already has proc-macro infrastructure (`crates/axiom-zones`).

(1) and (2) are load-bearing; (3) is ergonomics.

### D. FFI exposure — *missing; small; required by the host*

The generic embedding ABI (built alongside the rest of the headless-session FFI)
adds two calls that map onto the host's serialize/restore seam:

```c
/* returns bytes-written; out=NULL or cap=0 → returns the required length
   (size-probe), writing nothing */
size_t  axiom_session_snapshot(const Session* s, uint8_t* out, size_t cap);

/* returns 0 on success; nonzero error code on truncated/incompatible bytes */
int32_t axiom_session_restore(Session* s, const uint8_t* bytes, size_t len);
```

The two-call size-probe pattern (probe length, then fill a host-owned buffer)
keeps the Rust allocator from crossing the boundary. Errors return as codes,
never panics — `KernelResult` already gives this. Add a contract test that drives
both through the raw C ABI, mirroring `apps/axiom-netplay-ffi`'s
`ffi_round_trip_through_the_c_abi`.

### E. `startup_done` / startup-phase determinism — *correctness trap; decide + document*

`World.startup_done` is **not** in the snapshot (`world.rs:189`, "Systems are not
serialized"). Restoring into a freshly built world leaves it `false`, so
**startup systems re-run on the next active advance**. For a host that restores
into a fresh worker process (or rewinds a live session), that is silent
double-initialization unless one of:

- **(e1)** startup systems are author-only / idempotent (no gameplay mutation) —
  stated as an invariant; or
- **(e2)** `startup_done` (and any startup-phase cursor) joins the snapshot.

(e1) fits Axiom's "setup authors the scene, systems evolve it" model and is the
cleaner discipline — but it must be a *stated, tested* invariant, because the
snapshot drops it today.

**Resolved — (e2):** `write_snapshot`/`read_snapshot` now serialize and restore the
`startup_done` bit, so the restored world's startup phase matches the source's
faithfully. A mid-session snapshot (the scene's normal case) restores as *started*
and never re-runs authoring startup; a pre-startup snapshot restores as *not
started* and still runs it. This is the *restore-implies-started* rule done without
the shortcut of an unconditional `startup_done = true` on restore (which would
silently skip startup for a fresh-baseline snapshot of the general `World`
primitive). Tested by `restore_of_a_post_startup_snapshot_re_runs_update_but_not_startup`
and `restore_of_a_pre_startup_snapshot_still_runs_startup` (`world.rs`).

---

## 3. The branchless tagged-union codec (the one non-trivial pattern)

Enum/tagged-union component values are the only piece that is more than a few
lines, because deserialization must pick a variant by a tag read at runtime —
and the **Branchless Law** forbids `match` on that tag in spine code. The
sanctioned shape is a **read-dispatch table** indexed by the tag, with clean
out-of-range rejection:

This shape is provided once, branchlessly, by `BinaryReader::read_tagged` (a
sibling of the other primitive reads), so an enum's `reflect_read` is a single
call. An out-of-range tag is a deterministic `KernelErrorCode::InvalidDiscriminant`
(an unknown discriminant in an otherwise well-formed buffer — distinct from
`TruncatedData`, which means the bytes ran out).

```rust
// Write: tag byte, then the variant body.
fn reflect_write(&self, w: &mut BinaryWriter) {
    w.write_u8(self.tag());                 // 0,1,2… per variant, total over the enum
    self.write_body(w);                     // body itself branchless per variant
}

// Read: index a fixed table of per-variant readers; out-of-range → error, not panic.
fn reflect_read(r: &mut BinaryReader<'_>) -> KernelResult<Self> {
    r.read_tagged(&[read_v0, read_v1, read_v2])
}
```

Coverage Law obligations for this pattern: a round-trip test per variant, the
out-of-range-tag arm, and truncation at every byte prefix (the existing
`world.rs` `*_rejects_truncation_at_every_prefix` tests are the template).

---

## 4. How this maps onto an embedding host

```
host serialize() : byte[]        → axiom_session_snapshot   (opaque blob, §B)
host restore(byte[])             → axiom_session_restore
stored snapshot blob (byte[])    ← the buffer, verbatim
host-side RNG-state column       ← unused / null: the RNG lives inside the blob (§A)
host-side tick                   ← caller-owned (unchanged)
host-side schema-version column  ← Axiom's SchemaVersion header (already emitted, §0)
```

One opaque buffer satisfies persistence, rewind, recovery, and out-of-process
session workers.

---

## 5. Implementation checklist (ordered, with the laws each must satisfy)

1. **Kernel: `DeterministicRng` `state()` / `from_state()` + `Reflect`** — with
   round-trip and sequence-continuation tests (Coverage Law). *(§A — small)*
2. **Kernel: `Reflect` for `i32 / i64 / u8` + the branchless tagged-union codec
   pattern** — read-dispatch table, out-of-range rejected, truncation-at-every-
   prefix tests (Branchless + Coverage Laws). Optional `#[derive(Reflect)]`.
   *(§C / §3 — moderate; the real work)*
3. **`SessionSnapshot` aggregate `snapshot()` / `restore()`** bundling world bytes
   + RNG into one versioned blob; truncation/schema tests mirroring the existing
   `world.rs` suite. *(§B — small)*
4. **Decide §E**, document the chosen invariant, add a test that
   restore→advance does not double-run startup. *(small)*
5. **FFI: `axiom_session_snapshot` / `axiom_session_restore`** with the
   size-probe + error-code contract; a round-trip test through the raw C ABI.
   *(§D — small)*

Net: **one moderate task (the value-codec breadth) and four small ones.** The
serialization engine itself already exists — layered, versioned, and tested. The
work is extending its value coverage, adding RNG state, deciding the startup-phase
invariant, and exposing it across the embedding boundary.

**All five landed.** Item 1 (RNG), item 2 (`i32`/`i64`/`u8` + the
`BinaryReader::read_tagged` tagged-union helper), item 3
(`RunningApp::snapshot_session`/`restore_session`), item 4 (§E —
*restore implies started*, done by serializing the `startup_done` bit faithfully,
with both post-startup and pre-startup regression tests), and item 5 (the
`axiom_session_*` C ABI + a round-trip test). Every spine addition is branchless
and 100% covered; the netplay-FFI calls (an app) carry their own
round-trip/rng-continuity tests.

---

## 6. Deferred (explicitly out of this scope)

These were called out as optional/beyond the §5 checklist and remain future work:

- **`#[derive(Reflect)]` proc-macro** — ergonomics only; composite `Reflect` impls
  stay hand-written (the `ControllerState` pattern) until a derive is justified.
- **`Reflect` for `Option<T>` / `Vec<T>` / `String`** — no current spine consumer;
  add them with the type that first needs them.
- **The sim-core enum codecs + `SimWorld` snapshot** — `FactValue` / `Effect` /
  `CauseRef` etc. are their own pass per `modules/axiom-sim-core/PHASE_2_DEFERRED.md`.
  They now have the foundation they were waiting on: `i64` `Reflect` and the
  sanctioned `BinaryReader::read_tagged` tagged-union helper.
- **An engine-owned RNG *draw* API** — the host owns and draws its RNG; the engine
  only captures/bundles its state. A game draws via its own `DeterministicRng`.
