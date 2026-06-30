# Game API Contract

The stable surface a game author calls to build a complete game on the engine **without writing
engine internals**. This is the concrete companion to [`game-vocabulary.md`](game-vocabulary.md):
the vocabulary names the verbs; this document fixes their exact signatures, semantics, and return
values.

Signatures are written in TypeScript — the authoring SDK is the stable boundary. The deterministic
core is native; every entry below is exposed across the engine boundary and backed by the core. Where
the core already provides a capability (fixed-step tick, seeded RNG, `spawn`/`despawn`, raycast,
overlap, the 3D scene), this contract states the *shape that authoring code may rely on*, not a new
implementation.

---

## Status: implemented (2026-06-28)

**This contract is now realized.** The SPEC-00..14 program
([`specs/`](specs/)) has landed on `main`: every entry below is backed by a
native facade and projected into the `@axiom/game` TypeScript SDK.

The authoring surface a game is actually written in is the **Phaser-style
`Scene`** class (SPEC-14): an author subclasses `Scene` and reaches the engine
through factory namespaces (`this.add`, `this.physics`, `this.input`,
`this.tweens`, `this.sound`, `this.time`, `this.cameras`). The functional
primitives in this document — `createGame`/`onFixedUpdate`/`onRender`, `Sim`,
`World`, `Rng`, `Frame`, `Ui`, `NetSim` — are the **lower layer** that `Scene`
and its factories ride on; both ship together as `@axiom/game`. The
deterministic native core (the `axiom-frame` accumulator + `apps/axiom-game-runtime`'s
`WasmGame`) runs underneath.

Status of the once-deferred surfaces (a 2026-06-30 pass closed them):

- **§10 2D surface.** The neutral ordered draw-list (`axiom-draw2d` →
  host-owned `Draw2dList`) and **both** backends are complete: software and GPU
  (the GPU raster in its wgpu offscreen/`wasm32` arm) rasterize rect / circle /
  ellipse / line / particle / sprite / **path** / **linear+radial gradient** /
  **text glyph-runs** with per-shape **fill + stroke** and src-over alpha, at
  proven GPU↔software parity. §10.1 particles (incl. `[min,max]` ranged emitter
  fields), §10.2 `sampleAnimation`, and §10.3 render-targets are landed.
- **§11 3D.** `cylinder`, `emissive`/`roughness`/`opacity`, and a hemisphere
  ambient term landed; `v3`/`mat4`/`quat` route to the native `MathApi` (no TS
  twin). **3D translucency now blends** (`opacity` folds into the per-draw alpha
  on both backends; translucent draws sort back-to-front), authored
  `createMaterial` opacity/emissive/roughness reach the renderer, and
  author-supplied **`MeshData`** (`createMeshData`) rides the catalog
  resolved-geometry pipeline.
- **§16 multiplayer.** `NetSim`/`joinRoom`/`configureNet` plus the follow-ups —
  delta snapshot encoding, HS256 JWT admission, an unreliable datagram transport,
  and client-side prediction (`configureNet({ predictLocalPlayer })`) — all
  landed. **Physics net-prediction stays OFF by decision** (prediction is proven
  same-binary only; cross-instance deterministic physics, §17.6 / SPEC-10, is
  unresolved) — the opt-in flips with no API change once it is proven.
- The wasm runtime bridge and live browser presentation/audio are
  **browser-proven** — the native sandbox cannot run browser WebGPU / Web Audio.

---

## 0. Foundations

### 0.1 Two contracts, one engine

The API is split by determinism, and the split is load-bearing:

- **Simulation contract** (§2–§9) — deterministic and authoritative. Runs on a fixed timestep. The
  only clock is the tick counter; the only randomness is the seeded stream; the only inputs are the
  per-tick intent stream. Identical `(seed, config, input stream)` **must** produce byte-identical
  state every run.
- **Presentation contract** (§10–§13) — client-side and non-authoritative. May read real time, may
  interpolate, may drop frames. Nothing here may feed a value back into the simulation.

