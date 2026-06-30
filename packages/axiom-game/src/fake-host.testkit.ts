// A FAKE HostBridge that records every call, with scriptable return values, for
// the math / host-bridge / bindAction free-function tests. Kept in its own file
// so each fake is one class (max-classes-per-file).

import type { EllipseRadii, EmitterConfig, LineStyle, ShapeStyle, SpriteAnimation, SpriteOpts, TextMetrics, TextOpts } from "./draw2d-binding.ts";
import type { UiStyle, UiTextOpts, UiViewport } from "./ui-binding.ts";
import type {
  HostBridge,
  MusicOptions,
  Outcome,
  ScheduleOptions,
  SessionConfig,
  SoundOptions,
  ToneSpec,
} from "./host-binding.ts";
import type {
  CameraDescriptor,
  ControllerInput,
  ControllerSpec,
  GridField,
  LightDescriptor,
  MaterialDescriptor,
  MeshDataDescriptor,
  PerspectiveSpec,
} from "./host-descriptors.ts";
import type { Cell, Circle, Entity, FontSpec, Handle, Mat4, Quat, RayHit, Rect, Result, Transform, Vec2, Vec3 } from "./vocabulary.ts";

export class FakeHost implements HostBridge {
  public clampReturn = 0;
  public normalizeReturn = 0;
  public lerpCalls: (readonly [number, number, number])[] = [];
  public overlapReturn: readonly Entity[] = [];
  public config: SessionConfig = { params: {}, seed: 0n };
  public readyCount = 0;
  public clampCalls: (readonly [number, number, number])[] = [];
  public normalizeCalls: number[] = [];
  public overlapCalls: (readonly [number, number, number])[] = [];
  public overlapBoxReturn: readonly Entity[] = [];
  public overlapBoxCalls: { center: Vec3; halfExtents: Vec3 }[] = [];
  public raycastReturn: Result<RayHit> = undefined;
  public raycastCalls: { origin: Vec3; direction: Vec3; maxDistance: number }[] = [];
  public bindings: (readonly [string, readonly string[]])[] = [];
  public outcomes: Outcome[] = [];
  public outcomeSets: Readonly<Record<number, Outcome>>[] = [];

  // --- audio call log; voices/sounds get incrementing handles ---
  public loadedUrls: string[] = [];
  public playedSounds: (readonly [Handle, SoundOptions | undefined])[] = [];
  public stoppedVoices: Handle[] = [];
  public playedMusic: (readonly [readonly string[], MusicOptions | undefined])[] = [];
  public playedTones: ToneSpec[] = [];
  public scheduledSounds: (readonly [Handle, number, ScheduleOptions | undefined])[] = [];
  public masterVolumes: number[] = [];
  public muteStates: boolean[] = [];
  private nextHandle = 1;

  // --- grid / pathfinding: scripted returns + recorded fields/endpoints ---
  public gridPathReturn: readonly Cell[] | undefined = [];
  public gridReachableReturn = false;
  public gridDistanceReturn: readonly number[] = [];
  public gridStepReturn: Cell = { x: 0, y: 0 };
  public gridPathCalls: { field: GridField; start: Cell; goal: Cell }[] = [];
  public gridReachableCalls: { field: GridField; start: Cell; goal: Cell }[] = [];
  public gridDistanceCalls: { field: GridField; start: Cell }[] = [];
  public gridStepCalls: { field: GridField; from: Cell; target: Cell }[] = [];

  // --- 3D scene authoring call log; handles/entities get incrementing ids ---
  public meshKinds: number[] = [];
  public meshDatas: MeshDataDescriptor[] = [];
  public materials: MaterialDescriptor[] = [];
  public cameras: CameraDescriptor[] = [];
  public lights: LightDescriptor[] = [];
  public spawns: { mesh: Handle; material: Handle; transform: Transform }[] = [];
  public nodeTransforms: { entity: Entity; transform: Transform }[] = [];
  public nodeBounds: { entity: Entity; halfExtents: Vec3 }[] = [];
  public sceneClears = 0;
  public controllers: { spec: ControllerSpec; index: number }[] = [];
  public controls: ControllerInput[] = [];

