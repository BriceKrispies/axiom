# Worldgen / Overworld / Simulator — Requirements Audit

**Type:** Extraction & condensation audit (no code written, no source edited).
**Date:** 2026-06-19
**Method:** Read-only inspection of `docs/`, `data/core/defs/*.xml`, and `QUEUED_PROMPTS/`. Every conclusion is traced to a source file + heading/story id. Where a requirement is implied, it is labelled **INFERRED**. Where the docs disagree, the conflict is shown, not reconciled. Where evidence is insufficient, **UNCLEAR** is stated with what would need checking.

> Scope reminder: this audit extracts what the worldgen/simulator is **intended to do** from the documents. A listed epic/story does **not** imply implementation exists. Implementation status is only reported where a doc explicitly asserts it (e.g. `critical_bugs_audit.md`, `milestones.md` checklists, the `QUEUED_PROMPTS` closure report, or "Done" markers).

---

## 1. Document inventory

### 1a. Primary planning / requirements documents

| File | Why relevant |
|------|--------------|
| `docs/epics.md` | Master epic index: ten epics (SA, OW, SC, GW, PL, SV, GE, SP, EC, PR), status, milestone mapping, dependency graph, work order. |
| `docs/milestones.md` | Shippable slices M0–M8 + North star; exit checklists; master story↔milestone table; IDEAS→milestone map. |
| `docs/README.md` | Docs index; canonical Overworld / Game world / Planet preview terminology. |
| `docs/ideation/IDEAS.md` | Player-voice fantasy list (explicitly "not a backlog"); origin of product bets. |
| `docs/ideation/IDEAS_WORKFLOW.md` | Governance: how ideas become milestones/epics; defines doc roles and "player-visible flow" gate. |

### 1b. Epics directly governing worldgen / simulator

| File | Why relevant |
|------|--------------|
| `docs/epic_sim_api_platform.md` | SA-*: Godot↔C++ boundary, `SimAPI` autoload, async `WorldGenJob`, merged def boot, profile paths. |
| `docs/epic_overworld_generation.md` | OW-*: deterministic planet-scale globe, `PlanetGlobePipeline`, `PlanetSurfaceAtlas`, climate/hydrology stories, gen UI scenes. |
| `docs/epic_game_world_streaming.md` | GW-*: streamed metre-scale chunks from overworld atlas, `ChunkStore`, intents, inventory scaffold. |
| `docs/epic_sim_correctness.md` | SC-*: `worldgen_bench` gates, topology validation, determinism hashes, coord contract. |

### 1c. Epics for gameplay layers built on the simulator

| File | Why relevant |
|------|--------------|
| `docs/epic_player_play.md` | PL-*: play camera, avatar on chunks, interaction ray, possession UI shell. |
| `docs/epic_survival_colony.md` | SV-*: survival needs/threats, place/build, inventory consumption, era summaries. |
| `docs/epic_gameplay_emergence.md` | GE-*: guardrailed emergence via bias defs / profile weights. |
| `docs/epic_spirit_meta.md` | SP-*: spirit influence, possession, sim-time-only-while-possessing, meta summaries. |
| `docs/epic_ecology.md` | EC-*: template species, regional population scalars, agent LOD, North-star genetics. |
| `docs/epic_presentation.md` | PR-*: cel shading, terrain materials, lighting, foliage LOD (parallel, M8). |

### 1d. Reference / architecture / pipeline documents

| File | Why relevant |
|------|--------------|
| `docs/worldgen.md` | Legacy form→seed→genome overview; form keys; land vs water fraction; presets/blueprints. |
| `docs/worldgen_modern.md` | Algorithm-stage reference: icosphere, tectonics, elevation, erosion, climate, rivers, biomes, terrain-mesh derivation, atlas-vs-marshal split. |
| `docs/moddability.md` | Content-pack engine rules; def types; pipeline registration; intents; linter rules; SimAPI-only contract. |
| `docs/game_scene_order.md` | Boot/scene flow (steps 0–7, 5a/5b); where preview/play transition happens; what is in default profile. |
| `docs/critical_bugs_audit.md` | Implementation gaps vs epics (11 numbered items + risks); explicit resolved/open status. |
| `docs/performance_big_o_audit.md` | Complexity of worldgen + runtime streaming; measured bench timings; P1–P8 remediation status. |
| `docs/install.md` | Targets Godot 4.6 + C# + optional C++ GDExtension sim backend (platform constraint). |

### 1e. Data / def files defining concrete pipeline + content contracts

| File | Why relevant |
|------|--------------|
| `data/core/defs/world_gen_pipelines.xml` | Authoritative overworld stage order for `default_globe`, `performance_globe`, `deterministic_globe`. |
| `data/core/defs/game_world_gen_pipelines.xml` | Authoritative game-world chunk stage order for `default_terrain`. |
| `data/core/defs/planet_presets.xml` | Genome distributions for `earthlike`, `ocean_world`, `dry` (knobs, constraints, material weights). |
| `data/core/defs/game_profile.xml` | Active assembly: which pipelines, scenes, scripts bind for `profile_id="default"`. |
| `data/core/defs/biomes.xml` | Biome defs with surface palettes + plant spawn groups/rules (game-world content surface). |
| `data/manifest.xml`, `data/core/manifest.xml` | Pack load order / merged def root contract. |

### 1f. Supporting / contextual (read for completeness, lower requirement density)

| File | Why relevant |
|------|--------------|
| `QUEUED_PROMPTS/m0-game-world-terrain-pipeline.md` | M0 game-world pipeline closure report; asserts which GW/SC stories are headless-done vs pending manual play test; explicit scope guardrails. |
| `docs/cool_bugs/00{1-4}-*.md`, `docs/cool_bugs/README.md` | Post-mortems on terrain winding, underwater preview, elevation seams, land-fraction fit — context for OW-E12/E21 and SC gates. |
| `data/core/defs/{items,recipes,surfaces,plants,plant_groups,seasons,weather,tools,interactions}.xml` | Downstream content the simulator/gameplay will read; mostly later-phase (M1/M3/M4/M7). |
| `data/core/stamps/*.xml` | Stamp content (rivers, terrain features, POIs) — referenced by moddability as content layer. |

---

## 2. Raw extracted requirements (traceable)

Format: **[source file › heading/story id]** requirement. **INFERRED** prefixes implied requirements.

### Platform / Sim API (SA)

- **[epic_sim_api_platform.md › Vision]** Gameplay scripts call **only** `SimAPI`; C++ owns long-running work on worker threads; boot from a merged data root (`res://data/`).
- **[epic_sim_api_platform.md › SA-0.1]** `SimAPI.boot` loads merged `res://data/` so mods contribute defs/pipelines; `godot_boot_data_root` lint passes.
- **[epic_sim_api_platform.md › SA-0.2]** Async surface: `start_world_gen_async`, `poll_world_gen_async`, `cancel_world_gen_async`, `take_world_gen_async_result` callable from GDScript/C#.
- **[epic_sim_api_platform.md › SA-0.3]** Sync `apply_world_gen_form` retained for headless/tools, documented as blocking.
- **[epic_sim_api_platform.md › SA-0.4]** Stub backend (missing DLL) returns empty geometry + error string; `ok: false`; no silent success.
- **[epic_sim_api_platform.md › SA-E1]** Single `WorldGenJob` owned by `SimBridge`; `start` rejects if already running; `take_result` joins+clears.
- **[epic_sim_api_platform.md › SA-E2]** GDExtension async bindings in `sim_api_gdextension.cpp`; no duplicate job in sync path.
- **[epic_sim_api_platform.md › Goals]** Single job contract; states idle/running/done/cancelled/failed.
- **[epic_sim_api_platform.md › SA-E4]** `WorldGenFormParser` honours `world_size` → maps to `sim_voronoi_sites`.
- **[epic_sim_api_platform.md › SA-E5]** Guardrails on extreme Voronoi site counts (confirm/soft cap; optional `max_voronoi_sites` in profile).
- **[epic_sim_api_platform.md › SA-1.1/1.2, SA-3.2, SA-E3]** Boot before UI; entry-menu/load-screen/preview paths from profile only; `PresentationSceneEngine` prefers boot def root.
- **[moddability.md › Implementation rules]** No content ids hardcoded in C++/Godot; no XML parsing outside `DefDatabase`/`XmlLite`; no scene paths in gameplay scripts; Godot displays, C++ decides rules/state.

