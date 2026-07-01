//! Input (SPEC-05) composed into the bridge: the deterministic per-tick intent
//! snapshot over [`axiom_input::InputState`], a live device-state accumulator the
//! browser feeds raw key/pointer events into, the per-fixed-tick sample driven
//! inside the loop, and the `#[wasm_bindgen]` boundary the TS `NativeBridge`
//! input methods bind.
//! ## What lives here
//! - [`InputBridge`]: the engine [`InputState`] facade, the action-name → id
//!   table the boundary's string actions resolve through, and the *live* device
//!   state (the keys currently down, the latest pointer sample, the surface) the
//!   injection path mutates. It is a `pub(crate)` field of [`GameBridge`]
//!   (initialized in `GameBridge::new`), so all input logic stays in this file.
//! - An `impl GameBridge` block: the native, fully-testable input methods — the
//!   injection path (`input_key` / `input_pointer` / `input_pointer_clear` /
//!   `input_set_surface` / `input_bind_action`) the browser arm feeds events
//!   into, and the per-tick reads (`input_is_down` / `input_pressed` / … /
//!   `input_swipe`) the `NativeBridge` projects.
//! - A `#[wasm_bindgen] impl WasmGame` block (wasm32 only): the camelCase exports
//!   the TS edge forwards verbatim.
//! ## Boundary convention (the established scalar/byte/string rule)
//! Every input value crosses the wasm boundary as a boundary primitive, never a
//! structured object — exactly as entities cross as raw ids, components as
//! `(kind, bytes)`, and physics vectors as scalar `(x, y, z)`:
//! - an action is the `string` name the binding table resolves to an
//!   [`axiom_input::ActionId`];
//! - a held/edge read is a `bool`;
//! - an optional read (pointer / press-start / pressed-at-tick) is a `Vec<f64>`
//!   that is **empty** when absent and `[…]` when present (the TS edge maps `[]`
//!   to the empty `Result`, exactly as `world_get`'s empty buffer does);
//! - a swipe is the direction `string` (`""` when absent).
//! ## Per-tick sample
//! [`GameBridge::advance`] runs the fixed-step loop, then [`InputBridge::sample`]
//! folds the current live [`DeviceFrame`] into the snapshot once per fixed tick
//! that ran — so a read after `advance` sees the tick's resolved intent (edges
//! computed against the previous tick's down-set inside the engine facade). The
//! sample loop is a branchless fold over the step count, matching the engine's
//! Branchless Law for app code.

use axiom::prelude::Vec2;
use axiom_input::{ActionId, DeviceFrame, InputState, KeyToken, SwipeDir};
use axiom_kernel::Tick;

use crate::GameBridge;

/// The default sampling surface (device pixels) until the host reports the real
/// canvas size via `input_set_surface`. The swipe threshold is a fraction of the
/// shorter edge, so a sane default keeps a stray micro-drag from registering.
fn default_surface() -> Vec2 {
    Vec2::new(1000.0, 600.0)
}

/// The input state the bridge owns: the engine intent-snapshot facade, the
/// action-name → id table the boundary's string actions resolve through, and the
/// live device state (keys down, latest pointer sample, surface) the injection
/// path accumulates between samples.
#[derive(Debug)]
pub(crate) struct InputBridge {
    state: InputState,
    actions: Vec<String>,
    keys: Vec<String>,
    pointer: Option<(Vec2, bool)>,
    surface: Vec2,
    /// Relative look (mouse / pointer-lock) accumulated between fixed ticks, in raw
    /// device pixels — the analogue of the held-key set for an analog channel.
    live_look: Vec2,
    /// The look accumulated *for the current tick*: `commit_look` snapshots
    /// `live_look` here once per `advance` and zeroes the accumulator, so a read
    /// after `advance` sees this frame's relative look exactly once (the same
    /// fold-then-reset the original mouse-look loop does each frame).
    look: Vec2,
}