A capability that needs to affect gameplay lives in the simulation contract. A capability that only
needs to be *seen or heard* lives in the presentation contract.

### 0.2 Core types

```ts
type Entity   = number;          // opaque, stable for the entity's lifetime
type Ticks    = number;          // integer count of fixed steps
type Seconds  = number;          // real or simulated seconds (float)
type Handle   = number;          // opaque resource handle (texture, sound, mesh, body, …)

interface Vec2 { x: number; y: number }
interface Vec3 { x: number; y: number; z: number }
interface Rect { x: number; y: number; width: number; height: number }
interface Rgba { r: number; g: number; b: number; a: number }   // each 0..1

type Result<T> = T | null;       // null is a normal "absent/failed" outcome, never an exception
```

Rules that hold everywhere:

- **Handles and entities are opaque.** Never serialized into simulation state; a replay re-binds them.
- **`null` is a normal outcome.** Query misses, failed casts, out-of-bounds reads return `null`, not
  throws. The API does not throw for ordinary control flow.
- **Coordinates.** World space is right-handed; 2D uses `+x` right, `+y` up. Screen space (UI) uses
  `+x` right, `+y` down, origin top-left. Functions state which space they take.
- **Units.** Distances are world units; angles are radians; time in the sim is `Ticks` (never wall
  clock).

---

## 1. Application & frame model

```ts
interface GameConfig {
  fixedHz: number;          // simulation rate, e.g. 60. The fixed step is 1/fixedHz seconds.
  seed: bigint;             // simulation seed (see §3)
  surface: string;          // id of the render surface to bind
}

interface Game {
  start(): void;            // begin the loop
  pause(): void;
  resume(): void;
  stop(): void;             // tear down; releases all handles
}

function createGame(config: GameConfig): Game;
```

The engine owns the loop. The author registers callbacks; **the author never writes the loop**:

```ts
// Called exactly once per fixed tick, in order, with a constant dt. Deterministic.
function onFixedUpdate(cb: (sim: Sim) => void): void;

// Called once per rendered frame for presentation only. `alpha` is the 0..1 interpolation
// fraction between the last two ticks. Never mutates simulation state.
function onRender(cb: (frame: Frame, alpha: number) => void): void;
```

- `onFixedUpdate` may run 0..N times per rendered frame (accumulator). Its callback receives the
  simulation interface `Sim` (§2–§9).
- `onRender` runs once per frame with the presentation interface `Frame` (§10–§13).
- Neither returns a value.

---

## 2. Simulation context

```ts
interface Sim {
  readonly tick: Ticks;     // current fixed tick, monotonic from 0
  readonly dt: Seconds;     // constant fixed-step duration (1/fixedHz)
  readonly rng: Rng;        // §3
  readonly input: Input;    // §8 (already sampled into per-tick intents)
  readonly world: World;    // §4
  readonly time: Time;      // §9 timers + tick-driven state machines
}
```

`Sim` exposes **no wall-clock accessor**. Elapsed simulated time is `tick * dt`.

---

## 3. Deterministic randomness

A single seeded stream. Seeded from `GameConfig.seed`; reproducible. The author may derive
independent sub-streams by name.

```ts
interface Rng {
  next(): number;                       // uniform float in [0, 1)
  int(maxExclusive: number): number;    // uniform integer in [0, maxExclusive)
  range(min: number, max: number): number;        // uniform float in [min, max)
  bool(p?: number): boolean;            // true with probability p (default 0.5)
  pick<T>(items: readonly T[]): T;      // uniform element; throws-free (items must be non-empty)
  weighted<T>(items: readonly T[], weights: readonly number[]): T;  // weighted element
  shuffle<T>(array: T[]): void;         // in-place Fisher-Yates, deterministic
  stream(name: string): Rng;            // a named, independent, reproducible sub-stream
}
```

No simulation code may call any other randomness source.

---

## 4. Entities, components, queries