  // --- 2D drawing call log (SPEC-04); emitters/targets/textures get incrementing handles ---
  public draw2dCameras: { center: Vec2; zoom: number }[] = [];
  public draw2dRects: { bounds: Rect; style: ShapeStyle }[] = [];
  public draw2dCircles: { center: Vec2; radius: number; style: ShapeStyle }[] = [];
  public draw2dEllipses: { center: Vec2; radii: EllipseRadii; style: ShapeStyle }[] = [];
  public draw2dLines: { from: Vec2; to: Vec2; style: LineStyle }[] = [];
  public draw2dEmitters: EmitterConfig[] = [];
  public draw2dEmits: { id: Handle; at: Vec2; direction: Vec2 }[] = [];
  public draw2dAdvances: number[] = [];
  public draw2dTargets: { width: number; height: number }[] = [];
  public draw2dBegins: Handle[] = [];
  public draw2dEnds = 0;
  public draw2dFinishReturn: readonly number[] = [];
  public draw2dSamples: { anim: SpriteAnimation; elapsedSeconds: number; looping: boolean }[] = [];
  public draw2dSpriteCalls: { texture: Handle; opts: SpriteOpts }[] = [];
  public draw2dTextCalls: { value: string; opts: TextOpts }[] = [];
  public measureTextReturn: TextMetrics = { height: 0, width: 0 };
  public measureTextCalls: { value: string; font: FontSpec }[] = [];

  // --- presentation assets (SPEC-04 §10): records the loaded urls; textures mint handles ---
  public loadedTextureUrls: string[] = [];
  public loadedFontUrls: string[] = [];
  public fontReturn: FontSpec = { family: "monospace", size: 16 };

  // --- UI surface (SPEC-09): records the marshalled calls; scriptable button/viewport/draw-list/layout returns ---
  public uiBeginFrames: { viewport: UiViewport; pointer: Vec2; pressed: boolean }[] = [];
  public uiRects: { bounds: Rect; style: UiStyle }[] = [];
  public uiTexts: { value: string; opts: UiTextOpts }[] = [];
  public uiSprites: { texture: Handle; bounds: Rect }[] = [];
  public uiButtons: { bounds: Rect; label: string; style: UiStyle }[] = [];
  public uiViewportReturn: UiViewport = { height: 0, width: 0 };
  public uiDrawListReturn: Uint8Array = new Uint8Array();
  public uiSolveLayoutReturn: readonly number[] = [];
  public uiSolveLayoutCalls: { viewport: UiViewport; nodes: readonly number[] }[] = [];

  public clamp(value: number, low: number, high: number): number {
    this.clampCalls.push([value, low, high]);
    return this.clampReturn;
  }

  public normalizeAngle(angle: number): number {
    this.normalizeCalls.push(angle);
    return this.normalizeReturn;
  }

  // lerp computes the affine blend (the same value the native MathApi::lerp yields)
  // and records the call, so a test can assert BOTH the forwarded result and that
  // the SDK crossed the bridge (no local TS re-implementation).
  public lerp(start: number, end: number, fraction: number): number {
    this.lerpCalls.push([start, end, fraction]);
    return start + (end - start) * fraction;
  }

  // --- 2D math (deterministic, input-derived returns: the projection only forwards) ---
  public v2Add(lhs: Vec2, rhs: Vec2): Vec2 {
    return { x: lhs.x + rhs.x, y: lhs.y + rhs.y };
  }

  public v2Sub(lhs: Vec2, rhs: Vec2): Vec2 {
    return { x: lhs.x - rhs.x, y: lhs.y - rhs.y };
  }

  public v2Scale(vector: Vec2, scalar: number): Vec2 {
    return { x: vector.x * scalar, y: vector.y * scalar };
  }

  public v2Dot(lhs: Vec2, rhs: Vec2): number {
    return lhs.x * rhs.x + lhs.y * rhs.y;
  }

  public v2Len(vector: Vec2): number {
    return Math.hypot(vector.x, vector.y);
  }

  public v2Normalize(vector: Vec2): Vec2 {
    const length = Math.hypot(vector.x, vector.y);
    return { x: vector.x / length, y: vector.y / length };
  }

  public v2Dist(lhs: Vec2, rhs: Vec2): number {
    return Math.hypot(lhs.x - rhs.x, lhs.y - rhs.y);
  }

  public v2Lerp(lhs: Vec2, rhs: Vec2, fraction: number): Vec2 {
    return { x: lhs.x + (rhs.x - lhs.x) * fraction, y: lhs.y + (rhs.y - lhs.y) * fraction };
  }

  // --- pure predicates (the same geometry the native Aabb / Sphere computes) ---
  public aabbOverlap(lhs: Rect, rhs: Rect): boolean {
    return (
      lhs.x <= rhs.x + rhs.width &&
      lhs.x + lhs.width >= rhs.x &&
      lhs.y <= rhs.y + rhs.height &&
      lhs.y + lhs.height >= rhs.y
    );
  }

