//! The `#[wasm_bindgen]` boundary the TypeScript SDK binds — `wasm32`-only.
//!
//! This is deliberately thin: the deterministic work lives in [`GameBridge`],
//! tested natively. Here we only expose a JS-constructable [`WasmGame`] that wraps
//! that bridge and a per-frame [`WasmGame::advance`] the host's
//! `requestAnimationFrame` loop calls with the elapsed nanoseconds it measured.
//! `advance` hands back the integer [`StepReport`] so the JS presentation layer
//! computes its own interpolation fraction (`remainder_nanos / fixed_step_nanos`)
//! — no wall-clock value crosses into a fixed tick.
//!
//! This boundary also owns the embed seam's **host channel** (SPEC-12): on
//! construction it decodes the inbound [`HostSessionConfig`] from
//! `window.location.search` (before tick 0), and [`WasmGame::report_outcome`]
//! drains the engine's single [`HostOutcome`] back out to the parent frame via
//! `window.parent.postMessage` exactly once (latched). The pure decode/latch
//! logic is in [`crate::embed`]; only the browser calls live here.

use wasm_bindgen::prelude::*;

use axiom::prelude::HostOutcome;
use axiom_windowing::WindowingApi;

use crate::embed::{decode_session_config, session_params_json};
use crate::{demo_app, GameBridge};

/// Read the inbound host query string (`window.location.search`). Returns an
/// empty string if there is no window/location, so the decode falls back to the
/// default session config (seed `0`, no params).
fn host_query() -> String {
    web_sys::window()
        .and_then(|window| window.location().search().ok())
        .unwrap_or_default()
}

/// Post a raw JSON `payload` to the parent frame (the embed host channel,
/// SPEC-12). Best-effort: if there is no parent window (top-level, not embedded)
/// the post is simply skipped.
fn post_to_parent(payload: &str) {
    let parent = web_sys::window().and_then(|window| window.parent().ok().flatten());
    if let Some(parent) = parent {
        let _ = parent.post_message(&JsValue::from_str(payload), "*");
    }
}

/// Forward `outcome` to the parent frame as a JSON `"complete"` message — the one
/// universal word every hosted game speaks.
fn post_outcome_to_parent(outcome: &HostOutcome) {
    let won = outcome.won();
    let score = outcome.score().get();
    post_to_parent(&format!(
        "{{\"type\":\"complete\",\"won\":{won},\"score\":{score}}}"
    ));
}

/// The integer step budget one `advance` produced, marshalled to JS. The SDK's
/// platform-edge bridge reads these and computes the `0..1` interpolation
/// fraction itself (float math is unconstrained at the presentation boundary).
#[wasm_bindgen]
#[derive(Debug)]
pub struct StepReport {
    steps: u32,
    remainder_nanos: u64,
    fixed_step_nanos: u64,
}

#[wasm_bindgen]
impl StepReport {
    /// How many fixed simulation steps `advance` ran this frame.
    #[wasm_bindgen(getter)]
    pub fn steps(&self) -> u32 {
        self.steps
    }

    /// Sub-step time left banked after this frame, in `[0, fixed_step_nanos)`.
    /// Crosses as an f64 `number` (never a BigInt i64) so the Binaryen `wasm2js`
    /// fallback — which legalizes i64 into i32 pairs and has no BigInt ABI — can
    /// run; a nanosecond budget fits in 2^53 losslessly.
    #[wasm_bindgen(getter)]
    pub fn remainder_nanos(&self) -> f64 {
        self.remainder_nanos as f64
    }

    /// The fixed step size, so the SDK can compute `remainder_nanos / fixed_step_nanos`.
    /// An f64 `number` for the same `wasm2js`-fallback reason as `remainder_nanos`.
    #[wasm_bindgen(getter)]
    pub fn fixed_step_nanos(&self) -> f64 {
        self.fixed_step_nanos as f64
    }
}