### Overworld generation (OW)

- **[epic_overworld_generation.md › Vision/Goals]** Player configures overworld (seed, climate, resolution, plates); sim runs data-driven `PlanetGlobePipeline`, produces **overworld surface atlas** (regions on a unit sphere), keeps **planet genome + world seed** for the session.
- **[epic_overworld_generation.md › Goals]** Deterministic overworld: same form + pipeline → same region fields (mod `deterministic_globe` when needed).
- **[epic_overworld_generation.md › Goals]** Authoritative sim storage: atlas survives after generation; preview optional.
- **[epic_overworld_generation.md › Goals]** Location context: given a direction on the sphere, sim returns region id, plate, elevation, moisture, derived biome/climate.
- **[epic_overworld_generation.md › Goals]** Moddable pipeline: stage order from `world_gen_pipelines.xml`; no hardcoded stage lists in Godot.
- **[epic_overworld_generation.md › OW-3.1/3.6]** Form keys: `voronoi_sites`, `jitter`, `num_plate_regions`, `use_planet_terrain_mesh`, `world_gen_mode` (`default`/`max_determinism`), `use_simd`, `sim_voronoi_sites`, `world_size`.
- **[epic_overworld_generation.md › OW-4.4]** On success, `SimBridge` holds `WorldSeed` + `PlanetGenome`.
- **[epic_overworld_generation.md › OW-E1]** Overworld `PlanetSurfaceAtlas` built from `PlanetGlobe` after pipeline: per-region plate, elevation, moisture, positions; neighbours CSR for adjacency.
- **[epic_overworld_generation.md › OW-E2]** `SimBridge` owns the atlas for the session; globe not required for basic queries.
- **[epic_overworld_generation.md › OW-E3]** `locate_region(unit_dir)` and `sample_surface`: returns region id + scalar fields + derived temperature/biome from defs.
- **[epic_overworld_generation.md › OW-E4]** `SimAPI.sample_surface` exposes a small documented dict (not forwarding many arrays).
- **[epic_overworld_generation.md › OW-E5]** Pipeline stages declared in XML; unknown stage fails loudly in dev.
- **[epic_overworld_generation.md › OW-E7]** Prevailing-wind field per region (or global vector + latitude banding) from preset/genome; deterministic; optional `wind_field` stage.
- **[epic_overworld_generation.md › OW-E8]** Moisture advects with prevailing wind along region graph; ocean = source; 0–1 per region; runs after elevation, before `triangle_values`; bit-identical under `deterministic_globe`.
- **[epic_overworld_generation.md › OW-E9]** Rain shadows: orographic lift windward, moisture loss leeward when crossing ridges; stage `rain_shadow` (or folded into advection).
- **[epic_overworld_generation.md › OW-E10]** Optional debug viz of wind / moisture delta (no marshal-only arrays as source of truth).
- **[epic_overworld_generation.md › OW-E11]** `priority_flood` Barnes-style pit resolution on **region** elevation before `river_downflow`; monotonic drainage to ocean; saddle spill carved; deterministic; bench metric.
- **[epic_overworld_generation.md › OW-E12 (Done)]** Primal icosphere export only (`generate_planet_terrain_mesh_quad`); dual-derived render paths removed; outward winding enforced; <5% inward tris.
- **[epic_overworld_generation.md › OW-E13]** Gentler displacement: land radial scale `0.04`, subsea `0.3×` land scale, shared constant across `PlanetTerrainMesh`/`WorldGenPreviewExport`.
- **[epic_overworld_generation.md › OW-E14]** `river_carve` default in `default_globe`/`performance_globe` after `erosion`.
- **[epic_overworld_generation.md › OW-E15]** Bilateral (edge-preserving) elevation smooth; coastline/sea-level cells optionally locked; tunable passes.
- **[epic_overworld_generation.md › OW-E16 (Done core)]** Stream-power erosion replaces triangle `apply_hydraulic_erosion`; uplift from plate mountain seeds; in-loop `priority_flood`; sediment deposit + form-exposed `erosion_k` optional follow-ups.
- **[epic_overworld_generation.md › OW-E17]** Optional advanced stages: `terrain_warp` (FBM domain warp), `glacial_erosion`, extended climate; default off; no GPL import.
- **[epic_overworld_generation.md › OW-E18]** Region triangle rings validated before hydrology; optional `validate_topology` stage; abort/loud error in dev.
- **[epic_overworld_generation.md › OW-E19]** Marshal/preview export capped for huge region counts (`k_max_regions_with_debug_mesh`); optional LOD export.
- **[epic_overworld_generation.md › OW-E20]** Release transient globe memory (half-edge mesh, triangle buffers) after atlas build; retain CSR + per-region fields.
- **[epic_overworld_generation.md › OW-E21 (Done)]** Land (target) matches Land (result) at sea level 0; unified elevation baseline; `fit_land_coverage` additive shift after erosion; bench `land` gates 0/40/100% within ±3%.
- **[epic_overworld_generation.md › OW-P1]** Save/load overworld session header (seed, genome, atlas checksum/blob); versioned format.
- **[worldgen.md › §3]** `PlanetGenome` stores: L_star, a, e, S_eff, M_p, R_p, P_rot, obliquity, p0, albedo, greenhouse, water_fraction, precipitation, material_tags, schema_version. Derived values (g, P_orb, T_eq, scale_height…) computed on demand via `Planet(genome)`.
- **[worldgen.md › §5]** Presets `Earthlike`/`OceanWorld`/`Dry`; C++ built-in preset logic; XML documents same structure; `set_blueprint_to_random` randomises for variety.
- **[worldgen.md › Land vs water]** `water_fraction` is a planet-scale genome param (not applied to elevation); `land_fraction` is the terrain target fitted by `fit_land_coverage`; sea level stays 0.
- **[worldgen_modern.md › §10 / Streaming]** Connectivity fixed for one generation; all geology/climate/erosion mutate scalar fields, not 3D vertex positions; terrain mesh derived once radially (`k_planet_elevation_scale = 0.08`); streaming samples atlas continuously, not nearest-region-only.
- **[worldgen_modern.md › Session atlas vs preview marshal]** `PlanetSurfaceAtlasBuilder` copies compact gameplay fields into `SimBridge`; temperature derived at query time (`OverworldSurfaceSampler`), not stored; marshal dict is debug-only.
- **[world_gen_pipelines.xml]** `default_globe` order: topology → half_edge_mesh → region_neighbours → coarse_sim_upsample(opt) → tectonic_plates → plate_properties → elevation → erosion → fit_land_coverage → moisture → triangle_values → priority_flood → river_downflow → river_flow → river_carve → terrain_mesh(opt). `performance_globe` identical; `deterministic_globe` drops `coarse_sim_upsample` and `river_carve`.
- **[planet_presets.xml]** Earthlike has full knob set + constraints (insolation 0.85–1.15, surface_temp 273–310 K, gravity 8.5–11.5, pressure 70–140 kPa, eccentricity_max 0.2, max_attempts 32); OceanWorld water 0.85–0.95; Dry water 0.1–0.35.