  public pointInRect(point: Vec2, rect: Rect): boolean {
    return (
      point.x >= rect.x &&
      point.x <= rect.x + rect.width &&
      point.y >= rect.y &&
      point.y <= rect.y + rect.height
    );
  }

  public circleOverlap(lhs: Circle, rhs: Circle): boolean {
    return Math.hypot(lhs.center.x - rhs.center.x, lhs.center.y - rhs.center.y) <= lhs.radius + rhs.radius;
  }

  public overlapCircle(centerX: number, centerY: number, radius: number): readonly Entity[] {
    this.overlapCalls.push([centerX, centerY, radius]);
    return this.overlapReturn;
  }

  public overlapBox(center: Vec3, halfExtents: Vec3): readonly Entity[] {
    this.overlapBoxCalls.push({ center, halfExtents });
    return this.overlapBoxReturn;
  }

  public raycast(origin: Vec3, direction: Vec3, maxDistance: number): Result<RayHit> {
    this.raycastCalls.push({ direction, maxDistance, origin });
    return this.raycastReturn;
  }

  public bindAction(action: string, keys: readonly string[]): void {
    this.bindings.push([action, keys]);
  }

  public getSessionConfig(): SessionConfig {
    return this.config;
  }

  public notifyReady(): void {
    this.readyCount += 1;
  }

  public reportOutcome(outcome: Outcome): void {
    this.outcomes.push(outcome);
  }

  public reportOutcomes(results: Readonly<Record<number, Outcome>>): void {
    this.outcomeSets.push(results);
  }

  public loadSound(url: string): Handle {
    this.loadedUrls.push(url);
    return this.mint();
  }

  public playSound(id: Handle, opts?: SoundOptions): Handle {
    this.playedSounds.push([id, opts]);
    return this.mint();
  }

  public stopVoice(voice: Handle): void {
    this.stoppedVoices.push(voice);
  }

  public playMusic(urls: readonly string[], opts?: MusicOptions): Handle {
    this.playedMusic.push([urls, opts]);
    return this.mint();
  }

  public playTone(spec: ToneSpec): Handle {
    this.playedTones.push(spec);
    return this.mint();
  }

  public scheduleSound(id: Handle, atSeconds: number, opts?: ScheduleOptions): Handle {
    this.scheduledSounds.push([id, atSeconds, opts]);
    return this.mint();
  }

  public setMasterVolume(volume: number): void {
    this.masterVolumes.push(volume);
  }

  public setMuted(muted: boolean): void {
    this.muteStates.push(muted);
  }

  // --- grid / pathfinding (records the field the projection built, returns scripted) ---
  public gridPath(field: GridField, start: Cell, goal: Cell): readonly Cell[] | undefined {
    this.gridPathCalls.push({ field, goal, start });
    return this.gridPathReturn;
  }

  public gridReachable(field: GridField, start: Cell, goal: Cell): boolean {
    this.gridReachableCalls.push({ field, goal, start });
    return this.gridReachableReturn;
  }

  public gridDistanceField(field: GridField, start: Cell): readonly number[] {
    this.gridDistanceCalls.push({ field, start });
    return this.gridDistanceReturn;
  }

  public gridStepToward(field: GridField, from: Cell, target: Cell): Cell {
    this.gridStepCalls.push({ field, from, target });
    return this.gridStepReturn;
  }

  // --- 3D scene authoring (records the marshalled descriptor, mints a handle/entity) ---
  public createMesh(meshKind: number): Handle {
    this.meshKinds.push(meshKind);
    return this.mint();
  }

  public createMeshData(data: MeshDataDescriptor): Handle {
    this.meshDatas.push(data);
    return this.mint();
  }

  public createMaterial(material: MaterialDescriptor): Handle {
    this.materials.push(material);
    return this.mint();
  }

  public setCamera3D(camera: CameraDescriptor): void {
    this.cameras.push(camera);
  }

  public addLight(light: LightDescriptor): Entity {
    this.lights.push(light);
    return this.mint();
  }

  public spawnRenderable(mesh: Handle, material: Handle, transform: Transform): Entity {
    this.spawns.push({ material, mesh, transform });
    return this.mint();
  }

  public setNodeTransform(entity: Entity, transform: Transform): void {
    this.nodeTransforms.push({ entity, transform });
  }

  public setNodeBounds(entity: Entity, halfExtents: Vec3): void {
    this.nodeBounds.push({ entity, halfExtents });
  }

  public clearScene(): void {
    this.sceneClears += 1;
  }

  public createController(spec: ControllerSpec, index: number): Entity {
    this.controllers.push({ index, spec });
    return this.mint();
  }

  public controlFirstPerson(input: ControllerInput): void {
    this.controls.push(input);
  }