```ts
interface World {
  spawn(...components: Component[]): Entity;
  despawn(e: Entity): void;
  alive(e: Entity): boolean;

  get<C extends Component>(e: Entity, kind: ComponentKind<C>): Result<C>;
  set<C extends Component>(e: Entity, value: C): void;     // add or replace
  remove(e: Entity, kind: ComponentKind<C>): void;
  has(e: Entity, kind: ComponentKind<C>): boolean;

  query(...kinds: ComponentKind[]): Entity[];             // entities having all kinds
}
```

Built-in components every game may use:

```ts
interface Transform extends Component {                   // position/rotation/scale
  position: Vec3;        // 2D games use z = 0 (or a layer index)
  rotation: number;      // radians about z for 2D; quaternion form for 3D (see §11)
  scale: Vec3;
}
```

Authors declare their own components; the engine treats them as opaque typed records and never
inspects gameplay meaning (per the Vocabulary Law). `spawn` returns the new `Entity`; `query` returns
a stable-ordered array for the current tick.

### 4.1 Hierarchy

Entities form a transform hierarchy: a child's world transform is its local transform composed with
its parent's. Moving a parent moves its children rigidly — groups, attached parts, formations.

```ts
interface World {
  // …§4 above…
  setParent(child: Entity, parent?: Entity): void;         // omitted parent detaches to the root (the SDK's no-null lint law)
  parentOf(e: Entity): Result<Entity>;
  childrenOf(e: Entity): Entity[];
  worldTransform(e: Entity): Transform;     // resolved (composed) transform for this tick
}
```

The `Transform` component on an entity is its **local** transform; `worldTransform` returns the
composed result. Despawning a parent despawns its whole subtree.

---

## 5. Math & spatial queries

### 5.1 Scalars & vectors

```ts
function clamp(v: number, lo: number, hi: number): number;
function lerp(a: number, b: number, t: number): number;
function normalizeAngle(a: number): number;              // wrap to (-π, π]

const v2: {
  add(a: Vec2, b: Vec2): Vec2; sub(a: Vec2, b: Vec2): Vec2;
  scale(a: Vec2, s: number): Vec2; dot(a: Vec2, b: Vec2): number;
  len(a: Vec2): number; normalize(a: Vec2): Vec2;
  dist(a: Vec2, b: Vec2): number; lerp(a: Vec2, b: Vec2, t: number): Vec2;
};
// v3 / mat4 / quat: full equivalents for 3D (see §11).
```

### 5.2 Overlap & ray queries (authoritative)

```ts
function aabbOverlap(a: Rect, b: Rect): boolean;
function pointInRect(p: Vec2, r: Rect): boolean;
function circleOverlap(aCenter: Vec2, aR: number, bCenter: Vec2, bR: number): boolean;

// Scene queries against entities carrying spatial bounds:
function overlapBox(center: Vec3, halfExtents: Vec3): Entity[];   // every bounded entity intersecting the box
function overlapCircle(center: Vec2, radius: number): Entity[];
interface RayHit { entity: Entity; point: Vec3; distance: number }
function raycast(origin: Vec3, dir: Vec3, maxDistance: number): Result<RayHit>;  // nearest hit
```

Queries read committed world transforms for the current tick.

---

## 6. Grid / tilemap

A first-class integer grid, the substrate for tile games, board games, and procedural levels.

```ts
interface Grid<T = number> {
  readonly cols: number;
  readonly rows: number;
  get(x: number, y: number): T;            // out-of-bounds returns the grid's default cell
  set(x: number, y: number, value: T): void;
  inBounds(x: number, y: number): boolean;
  idx(x: number, y: number): number;       // row-major flat index
  fill(value: T): void;
  clone(): Grid<T>;
  forEach(cb: (value: T, x: number, y: number) => void): void;
}

function createGrid<T>(cols: number, rows: number, fill: T): Grid<T>;

// Tile ↔ world mapping for a grid placed at `origin` with square `cellSize`:
interface TileSpace {
  tileToWorld(x: number, y: number): Vec2;     // center of the cell
  worldToTile(p: Vec2): { x: number; y: number };
  snapToCell(p: Vec2): Vec2;                    // nearest cell center
}
function tileSpace(origin: Vec2, cellSize: number): TileSpace;
```

