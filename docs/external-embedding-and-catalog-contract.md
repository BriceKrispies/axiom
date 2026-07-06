# External Embedding & Catalog Contract

> Status: Draft
> Determinism: boundary (with sim/presentation prerequisites)
> Companions: [`game-api-contract.md`](game-api-contract.md) (authoring surface),
> [`specs/README.md`](specs/README.md) (per-subsystem specs), and SPEC-00/01/04/12/14.

## 0. Purpose and roles

This document states the contracts the engine must fulfill so that an **external web
host** — any application that embeds engine-built games in its own pages — can:

1. **discover** which games exist and how to load them (a catalog),
2. **embed and run** one game inline in a page it owns,
3. **feed** a game its session input, and
4. **receive a verifiable outcome** back.

It fixes the division of responsibility so neither side reaches into the other:

- **The engine** owns *how a game is built, loaded, run, and how it reports its outcome.*
  It emits self-describing, relocatable game units and a stable embed surface. It knows
  nothing about who may play, when, or for what reward.
- **The host** owns *which games are offered, to whom, and when.* Availability, selection,
  entitlement, scheduling, and reward are host policy. The host expresses its choices by
  **selecting from the catalog and passing the chosen set as input** — never by mutating
  engine state at runtime.

The seam is therefore **declarative**: the host hands the engine a manifest entry + a
session config and mounts it; the engine hands back one outcome. There is no engine-side
notion of "enabled/disabled," and no host-side reach into the running game.

The contracts below are numbered C1–C5. Each states the obligation, its determinism class,
the current tree state, and the gap to close. Where an existing SPEC already defines a
contract, this document references it rather than restating it.

---

## C1 — Loadable game unit + distribution manifest  *(boundary)*

**Obligation.** The build tooling must emit, per game, a **self-describing loadable unit**
and a **machine-readable manifest entry** that a host can catalog and serve without engine
source. The manifest is emitted by the build, not hand-authored, and each published version
is immutable.

**Manifest entry — required fields:**

| Field | Meaning |
|---|---|
| `id` | Stable, opaque slug identifying the game across versions. |
| `version` | Immutable version of this published build; a new build is a new version, never an in-place overwrite. |
| `title` | Human-readable display name (host may override for its own UI). |
| `entry` | The exported start symbol the loader invokes (see C2). |
| `mountKind` | The surface the game mounts into — `canvas` for engine-native games. |
| `runtime` | Loader + module locations, expressed **relative to `assetBase`** (relocatable — see C2). |
| `assetBase` | Base location the unit's bytes are fetched from; overridable so any static origin/CDN can serve an unmodified build. |
| `assets` | Reference to the game's asset manifest (the existing `axiom-asset-pack` `manifest.bin` shape). |
| `outcomeSchema` | The shape of the terminal outcome this game reports (see C3): whether it reports `won`, `score`, and which named `metrics`. |
| `capabilities` | What the game requires (e.g. 2D surface, audio, netcode) so a host can gate on support. |
| `determinismClass` | Whether the game's outcome is `sim`-authoritative and replay-verifiable (see C4) or presentation-only. |

**Guarantees.** A host can (a) enumerate games from a set of manifest entries, (b) fetch a
game's bytes from `assetBase` on any static host, and (c) know how to mount and what outcome
shape to expect — all without reading engine source or a live engine endpoint.

**Non-goals.** The manifest carries *what exists and how to load it*, never *who may play it*
or *what it is worth*. Availability and reward are host policy and MUST NOT appear here.

**Current state.** An asset manifest (`manifest.bin`, `tools/axiom-asset-pack` producer /
`modules/axiom-assets` consumer) exists. Per-app static bundling exists
(`scripts/package_app.py`) with a capability-detecting loader and a `wasm2js` fallback. A
per-game catalog *shape* exists only as a hand-authored dev artifact
(`apps/axiom-workspace/web/games-manifest.json`: `id`/`title`/`entry`/`bundle`/`canvas`). The
Rust-side per-game manifest / cartridge tier (`game_manifest.rs`, the `Game` package class)
was **removed as premature** (commit `fafb48e`).

**Gap.** Re-introduce a per-game **distribution** manifest — as a build-emitted artifact with
the fields above, not the removed internal `Game` class. Placement: a build/packaging tool
under `tools/` (sibling to `axiom-asset-pack`) that, given a game app, emits its manifest
entry alongside its bundle; the `games-manifest.json` field set is the seed schema.

---

## C2 — Embed & mount surface  *(boundary)*

**Obligation.** The engine must expose a **stable, framework-agnostic** way for a host page
to load and run **one** game and later tear it down, with no per-game bespoke wiring. Given a
manifest entry, a mount target (a DOM element or canvas the host provides), and a session
config, the host performs a fixed lifecycle:

```
loader = import(runtime.loader)          // ES module named by the manifest
await loader.init()                       // initialize the runtime (wasm module, capability detection)
handle = loader.mount(target, {           // mount ONE game into the host-owned element
    entry,                                // manifest `entry`
    sessionConfig,                        // C3 inbound seam
})
// ... game runs ...
handle.dispose()                          // tear down: release GPU/audio/loop, detach from target
```