  // --- 3D math (deterministic, input-derived returns: the projection only forwards) ---
  public v3Add(lhs: Vec3, rhs: Vec3): Vec3 {
    return { x: lhs.x + rhs.x, y: lhs.y + rhs.y, z: lhs.z + rhs.z };
  }

  public v3Sub(lhs: Vec3, rhs: Vec3): Vec3 {
    return { x: lhs.x - rhs.x, y: lhs.y - rhs.y, z: lhs.z - rhs.z };
  }

  public v3Scale(vector: Vec3, scalar: number): Vec3 {
    return { x: vector.x * scalar, y: vector.y * scalar, z: vector.z * scalar };
  }

  public v3Dot(lhs: Vec3, rhs: Vec3): number {
    return lhs.x * rhs.x + lhs.y * rhs.y + lhs.z * rhs.z;
  }

  public v3Cross(lhs: Vec3, rhs: Vec3): Vec3 {
    return {
      x: lhs.y * rhs.z - lhs.z * rhs.y,
      y: lhs.z * rhs.x - lhs.x * rhs.z,
      z: lhs.x * rhs.y - lhs.y * rhs.x,
    };
  }

  public v3Len(vector: Vec3): number {
    return Math.hypot(vector.x, vector.y, vector.z);
  }

  public v3Normalize(vector: Vec3): Vec3 {
    const length = Math.hypot(vector.x, vector.y, vector.z);
    return { x: vector.x / length, y: vector.y / length, z: vector.z / length };
  }

  public v3Dist(lhs: Vec3, rhs: Vec3): number {
    return Math.hypot(lhs.x - rhs.x, lhs.y - rhs.y, lhs.z - rhs.z);
  }

  public v3Lerp(lhs: Vec3, rhs: Vec3, fraction: number): Vec3 {
    return {
      x: lhs.x + (rhs.x - lhs.x) * fraction,
      y: lhs.y + (rhs.y - lhs.y) * fraction,
      z: lhs.z + (rhs.z - lhs.z) * fraction,
    };
  }

  public mat4Identity(): Mat4 {
    return [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];
  }

  // Elementwise sum: a deterministic function of BOTH operands (proves forwarding).
  public mat4Multiply(lhs: Mat4, rhs: Mat4): Mat4 {
    return lhs.map((value, index) => value + (rhs[index] ?? 0));
  }

