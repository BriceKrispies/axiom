# SPEC-08 — Audio (synthesis, playback, scheduling, analysis)

> Status: Landed
> Landed (2026-06-28): new module `axiom-audio` (`AudioApi` neutral core + `#[cfg(target_arch = "wasm32")]` Web Audio arm; `"audio"` added to `PLATFORM_FACING_MODULES`); `@axiom/game` projects `loadSound`/`playSound`/`playTone`/`playMusic`/`scheduleSound`/`setMasterVolume`/`setMuted`. Live playback and the optional §13.1 analyser are browser-proven — the native sandbox cannot run browser Web Audio. The §2 greenfield is now built.
> Contract: §13(.1)   Vocabulary: Audio-clock scheduling, Synthesis, Mute/volume, Sample/playlist playback, LFO, Live capture+FFT   Determinism: presentation

## 1. Summary

Authors need to make sound: load and play samples, stream music playlists,
synthesize tones (with ADSR + LFO), schedule against an audio clock, and set
master volume/mute. Optionally (§13.1) a game may capture a live audio stream
(microphone or system) and read its spectrum for audio-reactive visuals.

This is **presentation only** (contract §0.1, §13). Sound is *heard*, never
*simulated*: it is scheduled against an audio clock that runs independently of
the fixed sim tick, and **no audio value may ever re-enter the simulation**.
Most of the 11 games want feedback sound (a hit, a pickup, a UI blip) and
ambient music; a few want synthesis (procedural blips without assets); one — a
rhythm game — wants live-audio analysis and is the deliberate misfit handled in
§9.

## 2. Current state (verified)

- **Zero audio code exists anywhere in the engine.** A repo-wide grep finds no
  `AudioContext`, oscillator, synth/synthesis, FFT/analyser, gain node,
  envelope, or sample-playback symbol in any layer, module, app, or tool. This
  subsystem is entirely greenfield — there is nothing to extend.
- **The platform-API ban currently forbids it.** Module Law #9 (enforced by the
  source scan in `crates/xtask/src/hygiene.rs`) rejects any layer or module that
  references `web_sys`/`js_sys`/`wasm_bindgen`/`window.`/`document.`. Web Audio
  (`AudioContext`, oscillator, gain, `getUserMedia`) is reached through exactly
  those banned symbols, so **no engine code may touch it today**. Adding
  `axiom-audio` to the platform allowlist is therefore a prerequisite, not an
  optional nicety.
- **The model already exists.** `axiom-windowing` is the precedent: a
  deterministic, fully-covered, target-independent core (`WindowingApi`) plus a
  `#[cfg(target_arch = "wasm32")]` `web` arm that owns the real `wgpu`/`web-sys`
  binding, allowlisted in `PLATFORM_FACING_MODULES`. SPEC-08 follows it exactly.

## 3. Architectural placement

**New engine module `axiom-audio` (`modules/axiom-audio/`, `module.toml`,
`allowed_modules = []`).** One facade, `AudioApi`. Split into two arms, the same
shape as `axiom-windowing`:

1. **Neutral deterministic core (target-independent, the spine).** Audio is
   described as **data**: `ToneSpec`, sound/voice/analyser handles, a
   schedule/mix graph (what plays, at which audio-clock second, at what
   gain/pitch), and master volume/mute state. It owns **no Web API** — no
   `AudioContext`, no nodes. It is branchless and 100% covered: it is pure
   bookkeeping (allocate a `VoiceId`, validate a `ToneSpec`, fold a
   `setMasterVolume` into mix state, compute the band layout for an analyser).
   Allowed deps: `kernel` only (`HandleId` for sound/voice/input handles,
   `Ratio` for 0..1 volume/sustain/depth). No platform symbols compile here.