**Guarantees the engine must meet:**

- **Host-provided mount target.** The engine mounts into an element the host supplies; it
  does not create or own page chrome, and it renders no browse/select UI. That surface is the
  host's.
- **Multi-instance safe.** Two mounts on one page are isolated — no global singleton (loop,
  input, GPU device, audio context) that prevents a second instance or leaks between them.
- **Clean teardown.** `dispose()` stops the loop, releases device/context handles, removes
  listeners, and detaches from the target; a mount/dispose cycle leaves no residue.
- **Relocatable bytes.** The loader and its modules resolve their own assets relative to
  `assetBase` (C1), so an unmodified build runs from any static origin. Absolute root-anchored
  paths (`/pkg`, `/vendor`, `/dist`) that force domain-root hosting are a relocatability
  defect to remove.
- **No host-channel assumptions in the loader.** The loader does not hardcode where the host
  lives or how outcomes leave; that is C3's session channel, injected as data.

**Current state.** Loading today is bespoke: `import(loader) → default() → <entry>(canvas)`
(`apps/axiom-gallery`, `apps/axiom-game-runtime`). SPEC-00 defines the authoring boundary,
the frame model, and the `apps/axiom-game-runtime` runtime app; the `@axiom/game` SDK is the
authoring surface. There is **no standardized `mount(target, …)` / `dispose()` embed API**, no
framework-agnostic embed element, and SDK-hosted bundles use absolute `/pkg`,`/vendor`,`/dist`
URLs (domain-root only).

**Gap.** Promote the runtime app's boot into a **documented, relocatable embed API**
(`init`/`mount`/`dispose`, multi-instance safe) and make bundle asset resolution relative to
`assetBase`. This is an extension of SPEC-00's runtime app and the platform arm, not a new
engine module.

---

## C3 — Session config in, single verifiable outcome out  *(boundary)*

**Obligation.** Config flows **in** (a `seed` plus opaque `params`); exactly **one** terminal
outcome flows **out** (`won` / `score` / named `metrics`); nothing in between is part of the
contract. The engine resolves the session config **before tick 0** and delivers the outcome to
its host channel **exactly once**.

This contract is already specified — **SPEC-12 (Host bridge & persistence)** — with the
author-facing surface `getSessionConfig` / `notifyReady` / `reportOutcome` /
`reportOutcomes`, and the neutral data types `HostSessionConfig` / `HostOutcome` /
`HostOutcomeSet`. This section states the obligations that remain to make it usable by an
external host.

**Guarantees the engine must meet:**

- **Config before tick 0.** The full `HostSessionConfig` (seed + params) is resolved and
  immutable before the first fixed update, so it can seed the sim RNG (C4 / SPEC-01) without
  changing history mid-replay.
- **Exactly-once outcome.** `reportOutcome` / `reportOutcomes` latch: the first terminal
  report is delivered and any later one is a no-op. A game cannot report two terminal states.
- **Engine delivers to its host channel.** The game reports its outcome to the engine; the
  engine forwards exactly one validated outcome outward. The game never addresses the host
  directly.
- **Transport and trust are out of engine scope.** How the config arrives and how the outcome
  leaves (message channel, query string, token handshake, origin checks, retries) are host
  policy. The engine validates **shape**, never **trust**, and the host MUST authenticate and
  origin-check inbound config before it is minted.

**Current state.** SPEC-12's **neutral seam landed natively** (`axiom-host`:
`HostSessionConfig` / `HostOutcome` / `HostOutcomeSet`, plus a native emit-once outcome
latch). But the **TS↔wasm binding is stubbed** — `wasm-host.ts` `deferredBridge()` returns
inert no-ops for `getSessionConfig` / `notifyReady` / `reportOutcome` / `reportOutcomes`, and
no live inbound-config decode or outbound outcome emit is wired. So the author-facing path
does not reach a live channel.

**Gap.** Finish SPEC-12: bind the TS surface to the runtime app's `wasm_bindgen` exports;
decode an inbound session channel into `HostSessionConfig` before tick 0; drain the single
`HostOutcome` outward once. The channel binding lives in the runtime app / `wasm32` platform
arm (a documented allowlist edge), never in a portable module.

---

## C4 — Deterministic, replay-verifiable outcome  *(sim)*

**Obligation.** For a host to trust an outcome of value, the engine must guarantee that an
outcome is **reproducible from its inputs**: given the same `(seed, config, input stream)`, a
replay yields **byte-identical sim state** and the **identical terminal outcome**. The engine
must expose the inputs required to replay so a host **backend** can verify an outcome offline,
without a browser.

**Guarantees the engine must meet:**

- **Deterministic sim** (SPEC-01 and the cross-cutting determinism law in `specs/README.md`):
  the only clock is the fixed tick, the only randomness is the seeded stream, and input is a
  tick-indexed intent snapshot. Identical `(seed, config, input stream)` ⇒ identical state and
  identical per-tick state-hash sequence.