/// The JS-facing game object. Construct it with the fixed step (nanoseconds) and
/// the per-frame step ceiling, then call [`Self::advance`] once per host frame.
///
/// On construction it resolves the inbound [`HostSessionConfig`] from the host
/// query string (the embed seam's `getSessionConfig`, SPEC-12 §4.2); the seed is
/// fixed for the whole session and read via [`Self::seed`]. The single terminal
/// outcome is emitted once through [`Self::report_outcome`].
#[wasm_bindgen]
#[derive(Debug)]
pub struct WasmGame {
    pub(crate) bridge: GameBridge,
    /// The raw inbound host query string, kept so [`Self::session_params`] can
    /// re-project the opaque params map the game interprets.
    query: String,
    /// The live presenter (SPEC-11 3D / SPEC-04 2D). Idle until [`Self::bind_surface`]
    /// (3D) or [`Self::bind_2d_surface`] (2D) binds it to a canvas; the matching
    /// `render_scene` / `present_2d` then presents each frame through the engine's
    /// WebGPU → WebGL2 → Canvas 2D cascade. A game that binds neither pays nothing.
    windowing: WindowingApi,
    /// The CPU sprite/atlas textures a 2D game's `onRender` references, accumulated
    /// from [`Self::upload_2d_texture`] as the harness resolves them (fetch/decode in
    /// the app — the SPEC-04 "fetch in the app" rule). Handed to the windowing
    /// presenter each [`Self::present_2d`]; the engine, not the app, rasterizes them.
    textures_2d: Vec<(u64, u32, u32, Vec<u8>)>,
    /// The version of `textures_2d`, bumped on every upload so the presenter
    /// re-uploads the set to the live backend only when it changed (never per frame).
    textures_2d_generation: u32,
    /// The 3D mesh set, cached from the bridge so it is passed to the live presenter
    /// by reference every frame (mirroring how the 2D path passes `textures_2d`).
    /// The presenter gates the actual GPU re-upload on the generation, so passing it
    /// every frame is what makes a game's own meshes reach the backend once its async
    /// bind resolves — no runtime-side "already presented" flag that could race the
    /// bind. Refreshed from the bridge only when the mesh generation changed, so a
    /// steady stream of frames clones nothing.
    render_meshes: Vec<(u64, Vec<f32>, Vec<u32>)>,
    /// The generation of `render_meshes` (the bridge's mesh generation at the last
    /// refresh), handed to the presenter as the re-upload gate. `u32::MAX` so the
    /// first real generation refreshes the cache.
    render_meshes_generation: u32,
}

/// The opaque background a 2D frame clears to before its draws — the dark slate the
/// dev harness used (`#07090e`), now owned by the engine present path so both
/// backends of the cascade clear identically. Linear `0..1` per channel; both arms
/// write it as raw bytes (no gamma re-encode), so the on-screen colour is exactly
/// `(7, 9, 14)`.
const CLEAR_2D: [f32; 4] = [7.0 / 255.0, 9.0 / 255.0, 14.0 / 255.0, 1.0];

/// The canvas's intrinsic pixel dimensions (its `width`/`height` attributes), or
/// `None` if no element of that id is a canvas. The surface is sized to these so
/// the rendered aspect matches the on-screen canvas.
fn canvas_dimensions(canvas_id: &str) -> Option<(u32, u32)> {
    let canvas = web_sys::window()?
        .document()?
        .get_element_by_id(canvas_id)?
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .ok()?;
    Some((canvas.width(), canvas.height()))
}

