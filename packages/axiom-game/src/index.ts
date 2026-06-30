/*
 * Public entry point for the `@axiom/game` Phaser-style authoring SDK.
 *
 * Exposes the authoring surface (SPEC-00 §4.2 / SPEC-14): `createGame`, the free
 * `onFixedUpdate`/`onRender` registration functions, the deterministic loop core
 * (`GameLoop`, `stepFrame`) the platform edge drives, the `Sim`/`Frame`/`Scene`
 * authoring types, and the `NativeBridge`/`StepBudget` boundary contracts the
 * wasm runtime (`apps/axiom-game-runtime`) projects through. The browser RAF
 * driver lives in `raf-loop.ts` (the platform edge) and is bound by the host, not
 * re-exported here.
 *
 * Wave 4 CORE projected the synchronous data facades over the `NativeBridge`
 * seam: `Sim.rng` (SPEC-01), `Sim.world` (SPEC-02), `Sim.input` (SPEC-05), math
 * (SPEC-03), and the host bridge (SPEC-12, emit-exactly-once).
 *
 * Wave 4 TAIL adds the remaining author-facing surfaces, all bridge-backed and
 * fully covered against a fake bridge (no wasm):
 *   - `Sim.time` (SPEC-07) — `after`/`every`/`cancel` timers + `createMachine`,
 *     driven by the per-tick callback-dispatch PUMP (`TickPump`) the `GameLoop`
 *     runs once per fixed tick: the native `TickApi` reports the due ids each tick
 *     and the pump dispatches the held author closures, so a timer set at tick T
 *     with delay D fires deterministically at T+D;
 *   - `Sim.tweens` (SPEC-09) — `add`/`cancel`, sampled per fixed tick by the same
 *     pump through the native `TweenApi`;
 *   - `Sim.add.*` (SPEC-14) — retained `GameObject` handles wrapping an ECS entity
 *     (`sprite`/`text`/`rectangle`/`image`) with bridge-backed mutators;
 *   - `Sim.physics` (SPEC-10) — `physics.add.*` bodies, `applyImpulse`/`applyForce`
 *     /`applyTorque`, velocity setters, and world `setConfig`;
 *   - audio (SPEC-08) — the `loadSound`/`playSound`/`playTone`/… free functions,
 *     presentation-side over the `HostBridge`.
 *
 * Wave 4 FINAL completes the authoring surface with the last three projections,
 * all bridge-backed and fully covered against a fake bridge (no wasm):
 *   - SPEC-06 grid/path — `createGrid`/`Grid`/`tileSpace`/`TileSpace` plus the
 *     authoritative `gridPath`/`gridReachable`/`gridDistanceField`/`stepToward`
 *     queries, whose BFS / wavefront runs native-side (the projection feeds the
 *     core a passability mask and forwards the cell sequence / distance field);
 *   - SPEC-11 3D — `createMesh`/`createMaterial`/`setCamera3D`/`addLight`, and the
 *     `v3`/`mat4`/`quat` namespaces, every op of which routes to the NATIVE
 *     `MathApi` (one deterministic source of truth; no TS math twin);
 *   - SPEC-13 netcode — `NetSim` (the `Sim` widened with player addressing),
 *     `Intent`, `joinRoom(JoinConfig) → NetClient`, and `configureNet`, projected
 *     over a `NetTransport`/`NetParticipants` seam the runtime binds over
 *     `@axiom/client` (physics prediction stays OFF — authority state only).
 *
 * With these landed, the @axiom/game authoring surface is contract-complete modulo
 * the wasm runtime bridge (`apps/axiom-game-runtime`) that implements the seams.
 */

export { createGame } from "./game.ts";
export type { Game, GameConfig, GameStatus } from "./game.ts";

export { GameLoop } from "./game-loop.ts";
export { onFixedUpdate, onRender, GameRegistry, activeRegistry, useRegistry } from "./registry.ts";

export { mountScene } from "./scene-runtime.ts";
export type { MountedScene } from "./scene-runtime.ts";

export { stepFrame } from "./loop-core.ts";
export type { FixedUpdate, FrameStep, Render } from "./loop-core.ts";

export { makeFrame, makeSim } from "./sim.ts";
export type { Frame, Sim, SimContext } from "./sim.ts";

export { interpolationAlpha } from "./step-budget.ts";
export type { StepBudget } from "./step-budget.ts";

export type { BodyKind, NativeBridge, PointerSample, Swipe, TweenCurve } from "./native-bridge.ts";

export { Scene } from "./scene.ts";
export type { Cameras, SceneFactories, Sound } from "./scene.ts";

// SPEC-09 — the screen-space UI / HUD authoring surface + the flex layout solver.
export { makeUi } from "./ui.ts";
export type { Ui } from "./ui.ts";
export type { UiBridge, UiStyle, UiTextOpts, UiViewport } from "./ui-binding.ts";
export { solveLayout } from "./ui-layout.ts";
export type { LayoutNode } from "./ui-layout.ts";

// Wave 4 — the projected subsystem surfaces.

export { makeRng, StreamRng, ROOT_STREAM } from "./rng.ts";
export type { Rng } from "./rng.ts";

export { makeWorld, BridgeWorld } from "./world.ts";
export type { World } from "./world.ts";

export { makeInput, SnapshotInput, bindAction } from "./input.ts";
export type { Action, Input } from "./input.ts";

export { aabbOverlap, circleOverlap, clamp, lerp, normalizeAngle, pointInRect, v2 } from "./math.ts";

export { overlapBox, overlapCircle, raycast } from "./query.ts";