  public mat4Perspective(spec: PerspectiveSpec): Mat4 {
    return [spec.fovY, spec.aspect, spec.near, spec.far, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
  }

  public mat4LookAt(eye: Vec3, target: Vec3, up: Vec3): Mat4 {
    return [eye.x, eye.y, eye.z, target.x, target.y, target.z, up.x, up.y, up.z, 0, 0, 0, 0, 0, 0, 0];
  }

  public mat4Invert(matrix: Mat4): Mat4 {
    return matrix.map((value) => -value);
  }

  public mat4FromTRS(translation: Vec3, rotation: Quat, scale: Vec3): Mat4 {
    return [
      translation.x,
      translation.y,
      translation.z,
      rotation[0],
      rotation[1],
      rotation[2],
      rotation[3],
      scale.x,
      scale.y,
      scale.z,
      0,
      0,
      0,
      0,
      0,
      0,
    ];
  }

  public quatIdentity(): Quat {
    return [0, 0, 0, 1];
  }

  public quatFromEuler(pitch: number, yaw: number, roll: number): Quat {
    return [pitch, yaw, roll, 0];
  }

  public quatMultiply(lhs: Quat, rhs: Quat): Quat {
    return [lhs[0] * rhs[0], lhs[1] * rhs[1], lhs[2] * rhs[2], lhs[3] * rhs[3]];
  }

  public quatNormalize(quaternion: Quat): Quat {
    return [quaternion[0], quaternion[1], quaternion[2], quaternion[3]];
  }

  public quatToMat4(quaternion: Quat): Mat4 {
    return [
      quaternion[0],
      quaternion[1],
      quaternion[2],
      quaternion[3],
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
    ];
  }

  // --- 2D drawing (records the marshalled call; mints a handle for the id-returning verbs) ---
  public draw2dCamera2d(center: Vec2, zoom: number): void {
    this.draw2dCameras.push({ center, zoom });
  }

  public draw2dRect(bounds: Rect, style: ShapeStyle): void {
    this.draw2dRects.push({ bounds, style });
  }

  public draw2dCircle(center: Vec2, radius: number, style: ShapeStyle): void {
    this.draw2dCircles.push({ center, radius, style });
  }

  public draw2dEllipse(center: Vec2, radii: EllipseRadii, style: ShapeStyle): void {
    this.draw2dEllipses.push({ center, radii, style });
  }

  public draw2dLine(from: Vec2, to: Vec2, style: LineStyle): void {
    this.draw2dLines.push({ from, style, to });
  }

  public draw2dCreateEmitter(config: EmitterConfig): Handle {
    this.draw2dEmitters.push(config);
    return this.mint();
  }

  public draw2dEmit(id: Handle, at: Vec2, direction: Vec2): void {
    this.draw2dEmits.push({ at, direction, id });
  }

  public draw2dAdvanceParticles(dtSeconds: number): void {
    this.draw2dAdvances.push(dtSeconds);
  }

  public draw2dCreateRenderTarget(width: number, height: number): Handle {
    this.draw2dTargets.push({ height, width });
    return this.mint();
  }

  public draw2dBeginTarget(target: Handle): void {
    this.draw2dBegins.push(target);
  }

  public draw2dEndTarget(): void {
    this.draw2dEnds += 1;
  }

  public draw2dTargetTexture(target: Handle): Handle {
    return target;
  }

  public draw2dFinish(): readonly number[] {
    return this.draw2dFinishReturn;
  }

  public draw2dSprite(texture: Handle, opts: SpriteOpts): void {
    this.draw2dSpriteCalls.push({ opts, texture });
  }

  public draw2dText(value: string, opts: TextOpts): void {
    this.draw2dTextCalls.push({ opts, value });
  }

  public draw2dMeasureText(value: string, font: FontSpec): TextMetrics {
    this.measureTextCalls.push({ font, value });
    return this.measureTextReturn;
  }

  public loadTexture(url: string): Handle {
    this.loadedTextureUrls.push(url);
    return this.mint();
  }

  public loadFont(url: string): FontSpec {
    this.loadedFontUrls.push(url);
    return this.fontReturn;
  }

  // The reference flip-book sampler the native Draw2dApi::sample_animation owns:
  // index = floor(elapsed * fps), wrapped (rem_euclid) when looping else clamped
  // to the last frame; an empty book samples the inert zero-rect. Records the call
  // so a test can assert the facade forwarded the RESOLVED loop flag.
  public draw2dSampleAnimation(anim: SpriteAnimation, elapsedSeconds: number, looping: boolean): Rect {
    this.draw2dSamples.push({ anim, elapsedSeconds, looping });
    const count = Math.max(anim.frames.length, 1);
    const index = Math.floor(elapsedSeconds * anim.fps);
    const wrapped = ((index % count) + count) % count;
    const clamped = Math.min(Math.max(index, 0), count - 1);
    const chosen = [clamped, wrapped][Number(looping)] ?? 0;
    return anim.frames[chosen] ?? { height: 0, width: 0, x: 0, y: 0 };
  }

  // --- UI surface (records the marshalled call; returns the scripted value for the read-back verbs) ---
  public uiBeginFrame(viewport: UiViewport, pointer: Vec2, pressed: boolean): void {
    this.uiBeginFrames.push({ pointer, pressed, viewport });
  }

  public uiRect(bounds: Rect, style: UiStyle): void {
    this.uiRects.push({ bounds, style });
  }

  public uiText(value: string, opts: UiTextOpts): void {
    this.uiTexts.push({ opts, value });
  }

  public uiSprite(texture: Handle, bounds: Rect): void {
    this.uiSprites.push({ bounds, texture });
  }

  // Faithfully model the native `UiSurface::button` truth table: a button is
  // activated this frame iff the latest `beginFrame` pointer was inside `bounds`
  // on its press edge (`bounds.contains(pointer) & pressed_edge`). Before any
  // begin-frame the pointer is the inert origin with no press edge.
  public uiButton(bounds: Rect, label: string, style: UiStyle): boolean {
    this.uiButtons.push({ bounds, label, style });
    const frames = this.uiBeginFrames;
    if (frames.length === 0) {
      return false;
    }
    const frame = frames[frames.length - 1]!;
    return this.pointInRect(frame.pointer, bounds) && frame.pressed;
  }

  public uiViewport(): UiViewport {
    return this.uiViewportReturn;
  }

  public uiDrawList(): Uint8Array {
    return this.uiDrawListReturn;
  }

  public uiSolveLayout(viewport: UiViewport, nodes: readonly number[]): readonly number[] {
    this.uiSolveLayoutCalls.push({ nodes, viewport });
    return this.uiSolveLayoutReturn;
  }

  private mint(): Handle {
    const id = this.nextHandle;
    this.nextHandle += 1;
    return id;
  }
}