#[wasm_bindgen]
impl WasmGame {
    /// Build the deterministic demo game and wrap it in the bridge core. Installs
    /// the panic hook so a Rust panic surfaces as a readable JS error, and decodes
    /// the inbound session config (seed + params) before tick 0 — the seed keys
    /// the bridge's RNG hub for the whole session.
    ///
    /// `fixed_step_nanos` crosses as an f64 `number` (not a BigInt i64) so the
    /// Binaryen `wasm2js` fallback — which legalizes i64 into i32 pairs and has no
    /// BigInt ABI — can run; a 60 Hz step (~16.6M ns) is far inside 2^53. It is
    /// converted back to the internal `u64` here.
    #[wasm_bindgen(constructor)]
    pub fn new(fixed_step_nanos: f64, max_steps: u32) -> WasmGame {
        console_error_panic_hook::set_once();
        let query = host_query();
        let config = decode_session_config(&query);
        WasmGame {
            bridge: GameBridge::new(
                demo_app().build(),
                config.seed(),
                fixed_step_nanos as u64,
                max_steps,
            ),
            query,
            windowing: WindowingApi::new(),
            textures_2d: Vec::new(),
            textures_2d_generation: 0,
            render_meshes: Vec::new(),
            render_meshes_generation: u32::MAX,
        }
    }

    /// The low 32 bits of the host-supplied session seed (the determinism input
    /// the bridge's `Rng` is seeded from), fixed before tick 0 and constant for the
    /// whole session. The 64-bit seed crosses as two u32 `number` halves
    /// (`seed_lo` + `seed_hi`) — never a single BigInt i64 — so the Binaryen
    /// `wasm2js` fallback (which legalizes i64 into i32 pairs and has no BigInt ABI)
    /// can run; the TS edge recombines them, preserving the full 2^64 seed space so
    /// determinism stays byte-identical.
    #[wasm_bindgen(getter)]
    pub fn seed_lo(&self) -> u32 {
        self.bridge.seed() as u32
    }

    /// The high 32 bits of the session seed — the companion of [`Self::seed_lo`].
    #[wasm_bindgen(getter)]
    pub fn seed_hi(&self) -> u32 {
        (self.bridge.seed() >> 32) as u32
    }

    /// The decoded opaque session params as a JSON object string `{"k":"v",…}`
    /// (`sessionParams`, SPEC-12 §6). The engine never interprets a param — the
    /// game's TS edge `JSON.parse`s this into its own shape. `seed` is excluded
    /// (read via [`Self::seed`]).
    #[wasm_bindgen(js_name = sessionParams)]
    pub fn session_params(&self) -> String {
        session_params_json(&self.query)
    }

    /// Tell the parent frame the game has booted and is ready to be shown
    /// (`notifyReady`, SPEC-12) — a JSON `"ready"` message on the host channel.
    #[wasm_bindgen(js_name = notifyReady)]
    pub fn notify_ready(&self) {
        post_to_parent("{\"type\":\"ready\"}");
    }

    /// Re-post the latched terminal outcome to the parent frame, if one has been
    /// reported (`reportOutcomes`) — a host-requested flush. Returns whether an
    /// outcome existed to forward. The single emit-once latch is unchanged; this
    /// only re-sends what [`Self::report_outcome`] already latched.
    #[wasm_bindgen(js_name = reportOutcomes)]
    pub fn report_outcomes(&self) -> bool {
        match self.bridge.reported_outcome() {
            Some(outcome) => {
                post_outcome_to_parent(outcome);
                true
            }
            None => false,
        }
    }

    /// Report the terminal outcome (`reportOutcome`, SPEC-12 §4.2). The first
    /// call latches and forwards exactly one [`HostOutcome`] to the parent frame;
    /// any later call is a no-op. Returns whether this call was the one accepted.
    pub fn report_outcome(&mut self, won: bool, score: f64) -> bool {
        let accepted = self.bridge.report_outcome(won, score);
        if accepted {
            if let Some(latched) = self.bridge.reported_outcome() {
                post_outcome_to_parent(latched);
            }
        }
        accepted
    }

    /// Bank `elapsed_nanos` of real host time, run the resulting whole fixed
    /// ticks, and report the integer budget for the SDK to interpolate with.
    ///
    /// `elapsed_nanos` crosses as an f64 `number` (not a BigInt i64) for the same
    /// `wasm2js`-fallback reason as the constructor's `fixed_step_nanos`; a
    /// per-frame delta is tiny, far inside 2^53. It is converted to the internal
    /// `u64` here.
    pub fn advance(&mut self, elapsed_nanos: f64) -> StepReport {
        let budget = self.bridge.advance(elapsed_nanos as u64);
        StepReport {
            steps: budget.steps(),
            remainder_nanos: budget.remainder_nanos(),
            fixed_step_nanos: budget.fixed_step_nanos(),
        }
    }