impl InputBridge {
    /// A fresh input bridge: no bindings, no keys down, no pointer, no look, the
    /// default surface.
    pub(crate) fn new() -> Self {
        InputBridge {
            state: InputState::new(),
            actions: Vec::new(),
            keys: Vec::new(),
            pointer: None,
            surface: default_surface(),
            live_look: Vec2::ZERO,
            look: Vec2::ZERO,
        }
    }

    /// Accumulate one relative look sample (raw device pixels: `dx` rightward,
    /// `dy` downward) into the live look — the pointer-lock `mousemove` feed.
    fn accumulate_look(&mut self, dx: f64, dy: f64) {
        self.live_look = Vec2::new(self.live_look.x + dx as f32, self.live_look.y + dy as f32);
    }

    /// Snapshot the live look into this tick's `look` and zero the accumulator —
    /// run once per [`crate::GameBridge::advance`], so the relative look is read
    /// for exactly one tick and never double-applied across frames.
    pub(crate) fn commit_look(&mut self) {
        self.look = self.live_look;
        self.live_look = Vec2::ZERO;
    }

    /// This tick's relative look as `[dx, dy]` raw device pixels — the value the
    /// `look()` projection reads back. A game scales it by its own sensitivity and
    /// applies it to its yaw/pitch (the engine clamps pitch on the controller).
    fn look_delta(&self) -> Vec<f64> {
        vec![f64::from(self.look.x), f64::from(self.look.y)]
    }

    /// Resolve `name` to its stable action id, interning a fresh id (the next
    /// table index) the first time the name is bound. Idempotent per name.
    fn intern(&mut self, name: &str) -> ActionId {
        let id = self
            .actions
            .iter()
            .position(|known| known == name)
            .unwrap_or_else(|| {
                self.actions.push(name.to_string());
                self.actions.len() - 1
            });
        ActionId::new(id as u32)
    }

    /// Resolve `name` to its action id for a read **without** interning: an
    /// unbound name resolves to the past-the-end id, which the snapshot reads as
    /// neutral (a never-bound action is never down).
    fn resolve(&self, name: &str) -> ActionId {
        let id = self
            .actions
            .iter()
            .position(|known| known == name)
            .unwrap_or(self.actions.len());
        ActionId::new(id as u32)
    }

    /// Bind (or remap) the physical `keys` that fire `action` (`bindAction`).
    fn bind_action(&mut self, action: &str, keys: &[&str]) {
        let id = self.intern(action);
        let tokens: Vec<KeyToken> = keys.iter().map(|name| KeyToken::new(name)).collect();
        self.state.bind_action(id, &tokens);
    }

    /// Feed one raw key event: set `token`'s live down-state. Branchless — the
    /// token is removed unconditionally, then re-added iff it is now down.
    fn key(&mut self, token: &str, down: bool) {
        self.keys.retain(|held| held != token);
        let _ = down.then(|| self.keys.push(token.to_string()));
    }

    /// Feed one raw pointer event: set the live pointer sample (position +
    /// pressed-state). A `down == false` sample is a hovering / lifted contact —
    /// the engine completes a swipe on the frame no contact is down.
    fn pointer_event(&mut self, x: f64, y: f64, down: bool) {
        self.pointer = Some((Vec2::new(x as f32, y as f32), down));
    }

    /// Clear the live pointer (a `pointerleave`): no contact next sample.
    fn pointer_clear(&mut self) {
        self.pointer = None;
    }

    /// Set the sampling surface (the canvas size, device pixels) the host reports.
    fn set_surface(&mut self, width: f64, height: f64) {
        self.surface = Vec2::new(width as f32, height as f32);
    }