---

## 7. Pathfinding & steering (authoritative)

Operates over a `Grid`; `passable(value)` decides traversability.

```ts
type Cell = { x: number; y: number };
// Path endpoints bundled into one record: the SDK's lint law caps a function at
// 3 params, so the start/goal pair travels as a single `CellPair` argument.
type CellPair = { start: Cell; goal: Cell };

// Shortest path start→goal over 4-connectivity, or null if unreachable.
function gridPath(grid: Grid, ends: CellPair,
                  passable: (v: number) => boolean): Result<Cell[]>;

// Whether any path exists.
function gridReachable(grid: Grid, ends: CellPair,
                       passable: (v: number) => boolean): boolean;

// BFS distance from `start` to every cell (Infinity where unreachable).
function gridDistanceField(grid: Grid, start: Cell,
                           passable: (v: number) => boolean): Grid<number>;

// One greedy step from the pair's `start` toward its `goal`, minimizing a cost (Euclidean by default).
function stepToward(grid: Grid, ends: CellPair,
                    passable: (v: number) => boolean): Cell;
```

---

## 8. Input (sampled into the simulation)

Input is captured on the client, mapped through bindings, and delivered to the simulation as a
per-tick intent snapshot. Inside `onFixedUpdate`, `Sim.input` reads the snapshot for that tick.

```ts
type Action = string;            // an author-defined action name

interface Input {
  isDown(action: Action): boolean;       // held this tick
  pressed(action: Action): boolean;      // went down this tick (edge)
  released(action: Action): boolean;     // went up this tick (edge)
  axis(neg: Action, pos: Action): -1 | 0 | 1;   // composed 1-D axis

  pointer(): { pos: Vec2; down: boolean } | null;  // world-space pointer, null if absent
  pointerPressed(): Result<Vec2>;        // world position of a press this tick
  swipe(): "up" | "down" | "left" | "right" | null;  // completed gesture this tick

  // Tick at which the most recent press of `action` occurred, for timing-window judging.
  pressedAtTick(action: Action): Result<Ticks>;
}

// Configured once at setup; remappable:
function bindAction(action: Action, keys: string[]): void;
```

- Bindings map physical keys/buttons/gestures → action names; gameplay reads only action names.
- Edge accessors (`pressed`/`released`) are resolved per tick; auto-repeat is suppressed.
- `pressedAtTick` exists so rhythm/reaction games can judge against a tick window deterministically.

---

## 9. Time, lifecycle & state (simulation)

All tick-based, deterministic. **No wall-clock timers in the simulation.**

```ts
type TimerId = Handle;

// The timer + state-machine factory, reached at `Sim.time`.
interface Time {
  after(ticks: Ticks, cb: () => void): TimerId;     // one-shot
  every(ticks: Ticks, cb: () => void): TimerId;     // repeating
  cancel(id: TimerId): void;
  // Mint a tick-driven state machine from an ORDERED list of states + the initial.
  createMachine<S extends string>(states: readonly StateNode<S>[], initial: S): StateMachine<S>;
}

// Finite state machine for per-entity or per-game state (poses, AI modes, round phases).
interface StateMachine<S extends string> {
  readonly current: S;
  readonly ticksInState: Ticks;
  transition(to: S): void;
}
// One declared state: its `name` plus optional lifecycle closures. States are an
// ORDERED `StateNode[]` (not `Record<S, StateDef>`) so the name list is genuinely
// typed `S[]` (`states.map(n => n.name)`) — avoiding the `Object.keys(...) as S[]`
// downcast the SDK's lint law forbids; declaration order is the dense index order.
interface StateNode<S extends string> {
  name: S;
  onEnter?: (sm: StateMachine<S>) => void;
  onUpdate?: (sm: StateMachine<S>) => void;          // called each tick while active
  onExit?: (sm: StateMachine<S>) => void;
}
```

Entity lifecycle is `world.spawn`/`world.despawn` (§4). The engine is responsible for pooling;
authors do not manage object pools.

---