    /// The monotonic count of fixed ticks driven so far. Crosses as an f64
    /// `number` (not a BigInt i64) so the Binaryen `wasm2js` fallback — which
    /// legalizes i64 into i32 pairs and has no BigInt ABI — can run; a tick count
    /// is far inside 2^53.
    #[wasm_bindgen(getter)]
    pub fn current_tick(&self) -> f64 {
        self.bridge.tick() as f64
    }

    /// The durable simulation state as opaque bytes — the host stores or compares
    /// these to checkpoint or verify determinism.
    pub fn snapshot(&self) -> Vec<u8> {
        self.bridge.snapshot_sim()
    }

    /// Restore the durable simulation state from bytes a prior [`Self::snapshot`]
    /// produced, forking the live world to that recorded frame while the tick keeps
    /// advancing. The `@axiom/game` hot runtime uses this as the transactional
    /// checkpoint around a `soft_app_reload` migration (snapshot → migrate the live
    /// component bytes → on a migrator error, restore). Returns whether the bytes
    /// restored cleanly (a truncated / incompatible buffer is a deterministic `false`).
    pub fn restore(&mut self, bytes: Vec<u8>) -> bool {
        self.bridge.restore(&bytes)
    }

    // The seam a 3D game's boot path drives: bind a canvas once the scene is
    // authored, then present the authored scene each host frame. A 2D game (which
    // draws through the `draw2d` command stream the harness rasterizes) never calls
    // these.

    /// Bind the live 3D presenter to the `<canvas id=canvas_id>`: size the surface
    /// to the canvas's pixel dimensions, upload the authored scene's meshes and
    /// materials, and select the live backend (WebGPU → WebGL2 → Canvas 2D).
    /// `max_instances` caps the per-frame instance buffer. Call once **after** the
    /// author has built the 3D scene (so its meshes / materials exist). The backend
    /// init is asynchronous, so [`Self::render_scene`] is a no-op until it resolves
    /// (the first frames simply do not paint).
    #[wasm_bindgen(js_name = bindSurface)]
    pub fn bind_surface(&mut self, canvas_id: String, max_instances: u32) {
        let (width, height) = canvas_dimensions(&canvas_id).unwrap_or((960, 600));
        let _ = self.windowing.configure_surface(width, height);
        let meshes = self.bridge.mesh_set();
        let materials = self.bridge.material_set();
        self.windowing
            .bind_present_surface(&canvas_id, meshes, materials, max_instances);
    }

    /// Render the current 3D scene and present it to the bound surface
    /// (`renderScene`), reflecting every scene mutation the author made this frame.
    /// A no-op until [`Self::bind_surface`]'s backend init has resolved.
    #[wasm_bindgen(js_name = renderScene)]
    pub fn render_scene(&mut self) {
        // Hand the live presenter the current mesh set every frame (by reference),
        // versioned by the bridge's mesh generation; the presenter re-uploads to the
        // backend only when the generation changed. This is the peer of the 2D
        // texture-generation re-upload, and it is what lets a game's own meshes reach
        // the GPU: only the meshes present when the surface bound — the engine's demo
        // scene — are uploaded at bind, so without this a game's later meshes render
        // as "unknown mesh" and are skipped. The local cache is refreshed only when
        // the generation changed, so a steady stream of frames clones no geometry.
        let generation = self.bridge.mesh_generation();
        (generation != self.render_meshes_generation).then(|| {
            self.render_meshes = self.bridge.mesh_set();
            self.render_meshes_generation = generation;
        });
        self.windowing
            .update_present_meshes(&self.render_meshes, self.render_meshes_generation);
        let outcome = self.bridge.render_frame();
        let lights: Vec<(u32, [f32; 3], [f32; 3], f32)> = outcome
            .lights()
            .iter()
            .map(|light| (light.kind(), light.vec(), light.color(), light.intensity()))
            .collect();
        let batches = outcome.mesh_batches();
        let casters = outcome.mesh_batch_casters();
        self.windowing.present_frame(
            outcome.tick(),
            outcome.clear_color(),
            &lights,
            outcome.light_view_proj(),
            &batches,
            outcome.camera_view_proj(),
            &casters,
            outcome.sdf_scene().cloned(),
        );
    }