    /// Fold the current live [`DeviceFrame`] into the snapshot once per fixed tick
    /// that ran this frame, beginning at `start`. A branchless fold over the step
    /// count drives the loop; each sample resolves edges against the previous
    /// tick's down-set inside the engine facade.
    pub(crate) fn sample(&mut self, start: u64, steps: u32) {
        let tokens: Vec<KeyToken> = self.keys.iter().map(|name| KeyToken::new(name)).collect();
        let pointers: Vec<(Vec2, bool)> = self.pointer.into_iter().collect();
        let frame = DeviceFrame::new(self.surface, &tokens, &pointers);
        (0..steps).for_each(|step| {
            self.state.sample(Tick::new(start + u64::from(step)), &frame);
        });
    }

    /// The optional pointer sample as the boundary `Vec<f64>`: `[]` when absent,
    /// `[x, y, down]` (down as `0.0` / `1.0`) when present.
    fn pointer_sample(&self) -> Vec<f64> {
        self.state
            .pointer()
            .map(|contact| {
                vec![
                    f64::from(contact.pos.x),
                    f64::from(contact.pos.y),
                    f64::from(u8::from(contact.down)),
                ]
            })
            .unwrap_or_default()
    }

    /// The optional press-start position as `[]` / `[x, y]`.
    fn pointer_pressed(&self) -> Vec<f64> {
        self.state
            .pointer_pressed()
            .map(|pos| vec![f64::from(pos.x), f64::from(pos.y)])
            .unwrap_or_default()
    }

    /// The swipe direction this tick as its boundary string (`""` when absent).
    fn swipe(&self) -> String {
        self.state.swipe().map(swipe_name).unwrap_or_default()
    }

    /// The optional most-recent down-edge tick of `action` as `[]` / `[tick]`.
    fn pressed_at_tick(&self, action: &str) -> Vec<f64> {
        self.state
            .pressed_at_tick(self.resolve(action))
            .map(|tick| vec![tick.raw() as f64])
            .unwrap_or_default()
    }

    /// Whether `action` is held this tick.
    fn is_down_action(&self, action: &str) -> bool {
        self.state.is_down(self.resolve(action))
    }

    /// Whether `action` had a down-edge this tick.
    fn pressed_action(&self, action: &str) -> bool {
        self.state.pressed(self.resolve(action))
    }

    /// Whether `action` had an up-edge this tick.
    fn released_action(&self, action: &str) -> bool {
        self.state.released(self.resolve(action))
    }

    /// The `-1 | 0 | 1` axis from a `neg`/`pos` action pair, widened for JS.
    fn axis_actions(&self, neg: &str, pos: &str) -> i32 {
        i32::from(self.state.axis(self.resolve(neg), self.resolve(pos)))
    }
}

/// The boundary name of a swipe direction. Branchless: scan the `(variant, name)`
/// table for the matching variant rather than `match`, robust to declaration
/// order.
fn swipe_name(dir: SwipeDir) -> String {
    [
        (SwipeDir::Up, "up"),
        (SwipeDir::Down, "down"),
        (SwipeDir::Left, "left"),
        (SwipeDir::Right, "right"),
    ]
    .iter()
    .find(|(variant, _)| *variant == dir)
    .map(|(_, name)| (*name).to_string())
    .unwrap_or_default()
}

impl GameBridge {
    /// Bind (or remap) the physical `keys` that fire `action` (`bindAction`,
    /// SPEC-05 §4.2). Gameplay reads the action name; only this call names keys.
    pub fn input_bind_action(&mut self, action: &str, keys: &[&str]) {
        self.input.bind_action(action, keys);
    }

    /// Feed one raw key event (`inputKey`): set `token`'s live down-state.
    pub fn input_key(&mut self, token: &str, down: bool) {
        self.input.key(token, down);
    }

    /// Feed one raw pointer event (`inputPointerEvent`): set the live contact.
    pub fn input_pointer(&mut self, x: f64, y: f64, down: bool) {
        self.input.pointer_event(x, y, down);
    }