### Determinism / correctness / QA (SC)

- **[epic_sim_correctness.md › Vision]** Every `default_globe` run produces a closed dual region–triangle structure, outward-facing exported meshes, and **bit-identical hashes under `deterministic_globe`** for fixed inputs.
- **[epic_sim_correctness.md › SC-E1]** `worldgen_bench` fails CI when `bad_adjacency > 0` or `tris_not_in_3_rings > 0` on reference configs.
- **[epic_sim_correctness.md › SC-E2 (Done)]** Inward terrain triangle rate gated <5% on reference `default_globe`+terrain_mesh.
- **[epic_sim_correctness.md › SC-E3]** Determinism regression: `deterministic_globe` prints stable hash of region elevation + moisture (+flow optional); golden per platform or tolerance doc.
- **[epic_sim_correctness.md › SC-E4]** Topology validation as pipeline stage/post-pass (`validate_topology`); optional auto-fail in dev.
- **[epic_sim_correctness.md › SC-E5]** Ring repair/regen policy documented (runbook).
- **[epic_sim_correctness.md › SC-E6]** Steep-edge / orphan-flow metrics after priority-flood.
- **[epic_sim_correctness.md › SC-E7]** Presentation-vs-sim coordinate contract tested (sim Z-up → Godot Y-up via `PresentationCoords`; no duplicate transforms).
- **[epic_sim_correctness.md › SC-E8 (Done)]** `worldgen_bench gameworld` gate: max 6 m adjacent vertex Δ, 0.05 m chunk-edge seam Δ.
- **[epic_sim_correctness.md › Out of scope]** Full floating-point portability across CPUs is **not** guaranteed; SIMD paths have documented known limits.

### Game world streaming (GW)

- **[epic_game_world_streaming.md › Vision]** Where the player stands, the sim runs a **second procedural pass at metre scale**, streams chunks in/out, keeps **authoritative** cell state; Godot instantiates `ChunkNode` from diffs only; tools send intents that mutate sim and emit `CellChanged`.
- **[epic_game_world_streaming.md › Goals]** Chunk content seeded from `WorldSeed` + overworld atlas sample (plate, elev, moist, biome); `ChunkStore` is source of truth; stream by focus radius; player edits via intents; documented `ChunkCoord ↔ sphere point ↔ region` map; preview/game-world mutually exclusive.
- **[epic_game_world_streaming.md › GW-E1]** `GameWorldLocalMap` defines chunk ↔ unit_dir; anchor/region-local frame documented.
- **[epic_game_world_streaming.md › GW-E2]** `GameWorldChunkGenerator` uses overworld atlas + seed per coord; `GameWorldPipeline` runs `game_world_gen_pipelines.xml`; height from atlas elev/moist.
- **[epic_game_world_streaming.md › GW-E3]** Loaded chunks survive regen unless invalidated; edits persist in memory.
- **[epic_game_world_streaming.md › GW-E4]** Dig intents lower height and emit `CellChanged`.
- **[epic_game_world_streaming.md › GW-E5]** Cell layers beyond height (surface, moisture override).
- **[epic_game_world_streaming.md › GW-E6]** Game-world gen rules in defs (biome spawn tables) via `DefDatabase` (M4).
- **[epic_game_world_streaming.md › GW-E7]** Save/load of chunk edit blobs: seed + genome + atlas id + per-chunk deltas.
- **[epic_game_world_streaming.md › GW-E8]** Optional `GameWorldPatch` per overworld region (intermediate tier) if single region scalar too coarse.
- **[epic_game_world_streaming.md › GW-E9]** `ChunkStore` unloads far chunks in sim; emits `ChunkUnloaded`; preserves edited chunks.
- **[epic_game_world_streaming.md › GW-E10/E11]** `IntentRouter` wired from GDExtension intents; dig handler registered (replaces empty `apply_intent_dig`).
- **[epic_game_world_streaming.md › GW-E12/E18]** `sample_macro` uses continuous overworld scalars (IDW on primary region + `region_neighbours`), not nearest-region-only.
- **[epic_game_world_streaming.md › GW-E16/E17]** `game_world_gen_pipelines.xml` loaded like overworld pipelines; `GameWorldPipeline` runs registered stages per chunk; default id `default_terrain`.
- **[epic_game_world_streaming.md › GW-E19]** Smooth walkable chunk terrain: coherent `detail_noise`, shared border heights; SC-E8 gate.
- **[epic_game_world_streaming.md › GW-E14/E15/UI.3]** Sim-owned session inventory exposed via SimAPI; dig adds yield stacks; minimal hotbar/grid UI (M1, design TBD).
- **[epic_game_world_streaming.md › Coordinate notes]** `CHUNK_SIZE` = 16 cells; cell size 1 m → 256×256 m per chunk; overworld regions 1k–65k; one region ≠ one chunk.
- **[game_world_gen_pipelines.xml]** `default_terrain` stages: `sample_macro` → `base_height` → `detail_noise` → `build_height_grid`.
- **[QUEUED_PROMPTS/m0-game-world-terrain-pipeline.md › Closure report]** GW-E16–E19, SC-E8, GW-E1/E2/E12, GW-2.4/2.5/7.0b/7.0c are headless-done; GW-7.0, 7.1, 7.2, 1.2 pending **manual Godot play test**.

### Player / survival / spirit / emergence / ecology / presentation (built on simulator)

- **[epic_player_play.md › PL-0.1–0.3, 1.1]** Play camera; avatar movement respecting streamed terrain height; interaction ray for cells/entities; possession HUD shell (M2/M5).
- **[epic_survival_colony.md › SV-0.1–0.4, 1.1, 2.1]** ≥1 survival need; ≥1 environmental threat tied to overworld/biome; simple place/build via intents consuming inventory; M3 crafting reuses M1 inventory API; era summaries (M6); colony depth (North star).
- **[epic_gameplay_emergence.md › GE-0.1–0.3, 1.1]** Gameplay bias defs at boot; world-gen/spawn presets apply bias weights deterministically; bias ids from `game_profile.xml`; unlock tables (later).
- **[epic_spirit_meta.md › SP-0.1–0.2]** Sim clock paused unless possession active; Godot loop respects sim pause (no duplicate clocks).
- **[epic_spirit_meta.md › SP-1.1/2.1/3.1]** Influence intents without possession; possess registered body archetypes; multigenerational summaries.
- **[epic_ecology.md › EC-0.1–0.3]** Template species defs; regional population scalars per region/biome deterministic from seed; local agent spawn from regional scalars ("LOD for life").
- **[epic_ecology.md › EC-1.1–1.3]** North star: genetic-similarity continuum, deterministic crossbreed/evolution, player-authored species.
- **[epic_presentation.md › PR-0.1–0.3, 1.1]** Cel-shaded chunk terrain; documented lighting rig; biome-tinted terrain from `sample_surface`/chunk biome id; stylised foliage LOD (later). Presentation consumes sim outputs; does not own gameplay state.