    // The seam a 2D game's boot path drives: bind a canvas, upload the sprite/atlas
    // textures the app fetched, then present the authored `draw2d` command list each
    // frame. The engine rasterizes through the SAME WebGPU → WebGL2 → Canvas 2D
    // cascade as 3D — no TypeScript Canvas2D interpreter. A 3D game never calls these.

    /// Bind the live presenter to the `<canvas id=canvas_id>` for **2D** presentation:
    /// size the surface to the canvas's pixel dimensions and select the live backend
    /// (WebGPU → WebGL2 → Canvas 2D). 2D needs no uploaded mesh/material set, so they
    /// bind empty (a `1`-instance buffer the 2D path never fills). Call once at boot;
    /// the backend init is asynchronous, so [`Self::present_2d`] is a no-op until it
    /// resolves (the first frames simply do not paint).
    #[wasm_bindgen(js_name = bind2dSurface)]
    pub fn bind_2d_surface(&mut self, canvas_id: String) {
        let (width, height) = canvas_dimensions(&canvas_id).unwrap_or((960, 540));
        let _ = self.windowing.configure_surface(width, height);
        self.windowing
            .bind_present_surface(&canvas_id, Vec::new(), Vec::new(), 1);
    }

    /// Upload one sprite/atlas texture the 2D draw stream references
    /// (`upload2dTexture`): `(id, width, height, RGBA8 pixels)`, resolved app-side
    /// (the harness fetch/decodes textures and bakes the font atlas). Replaces any
    /// texture already under `id` and bumps the set's version so the next
    /// [`Self::present_2d`] re-uploads it to the live backend exactly once.
    #[wasm_bindgen(js_name = upload2dTexture)]
    pub fn upload_2d_texture(&mut self, id: f64, width: u32, height: u32, pixels: Vec<u8>) {
        let id = id as u64;
        let entry = (id, width, height, pixels);
        match self.textures_2d.iter_mut().find(|(tid, ..)| *tid == id) {
            Some(slot) => *slot = entry,
            None => self.textures_2d.push(entry),
        }
        self.textures_2d_generation = self.textures_2d_generation.wrapping_add(1);
    }

    /// Present the authored 2D frame (`present2d`): drain the `draw2d` builder into
    /// its layer-sorted [`Draw2dList`](axiom_host::Draw2dList) and hand it — with the
    /// uploaded textures — to the windowing presenter, which rasterizes it through the
    /// live backend. Call once per host frame, **after** the author's `onRender`
    /// draws. A no-op until [`Self::bind_2d_surface`]'s backend init has resolved.
    #[wasm_bindgen(js_name = present2d)]
    pub fn present_2d(&mut self) {
        let list = self.bridge.draw2d_finish_list();
        // The frame identity the dev scrubber records under — the monotonic fixed
        // tick, the 2D peer of `render_scene`'s `outcome.tick()`. Read here (not
        // threaded through JS) so the harness `game.present2d()` call is unchanged.
        let tick = self.bridge.tick();
        self.windowing.present_2d(
            tick,
            &list,
            &self.textures_2d,
            self.textures_2d_generation,
            CLEAR_2D,
        );
    }

    /// Whether the loop should step the simulation this frame (`isInteractive`):
    /// `true` when live and focused, `false` while the frame-scrubber overlay is
    /// scrubbing or after focus loss (Escape / blur / tab hidden). The SDK loop
    /// gates its `advance` on this so the sim freezes exactly when the overlay says;
    /// [`Self::render_scene`] / [`Self::present_2d`] keep presenting (the frozen or
    /// scrubbed frame).
    #[wasm_bindgen(js_name = isInteractive)]
    pub fn is_interactive(&self) -> bool {
        self.windowing.is_interactive()
    }