2. **Platform playback arm (`#[cfg(target_arch = "wasm32")]`, a submodule like
   `windowing_api/web.rs`).** The real Web Audio binding: it constructs an
   `AudioContext`, realizes the core's schedule into oscillator/gain/buffer
   nodes, streams `playMusic` buffers, and (for §13.1) opens
   `getUserMedia`/`getDisplayMedia` and reads an `AnalyserNode`. It adds **no
   public surface** — only further `impl AudioApi` blocks driven from the
   core's data. None of it compiles on native, so the coverage gate never sees
   it (apps/wasm-arm exclusion, exactly as windowing's `web` arm).

**Module Law #9 amendment (explicit, deliberate).** Landing this module
**requires adding `"audio"` to `PLATFORM_FACING_MODULES` in
`crates/xtask/src/hygiene.rs`**, with a doc comment stating it owns the real
Web Audio arm behind a native-clean core. This is an amendment to the allowlist,
**not a default** — it is the same sanctioned escape windowing/gpu-backend use,
and adding it is a conscious widening of the platform edge, justified by the
fact that audio output is physically impossible without a host audio API.

**Module Law #13 (junk-drawer / support).** `axiom-audio` is a normal engine
module, not a support crate; it classifies cleanly and needs no #13 exemption.
The note here is only that the *core* must not become a "misc DSP" drawer:
every type in it is a named audio data contract, not a utility bag.

The determinism boundary is physical: the **core decides *what* and *when* (in
audio-clock seconds)**, deterministically and covered; the **wasm arm drives the
real `AudioContext` clock and emits sound**, impure and presentation-only. No
value crosses back the other way.

## 4. API surface

### 4.1 Native (`axiom-audio`, presentation-class)

Core facade (target-independent), sketch:

```rust
pub struct AudioApi { /* mix graph, master gain/mute, handle allocators */ }

impl AudioApi {
    pub fn new() -> Self;
    pub fn load_sound(&mut self, url: &str) -> SoundId;
    pub fn play_sound(&mut self, id: SoundId, opts: PlayOpts) -> VoiceId;
    pub fn stop_voice(&mut self, id: VoiceId);
    pub fn play_music(&mut self, urls: &[&str], opts: MusicOpts) -> VoiceId;
    pub fn play_tone(&mut self, spec: ToneSpec) -> VoiceId;          // validated synth
    pub fn schedule_sound(&mut self, id: SoundId, at: AudioSeconds, opts: PlayOpts) -> VoiceId;
    pub fn set_master_volume(&mut self, v: Ratio);                  // 0..1
    pub fn set_muted(&mut self, muted: bool);
    pub fn take_pending(&mut self) -> ScheduledBatch;               // what the wasm arm realizes
}
```

The `#[cfg(target_arch = "wasm32")]` arm adds `impl AudioApi` methods that drain
`take_pending` into Web Audio nodes and back the §13.1 capture/analyser. Volumes
are `Ratio`, never naked `f32` (public-API rule, per `axiom-windowing`).

### 4.2 TS authoring projection (the contract, §13 + §13.1)

```ts
type SoundId = Handle;   type VoiceId = Handle;

function loadSound(url: string): SoundId;
function playSound(id: SoundId, opts?: { volume?: number; pitch?: number; loop?: boolean }): VoiceId;
function stopVoice(id: VoiceId): void;
function playMusic(urls: string[], opts?: { loop?: boolean; crossfadeSeconds?: number }): VoiceId;

interface ToneSpec {
  wave: "sine" | "square" | "sawtooth" | "triangle";
  freq: number; duration: Seconds;
  envelope?: { attack: Seconds; decay: Seconds; sustain: number; release: Seconds };
  volume?: number;
  lfo?: { freq: number; depth: number };           // frequency modulation
}
function playTone(spec: ToneSpec): VoiceId;

function scheduleSound(id: SoundId, atSeconds: Seconds, opts?: { volume?: number }): VoiceId;
function setMasterVolume(v: number): void;          // 0..1
function setMuted(muted: boolean): void;

// §13.1 — live capture + analysis (optional, last)
type AudioInput = Handle;
function openAudioInput(source: "microphone" | "system"): Promise<AudioInput>;   // user-gated
interface Analyser { bands(count: number): Float32Array; level(): number }       // 0..1
function createAnalyser(input: AudioInput): Analyser;
```

Held to the TS spine laws (tsgo, Oxlint branch ban, 100% coverage). `loadSound`
returns a handle immediately; the fetch/decode is the app's job (it owns the
network and the wasm marshalling), mirroring how the demo app owns asset fetch.

## 5. Data contracts

- **Handles** — `SoundId`, `VoiceId`, `AudioInput` are opaque `HandleId`s,
  presentation-side only; never serialized into sim state.
- **`ToneSpec`** — the neutral synthesis description (wave kind as a field
  discriminant, freq, duration, optional ADSR envelope, optional LFO, volume).
  Carrying `wave` as data, not a branch, is what keeps the core branchless.
- **`AudioSeconds`** — a distinct newtype for the **audio clock**, explicitly
  *not* `Ticks` and not the sim `Seconds`. The type wall stops a scheduling time
  from being confused with a tick.