---

## 3. Condensed requirements model

### Purpose of the system
Generate a **unique, deterministic planet per player** and let the player eventually live on a streamed, editable slice of it. The system is split into a read-only macro **overworld** (whole-sphere atlas) and a mutable micro **game world** (streamed metre-scale chunks), both owned by a C++ simulator and surfaced to Godot through one `SimAPI` door. *(epics.md › Quick index; README.md › Terminology; milestones.md › M0.)*

### Core product bet
A "survival / basebuilder / terraformer / nature-enjoyer / time-passing game like RimWorld" set on a unique, earth-scale, emergent procedural world, using clever streaming and aggregated generations. *(IDEAS.md › features.)* Engineering bet: a **content-pack engine** where content + assembly are data (XML defs, `game_profile`, pipelines) and only runtime systems are compiled. *(moddability.md › Three layers.)*

### What the overworld is
A generated macro planet: a low-resolution **spherical region atlas** (plates, elevation, moisture, derived temperature/biome) built **once per session**, read-only baseline for play, stored authoritatively in `SimBridge` as `PlanetSurfaceAtlas`. *(README.md › Terminology; epic_overworld_generation.md › Terminology, OW-E1/E2.)*

### What the game world is
A micro, high-detail, **player-editable** local simulation streamed as 16×16-cell chunks (1 m cells, 256 m/chunk) around the player's focus, generated by a second metre-scale procedural pass seeded from the overworld atlas + world seed. `ChunkStore` is the source of truth; Godot renders diffs only. *(epic_game_world_streaming.md › Vision, Goals, Coordinate notes.)*

### What the simulator owns
Authoritative world state: defs (`DefDatabase`), world seed, planet genome, overworld atlas, ECS, chunk store + edit overlay, intent routing, rules, pipelines, the async `WorldGenJob`. *(moddability.md › Simulation vs on-screen visuals; epic_sim_api_platform.md › Architecture.)* Determinism, topology correctness, and bench gates are also sim-side responsibilities. *(epic_sim_correctness.md › Vision.)*

### What Godot/presentation owns
Form UI, progress/cancel, optional debug preview, view nodes (`ChunkNode`, `SpherePreview`), input→intents, coordinate conversion once via `PresentationCoords`. Views are "dumb renderers" that must not call `SimAPI` for sim state. *(moddability.md › Simulation vs on-screen visuals, Linter rules; game_scene_order.md.)*

### Determinism requirements
Same form + pipeline → same region fields. `deterministic_globe` pipeline must be **bit-identical** for fixed inputs (moisture advection, rain shadow, priority-flood all included). Determinism regression via hash of region elevation + moisture (+flow). Full FP portability across CPUs is **explicitly out of scope**; SIMD limits documented. Ecology/population trajectories also required deterministic from seed. *(epic_overworld_generation.md › Goals, OW-E8/E11; epic_sim_correctness.md › SC-E3, Out of scope; epic_ecology.md › Goals.)*

### Data/model requirements
`WorldSeed` (value, height_scale, octaves, frequency) + `PlanetGenome` (full orbital/physical knob set + material_tags + schema_version, derived values on demand) stored in bridge after gen. `PlanetSurfaceAtlas` = fixed topology + per-region plate/elevation/moisture + positions + CSR neighbours; temperature derived at query time, not stored. *(worldgen.md › §2–4; worldgen_modern.md › Session atlas; OW-E1.)*

### Pipeline/modding requirements
Overworld and game-world stage **order comes from XML** (`world_gen_pipelines.xml`, `game_world_gen_pipelines.xml`), not hardcoded in Godot/C++. Stages registered by stable `stage_id`; mods override via later packs / alternate pipeline id in profile; unknown stage fails loudly in dev. Boot from merged `res://data/` root with `manifest.xml` load order; later packs override same ids. *(moddability.md › World generation pipeline, Pack loading; OW-E5; GW-E16.)*

### Planet genome / preset requirements
Presets `earthlike`/`ocean_world`/`dry` defined as distributions + constraints + material weights in `planet_presets.xml`; `PlanetGenerator.generate_from_blueprint` samples a `PlanetGenome`; random blueprint path for variety; preset `water_fraction` defaults the land slider. *(worldgen.md › §3,5; planet_presets.xml.)*

### Surface atlas requirements
Built by `PlanetSurfaceAtlasBuilder` from `PlanetGlobe` after the pipeline; owned by `SimBridge` for the session; supports `locate_region(unit_dir)` + `sample_surface` returning region id + scalars + derived temperature/biome; spatial index (`RegionLocator`) for fast lookup. *(OW-E1–E3; worldgen_modern.md › Session atlas; performance_big_o_audit.md › P1 Done.)*

### Query/API requirements
`SimAPI` exposes: `boot`, async world-gen (`start/poll/cancel/take`), sync `apply_world_gen_form` (tools), `sample_surface`, `request_chunks`, `poll_diffs`, `apply_intent`, presentation path getters, `commit_overworld_for_play`, `enter_game_world`. `sample_surface` returns a small documented dict, not many forwarded arrays. *(epic_sim_api_platform.md › SA-0.2/0.3; OW-E4; GW-0.1/0.2; game_scene_order.md › 5b.)*

### Climate requirements
Temperature derived from latitude + elevation (+ optional axial tilt), computed at query time. Biome assignment maps temperature/moisture/elevation; intended to be data-driven from `biomes.xml` at sample time (G4, Later). *(worldgen_modern.md › §7,9; OW-E3; epic_overworld_generation.md › G4.)*

### Wind/moisture/rain-shadow requirements
Prevailing-wind field from preset/genome (OW-E7); moisture advects upwind→downwind on the region graph with ocean as source (OW-E8); rain shadow gives windward orographic lift + leeward depletion (OW-E9); optional debug overlays (OW-E10). All **Later** (G3) and must stay deterministic. Current moisture is ocean-distance BFS only (`MoistureAssigner`). *(epic_overworld_generation.md › OW-E7–E10, Technical reference.)*

### Terrain/hydrology requirements
Stream-power erosion on the region graph (OW-E16, done core); `priority_flood` pit resolution before rivers (OW-E11); `fit_land_coverage` additive sea-level-0 land fitting (OW-E21, done); `river_downflow`/`river_flow`/`river_carve` triangle flow + valley carving; edge-preserving smooth (OW-E15); gentler radial displacement (OW-E13). Terrain mesh is a height field on a fixed icosphere; primal-quad export only. *(worldgen_modern.md › §6–10; OW-E11–E16, E21; world_gen_pipelines.xml.)*

### Streaming/game-world requirements
`GameWorldLocalMap` (GW-E1), atlas-seeded `GameWorldChunkGenerator`/`GameWorldPipeline` (GW-E2/E16/E17), continuous `sample_macro` (GW-E18), coherent detail + shared seams (GW-E19), focus-radius load (`k_stream_radius_chunks=2`, ≤25 chunks), sim-side unload (GW-E9), persisted edits (GW-E3), chunk save/load (GW-E7), intents/dig (GW-E4/E10/E11). *(epic_game_world_streaming.md; game_world_gen_pipelines.xml; performance_big_o_audit.md › Runtime paths.)*

