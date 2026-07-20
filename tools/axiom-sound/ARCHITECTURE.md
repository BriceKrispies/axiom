# axiom-sound — architecture

## Why this is tooling, not runtime engine code

`axiom-sound` generates **assets** (MP3 files) that an app consumes as ordinary
data. It is not part of the engine's runtime: nothing in `crates/`, `modules/`,
or `apps/` depends on it, and it depends on nothing in the engine graph. By its
`tools/` location it classifies as a **Tool** under the Axiom Module Law, so the
architecture checker (`cargo xtask check-architecture`) and the coverage /
branchless gates do not — and must not — cover it. It is a self-contained
Node/TypeScript npm package (invisible to `cargo metadata`, exactly like
`tools/lints` and the `packages/*` TS packages), wired into the repo only
through the root `Makefile`.

The tool is Node-based because its core work — running the **real Strudel**
transpiler/evaluator and rendering audio through a browser
`OfflineAudioContext` — is inherently JavaScript + browser. A Rust tool would
have to shell into Node to do the same thing.

## Why Strudel never ships in an app

Strudel is **AGPL** and is a live-coding *authoring/synthesis* system, not a
runtime the engine needs. It is used here only at build time, inside this tool,
to turn a `.strudel` source into fixed PCM. The engine and apps never import,
link, or bundle it. Keeping it isolated in a Tool is both a licensing boundary
(the AGPL obligations stay with the tool, not the shipped game) and an
architectural one (the runtime stays free of a heavyweight synthesis dependency;
apps consume a plain MP3).

## The pipeline

```
.strudel source
  → front matter parse + field validation        (frontmatter.ts, config.ts, ids.ts)
  → transpile (real Strudel transpiler)           ┐
  → evaluate to a Pattern                          │ browser harness
  → assert a valid Pattern                         │ (render/*)
  → query the pattern over its finite range        ┘
  → OfflineAudioContext render → Float32 PCM       (render/browser-entry.ts)
  → PCM validation (silence/clip/finite/shape)     (pcm.ts)
  → 16-bit WAV intermediate (cached)               (wav.ts, cache.ts)
  → FFmpeg CBR MP3 encode → temp file              (encode.ts)
  → FFprobe validation                             (encode.ts)
  → atomic publish to assets/audio/<id>.mp3        (encode.ts)
  → atomic manifest update                         (manifest.ts, atomicwrite.ts)
```

`check` runs the first block (through the pattern query) and writes nothing.
`build` runs `check` and then the rest. Neither renders invalid source.

## Browser harness isolation

`render/harness.ts` launches **one** headless Chromium for an invocation and
reuses it across a multi-sound build, but runs **each sound in a fresh
`browser.newContext()`** that is disposed afterwards. Strudel keeps module-level
state (the audio-context singleton, the registered-sound registry, the global
eval scope); a fresh context per sound guarantees no state leaks from one asset
to the next. `render/page.ts` bundles the in-browser entry once with esbuild and
serves it inline.

## Offline rendering

There is no public one-call Strudel "bounce" API, so `render/browser-entry.ts`
assembles one from public exports — the "minimal exporter" the task sanctions,
built without reaching into private module paths:

1. `superdough`'s `setAudioContext(new OfflineAudioContext(ch, frames, 48000))`
   makes every internal `getAudioContext()` render into our offline context.
2. `initAudio()` + `registerSynthSounds()` prepare the voices. Worklets are
   **enabled**: superdough's worklet effects (`distort`/`shape`/`coarse`/`crush`)
   construct `AudioWorkletNode`s, and their modules load from the packages' own
   inline `data:text/javascript;base64` URLs — resolved in-memory, no network.
3. `evalScope(core, mini, tonal, webaudio)` hoists Strudel's control functions
   into global scope, exactly as the REPL does.
4. `evaluate(code, transpiler)` produces the `Pattern`; `pattern.queryArc(0,
   seconds·cps)` yields the events (`Hap`s).
5. Each hap with an onset is scheduled via `superdough(value, tSec, durSec, cps,
   cycle)`; `ctx.startRendering()` returns the `AudioBuffer`, read out as
   Float32 channels and returned as base64.