    // The `NativeBridge` rng methods, marshalled to the bridge's seeded
    // [`crate::RngHub`]. The `js_name` is the camelCase identifier the TS
    // `bridgeFromWasm` adapter forwards verbatim (`game.rngUnit`, ...). Stream
    // ids are opaque JS numbers the hub owns; id `0` is the root.

    /// A uniform float in `[0, 1)` from `stream` (`Rng::unit`).
    #[wasm_bindgen(js_name = rngUnit)]
    pub fn rng_unit(&mut self, stream: u32) -> f64 {
        self.bridge.rng_unit(stream)
    }

    /// A uniform integer in `[0, max_exclusive)` from `stream` (`Rng::int`).
    #[wasm_bindgen(js_name = rngBelow)]
    pub fn rng_below(&mut self, stream: u32, max_exclusive: u32) -> u32 {
        self.bridge.rng_below(stream, u64::from(max_exclusive)) as u32
    }

    /// The index `weights` selects, drawn proportionally to the weights, from
    /// `stream` (`Rng::weighted`). JS weights are plain numbers; each is floored
    /// to a non-negative integer weight (the exact, cross-machine form the
    /// entropy facade selects over).
    #[wasm_bindgen(js_name = rngWeighted)]
    pub fn rng_weighted(&mut self, stream: u32, weights: &[f64]) -> u32 {
        let weights: Vec<u64> = weights.iter().map(|&w| w.max(0.0) as u64).collect();
        self.bridge.rng_weighted(stream, &weights)
    }

    /// A Fisher-Yates permutation of `[0, length)` the core drew from `stream`
    /// (`Rng::permutation`). Returned as a real JS `number[]` (not a typed array)
    /// so it matches the contract's `readonly number[]` and the projection can map
    /// the author's array through it.
    #[wasm_bindgen(js_name = rngPermutation)]
    pub fn rng_permutation(&mut self, stream: u32, length: u32) -> Vec<JsValue> {
        self.bridge
            .rng_permutation(stream, length)
            .into_iter()
            .map(|index| JsValue::from_f64(f64::from(index)))
            .collect()
    }

    /// Resolve the deterministic id of the named sub-stream of `parent`
    /// (`Rng::stream`). Idempotent: the same `(parent, name)` resolves to the same
    /// id.
    #[wasm_bindgen(js_name = rngStream)]
    pub fn rng_stream(&mut self, parent: u32, name: String) -> u32 {
        self.bridge.rng_stream(parent, &name)
    }

    // The `NativeBridge` world methods, marshalled to the bridge's retained world
    // over the app's dynamic component store. Entity handles cross as JS `number`s
    // (f64) so they match the contract's `Entity = number`; a component crosses as
    // a `(kind: string, fields: Uint8Array)` pair per the convention in
    // [`crate::world`]. `worldSpawn` is composed at the TS edge from
    // `worldSpawn`(empty) + a `worldSet` per component, so the boundary stays
    // scalar / byte / string only.

    /// Spawn a bare entity, returning its id as a JS number (`worldSpawn`'s root).
    #[wasm_bindgen(js_name = worldSpawn)]
    pub fn world_spawn(&mut self) -> f64 {
        self.bridge.world_spawn() as f64
    }

    /// Despawn one entity (`worldDespawn`); a stale handle is a clean no-op.
    #[wasm_bindgen(js_name = worldDespawn)]
    pub fn world_despawn(&mut self, entity: f64) {
        self.bridge.world_despawn(entity as u64);
    }

    /// Despawn an entity and its whole subtree (`worldDespawnSubtree`).
    #[wasm_bindgen(js_name = worldDespawnSubtree)]
    pub fn world_despawn_subtree(&mut self, entity: f64) {
        self.bridge.world_despawn_subtree(entity as u64);
    }