- **Recordable inputs.** The engine can record the tick-indexed input/intent stream for a
  session (the deterministic recorder `axiom-recording` is the basis) and expose it as a
  portable artifact alongside the seed.
- **Headless replay verifier.** A host backend can re-execute `(seed, config, recorded input
  stream)` **headlessly** (no GPU/DOM) and obtain the same terminal `HostOutcome`. Verification
  compares the replayed outcome (and optionally the state-hash sequence) to what the client
  reported.
- **Outcome is a pure function of final sim state.** `reportOutcome` is derived from
  deterministic state; a replay reproduces the same outcome. The *act* of reporting is a
  boundary side-effect (C3) and is out of the sim.

**Current state.** SPEC-01 is **Landed** (seeded `Rng`, single-clock discipline). The
determinism/replay law is defined and binding on all sim-class specs.
`axiom-recording` is a deterministic, memory-bounded frame recorder over opaque bytes — but it
is **never exported or sent**, and there is **no defined headless replay-verification entry**
as a contract.

**Gap.** Define and expose a **replay-verification contract**: a portable
`(seed, config, input stream)` artifact plus a headless entry that reproduces the
`HostOutcome`. Only a game whose outcome is thus reproducible may declare
`determinismClass = sim` in its manifest (C1); a presentation-only game reports an outcome the
host treats as unverified.

**Authoring note (the "determinism migration").** A game's outcome is verifiable only if the
game is **authored on the engine's deterministic primitives** — the seeded RNG (SPEC-01) and
the fixed tick, never an ambient wall clock or ambient randomness. This is an authoring
obligation the sim-class contract already enforces; a game ported from non-deterministic
sources must move its outcome-affecting logic onto these primitives before it can claim
`determinismClass = sim`.

---

## C5 — Presentation surfaces (prerequisites)  *(presentation)*

**Obligation.** A game must be able to present on the surface its `capabilities` (C1) declare.
For 2D games this is the 2D surface; for 3D, the 3D scene surface.

**Current state.** **SPEC-04 (2D surface) is Landed** — the neutral `Draw2dList` core, the
`@axiom/game` `Frame` 2D projection (shapes, text glyph-runs, sprites, particles, gradients,
render targets, camera/transform, layer sort), with proven GPU↔software raster parity.
**SPEC-11 (3D scene surface) is Landed.** Input (SPEC-05), audio (SPEC-08), UI/HUD & tween
(SPEC-09) are Landed.

**Note.** Any external inventory claiming the engine "exposes no 2D authoring surface" is
**out of date** relative to SPEC-04's landed status; a 2D game authored on the `@axiom/game`
`Frame` surface has the presentation vocabulary it needs. This contract therefore adds no new
engine work — it records 2D/3D presentation as a **satisfied prerequisite** and requires only
that a game declare the surfaces it uses in its manifest `capabilities`.

---

## Summary — contracts, backing spec, and gap

| # | Contract | Determinism | Backing spec | State | Gap to close |
|---|---|---|---|---|---|
| C1 | Loadable game unit + distribution manifest | boundary | (new) — seed from `games-manifest.json`; asset manifest per `axiom-asset-pack` | asset manifest + per-app bundling exist; per-game manifest tier **removed** | Build-emitted per-game distribution manifest (new packaging tool) |
| C2 | Embed & mount surface (`init`/`mount`/`dispose`) | boundary | SPEC-00 (runtime app, frame model) | bespoke `import→default→entry`; no mount/dispose API; non-relocatable paths | Documented, relocatable, multi-instance embed API |
| C3 | Session config in, single outcome out | boundary | SPEC-12 (host bridge) | neutral seam **landed native**; **TS↔wasm binding stubbed**, no live channel | Bind the TS surface + live inbound/outbound channel in the runtime app |
| C4 | Deterministic, replay-verifiable outcome | sim | SPEC-01 + determinism law; `axiom-recording` | determinism landed; recorder exists but not exported; no headless verifier | Portable `(seed,config,inputs)` artifact + headless replay-verify entry |
| C5 | Presentation surfaces (2D/3D) | presentation | SPEC-04, SPEC-11 (+05/08/09) | **Landed** | None — satisfied prerequisite; declare surfaces in manifest `capabilities` |

**Build order.** C5 is already met. C1 (manifest) and C2 (embed API) are independent and can
land in parallel; both are prerequisites for a host to load a game at all. C3 (finish SPEC-12's
live binding) is required before a host can feed input and receive an outcome. C4 (replay
verification) is required before any outcome may be trusted for value, and depends only on the
already-landed determinism spine plus an exported recording + headless verifier.

**One-line boundary.** The engine emits *relocatable, self-describing, deterministically
replay-verifiable game units and a stable mount surface*; the host decides *which of them a
given player may run, and what an outcome is worth* — by selecting from the manifest and
passing the choice in, never by reaching into a running game.