### Ecology/gameplay-emergence requirements
Ecology v0: template species + regional population scalars + local agent spawn ("LOD for life") at M7; full genetics North star. Emergence: bias defs at boot steer world-gen/spawn tables deterministically (M4); unlock tables later. *(epic_ecology.md; epic_gameplay_emergence.md.)*

### Preview/debug requirements
`SpherePreview` + `SpherePreviewOverlay` are **debug Godot views of overworld data, explicitly not the game world / not gameplay**; toggle layers without re-running gen; large worlds defer heavy mesh apply; preview should eventually sample atlas rather than own data; marshal dict is debug-only. *(README.md › Terminology; epic_overworld_generation.md › OW-5.*, 6.1, OW-E6; game_scene_order.md › steps 5/6.)*

### Persistence/save-load requirements
Overworld session header save/load (seed, genome, atlas checksum/blob, versioned) — OW-P1, Later. Game-world chunk edit save (seed + genome + atlas id + per-chunk deltas, incl. inventory blob later) — GW-E7, Later. Save/load **file format is out of scope** for SA and OW core epics. *(OW-P1; GW-E7; epic_sim_api_platform.md › Out of scope.)*

### Performance/memory requirements
Non-blocking world gen on worker thread; marshal on main thread after join. Marshal/debug caps for huge region counts (OW-E19); release transient globe memory after atlas build (OW-E20). Measured: elevation+erosion ≈85% of gen wall time at interactive scales; `locate_region` was O(R)/sample (P1 spatial index done); chunk gen amortised across frames (P5 done); packed arrays across boundary (P6 done). Points slider capped at icosphere subdivision 9 (~2.6M sites). *(performance_big_o_audit.md; OW-E19/E20; git log d089c20.)*