export { getSessionConfig, notifyReady, reportOutcome, reportOutcomes } from "./host.ts";

export { bindNative } from "./host-binding.ts";
export type {
  HostBridge,
  MusicOptions,
  Outcome,
  ScheduleOptions,
  SessionConfig,
  SoundOptions,
  ToneEnvelope,
  ToneLfo,
  ToneSpec,
} from "./host-binding.ts";

// The 2D drawing seam (SPEC-04 §10), behind `Frame` — `HostBridge` extends it.
export type {
  Draw2dBridge,
  EllipseRadii,
  EmitterConfig,
  LineStyle,
  ShapeStyle,
  SpriteAnimation,
  SpriteOpts,
  TextMetrics,
  TextOpts,
} from "./draw2d-binding.ts";

// The pure flip-book sampler (SPEC-04 §10.2) — the one draw2d free function.
export { sampleAnimation } from "./draw2d.ts";

// Presentation asset loaders (SPEC-04 §10): fetch in the app, stable handles.
export { loadFont, loadTexture } from "./loader.ts";

// Wave 4 TAIL — the pump-driven and retained-object surfaces.

export { makeTime } from "./time.ts";
export type { Time } from "./time.ts";

export { BridgeStateMachine } from "./state-machine.ts";
export type { MachineInit, StateMachine, StateNode, TickDriven } from "./state-machine.ts";

export { makeTweens, EASES } from "./tweens.ts";
export type { Ease, Tweens, TweenSpec } from "./tweens.ts";

export { TickPump } from "./pump.ts";

export { makeAdd, GameObject } from "./game-object.ts";
export type { Add, RectangleStyle } from "./game-object.ts";

export { makePhysics } from "./physics.ts";
export type { Body, Physics, PhysicsAdd, PhysicsConfig } from "./physics.ts";

export {
  loadSound,
  playMusic,
  playSound,
  playTone,
  scheduleSound,
  setMasterVolume,
  setMuted,
  stopVoice,
} from "./sound.ts";

export { orElse, whenPresent } from "./control-flow.ts";

// The capstone sample game (a top-down arena over the surfaces above; replay-proven in test/arena.test.ts).
export { Arena } from "./sample/arena.ts";

// Wave 4 FINAL — grid/path (SPEC-06), 3D (SPEC-11), netcode (SPEC-13).

export {
  BridgeGrid,
  createGrid,
  gridDistanceField,
  gridPath,
  gridReachable,
  stepToward,
  tileSpace,
} from "./grid.ts";
export type { CellPair, Grid, TileSpace } from "./grid.ts";

export type {
  CameraDescriptor,
  GridField,
  LightDescriptor,
  MaterialDescriptor,
  PerspectiveSpec,
} from "./host-descriptors.ts";

export { mat4, quat, v3 } from "./math3d.ts";

export {
  addLight,
  clearScene,
  controlFirstPerson,
  createController,
  createMaterial,
  createMesh,
  createMeshData,
  setCamera3D,
  setNodeBounds,
  setNodeTransform,
  spawnRenderable,
} from "./scene3d.ts";
export type { Camera3D, FirstPersonControl, Light, MaterialSpec, MeshData, MeshKind } from "./scene3d.ts";
export type { ControllerSpec } from "./host-descriptors.ts";

export {
  bindNetTransport,
  boundNetConfig,
  boundNetRestore,
  boundNetSnapshot,
  configureNet,
  joinRoom,
  makeNetSim,
  onRestore,
  onSnapshot,
} from "./net.ts";

// SPEC-13 §16.3/§16.6 — the room hosting + matchmaking lobby surface (seam-bound, inert until the runtime binds).
export { bindMatchmaker, bindRoomHost, hostRoom, matchmake } from "./net-room.ts";
export type { Match, Matchmaker, MatchmakeOptions, Room, RoomConfig, RoomHostFactory, RoomId } from "./net-room.ts";

// SPEC-13 §16.1/§16.5 — the authority-snapshot participant-block decoder feeding makeNetSim each snapshot.
export { makeNetParticipants } from "./net-participants.ts";
export type { DecodedSnapshot } from "./net-participants.ts";

// SPEC-13 §16.5 — the delta-transparent inbound-snapshot path: a delta frame reconstructs the same full participant state a full keyframe carries, so the author always sees full state.
export { makeSnapshotIntake, reconstructSnapshot } from "./net-snapshot.ts";
export type { SnapshotFrameKind, SnapshotIntake } from "./net-snapshot.ts";

/*
 * The `@axiom/client` binding for the net seam (SPEC-13): the runtime adapts a real
 * `AxiomClient` into the `NetTransport` factory, and owns the deterministic
 * flat-`Intent` wire codec. The covered spine binds against the injected
 * `AxiomClientLike`; the app supplies the real client at boot (see axiom-net.ts).
 */
export { axiomNetFactory, decodeIntent, encodeIntent, netTransportFromClient } from "./axiom-net.ts";
export type { AxiomClientLike, NetBind, PredictionGate } from "./axiom-net.ts";
export type {
  ConnStatus,
  Intent,
  JoinConfig,
  NetCarrier,
  NetClient,
  NetConfig,
  NetParticipants,
  NetSim,
  NetTransport,
  NetTransportFactory,
} from "./net.ts";

export type {
  Cell,
  Circle,
  Component,
  ComponentKind,
  Entity,
  FontSpec,
  Handle,
  Mat4,
  PlayerId,
  Quat,
  RayHit,
  Rect,
  Result,
  Rgba,
  Seconds,
  TextureId,
  Ticks,
  Transform,
  Vec2,
  Vec3,
} from "./vocabulary.ts";