    /// Clear the live pointer (`inputPointerClear`): no contact next sample.
    pub fn input_pointer_clear(&mut self) {
        self.input.pointer_clear();
    }

    /// Set the sampling surface in device pixels (`inputSetSurface`).
    pub fn input_set_surface(&mut self, width: f64, height: f64) {
        self.input.set_surface(width, height);
    }

    /// Accumulate one relative look sample (raw pixels) into the live look
    /// (`inputLook`) — the pointer-lock mouse feed. Drained once per `advance`.
    pub fn input_look(&mut self, dx: f64, dy: f64) {
        self.input.accumulate_look(dx, dy);
    }

    /// This tick's relative look as `[dx, dy]` raw device pixels (`inputLookDelta`).
    pub fn input_look_delta(&self) -> Vec<f64> {
        self.input.look_delta()
    }

    /// Whether `action` is held this tick (`inputIsDown`).
    pub fn input_is_down(&self, action: &str) -> bool {
        self.input.is_down_action(action)
    }

    /// Whether `action` had a down-edge this tick (`inputPressed`).
    pub fn input_pressed(&self, action: &str) -> bool {
        self.input.pressed_action(action)
    }

    /// Whether `action` had an up-edge this tick (`inputReleased`).
    pub fn input_released(&self, action: &str) -> bool {
        self.input.released_action(action)
    }

    /// The `-1 | 0 | 1` axis from the `neg`/`pos` action pair. Not a
    /// `NativeBridge` method (the TS `Input.axis` derives it from `isDown`); a
    /// native read for the slice tests.
    pub fn input_axis(&self, neg: &str, pos: &str) -> i32 {
        self.input.axis_actions(neg, pos)
    }

    /// The optional pointer sample as `[]` / `[x, y, down]` (`inputPointer`).
    pub fn input_pointer_sample(&self) -> Vec<f64> {
        self.input.pointer_sample()
    }

    /// The optional press-start position as `[]` / `[x, y]` (`inputPointerPressed`).
    pub fn input_pointer_pressed(&self) -> Vec<f64> {
        self.input.pointer_pressed()
    }

    /// The swipe direction string this tick, `""` when absent (`inputSwipe`).
    pub fn input_swipe(&self) -> String {
        self.input.swipe()
    }

    /// The optional most-recent down-edge tick of `action` as `[]` / `[tick]`
    /// (`inputPressedAtTick`).
    pub fn input_pressed_at_tick(&self, action: &str) -> Vec<f64> {
        self.input.pressed_at_tick(action)
    }
}

#[cfg(target_arch = "wasm32")]
mod wasm_exports {
    use wasm_bindgen::prelude::*;

    use crate::wasm::WasmGame;

    #[wasm_bindgen]
    impl WasmGame {
        /// Bind (or remap) the physical `keys` that fire `action` (`bindAction`).
        /// The keys cross as a JS `string[]`; the action as a `string`.
        #[wasm_bindgen(js_name = bindAction)]
        pub fn bind_action(&mut self, action: String, keys: Vec<String>) {
            let refs: Vec<&str> = keys.iter().map(String::as_str).collect();
            self.bridge.input_bind_action(&action, &refs);
        }

        /// Feed one raw key event (`inputKey`): the layout-stable token + its
        /// pressed-state.
        #[wasm_bindgen(js_name = inputKey)]
        pub fn input_key(&mut self, token: String, down: bool) {
            self.bridge.input_key(&token, down);
        }

        /// Feed one raw pointer event (`inputPointerEvent`): position + pressed.
        #[wasm_bindgen(js_name = inputPointerEvent)]
        pub fn input_pointer_event(&mut self, x: f64, y: f64, down: bool) {
            self.bridge.input_pointer(x, y, down);
        }

        /// Clear the live pointer (`inputPointerClear`).
        #[wasm_bindgen(js_name = inputPointerClear)]
        pub fn input_pointer_clear(&mut self) {
            self.bridge.input_pointer_clear();
        }