## 10. Presentation: 2D surface

Immediate-mode, called from `onRender`. The engine batches draws; ordering is by `layer` (lower
drawn first), then call order. All positions are world space unless a draw targets the screen layer
(§13). Non-authoritative.

```ts
interface Frame {
  camera2D(view: { center: Vec2; zoom: number }): void;

  // Shapes
  rect(r: Rect, style: FillStroke & Common): void;
  circle(center: Vec2, radius: number, style: FillStroke & Common): void;
  ellipse(center: Vec2, rx: number, ry: number, rotation: number, style: FillStroke & Common): void;
  line(a: Vec2, b: Vec2, style: { color: Rgba; width: number } & Common): void;
  path(points: Vec2[], style: FillStroke & Common & { closed?: boolean }): void;

  // Sprites
  sprite(texture: TextureId, opts: SpriteOpts): void;

  // Text
  text(value: string, opts: TextOpts): void;
  measureText(value: string, font: FontSpec): { width: number; height: number };

  // Paints
  linearGradient(from: Vec2, to: Vec2, stops: GradientStop[]): Paint;
  radialGradient(center: Vec2, radius: number, stops: GradientStop[]): Paint;
}

interface Common  { layer?: number; alpha?: number; shadow?: { color: Rgba; blur: number } }
interface FillStroke { fill?: Rgba | Paint; stroke?: Rgba; strokeWidth?: number }
type GradientStop = { offset: number; color: Rgba };
type Paint = Handle;

interface SpriteOpts extends Common {
  pos: Vec2; rotation?: number; scale?: Vec2;
  anchor?: Vec2;            // 0..1 within the sprite; default {0.5,0.5}
  tint?: Rgba; flipX?: boolean; flipY?: boolean;
  source?: Rect;           // sub-rect of the texture (atlas / flip-book frame)
}
interface TextOpts extends Common {
  pos: Vec2; font: FontSpec; color: Rgba;
  align?: "left" | "center" | "right";
}
type FontSpec = { family: string; size: number; weight?: number };
```

