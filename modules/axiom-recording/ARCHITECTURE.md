# `axiom-recording` — architecture

A deterministic, memory-bounded **frame recorder and scrubber**. It records the
artifacts a higher tier produces for each frame as **opaque canonical bytes**,
keeps them in a bounded ring buffer, lets a caller scrub/step through retained
frames without disturbing the live timeline, and proves replay determinism by
comparing two recordings byte-for-byte.

## Placement

`Engine module` — an **isolated** one (`module.toml`: `kind = "engine-module"`,
`allowed_modules = []`). It composes no other module and is composed only by
apps (or a future feature module that lists it). It builds on exactly one layer:
the **kernel**.

```
kernel (FrameIndex / Tick / KernelError / KernelResult)
   └── axiom-recording   (this module)
```

## What it is, and is not

It records four opaque artifact byte payloads per frame — `input`, `runtime`,
`state`, `render` — indexed by kernel `FrameIndex`/`Tick`. **It never interprets
those bytes.** It does not know what a scene, renderer, GPU backend, ECS world,
asset, input device, or editor panel is. Byte equality is the source of truth
for determinism; the per-artifact FNV-1a hashes it computes are diagnostics only.

Deliberately **out of scope** (and enforced absent by `tests/architecture.rs`):
video recording, screenshots, raw pixel / GPU-texture frames, save-to-disk,
compression, fork-from-frame, rollback netcode, editor panels, scene/render/asset
inspection, and browser storage.

It is **deterministic and pure**: no wall-clock time, no randomness, no global
mutable state, no file IO, no browser/DOM/GPU APIs. Recording and comparison are
pure functions of the bytes handed in — the same inputs always yield the same
timeline and the same determinism report.

## Public surface — one facade

`lib.rs` exposes exactly one public item: **`RecordingApi`**. Every other type is
declared `pub` inside a *private* module and returned **opaquely** through the
facade (the same pattern `axiom-render` uses for its contract types):

| Type                | Role                                                        |
|---------------------|-------------------------------------------------------------|
| `FrameCapture`      | one frame's opaque artifact bytes + diagnostic hashes       |
| `FrameTimeline`     | the bounded ring buffer of captures (crate-internal)        |
| `TimelineMode`      | `Live` vs `Scrubbing { selected_frame }` (a value type)     |
| `DeterminismReport` | the first-divergence result of comparing two recordings     |
| `ArtifactKind`      | which artifact diverged (`Input`/`Runtime`/`State`/`Render`/`Final`) |

A caller holds these by inference and reads their public accessors; it never
names them. This keeps the module a black box with a stable shape while still
handing back rich, inspectable values.

## Memory model

`FrameTimeline` enforces **two** hard bounds simultaneously:

* `max_frames` — the maximum number of retained captures, and
* `max_bytes` — the maximum accounted memory (`size_of::<FrameCapture>()` plus
  the four byte arrays, summed over all retained captures).

`record_frame` rejects a single capture larger than the whole `max_bytes` budget
(it could never fit), then appends and **evicts the oldest captures** until both
bounds hold again, preserving insertion order. The eviction count is *computed*
(the max of the frame-overflow and the byte-overflow prefix length) rather than
discovered by a conditional loop, so the spine stays branchless.

Two documented defaults:

| Profile        | `max_frames` | `max_bytes` |
|----------------|--------------|-------------|
| `browser_safe` | 3,600        | 64 MiB      |
| `native`       | 10,000       | 128 MiB     |

## Scrubbing

`TimelineMode` is `Live` or `Scrubbing { selected_frame }`. Scrubbing is a
**read-only view**: entering scrub, stepping back/forward, and resuming never
mutate the timeline or evict anything. `RecordingApi` tracks the cursor as a
`live` flag plus a `selected` frame index (not as a `TimelineMode` value), so the
module never has to destructure the mode enum — `mode()` *constructs* the enum on
demand. Stepping past either edge, or on an empty timeline, returns a
deterministic `KernelError` rather than panicking.

## Error model

Every failure path returns a deterministic `KernelError` (scope `Memory`, code
`OutOfBounds`) — never a panic. The catalog lives in `error.rs` as runtime
constructor functions (not `const` items, so each construction is ordinary
executed code the tests can cover): zero `max_frames`/`max_bytes`, capture too
large, frame missing/evicted, empty timeline, no previous/next frame at an edge,
and the two timeline-shape mismatches (different length, different aligned frame
index) raised by comparison.

## Determinism comparison

`compare_with` (→ `compare_timelines`) compares an original recording against a
replay. A length or aligned-frame-index mismatch is a **structural** error (the
two recordings are not the same shape). Otherwise it walks aligned captures and
reports the **first** divergence in a fixed order — identity (`tick`), then the
four artifact byte arrays (`input` → `runtime` → `state` → `render`) — as a
`DeterminismReport`: which frame, which `ArtifactKind`, the first differing byte
index, and the two frames' diagnostic `final_hash`es. There is intentionally no
`final_hash` comparison arm: `final_hash` is a pure function of the frame index,
tick, and the four artifact hashes, so if all of those match it is necessarily
equal and could never be the *first* divergence.

## Module structure

```
src/
  lib.rs                 # module doc + the single `pub use` facade
  recording_api.rs       # RecordingApi — the facade
  frame_capture.rs       # FrameCapture — opaque per-frame artifacts + hashes
  frame_timeline.rs      # FrameTimeline — bounded ring buffer + eviction
  timeline_mode.rs       # TimelineMode — Live | Scrubbing
  determinism_report.rs  # DeterminismReport + the comparison
  artifact_kind.rs       # ArtifactKind — which artifact diverged
  hash.rs                # local deterministic FNV-1a (diagnostics only)
  error.rs               # deterministic KernelError constructors
tests/
  architecture.rs        # facade-only + hygiene boundary scan
  replay_determinism.rs  # record → replay → compare proof
```

## Laws upheld

* **Module Law** — one facade (`RecordingApi`); `allowed_modules = []`; no
  app/tool/other-module deps; no browser/GPU/DOM; no console/placeholder macros;
  no `utils`/`helpers` junk drawers.
* **Branchless Law** — non-test code contains no `if`/`match`/`for`/`while`/
  `loop`/`&&`/`||`/`?`; all logic is iterator combinators, `Option`/`Result`
  adapters, and table/arithmetic selection.
* **Coverage Law** — 100% regions/lines/functions/branches, verified by
  `cargo +nightly llvm-cov -p axiom-recording --branch`.
* **Determinism** — pure, clock-free, randomness-free, no global state.