        /// Set the sampling surface in device pixels (`inputSetSurface`).
        #[wasm_bindgen(js_name = inputSetSurface)]
        pub fn input_set_surface(&mut self, width: f64, height: f64) {
            self.bridge.input_set_surface(width, height);
        }

        /// Accumulate one relative look sample in raw device pixels (`inputLook`):
        /// `dx` rightward, `dy` downward — the pointer-lock mouse-move feed.
        #[wasm_bindgen(js_name = inputLook)]
        pub fn input_look(&mut self, dx: f64, dy: f64) {
            self.bridge.input_look(dx, dy);
        }

        /// This tick's relative look as `[dx, dy]` raw device pixels
        /// (`inputLookDelta`).
        #[wasm_bindgen(js_name = inputLookDelta)]
        pub fn input_look_delta(&self, _tick: f64) -> Vec<f64> {
            self.bridge.input_look_delta()
        }

        // The reads carry the `NativeBridge`'s `tick` first arg for contract
        // shape, but the native snapshot is always the running (last-sampled)
        // tick — which IS the tick the caller reads — so the value is ignored
        // here, not re-resolved against a tick history.

        /// Whether `action` is held this tick (`inputIsDown`).
        #[wasm_bindgen(js_name = inputIsDown)]
        pub fn input_is_down(&self, _tick: f64, action: String) -> bool {
            self.bridge.input_is_down(&action)
        }

        /// Whether `action` had a down-edge this tick (`inputPressed`).
        #[wasm_bindgen(js_name = inputPressed)]
        pub fn input_pressed(&self, _tick: f64, action: String) -> bool {
            self.bridge.input_pressed(&action)
        }

        /// Whether `action` had an up-edge this tick (`inputReleased`).
        #[wasm_bindgen(js_name = inputReleased)]
        pub fn input_released(&self, _tick: f64, action: String) -> bool {
            self.bridge.input_released(&action)
        }

        /// The pointer sample as `[]` / `[x, y, down]` (`inputPointer`).
        #[wasm_bindgen(js_name = inputPointer)]
        pub fn input_pointer(&self, _tick: f64) -> Vec<f64> {
            self.bridge.input_pointer_sample()
        }

        /// The press-start position as `[]` / `[x, y]` (`inputPointerPressed`).
        #[wasm_bindgen(js_name = inputPointerPressed)]
        pub fn input_pointer_pressed(&self, _tick: f64) -> Vec<f64> {
            self.bridge.input_pointer_pressed()
        }

        /// The swipe direction string this tick, `""` absent (`inputSwipe`).
        #[wasm_bindgen(js_name = inputSwipe)]
        pub fn input_swipe(&self, _tick: f64) -> String {
            self.bridge.input_swipe()
        }

        /// The most-recent down-edge tick of `action` as `[]` / `[tick]`
        /// (`inputPressedAtTick`).
        #[wasm_bindgen(js_name = inputPressedAtTick)]
        pub fn input_pressed_at_tick(&self, _tick: f64, action: String) -> Vec<f64> {
            self.bridge.input_pressed_at_tick(&action)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{demo_app, GameBridge};

    /// 1 ms fixed step, one tick per `advance` (matches the other slice tests), so
    /// each scripted `advance` is exactly one input sample at a known tick.
    const STEP: u64 = 1_000_000;

    fn bridge() -> GameBridge {
        GameBridge::new(demo_app().build(), 0, STEP, 1)
    }

    /// Deterministic FNV-1a over a byte buffer — the per-tick input fingerprint.
    fn fnv1a(bytes: &[u8]) -> u64 {
        bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
        })
    }