`cps = 0.5` (Strudel's REPL default: one cycle = two seconds) is part of the
renderer contract (`RENDERER_VERSION`). Pure oscillators are deterministic, so
the same source renders byte-identical PCM every time (proven by
`test/exporter.test.ts`).

## Realtime render mode (`render = "realtime"`)

A distorted signal decaying through a `.room()` reverb tail generates denormal
floats, and the `OfflineAudioContext` render thread does not flush them to zero,
so those renders stall indefinitely. Chrome's *realtime* audio thread does flush
denormals, so `render/realtime-entry.ts` offers a second path: a live
`AudioContext` played in wall-clock time with the master mix captured.

- The master bus is teed by intercepting the one native `connect(...)` whose
  target is `ctx.destination` (superdough's `destinationGain`) into a small
  recorder `AudioWorkletProcessor` (registered from an inline `data:` URL) that
  posts stereo PCM blocks to the Node side.
- Events are placed by a **lookahead scheduler** (schedule ≤0.5 s ahead of the
  playhead, then sleep) so node creation is spread across the realtime playback
  instead of a one-shot upfront burst.
- A **peak guard** attenuates (never boosts) the captured mix to sit just under
  full scale — a full mix can otherwise sum past `1.0`, which the encoder rejects.
- Realtime is **non-deterministic** (live noise, wall-clock timing) and takes at
  least the sound's own length; `RenderHarness.renderRealtime` widens the command
  timeout accordingly. Pinned by `test/realtime-exporter.test.ts`.

The mode is part of the cache key (`hash.ts`), so switching a sound between
`offline` and `realtime` re-renders it.

## Network isolation

The whole render must be offline and reproducible. `render/harness.ts`
intercepts **all** requests at the context level: the in-memory page is served
by the route handler, and **every other request is recorded and aborted**. Any
recorded request fails the command with `NETWORK_ACCESS_ATTEMPTED` (this is the
`samples(...)` / remote-sample-bank guard). A short settle wait after
evaluate/render lets fire-and-forget fetches reach the handler.

## Validation and atomic publication

- **PCM** (`pcm.ts`) is rejected before encoding for: empty, wrong channel
  count / sample rate / duration, non-finite samples, silence (RMS below a
  documented ~-60 dBFS floor), or clipping (peak ≥ full scale, with the measured
  peak reported).
- **Encoding** (`encode.ts`) writes MP3 to a temp file in the destination
  directory, validates it with FFprobe (codec = mp3, channels, sample rate,
  duration within tolerance), and only then **renames** it over the target.
  A rename within a directory is atomic on POSIX and Windows, so a reader never
  sees a partial file and a failed encode never clobbers the previous asset. The
  temp file is always cleaned up on failure. FFmpeg runs with `+bitexact` and
  stripped metadata (`-map_metadata -1`, no id3, no Xing) for reproducible bytes.
- **Manifest** (`manifest.ts`) is serialized deterministically (sorted ids,
  fixed field order), written atomically (`atomicwrite.ts`: temp + fsync +
  rename, refusing a symlinked destination), preserves unrelated entries, and is
  not rewritten when the bytes are unchanged.

## Caching and source hashes

`hash.ts` computes a `sourceSha256` over the complete source, the parsed config,
the exact pinned Strudel versions (read from `package.json`), the tool version,
the renderer version, and the encoder signature. `build` skips a sound whose
manifest entry's `sourceSha256` matches and whose MP3 exists. Because the cache
key is source-derived (not encoded-file-derived), it is stable across machines
even if MP3 bytes were to differ. The lossless WAV lives in a per-app,
gitignored cache under `<tool>/.axiom-cache/sound/<appKey>/`.

## Generated-manifest ownership

The manifest is entirely owned by this tool (`generatedBy: "axiom-sound"`).
`build` adds/updates entries; `clean` removes the manifest, the MP3s it lists,
and the app's WAV cache. Authored `.strudel` sources are never modified or
removed by any command.

## Module map

| area              | files                                                        |
|-------------------|-------------------------------------------------------------|
| CLI               | `bin/axiom-sound.mjs`, `src/cli.ts`, `src/options.ts`       |
| errors / output   | `src/errors.ts`, `src/output.ts`                           |
| source model      | `src/frontmatter.ts`, `src/config.ts`, `src/ids.ts`, `src/sources.ts`, `src/pipeline.ts` |
| app / paths       | `src/appdir.ts`, `src/cache.ts`                            |
| hashing / manifest| `src/hash.ts`, `src/versions.ts`, `src/manifest.ts`, `src/atomicwrite.ts` |
| audio             | `src/wav.ts`, `src/pcm.ts`, `src/encode.ts`               |
| render harness    | `src/render/harness.ts`, `render/page.ts`, `render/browser-entry.ts`, `render/protocol.ts` |
| commands          | `src/commands/{new,check,build,list,clean,preview}.ts`     |