Resource loading (handles are stable for the game's lifetime):

```ts
function loadTexture(url: string): TextureId;      // returns immediately; usable once resolved
function loadFont(url: string): FontSpec;
```

### 10.1 Particles

```ts
type EmitterId = Handle;
interface EmitterConfig {
  count: number;
  lifetime: [Seconds, Seconds];        // random range
  speed: [number, number];
  spread: number;                      // emission cone half-angle (radians)
  gravity?: Vec2;
  size: [number, number];
  colorStart: Rgba; colorEnd: Rgba;    // interpolated over lifetime (alpha included)
  layer?: number;
}
function createEmitter(config: EmitterConfig): EmitterId;
function emit(id: EmitterId, at: Vec2, direction?: Vec2): void;   // burst
```

Particles are simulated and drawn by the engine on the presentation clock.

### 10.2 Flip-book animation

```ts
interface SpriteAnimation { frames: Rect[]; fps: number }   // frames are atlas sub-rects
function sampleAnimation(anim: SpriteAnimation, elapsed: Seconds, loop?: boolean): Rect;
```

### 10.3 Render targets

Draw into an off-screen surface once and reuse it as a texture — for static layers (backgrounds,
baked geometry) and full-surface effects.

```ts
type RenderTargetId = Handle;
function createRenderTarget(width: number, height: number): RenderTargetId;
function drawTo(target: RenderTargetId, draw: (frame: Frame) => void): void;  // render into it
function targetTexture(target: RenderTargetId): TextureId;                    // use as a sprite texture
```

A target persists until released; redraw only when its contents change. (The engine also batches
static drawables automatically — render targets are the explicit form for caching or post-effects.)

---

## 11. Presentation: 3D scene

Retained scene graph (the engine's existing model). Listed for contract completeness.

```ts
function createMesh(kind: "box" | "sphere" | "cylinder" | MeshData): MeshId;
function createMaterial(spec: {
  baseColor: Rgba; emissive?: Rgba; roughness?: number; opacity?: number;
}): MaterialId;

// Renderable entity: Transform + a mesh + a material.
interface Renderable extends Component { mesh: MeshId; material: MaterialId }

function setCamera3D(cam: { position: Vec3; target: Vec3; fovY: number; near: number; far: number }): void;
function addLight(light:
  | { kind: "directional"; direction: Vec3; color: Rgba; intensity: number }
  | { kind: "point"; position: Vec3; color: Rgba; intensity: number }
): Entity;

// 3D math:
const v3: { add; sub; scale; dot; cross; len; normalize; dist; lerp /* … */ };
const mat4: { identity; multiply; perspective; lookAt; invert; fromTRS /* … */ };
const quat: { identity; fromEuler; multiply; normalize; toMat4 /* … */ };
```

---

## 12. Presentation: animation & easing

Tweens run on the presentation clock; they animate display values only.

```ts
type Ease = "linear" | "quadIn" | "quadOut" | "quadInOut" | "cubicOut" | "expoOut" | "backOut";
type TweenId = Handle;
interface TweenSpec {
  from: number; to: number; duration: Seconds; ease?: Ease;
  onUpdate: (value: number) => void;
  onComplete?: () => void;
}
function tween(spec: TweenSpec): TweenId;
function cancelTween(id: TweenId): void;
```

---

## 13. Presentation: audio

Client-side, non-authoritative. Scheduled against an audio clock independent of the sim tick; **no
audio result may re-enter the simulation.**

```ts
type SoundId = Handle;   // a loaded sample
type VoiceId = Handle;   // a playing instance

function loadSound(url: string): SoundId;

// Sample playback
function playSound(id: SoundId, opts?: { volume?: number; pitch?: number; loop?: boolean }): VoiceId;
function stopVoice(id: VoiceId): void;

// Music (streamed / sequential playlist)
function playMusic(urls: string[], opts?: { loop?: boolean; crossfadeSeconds?: number }): VoiceId;

// Synthesis (no asset required)
interface ToneSpec {
  wave: "sine" | "square" | "sawtooth" | "triangle";
  freq: number;                                  // Hz
  duration: Seconds;
  envelope?: { attack: Seconds; decay: Seconds; sustain: number; release: Seconds };
  volume?: number;
  lfo?: { freq: number; depth: number };         // frequency modulation
}
function playTone(spec: ToneSpec): VoiceId;

// Scheduling & mix
function scheduleSound(id: SoundId, atSeconds: Seconds, opts?: { volume?: number }): VoiceId;
function setMasterVolume(v: number): void;        // 0..1
function setMuted(muted: boolean): void;
```

### 13.1 Audio input & analysis (non-authoritative)

Capture an external audio stream and read its spectrum — for visualizers or audio-reactive
presentation.

```ts
type AudioInput = Handle;
function openAudioInput(source: "microphone" | "system"): Promise<AudioInput>;   // user-gated
interface Analyser {
  bands(count: number): Float32Array;   // normalized energy per frequency band, current frame
  level(): number;                      // overall RMS level, 0..1
}
function createAnalyser(input: AudioInput): Analyser;
```

**Determinism constraint.** Captured audio is external, irreproducible input. Values read here **must
not enter the simulation contract** (§17). A game that drives *gameplay* from a live audio signal is,
by definition, **not** authoritative and must run in local (non-networked) mode. To keep gameplay
authoritative, drive it from authored data and use analysis for presentation only.

---

## 14. Presentation: UI / HUD overlay

A screen-space layer drawn after the world. UI uses screen coordinates and is camera-independent.

```ts
interface Ui {
  // Screen-space variants of the 2D surface (origin top-left, +y down).
  rect(r: Rect, style: FillStroke & Common): void;
  text(value: string, opts: TextOpts): void;
  sprite(texture: TextureId, opts: SpriteOpts): void;

  // Immediate-mode button: draws and returns whether it was activated this frame.
  button(bounds: Rect, label: string, style?: FillStroke): boolean;

  readonly viewport: { width: number; height: number };
}

// Responsive layout solver: resolve a tree of boxes against the viewport.
interface LayoutNode { id: string; direction?: "row" | "column"; grow?: number; basis?: number;
                       aspect?: number; children?: LayoutNode[] }
function solveLayout(root: LayoutNode, viewport: Rect): Record<string, Rect>;
```

Menus, pause screens, and result screens are composed by the author from these primitives plus an
author-held screen state (`StateMachine`, §9). The engine provides drawing and hit-testing, not a
widget framework.

---

## 15. Host bridge

The engine runs inside a host environment that supplies configuration and receives the outcome. This
boundary is intentionally minimal and host-agnostic.

```ts
// Read host-supplied configuration (seed plus opaque string/number parameters).
function getSessionConfig(): { seed: bigint; params: Record<string, string | number> };

// Signal readiness once the first frame can render.
function notifyReady(): void;

// Emit the terminal outcome exactly once. The engine forwards it to the host channel.
interface Outcome { won: boolean; score: number; metrics?: Record<string, number> }
function reportOutcome(o: Outcome): void;
```

The engine owns delivery of `reportOutcome` to its host; the game does not address the host directly,
and the host channel is not part of this contract.

---

## 16. Multiplayer & netcode

Realtime, server-authoritative multiplayer **reuses the simulation contract unchanged** (§2–§9). The
same deterministic `onFixedUpdate` runs in three deployments:

- **local** — single authority, no network (the default).
- **authority** — the server-side authoritative instance for a room.
- **predicted** — a client-side copy run ahead of the authority for responsiveness.

Because the simulation is deterministic, the engine performs snapshotting, delta replication, client
prediction, and reconciliation. The author writes one simulation; the engine deploys it three ways.

### 16.1 Players & per-player input

In a networked game the simulation callback receives `NetSim`, which addresses input by player:

```ts
type PlayerId = number;

interface NetSim extends Sim {
  players(): PlayerId[];                  // seated players, stable order
  inputOf(player: PlayerId): Input;       // that player's per-tick intents (§8)
  joinedThisTick(): PlayerId[];
  leftThisTick(): PlayerId[];
}

function onFixedUpdate(cb: (sim: NetSim) => void): void;   // networked overload
```

`Sim.input` (the single-player accessor) equals `inputOf(localPlayer)` in local mode.

### 16.2 Intents

A player's command for a tick is an author-defined, serializable record. The engine owns the wire
codec from this one definition — there is no hand-written client/server codec twin.

```ts
type Intent = Record<string, number | string | boolean>;   // author-defined shape, per game
```

Intents are bounded and validated at the edge; a malformed or oversized intent is rejected, not
applied. Rejection is a normal outcome surfaced to the sender (§16.4), never a crash.

### 16.3 Rooms & authority

A room hosts one authoritative simulation shared by its players.

```ts
type RoomId = string;

interface RoomConfig {
  maxPlayers: number;
  seed: bigint;
  fixedHz: number;
  botFill?: { afterTicks: Ticks };   // optional: seat an author-driven bot in an empty slot
}

interface Room { readonly id: RoomId; players(): PlayerId[]; close(): void }
function hostRoom(config: RoomConfig): Room;   // stand up the authoritative instance
```

The authority advances the room at `fixedHz`, applies each player's accepted intents, runs the
author's `onFixedUpdate`, and replicates state to clients. A `botFill` slot appears to the simulation
as an ordinary `PlayerId`; the author supplies its intents from game code (its behaviour is game
logic, not engine).

### 16.4 Client connection

```ts
type ConnStatus = "connecting" | "connected" | "disconnected";

interface NetClient {
  status(): ConnStatus;
  localPlayer(): Result<PlayerId>;     // assigned on join; null until connected
  sendIntent(intent: Intent): void;    // local player's command for the current tick
  onStatus(cb: (s: ConnStatus) => void): void;
  onRejected(cb: (reason: string) => void): void;   // an intent was refused by the authority
  leave(): void;
}

interface JoinConfig { url: string; roomId: RoomId; token?: string }   // token is opaque auth
function joinRoom(config: JoinConfig): NetClient;
```

`status` stays `connecting` until the authority admits the client — the authority, not the socket,
decides membership. `token` is an opaque credential the engine forwards to the authority; its
issuance and meaning are outside this contract.

### 16.5 Replication, prediction & reconciliation

The engine snapshots authoritative state (the component store + sim tick + RNG state) automatically
and sends per-tick deltas. Authors holding authoritative state **outside** the component store
provide:

```ts
function onSnapshot(cb: () => Uint8Array): void;   // serialize extra authoritative state
function onRestore(cb: (bytes: Uint8Array) => void): void;
```

Prediction and interpolation are **configured, not hand-written** — the deterministic sim makes
reconciliation automatic (snap to the latest authoritative state, replay unacknowledged local
intents):

```ts
interface NetConfig {
  predictLocalPlayer: boolean;        // re-run local intents ahead of the authority
  interpolateRemote: boolean;         // smooth non-local entities between snapshots
  interpolationDelayTicks?: number;   // render remote state this many ticks in the past
}
function configureNet(cfg: NetConfig): void;
```

### 16.6 Matchmaking & outcomes

Matchmaking is delegated to the host; the engine exposes only a thin request:

```ts
interface Match { roomId: RoomId; url: string }
function matchmake(opts?: { mode?: string }): Promise<Match>;
```

In a room the outcome is per-player; the authority reports each participant's result (see `Outcome`,
§15):

```ts
function reportOutcomes(results: Record<PlayerId, Outcome>): void;
```

---

## 17. Determinism & replay requirements

Binding constraints on every simulation-contract API (§2–§9):

1. **Single clock.** The only time source in the simulation is the fixed tick. Wall-clock and
   frame-delta values are not exposed to simulation code.
2. **Single randomness source.** All simulation randomness flows through `Rng` (§3), seeded from
   `GameConfig.seed`.
3. **Input as a tick-indexed intent stream.** Raw device events are sampled to a per-tick snapshot
   before the simulation sees them.
4. **Reproducibility.** Given identical `(seed, config, input stream)`, the simulation must produce
   identical state every run. The engine exposes a per-tick state hash and a record/replay facility;
   a replay must reproduce the hash sequence exactly.
5. **Presentation is excluded.** Rendering, audio, tweens, and particles (§10–§14) may use real time
   and interpolation and are outside the determinism guarantee. No value produced there may be read
   back into the simulation.
6. **Cross-instance determinism (networked).** The authoritative and predicted simulations must
   produce bit-identical state from identical `(seed, config, per-player intent stream)` across
   machines, so prediction reconciles without drift. This requires deterministic arithmetic in the
   simulation contract; presentation math is unconstrained.

---

## 18. Implementation order

The dependency-respecting order to build the contract:

1. **Foundations & frame model** (§0–§2) — types, handles, fixed-step loop, `onFixedUpdate`/`onRender`.
2. **Determinism substrate** (§3, §17) — seeded `Rng`, tick clock, state hash + replay hooks.
3. **Entities, hierarchy & math** (§4–§5) — components, queries, parent/child transforms, vector/overlap/ray helpers.
4. **2D surface** (§10) — shapes, sprites, text, gradients, camera; then particles, flip-book, render targets.
   *The single largest new surface.*
5. **Input** (§8) — bindings, edge detection, pointer/touch, `pressedAtTick`.
6. **Grid, pathfinding, tile space** (§6–§7).
7. **Timers & state machines** (§9).
8. **Audio** (§13) — sample playback, then synthesis, then scheduling/mix; analysis (§13.1) last and optional.
9. **UI / HUD overlay** (§14) and **tween/easing** (§12).
10. **Physics extensions** — rigid-body dynamics (torque, impulse, friction, damping, broadphase)
    layered on the existing collision shapes, fixed-step and deterministic.
11. **Multiplayer & netcode** (§16) — per-player input, rooms/authority, client connection, then
    prediction/reconciliation. Requires cross-instance determinism (§17.6).
12. **3D scene** (§11) and **host bridge** (§15) round out the contract.
```