    /// Set (or replace) `entity`'s component of `kind` from its field `bytes`
    /// (`worldSet`). An unknown kind / stale entity / bad bytes is a clean no-op.
    #[wasm_bindgen(js_name = worldSet)]
    pub fn world_set(&mut self, entity: f64, kind: String, bytes: &[u8]) {
        self.bridge.world_set(entity as u64, &kind, bytes);
    }

    /// Read `entity`'s component of `kind` as field bytes (`worldGet`) — an empty
    /// buffer on a miss / dead entity / unknown kind (the TS edge maps `[]` →
    /// the empty `Result`).
    #[wasm_bindgen(js_name = worldGet)]
    pub fn world_get(&self, entity: f64, kind: String) -> Vec<u8> {
        self.bridge.world_get(entity as u64, &kind)
    }

    /// Every entity carrying *all* the named `kinds`, in ascending-id order
    /// (`worldQuery`). Returned as a real JS `number[]`.
    #[wasm_bindgen(js_name = worldQuery)]
    pub fn world_query(&self, kinds: Vec<String>) -> Vec<JsValue> {
        let refs: Vec<&str> = kinds.iter().map(String::as_str).collect();
        self.bridge
            .world_query(&refs)
            .into_iter()
            .map(|id| JsValue::from_f64(id as f64))
            .collect()
    }

    /// The direct children of `entity`, in ascending-id order (`worldChildrenOf`),
    /// as a real JS `number[]`.
    #[wasm_bindgen(js_name = worldChildrenOf)]
    pub fn world_children_of(&self, entity: f64) -> Vec<JsValue> {
        self.bridge
            .world_children_of(entity as u64)
            .into_iter()
            .map(|id| JsValue::from_f64(id as f64))
            .collect()
    }

    /// Whether `entity` names a live node (`worldAlive`).
    #[wasm_bindgen(js_name = worldAlive)]
    pub fn world_alive(&self, entity: f64) -> bool {
        self.bridge.world_alive(entity as u64)
    }

    /// Whether `entity` carries a component of `kind` (`worldHas`).
    #[wasm_bindgen(js_name = worldHas)]
    pub fn world_has(&self, entity: f64, kind: String) -> bool {
        self.bridge.world_has(entity as u64, &kind)
    }

    /// Remove `entity`'s component of `kind` (`worldRemove`).
    #[wasm_bindgen(js_name = worldRemove)]
    pub fn world_remove(&mut self, entity: f64, kind: String) {
        self.bridge.world_remove(entity as u64, &kind);
    }

    /// Re-parent `child` under `parent` (`worldSetParent`); a rejected link is a
    /// clean no-op. A `null`/`undefined` parent (`None`) detaches `child` to the
    /// hierarchy root (SPEC-02 §4.2: "null detaches to the root").
    #[wasm_bindgen(js_name = worldSetParent)]
    pub fn world_set_parent(&mut self, child: f64, parent: Option<f64>) {
        self.bridge
            .world_set_parent(child as u64, parent.map(|parent| parent as u64));
    }

    /// `entity`'s parent as `[]` / `[parent]` (`worldParentOf`).
    #[wasm_bindgen(js_name = worldParentOf)]
    pub fn world_parent_of(&self, entity: f64) -> Vec<f64> {
        self.bridge.world_parent_of(entity as u64)
    }

    /// `entity`'s authoritative world transform (`worldWorldTransform`) as `[]`
    /// or the flat 10-tuple `[tx, ty, tz, qx, qy, qz, qw, sx, sy, sz]`.
    #[wasm_bindgen(js_name = worldWorldTransform)]
    pub fn world_world_transform(&self, entity: f64) -> Vec<f64> {
        self.bridge.world_world_transform(entity as u64)
    }
}

/// Page entry: install the panic hook. The page then constructs a [`WasmGame`]
/// and drives it from its own `requestAnimationFrame` loop (in the TS SDK).
#[wasm_bindgen]
pub fn start() {
    console_error_panic_hook::set_once();
}