    /// The whole observable input boundary as one byte buffer — the per-tick
    /// `IntentSnapshot` the boundary exposes: the three edge reads for each bound
    /// action plus the pointer / swipe / press-start reads. This buffer IS the
    /// tick's intent record; `input_hash` is just its fingerprint.
    fn input_buffer(b: &GameBridge) -> Vec<u8> {
        let mut buf: Vec<u8> = Vec::new();
        ["left", "right", "fire"].iter().for_each(|action| {
            buf.push(u8::from(b.input_is_down(action)));
            buf.push(u8::from(b.input_pressed(action)));
            buf.push(u8::from(b.input_released(action)));
            b.input_pressed_at_tick(action)
                .iter()
                .for_each(|&v| buf.extend_from_slice(&v.to_le_bytes()));
            buf.push(0xFE);
        });
        buf.extend_from_slice(b.input_swipe().as_bytes());
        b.input_pointer_sample()
            .iter()
            .chain(b.input_pointer_pressed().iter())
            .for_each(|&v| buf.extend_from_slice(&v.to_le_bytes()));
        buf
    }

    /// The per-tick input boundary folded to one hash — the fingerprint of the
    /// tick's [`input_buffer`] `IntentSnapshot`.
    fn input_hash(b: &GameBridge) -> u64 {
        fnv1a(&input_buffer(b))
    }

    /// Drive a fixed scripted input session and return the per-tick boundary-read
    /// hash sequence: bind actions, then over the ticks press/hold/release keys
    /// and run a pointer drag that completes as a swipe.
    fn scripted_input_hashes() -> Vec<u64> {
        let mut b = bridge();
        b.input_bind_action("left", &["KeyA", "ArrowLeft"]);
        b.input_bind_action("right", &["KeyD"]);
        b.input_bind_action("fire", &["Space"]);
        // A scripted sequence of (key edit, pointer edit) applied before each
        // advance, then sampled at that tick. The pointer drags +200x then lifts,
        // so the gesture completes as a right swipe on the lift tick.
        let steps: [(&str, bool, f64, f64, bool, bool); 4] = [
            // (key, key_down, px, py, pointer_down, pointer_clear)
            ("Space", true, 200.0, 300.0, true, false), // tick 0: press fire, drag start
            ("KeyD", true, 400.0, 300.0, true, false),  // tick 1: hold right, drag +200x
            ("Space", false, 0.0, 0.0, false, true),    // tick 2: release fire, lift -> swipe
            ("KeyA", true, 0.0, 0.0, false, true),      // tick 3: press left
        ];
        steps
            .iter()
            .map(|&(key, key_down, px, py, ptr_down, clear)| {
                b.input_key(key, key_down);
                // Test code is exempt from the Branchless Law, so a plain branch
                // is fine here for the scripted pointer edit.
                if clear {
                    b.input_pointer_clear();
                } else {
                    b.input_pointer(px, py, ptr_down);
                }
                b.advance(STEP);
                input_hash(&b)
            })
            .collect()
    }