### Correctness/QA requirements
Closed dual region–triangle topology validated before hydrology; outward-facing meshes; bench gates (`gate`, `gameworld`, `det`, `locator`) wired into CI/pre-commit; topology rings, inward-tri rate, chunk seams, land-fraction all gated. *(epic_sim_correctness.md; critical_bugs_audit.md › #9–11; cool_bugs/.)*

### Out-of-scope items (per docs)
- Game-world chunks + terrain edits → out of OW epic. *(epic_overworld_generation.md › Out of scope.)*
- Save/load **file format** → out of SA and OW core. *(epic_sim_api_platform.md, epic_overworld_generation.md › Out of scope.)*
- Replacing debug `SpherePreview` with final art → out of OW. *(epic_overworld_generation.md › Out of scope.)*
- Overworld pipeline stages, full overworld mesh at gameplay LOD, multiplayer chunk sync, advanced ecology/fluid sim, full crafting/colony stockpiles → out of GW. *(epic_game_world_streaming.md › Out of scope.)*
- Godot-only winding fix as the sim correctness solution; full FP portability across CPUs → out of SC. *(epic_sim_correctness.md › Out of scope.)*
- BotW parity / licensed-look; gameplay state in presentation → out of PR. *(epic_presentation.md › Out of scope.)*

### Deferred/later-phase items
Climate refinement (OW-E7–E10, G3); advanced terrain (OW-E17, G4b); biome derivation from rules (G4); overworld persistence (OW-P1, G5); marshal-from-atlas (OW-E6); cell layers/content (GW-E5/E6, S4); chunk save (GW-E7, S5); region-patch tier (GW-E8, S6); determinism hash / coord contract (SC-E3/E7); all of PL/SV/GE/SP/EC/PR beyond M0 (M2–M8 + North star). *(milestones.md › master table; epic phase tables.)*

### Open questions and ambiguities
- **Inventory location/yield/persistence** (SimBridge slot vs ECS component; dig-yield source; tool vs material slots) — open in M1. *(milestones.md › M1 open design questions.)*
- **OW-E11 bench metric** "TBD"; **OW-E9** ridge-cross factor "configurable" but unspecified. *(epic_overworld_generation.md › OW-E11, E9.)*
- **GW-E8** region-patch tier conditional ("if single region scalar too coarse") — trigger threshold undefined.
- **`PlanetGlobe`/`PlanetSurfaceAtlas`/`PlanetGlobePipeline` naming** flagged for a "later refactor". *(epic_overworld_generation.md › Terminology.)*
- **Biome derivation** intended from `biomes.xml` but UNCLEAR whether wired today (would need to check `OverworldSurfaceSampler`/`biomes.xml` consumer code; out of scope for this doc-only audit).

### Requirements that appear to be missing a player-visible loop
The entire M0 scope ends at **"walk on streamed terrain"** with **no player avatar verbs**: generate → optional debug preview → Start → chunks stream around a focus. *(milestones.md › M0 Definition.)* As of the M0 closure report, even "enter game world / walk" is **pending manual Godot play test** — not confirmed player-visible. *(QUEUED_PROMPTS/m0-game-world-terrain-pipeline.md › Pending.)* The first genuine player **actions** (dig/terraform) are M1 and currently **no-op** in code. *(critical_bugs_audit.md › #7.)* First **goal/tension** (needs, threats) is M3; spirit/possession M5; ecology M7. So: a large, well-specified **substrate** exists with **no scheduled-and-implemented gameplay loop** before M1–M3.

---

## 4. Phase map (from repo docs only)

| Phase bucket | Items | Source |
|--------------|-------|--------|
| **M0 — current/active** | SA boot+async+form; OW atlas in bridge (E1–E3), gen UI/load/preview, E18 topology, E12/E21 done; SC-E1/E2/E4/E8; GW skeleton→play (E1/E2/E12/E16–E19, 7.0–7.2, 1.2) | milestones.md › M0; epics phase tables |
| **M1** | Dig/terraform via IntentRouter (GW-E4/E10/E11/7.3/UI.1); edits persist + sim unload (GW-E3/E9); inventory scaffold (GW-E14/E15/UI.3, SV-0.4 contract) | milestones.md › M1 |
| **M2** | Playable avatar, play camera, interaction ray (PL-0.*) | epic_player_play.md |
| **M3** | Minimal survival need + threat + place/build (SV-0.1–0.4) | epic_survival_colony.md |
| **M4** | Guardrailed emergence bias defs (GE-0.*); GW-E6 biome spawn defs | epic_gameplay_emergence.md; GW table |
| **M5** | Spirit influence + possession + sim-time-gate (SP-0–2) | epic_spirit_meta.md |
| **M6** | Meta-generation summaries (SP-3.1, SV-1.1) | milestones.md |
| **M7** | Ecology v0 (EC-0.*) | epic_ecology.md |
| **M8** | Presentation cel pass (PR-0.*) — parallel, non-blocking | epic_presentation.md |
| **Later** | OW climate G3 (E7–E10), terrain G3b/G4b (E13/E15/E17), G4 biome rules, G5 persistence (P1), E6 marshal-from-atlas, E19/E20 caps; GW cell layers/save/patch (E5/E7/E8); SC-E3/E5/E6/E7; GE-1.1; PR-1.1 | epic phase tables; milestones master table |
| **North star** | Full genetics (EC-1.*), colony depth (SV-2.1), full possession zoo | milestones.md › North star |

### Phase-ordering conflicts (called out, not reconciled)

1. **"Time only moves when possessing" — scheduled vs decided-against.**
   - `epic_spirit_meta.md › Vision/SP-0.1` and `milestones.md › M5` treat sim-time-pause-unless-possessing as a **required M5 feature**.
   - `IDEAS.md › features` marks the same idea **`never` — "DECIDED AGAINST"**.
   - These directly contradict. UNCLEAR which is current; would need a product-owner decision. The audit does not resolve it.

2. **Pipeline stage order — docs vs XML.** The authoritative `world_gen_pipelines.xml` `default_globe` runs `elevation → erosion → fit_land_coverage → moisture → … → river_carve → terrain_mesh`. But `moddability.md › World generation pipeline` lists `…elevation → moisture → triangle_values → priority_flood → river_downflow → river_flow → erosion → terrain_mesh` (erosion **after** rivers, no `fit_land_coverage`/`river_carve`/`coarse_sim_upsample`), and `worldgen_modern.md › Mapping to Growth` lists a third variant (no `fit_land_coverage`). The prose docs are stale relative to the XML; treat the **XML as authoritative** for current order.

3. **OW-E4 milestone.** `milestones.md › master table` lists OW-E4 as **M1** ("stretch M0"); `epic_overworld_generation.md › Phases` puts OW-E4 in **G2 — Later**. Minor conflict.

4. **OW-E14/E16/E12/E21 vs phase tables.** OW-E12 and OW-E21 are marked **Done** inline and in the master table, but still appear inside the **"Later" G3b** phase list in `epic_overworld_generation.md › Phases`. The phase table is stale for completed stories.

5. **`critical_bugs_audit.md` item #1 "partial" vs OW-E1/E2 "M0 / done-for-session".** Atlas exists in bridge (session play) but full globe retained + marshal duplication open (OW-E6/E20). Not a hard conflict, but status is split across docs.

---

## 5. Dependency chains

Major chains derived from `epics.md › Dependency overview`, epic "Depends on" fields, and architecture diagrams:

**Core substrate chain (M0):**
`DefDatabase (merged boot) -> WorldSeed + PlanetGenome (SeedGenerator/PlanetGenerator) -> PlanetGlobePipeline (world_gen_pipelines.xml) -> PlanetGlobe -> PlanetSurfaceAtlasBuilder -> SimBridge atlas ownership -> locate_region / sample_surface -> GameWorldLocalMap -> GameWorldPipeline (sample_macro continuous) -> ChunkStore -> DiffQueue -> WorldViewManager -> ChunkNode (player-visible terrain)`

**Platform/API enabling chain:**
`SimAPI.boot(res://data/) -> game_profile.xml bindings -> presentation scene/script paths -> WorldGenMenu -> WorldGenCardFormBinder -> start_world_gen_async -> WorldGenJob (worker) -> poll/cancel -> take_world_gen_async_result (main thread marshal) -> WorldGenPreviewApplier -> SpherePreview (debug) -> commit_overworld_for_play -> enter_game_world -> streaming`

**Correctness gating chain:**
`PlanetGlobePipeline -> region_neighbours/SphereDual -> validate_topology (OW-E18/SC-E4) -> SC-E1 ring gate -> hydrology (priority_flood/rivers) -> terrain_mesh -> SC-E2 inward-tri gate; deterministic_globe -> SC-E3 hash; GameWorldPipeline -> SC-E8 gameworld seam gate`

**Climate refinement chain (Later):**
`PlanetGenome/preset -> wind_field (OW-E7) -> moisture_advection (OW-E8) -> rain_shadow (OW-E9) -> moisture field -> river/biome fields -> OW-E10 debug overlay`

**Terrain/hydrology chain:**
`elevation (RBG distance fields) -> erosion (stream-power, OW-E16) -> fit_land_coverage (OW-E21) -> priority_flood (OW-E11) -> river_downflow/flow -> river_carve (OW-E14) -> prepare_elevation_for_terrain_mesh (smooth, OW-E13/E15) -> generate_planet_terrain_mesh_quad (OW-E12 outward winding)`

**Gameplay-on-substrate chain:**
`GW M0 streaming -> GW M1 dig/intents (IntentRouter) + inventory (GW-E14) -> PL M2 avatar/camera/ray -> SV M3 needs/threat/build (consumes inventory) -> GE M4 bias -> SP M5 possession + time gate -> SP/SV M6 meta summaries -> EC M7 populations -> PR M8 cel art (parallel)`

**Performance unblock chain:**
`locate_region O(R) -> RegionLocator spatial index (P1) -> viable per-chunk sample_macro -> chunk gen amortisation (P5) + packed-array marshal (P6) -> bounded frame cost`

---

## 6. Final condensed summary (plain English)

**What the system is ultimately supposed to become.** Growth's worldgen/simulator is meant to be a **deterministic, moddable planet engine**. From a seed and a planet preset it generates a whole spherical "overworld" — tectonic plates, elevation, erosion-shaped relief, moisture, rivers, and (later) wind-driven climate and rain shadows — and stores it as a compact, queryable **surface atlas** owned by the C++ simulator. On top of that macro baseline it runs a **second, metre-scale procedural pass** wherever the player is, streaming editable 16×16-cell chunks in and out and keeping their state authoritative in the sim. Godot is strictly the presentation/input layer behind a single `SimAPI` door; all rules, world state, determinism, and content live in C++ + XML data packs so mods can change content, assembly, and pipeline order without recompiling. The long-horizon vision (IDEAS) layers a **survival/basebuilder/terraformer** loop, **guardrailed emergence**, a **spirit/possession** model with multigenerational time, and an **emergent ecology** of genetically-continuous species onto this substrate.

**World simulator/substrate vs playable game loop — they are not the same thing, and the docs are careful about this.** The "world simulator/substrate" is the overworld atlas + game-world chunk streaming + determinism/QA machinery (epics SA, OW, SC, GW). The "playable game loop" — verbs, goals, tension, progression — lives in *separate, later* epics (GW dig at M1; PL avatar at M2; SV needs/threats at M3; SP spirit at M5; EC ecology at M7). The debug **planet preview (`SpherePreview`) is explicitly defined as a developer view of overworld data, not gameplay** (`README.md › Terminology`; `epic_overworld_generation.md › OW-5.*`), and the rules of this audit forbid treating it as a loop.

**Does the repository currently define a player-visible gameplay loop?** **Not yet — and where one is scheduled, it is not confirmed implemented.** The active milestone M0 is defined purely as *generate → optional debug preview → press Start → see overworld-shaped chunks stream around a focus* (`milestones.md › M0 Definition`). That is substrate, not a loop: there is no avatar verb in M0. Even the "enter game world / walk on terrain" steps (GW-7.0/7.1/7.2/1.2) are recorded as **pending manual Godot play test**, i.e. not yet verified player-visible (`QUEUED_PROMPTS/m0-game-world-terrain-pipeline.md`). The earliest real player *action* — dig/terraform — is **M1 and is currently a no-op in code** (`critical_bugs_audit.md › #7`: `apply_intent_dig` empty, `IntentRouter` bypassed). The earliest player *goal/tension* (a survival need + threat) is **M3**. So the repository today defines a richly specified simulator with a **deferred, not-yet-working** gameplay loop; the first loop will appear at M1 (dig→collect→place) and M3 (survive). Anyone deciding "what is this supposed to do" should read it as: **the planet/streaming substrate is the scoped, near-complete deliverable; the game is intentionally downstream and largely unbuilt.**

**One unresolved product contradiction worth surfacing:** the "time only advances while possessing" mechanic is scheduled as a required M5 feature in the spirit epic and milestones, but `IDEAS.md` marks the same idea **`never` / "DECIDED AGAINST."** This needs an owner decision; it is not reconciled here.

---

## 7. Trace matrix

Legend — Category: PLAT=platform/API, OW=overworld, HYD=terrain/hydrology, CLIM=climate, ATLAS=atlas/query, STREAM=game-world streaming, QA=correctness, GAME=gameplay layer, PERSIST=save/load, PERF=performance, PREVIEW=debug. Player-visible impact: **None** (engine only), **Indirect** (shapes what player later sees), **Direct** (player sees/does it). E/I = Explicit / Inferred.

| Req ID | Requirement | Category | Source file | Heading/story | Phase | E/I | Depends on | Player-visible |
|--------|-------------|----------|-------------|---------------|-------|-----|------------|----------------|
| SA-0.1 | Boot merged `res://data/` root | PLAT | epic_sim_api_platform.md | SA-0.1 | M0 | E | — | None |
| SA-0.2 | Async start/poll/cancel/take API | PLAT | epic_sim_api_platform.md | SA-0.2 | M0 | E | SA-E1/E2 | Indirect |
| SA-0.3 | Sync gen retained for tools | PLAT | epic_sim_api_platform.md | SA-0.3 | Later | E | SA-0.2 | None |
| SA-0.4 | Stub backend errors `ok:false` | PLAT | epic_sim_api_platform.md | SA-0.4 | Later | E | — | Indirect |
| SA-E1 | Single `WorldGenJob` on `SimBridge` | PLAT | epic_sim_api_platform.md | SA-E1 | M0 | E | — | None |
| SA-E2 | GDExtension async bindings | PLAT | epic_sim_api_platform.md | SA-E2 | M0 | E | SA-E1 | None |
| SA-E4 | `world_size` → `sim_voronoi_sites` | PLAT | epic_sim_api_platform.md | SA-E4 | M0 | E | — | Indirect |
| SA-E5 | Guardrails on extreme site counts | PLAT | epic_sim_api_platform.md | SA-E5 | M0 | E | — | Direct |
| SA-1.x/3.2/E3 | Profile-driven scene paths; boot before UI | PLAT | epic_sim_api_platform.md | SA-1.1/1.2/3.2/E3 | M0 | E | SA-0.1 | None |
| OW-cfg | Player configures seed/climate/res/plates | OW | epic_overworld_generation.md | OW-3.1/3.6 | M0 | E | SA form | Direct |
| OW-det | Deterministic overworld per form+pipeline | OW | epic_overworld_generation.md | Goals | M0 | E | pipeline | Indirect |
| OW-4.4 | Seed+genome stored in bridge | ATLAS | epic_overworld_generation.md | OW-4.4 | M0 | E | gen | None |
| OW-E1 | Build `PlanetSurfaceAtlas` from globe | ATLAS | epic_overworld_generation.md | OW-E1 | M0 | E | pipeline | Indirect |
| OW-E2 | `SimBridge` owns atlas for session | ATLAS | epic_overworld_generation.md | OW-E2 | M0 | E | OW-E1 | Indirect |
| OW-E3 | `locate_region`+`sample_surface` | ATLAS | epic_overworld_generation.md | OW-E3 | M0 | E | OW-E1/E2 | Indirect |
| OW-E4 | `SimAPI.sample_surface` small dict | ATLAS | epic_overworld_generation.md | OW-E4 | M1 (mst) / Later (epic) ⚠ | E | OW-E3 | Direct |
| OW-E5 | Pipeline stages declared in XML | OW | epic_overworld_generation.md | OW-E5 | Later | E | — | None |
| OW-E7 | Prevailing-wind field | CLIM | epic_overworld_generation.md | OW-E7 | Later | E | genome | Indirect |
| OW-E8 | Wind-advected moisture | CLIM | epic_overworld_generation.md | OW-E8 | Later | E | OW-E7 | Indirect |
| OW-E9 | Rain shadows | CLIM | epic_overworld_generation.md | OW-E9 | Later | E | OW-E7/E8 | Indirect |
| OW-E10 | Wind/moisture debug overlay | PREVIEW | epic_overworld_generation.md | OW-E10 | Later | E | OW-E8 | None |
| OW-E11 | `priority_flood` region drainage | HYD | epic_overworld_generation.md | OW-E11 | Later | E | topology | Indirect |
| OW-E12 | Outward primal-quad mesh only (**Done**) | HYD/QA | epic_overworld_generation.md | OW-E12 | Done | E | terrain_mesh | Indirect |
| OW-E13 | Gentler radial displacement | HYD | epic_overworld_generation.md | OW-E13 | Later | E | terrain_mesh | Indirect |
| OW-E14 | `river_carve` default | HYD | epic_overworld_generation.md | OW-E14 | Later | E | erosion | Indirect |
| OW-E15 | Bilateral elevation smooth | HYD | epic_overworld_generation.md | OW-E15 | Later | E | elevation | Indirect |
| OW-E16 | Stream-power erosion (**Done core**) | HYD | epic_overworld_generation.md | OW-E16 | Done(core) | E | elevation | Indirect |
| OW-E17 | Domain warp/glacial/extended climate | HYD/CLIM | epic_overworld_generation.md | OW-E17 | Later(opt) | E | — | Indirect |
| OW-E18 | Validate region rings pre-hydrology | QA | epic_overworld_generation.md | OW-E18 | M0 | E | region_neighbours | None |
| OW-E19 | Cap marshal/debug for huge worlds | PERF | epic_overworld_generation.md | OW-E19 | Later | E | — | Indirect |
| OW-E20 | Release transient globe memory | PERF | epic_overworld_generation.md | OW-E20 | Later | E | OW-E1 | None |
| OW-E21 | Land target=result @ sea level 0 (**Done**) | HYD | epic_overworld_generation.md | OW-E21 | Done | E | erosion | Direct |
| OW-P1 | Overworld session save/load | PERSIST | epic_overworld_generation.md | OW-P1 | Later | E | OW-E1 | Direct |
| OW-5.x/6.1 | Optional debug preview + layer toggles | PREVIEW | epic_overworld_generation.md | OW-5.1–5.5/6.1 | M0/Later | E | marshal/atlas | None (debug) |
| GEN-genome | `PlanetGenome` full knob set + derived | ATLAS | worldgen.md | §3 | M0 | E | preset | Indirect |
| GEN-preset | Presets earthlike/ocean/dry distributions | OW | planet_presets.xml | presets | M0 | E | — | Direct |
| GEN-land | land_fraction terrain target vs water_fraction genome | HYD | worldgen.md | Land vs water | M0(Done E21) | E | fit_land_coverage | Direct |
| PIPE-globe | `default_globe` XML stage order authoritative | OW | world_gen_pipelines.xml | default_globe | M0 | E | DefDatabase | None |
| PIPE-det | `deterministic_globe` pipeline variant | QA | world_gen_pipelines.xml | deterministic_globe | M0 | E | — | Indirect |
| PIPE-perf | `performance_globe` (fewer iters) auto for large | PERF | world_gen_pipelines.xml; perf audit | P8 | M0(Done) | E | — | Indirect |
| SC-E1 | CI fail on invalid region rings | QA | epic_sim_correctness.md | SC-E1 | M0 | E | topology | None |
| SC-E2 | Inward-tri rate gated <5% (**Done**) | QA | epic_sim_correctness.md | SC-E2 | Done | E | OW-E12 | None |
| SC-E3 | Determinism hash regression | QA | epic_sim_correctness.md | SC-E3 | Later | E | deterministic_globe | None |
| SC-E4 | `validate_topology` stage/post-pass | QA | epic_sim_correctness.md | SC-E4 | M0 | E | region_neighbours | None |
| SC-E7 | Sim↔presentation coord contract test | QA | epic_sim_correctness.md | SC-E7 | Later | E | PresentationCoords | None |
| SC-E8 | `gameworld` seam gate (**Done**) | QA | epic_sim_correctness.md | SC-E8 | Done | E | GW-E19 | None |
| SC-fp | FP portability across CPUs out of scope | QA | epic_sim_correctness.md | Out of scope | n/a | E | — | None |
| GW-E1 | `GameWorldLocalMap` chunk↔unit_dir | STREAM | epic_game_world_streaming.md | GW-E1 | M0 | E | OW-E3 | Indirect |
| GW-E2 | Atlas+seed chunk generator | STREAM | epic_game_world_streaming.md | GW-E2 | M0 | E | GW-E1/OW-E3 | Direct |
| GW-E12/E18 | Continuous `sample_macro` (IDW) | STREAM | epic_game_world_streaming.md | GW-E12/E18 | M0 | E | OW-E3 | Direct |
| GW-E16/E17 | XML-driven `GameWorldPipeline` | STREAM | epic_game_world_streaming.md; game_world_gen_pipelines.xml | GW-E16/E17 | M0(Done) | E | DefDatabase | Indirect |
| GW-E19 | Smooth chunks, shared seams (**Done**) | STREAM | epic_game_world_streaming.md | GW-E19 | M0(Done) | E | GW-E18 | Direct |
| GW-7.0/1/2 | Enter play; meshes tile; place chunks | STREAM | epic_game_world_streaming.md | GW-7.0/7.1/7.2 | M0 (pending play test) | E | GW-E2 | Direct |
| GW-1.2 | Enter game world after gen | STREAM | epic_game_world_streaming.md | GW-1.2 | M0 (pending play test) | E | GW-7.0 | Direct |
| GW-2.2/2.3/E9 | Focus-radius load + unload | STREAM/PERF | epic_game_world_streaming.md | GW-2.2/2.3/E9 | M0/M1 | E | GW-E1 | Direct |
| GW-E3 | Persist edits across regen | STREAM | epic_game_world_streaming.md | GW-E3 | M1 | E | ChunkStore | Direct |
| GW-E4/E10/E11 | Dig intents via IntentRouter (now no-op) | GAME | epic_game_world_streaming.md; critical_bugs_audit.md | GW-E4/E10/E11; #7 | M1 | E | GW M0 | Direct |
| GW-E5 | Cell layers beyond height | STREAM | epic_game_world_streaming.md | GW-E5 | Later | E | GW-E4 | Direct |
| GW-E6 | Biome spawn rules from defs | GAME | epic_game_world_streaming.md; biomes.xml | GW-E6 | M4 | E | DefDatabase | Direct |
| GW-E7 | Chunk edit save/load | PERSIST | epic_game_world_streaming.md | GW-E7 | Later | E | GW-E3 | Direct |
| GW-E8 | Region-patch intermediate tier | STREAM | epic_game_world_streaming.md | GW-E8 | Later(opt) | E | OW-E3 | Indirect |
| GW-E14/E15/UI.3 | Sim inventory + dig yield + UI | GAME | epic_game_world_streaming.md | GW-E14/E15/UI.3 | M1 | E | GW-E11 | Direct |
| PL-0.1–0.3 | Play camera, avatar, interaction ray | GAME | epic_player_play.md | PL-0.* | M2 | E | GW M0 | Direct |
| PL-1.1 | Possession HUD shell | GAME | epic_player_play.md | PL-1.1 | M5 | E | SP | Direct |
| SV-0.1–0.4 | Need, threat, place/build, inventory reuse | GAME | epic_survival_colony.md | SV-0.* | M3 (0.4 doc M1) | E | GW-E14, PL | Direct |
| SV-1.1 | Era summary between sessions | GAME | epic_survival_colony.md | SV-1.1 | M6 | E | session meta | Direct |
| SV-2.1 | Colony depth (jobs/raids/economy) | GAME | epic_survival_colony.md | SV-2.1 | North star | E | EC/SP/GE | Direct |
| GE-0.1–0.3 | Bias defs steer gen/spawn from profile | GAME | epic_gameplay_emergence.md | GE-0.* | M4 | E | SA-0.1 | Indirect |
| GE-1.1 | Unlock tables gated by bias | GAME | epic_gameplay_emergence.md | GE-1.1 | Later | E | GE-0.* | Direct |
| SP-0.1/0.2 | Sim time paused unless possessing ⚠ | GAME | epic_spirit_meta.md | SP-0.1/0.2 | M5 | E | PL/GW | Direct |
| SP-1.1/2.1 | Influence + possess archetypes | GAME | epic_spirit_meta.md | SP-1.1/2.1 | M5 | E | IntentRouter | Direct |
| SP-3.1 | Multigenerational summaries | GAME | epic_spirit_meta.md | SP-3.1 | M6 | E | session meta | Direct |
| EC-0.1–0.3 | Template species, regional pops, local spawn | GAME | epic_ecology.md | EC-0.* | M7 | E | sample_surface | Direct |
| EC-1.1–1.3 | Genetic continuum, crossbreed, UGC species | GAME | epic_ecology.md | EC-1.* | North star | E | EC-0.* | Direct |
| PR-0.1–0.3 | Cel terrain, lighting, biome tint | GAME | epic_presentation.md | PR-0.* | M8 | E | GW-7.1 | Direct |
| PR-1.1 | Stylised foliage LOD | GAME | epic_presentation.md | PR-1.1 | Later | E | PR-0.* | Direct |
| PERF-P1 | Spatial index for `locate_region` (**Done**) | PERF | performance_big_o_audit.md | P1 | M0(Done) | E | atlas | Indirect |
| PERF-P5/P6 | Chunk-gen amortisation + packed marshal (**Done**) | PERF | performance_big_o_audit.md | P5/P6 | M0(Done) | E | streaming | Direct |
| PERF-cap | Points slider capped at icosphere subdiv 9 | PERF | git log; worldgen.md | d089c20 | M0 | E | — | Direct |
| R-INF-1 | Biome derivation should be data-driven from `biomes.xml` at sample time | OW/CLIM | epic_overworld_generation.md; biomes.xml | Goals/G4 | Later | **INFERRED** | OW-E3 | Indirect |
| R-INF-2 | One persistent `Main` scene; no `ChangeSceneToFile` | PLAT | game_scene_order.md | Boot sequence | M0 | **INFERRED** | — | Indirect |
| R-INF-3 | Only one visual mode drives `WorldRoot` at a time | STREAM/PREVIEW | moddability.md; game_scene_order.md | Sim vs visuals; 5/7 | M0 | **INFERRED** | GW-2.4/2.5 | Indirect |
| R-INF-4 | Godot targets 4.6 + C# + optional C++ GDExtension | PLAT | install.md | Quick start | M0 | **INFERRED** | — | None |

⚠ = involved in a documented conflict (see §4): OW-E4 milestone split; SP-0.x "time gate" vs IDEAS "decided against."

---

*End of audit. This document extracts and condenses intent from repository documents and data defs as of 2026-06-19; it asserts no implementation status beyond what those documents explicitly claim.*