- **`ScheduledBatch` / mix state** — the data the core hands the platform arm:
  voices to start/stop, their gain/pitch, scheduled start times, master
  gain/mute. This is the entire core→arm contract; nothing flows back.

## 6. Determinism

Audio is **presentation-excluded (contract §17.5)** and this is the central
constraint of the spec:

- It runs on the **presentation/audio clock (real seconds)**, which is
  independent of the fixed sim tick. `scheduleSound(atSeconds)` and `playMusic`
  crossfades are timed against the `AudioContext` clock, never the tick counter.
- **No audio value may be read back into a `sim`-class API.** The core exposes
  *no* getter that a fixed update could sample — no "current playback position",
  no "is voice playing" that sim code can branch on. The data only flows
  sim/app → audio core → wasm arm → speakers.
- The **core is still spine**: branchless and 100% covered. Determinism of the
  *core* means "same calls ⇒ same `ScheduledBatch`", a replay/golden property
  on the data, not audible reproducibility (the real `AudioContext` clock is
  inherently non-reproducible and lives only in the uncovered wasm arm).
- **§13.1 is hard-walled.** Captured microphone/system audio is **external,
  irreproducible input**. `Analyser.bands()`/`level()` return live presentation
  values that **must not enter the simulation contract**. There is deliberately
  no native projection of an analyser reading into any `Sim` accessor; the
  values exist only on the presentation side. A game that drives gameplay from
  them is non-authoritative by definition (§9).

## 7. Acceptance / proof

- **Core:** 100% coverage, branchless, no platform symbols (verified by the
  `hygiene.rs` scan compiling clean on native). Golden test: a fixed sequence of
  `play_tone`/`schedule_sound`/`set_master_volume`/`set_muted` calls produces a
  byte-identical `ScheduledBatch` — the data-level replay property.
- **`ToneSpec` validation:** every wave kind, envelope present/absent, LFO
  present/absent, and out-of-range volume/sustain (clamped via `Ratio`) is
  covered with assertions, not coverage theater.
- **Platform arm:** `#[cfg(target_arch = "wasm32")]`, outside the coverage gate;
  verified live via the Playwright controller (a tone audibly plays; an analyser
  reports non-zero `level()` against a known input) — the same out-of-gate
  browser verification path windowing uses.
- **Amendment proof:** `cargo xtask check-architecture` passes with `"audio"`
  added to `PLATFORM_FACING_MODULES`, and rejects the module if the entry is
  removed while the wasm arm references Web Audio (proving the allowlist is the
  real gate, not coincidence).

## 8. Dependencies & order

- **Lands after SPEC-00** (the boundary/loop and `Handle` type) — it needs the
  wasm-bindgen app to marshal the facade and a presentation clock to schedule
  against. No sim spec blocks it.
- **Internal order (contract §18.8):** sample playback → synthesis → scheduling
  / mix → **§13.1 analysis last and optional**. The analyser arm can be deferred
  entirely without blocking the rest of the module.
- **Depended on by:** nothing in the spine (it is a leaf presentation module).
  Apps consume `AudioApi` directly; no feature module needs to compose it yet.

## 9. Open questions

- **The rhythm-game misfit (authority vs live audio).** A rhythm/"audio-reactive
  gameplay" game wants beat detection to *drive scoring* — but §13.1 forbids
  captured audio from entering the sim. **Decision:** two modes, and the author
  picks one. (a) **Authoritative:** the game runs off an **authored beat chart**
  (deterministic data on the tick), and live analysis is used *only* for
  presentation polish; this is networkable and replayable. (b) **Client-only
  live mode:** gameplay genuinely reacts to a live mic/system signal — this is
  **non-authoritative by definition** and must run **local-only** (no
  authority/predicted deployment, no replay guarantee). The engine offers both
  surfaces but never lets (b) masquerade as (a).
- **Decode ownership.** Does sample decode (`ArrayBuffer` → audio buffer) live in
  the wasm arm or the app? Lean app-side (it owns fetch), with the core holding
  only the handle — mirroring SPEC-00's handle-table lean.
- **`crossfadeSeconds` semantics.** Equal-power vs linear crossfade for
  `playMusic`. Presentation-only, so deferred to the wasm arm; the core only
  records the requested seconds.
- **`pitch` realization.** Playback-rate resample vs preserve-pitch shift —
  again a wasm-arm choice; the core stores the requested ratio and never
  interprets it.