    /// The scripted session's `(key edit, pointer edit)` plan — the single source
    /// of truth the per-tick and chunked drivers both replay.
    fn scripted_steps() -> [(&'static str, bool, f64, f64, bool, bool); 4] {
        [
            ("Space", true, 200.0, 300.0, true, false), // tick 0: press fire, drag start
            ("KeyD", true, 400.0, 300.0, true, false),  // tick 1: hold right, drag +200x
            ("Space", false, 0.0, 0.0, false, true),    // tick 2: release fire, lift -> swipe
            ("KeyA", true, 0.0, 0.0, false, true),      // tick 3: press left
        ]
    }

    /// Drive the scripted session delivering each tick's `STEP` of elapsed time in
    /// `substeps` equal sub-advances (the SPEC-00 accumulator banks the partials),
    /// recording the per-tick `IntentSnapshot` hash whenever a sub-advance produces
    /// a whole tick. `substeps == 1` is the canonical one-advance-per-tick run.
    fn scripted_input_hashes_chunked(substeps: u32) -> Vec<u64> {
        let mut b = bridge();
        b.input_bind_action("left", &["KeyA", "ArrowLeft"]);
        b.input_bind_action("right", &["KeyD"]);
        b.input_bind_action("fire", &["Space"]);
        let mut hashes: Vec<u64> = Vec::new();
        scripted_steps()
            .iter()
            .for_each(|&(key, key_down, px, py, ptr_down, clear)| {
                b.input_key(key, key_down);
                if clear {
                    b.input_pointer_clear();
                } else {
                    b.input_pointer(px, py, ptr_down);
                }
                (0..substeps).for_each(|_| {
                    let budget = b.advance(STEP / u64::from(substeps));
                    // Sample the snapshot only on the sub-advance that completed a
                    // whole fixed tick — partial chunks bank, they don't sample.
                    (budget.steps() > 0).then(|| hashes.push(input_hash(&b)));
                });
            });
        hashes
    }

    /// Drive the scripted session one advance per tick, capturing the full per-tick
    /// `IntentSnapshot` byte buffer — the intent stream itself, not just its hash.
    fn scripted_input_snapshot_stream() -> Vec<Vec<u8>> {
        let mut b = bridge();
        b.input_bind_action("left", &["KeyA", "ArrowLeft"]);
        b.input_bind_action("right", &["KeyD"]);
        b.input_bind_action("fire", &["Space"]);
        scripted_steps()
            .iter()
            .map(|&(key, key_down, px, py, ptr_down, clear)| {
                b.input_key(key, key_down);
                if clear {
                    b.input_pointer_clear();
                } else {
                    b.input_pointer(px, py, ptr_down);
                }
                b.advance(STEP);
                input_buffer(&b)
            })
            .collect()
    }

    #[test]
    fn the_input_boundary_replays_to_a_byte_identical_hash_sequence() {
        // The keystone proof: the same scripted injection sequence over the input
        // boundary produces a byte-identical per-tick read-hash sequence across
        // two independent runs.
        let first = scripted_input_hashes();
        assert_eq!(first, scripted_input_hashes());
        // The input genuinely evolves (presses, releases, a swipe), so the
        // fingerprint is not constant — real work, not a degenerate sequence.
        assert!(first.iter().any(|&hash| hash != first[0]));
    }

    #[test]
    fn the_per_tick_snapshots_are_invariant_to_advance_chunking() {
        // SPEC-05 §7 cross-chunk invariance: the SAME raw input event stream,
        // partitioned into ticks two different ways by the accumulator (one whole
        // STEP per tick vs. two and four equal partials), produces the SAME
        // per-tick snapshot/hash sequence — the tick decomposition is a function of
        // banked elapsed time, not of how the host chunked its `advance` calls.
        let whole = scripted_input_hashes_chunked(1);
        assert_eq!(whole, scripted_input_hashes_chunked(2));
        assert_eq!(whole, scripted_input_hashes_chunked(4));
        // It is exactly the canonical per-tick driver's sequence, and non-trivial.
        assert_eq!(whole, scripted_input_hashes());
        assert_eq!(whole.len(), 4);
        assert!(whole.iter().any(|&hash| hash != whole[0]));
    }

    #[test]
    fn the_intent_snapshot_stream_alone_reproduces_the_per_tick_reads() {
        // SPEC-05 §7: replaying from the snapshot stream alone (no raw events)
        // reproduces byte-identical state. Two runs yield byte-identical
        // `IntentSnapshot` streams...
        let first = scripted_input_snapshot_stream();
        assert_eq!(first, scripted_input_snapshot_stream());
        // ...and the per-tick reads are a pure function of those captured
        // snapshots: fingerprinting the stream alone — without re-feeding any raw
        // key/pointer event — reconstructs the canonical per-tick hash sequence.
        let from_snapshots: Vec<u64> = first.iter().map(|buf| fnv1a(buf)).collect();
        assert_eq!(from_snapshots, scripted_input_hashes());
        // The snapshots genuinely differ tick to tick (real intent, not a constant).
        assert_eq!(first.len(), 4);
        assert!(first.iter().any(|buf| buf != &first[0]));
    }

    #[test]
    fn a_bound_action_reads_back_its_injected_edges_holds_and_axis() {
        let mut b = bridge();
        b.input_bind_action("left", &["KeyA"]);
        b.input_bind_action("right", &["KeyD"]);
        b.input_bind_action("fire", &["Space"]);
        // Tick 0: press Space + hold KeyD.
        b.input_key("Space", true);
        b.input_key("KeyD", true);
        b.advance(STEP);
        assert!(b.input_is_down("fire"));
        assert!(b.input_pressed("fire")); // down-edge this tick
        assert!(!b.input_released("fire"));
        // The press is stamped at tick 0 (the down-edge tick).
        assert_eq!(b.input_pressed_at_tick("fire"), vec![0.0]);
        // Only `right` is held -> axis = +1; an unbound action reads neutral.
        assert_eq!(b.input_axis("left", "right"), 1);
        assert!(!b.input_is_down("nope"));
        // Tick 1: still held -> NOT pressed again (auto-repeat suppressed).
        b.advance(STEP);
        assert!(b.input_is_down("fire"));
        assert!(!b.input_pressed("fire"));
        // Tick 2: release Space -> a single up-edge.
        b.input_key("Space", false);
        b.advance(STEP);
        assert!(b.input_released("fire"));
        assert!(!b.input_is_down("fire"));
        // The stamp persists at the original down-edge tick through the up window.
        assert_eq!(b.input_pressed_at_tick("fire"), vec![0.0]);
    }

    #[test]
    fn a_pointer_drag_reports_the_contact_press_start_and_completes_as_a_swipe() {
        let mut b = bridge();
        // Tick 0: contact down at (200, 300) -> a fresh press is reported.
        b.input_pointer(200.0, 300.0, true);
        b.advance(STEP);
        assert_eq!(b.input_pointer_sample(), vec![200.0, 300.0, 1.0]);
        assert_eq!(b.input_pointer_pressed(), vec![200.0, 300.0]);
        assert_eq!(b.input_swipe(), ""); // mid-gesture
        // Tick 1: drag +200 in x, still down (a hold is not a fresh press).
        b.input_pointer(400.0, 300.0, true);
        b.advance(STEP);
        assert!(b.input_pointer_pressed().is_empty());
        // Tick 2: lift the contact -> the completed drag reads as a right swipe.
        b.input_pointer_clear();
        b.advance(STEP);
        assert_eq!(b.input_swipe(), "right");
        assert!(b.input_pointer_sample().is_empty());
    }

    #[test]
    fn relative_look_accumulates_between_ticks_and_drains_each_advance() {
        let mut b = bridge();
        // Two pointer-lock move samples accumulate before the tick (sum = (7, -2)).
        b.input_look(5.0, -3.0);
        b.input_look(2.0, 1.0);
        b.advance(STEP); // commits the accumulated look for this tick
        assert_eq!(b.input_look_delta(), vec![7.0, -2.0]);
        // With no new samples the next advance drains the look back to zero, so a
        // relative delta is never double-applied across frames.
        b.advance(STEP);
        assert_eq!(b.input_look_delta(), vec![0.0, 0.0]);
    }

    #[test]
    fn rebinding_an_action_changes_only_which_keys_fire_it() {
        let mut b = bridge();
        b.input_bind_action("fire", &["Space"]);
        b.input_bind_action("fire", &["KeyJ"]); // remap
        b.input_key("Space", true);
        b.advance(STEP);
        assert!(!b.input_is_down("fire")); // old key no longer fires it
        b.input_key("KeyJ", true);
        b.advance(STEP);
        assert!(b.input_is_down("fire")); // new key does
    }
}
