/*
 * engine/renderer.ts — the WebGL2 forward renderer behind the retained-scene
 * contract in `api.ts`. A module-level singleton (`initRenderer` once, then the
 * free functions the game's `scene.ts` calls): meshes are uploaded into VAOs
 * (`createMesh` caches one geometry per primitive kind, `createMeshData` takes
 * custom triangle lists), materials are Lambert diffuse + additive emissive +
 * opacity, lights are directional/point (Σ Lambert with a soft 1/(1+0.08·d²)
 * point falloff plus a small constant ambient), and the camera is a look-at
 * perspective. `renderScene` clears to dark navy, draws opaque nodes first,
 * then translucent nodes (opacity < 1) back-to-front with blending on and
 * depth writes off. Face culling stays off — several generated/thin meshes are
 * seen from both sides, so the shader flips back-face normals instead. Model
 * matrices are recomputed per frame from stored transforms (< 500 nodes).
 */

import type { Camera3D, Entity, Handle, Light, MaterialSpec, MeshData, MeshKind, Transform } from "./api.ts";
import { type Mat4, fromTrs, lookAt, multiply, perspective } from "./mat4.ts";
import { unitBox, unitCylinderY, unitSphere } from "./meshes.ts";

// ── shader source ─────────────────────────────────────────────────────────────

const MAX_DIR_LIGHTS = 8;
const MAX_POINT_LIGHTS = 8;
const AMBIENT = 0.12;
/** Background clear color, ≈ #05060a dark navy. */
const CLEAR = [5 / 255, 6 / 255, 10 / 255] as const;

const VERT_SRC = `#version 300 es
layout(location = 0) in vec3 aPosition;
layout(location = 1) in vec3 aNormal;
uniform mat4 uModel;
uniform mat4 uViewProj;
out vec3 vNormal;
out vec3 vWorldPos;
void main() {
  vec4 world = uModel * vec4(aPosition, 1.0);
  vWorldPos = world.xyz;
  // Upper-3x3 of the model matrix; renormalized per-fragment (uniform enough
  // visually even under non-uniform scale).
  vNormal = mat3(uModel) * aNormal;
  gl_Position = uViewProj * world;
}
`;

const FRAG_SRC = `#version 300 es
precision highp float;
const int MAX_DIR = ${MAX_DIR_LIGHTS};
const int MAX_POINT = ${MAX_POINT_LIGHTS};
uniform int uDirCount;
uniform vec3 uDirDir[MAX_DIR];      // normalized travel direction of the light
uniform vec3 uDirColor[MAX_DIR];    // color * intensity
uniform int uPointCount;
uniform vec3 uPointPos[MAX_POINT];
uniform vec3 uPointColor[MAX_POINT]; // color * intensity
uniform vec4 uBaseColor;
uniform vec3 uEmissive;
uniform float uOpacity;
in vec3 vNormal;
in vec3 vWorldPos;
out vec4 outColor;
void main() {
  vec3 n = normalize(vNormal);
  // Culling is off; make back faces shade like front faces for thin meshes.
  n = gl_FrontFacing ? n : -n;
  vec3 lit = vec3(${AMBIENT});
  for (int i = 0; i < MAX_DIR; i++) {
    if (i >= uDirCount) { break; }
    lit += max(dot(n, -uDirDir[i]), 0.0) * uDirColor[i];
  }
  for (int i = 0; i < MAX_POINT; i++) {
    if (i >= uPointCount) { break; }
    vec3 toLight = uPointPos[i] - vWorldPos;
    float d = length(toLight);
    vec3 l = toLight / max(d, 1e-5);
    lit += (max(dot(n, l), 0.0) / (1.0 + 0.08 * d * d)) * uPointColor[i];
  }
  outColor = vec4(uBaseColor.rgb * lit + uEmissive, uOpacity);
}
`;

// ── internal state ────────────────────────────────────────────────────────────

interface GpuMesh {
  readonly vao: WebGLVertexArrayObject;
  readonly buffers: readonly WebGLBuffer[];
  readonly indexCount: number;
}

interface GpuMaterial {
  readonly baseColor: readonly [number, number, number, number];
  readonly emissive: readonly [number, number, number];
  readonly opacity: number;
}

interface SceneNode {
  readonly mesh: Handle;
  readonly material: Handle;
  transform: Transform;
}

interface DirLight {
  readonly direction: readonly [number, number, number];
  readonly color: readonly [number, number, number];
}

interface PointLight {
  readonly position: readonly [number, number, number];
  readonly color: readonly [number, number, number];
}

interface Uniforms {
  readonly model: WebGLUniformLocation;
  readonly viewProj: WebGLUniformLocation;
  readonly baseColor: WebGLUniformLocation;
  readonly emissive: WebGLUniformLocation;
  readonly opacity: WebGLUniformLocation;
  readonly dirCount: WebGLUniformLocation;
  readonly dirDir: WebGLUniformLocation;
  readonly dirColor: WebGLUniformLocation;
  readonly pointCount: WebGLUniformLocation;
  readonly pointPos: WebGLUniformLocation;
  readonly pointColor: WebGLUniformLocation;
}

interface RendererState {
  readonly canvas: HTMLCanvasElement;
  readonly gl: WebGL2RenderingContext;
  readonly program: WebGLProgram;
  readonly uniforms: Uniforms;
  readonly meshes: Map<Handle, GpuMesh>;
  readonly meshKindCache: Map<MeshKind, Handle>;
  readonly materials: Map<Handle, GpuMaterial>;
  readonly nodes: Map<Entity, SceneNode>;
  readonly dirLights: DirLight[];
  readonly pointLights: PointLight[];
  camera: Camera3D;
}

let state: RendererState | null = null;
let nextEntity: Entity = 1;
let nextHandle: Handle = 1;

const requireState = (): RendererState => {
  if (state === null) {
    throw new Error("renderer: initRenderer(canvas) must be called before any other renderer function");
  }
  return state;
};

// ── GL setup helpers ──────────────────────────────────────────────────────────

const compileShader = (gl: WebGL2RenderingContext, type: number, src: string): WebGLShader => {
  const shader = gl.createShader(type);
  if (shader === null) {
    throw new Error("renderer: createShader failed");
  }
  gl.shaderSource(shader, src);
  gl.compileShader(shader);
  if (gl.getShaderParameter(shader, gl.COMPILE_STATUS) !== true) {
    const log = gl.getShaderInfoLog(shader) ?? "(no log)";
    gl.deleteShader(shader);
    throw new Error(`renderer: shader compile failed: ${log}`);
  }
  return shader;
};

const linkProgram = (gl: WebGL2RenderingContext): WebGLProgram => {
  const program = gl.createProgram();
  if (program === null) {
    throw new Error("renderer: createProgram failed");
  }
  const vert = compileShader(gl, gl.VERTEX_SHADER, VERT_SRC);
  const frag = compileShader(gl, gl.FRAGMENT_SHADER, FRAG_SRC);
  gl.attachShader(program, vert);
  gl.attachShader(program, frag);
  gl.linkProgram(program);
  if (gl.getProgramParameter(program, gl.LINK_STATUS) !== true) {
    throw new Error(`renderer: program link failed: ${gl.getProgramInfoLog(program) ?? "(no log)"}`);
  }
  gl.deleteShader(vert);
  gl.deleteShader(frag);
  return program;
};

const uniform = (gl: WebGL2RenderingContext, program: WebGLProgram, name: string): WebGLUniformLocation => {
  const loc = gl.getUniformLocation(program, name);
  if (loc === null) {
    throw new Error(`renderer: uniform ${name} not found (optimized out?)`);
  }
  return loc;
};

// ── public API ────────────────────────────────────────────────────────────────

/** Initialize the singleton renderer on `canvas`: WebGL2 context, the one
 * Lambert program, depth test on, culling off. Throws if WebGL2 is unavailable. */
export const initRenderer = (canvas: HTMLCanvasElement): void => {
  const gl = canvas.getContext("webgl2", { antialias: true });
  if (gl === null) {
    throw new Error("renderer: WebGL2 is not available in this browser/canvas");
  }
  const program = linkProgram(gl);
  const uniforms: Uniforms = {
    model: uniform(gl, program, "uModel"),
    viewProj: uniform(gl, program, "uViewProj"),
    baseColor: uniform(gl, program, "uBaseColor"),
    emissive: uniform(gl, program, "uEmissive"),
    opacity: uniform(gl, program, "uOpacity"),
    dirCount: uniform(gl, program, "uDirCount"),
    dirDir: uniform(gl, program, "uDirDir"),
    dirColor: uniform(gl, program, "uDirColor"),
    pointCount: uniform(gl, program, "uPointCount"),
    pointPos: uniform(gl, program, "uPointPos"),
    pointColor: uniform(gl, program, "uPointColor"),
  };
  gl.enable(gl.DEPTH_TEST);
  gl.depthFunc(gl.LEQUAL);
  gl.disable(gl.CULL_FACE);
  gl.viewport(0, 0, canvas.width, canvas.height);
  state = {
    canvas,
    gl,
    program,
    uniforms,
    meshes: new Map(),
    meshKindCache: new Map(),
    materials: new Map(),
    nodes: new Map(),
    dirLights: [],
    pointLights: [],
    camera: {
      position: { x: 0, y: 2, z: 6 },
      target: { x: 0, y: 0, z: 0 },
      fovY: Math.PI / 3,
      near: 0.1,
      far: 200,
    },
  };
};

/** Resize the canvas backing store and the GL viewport. */
export const resizeRenderer = (width: number, height: number): void => {
  const st = requireState();
  st.canvas.width = Math.max(1, Math.floor(width));
  st.canvas.height = Math.max(1, Math.floor(height));
  st.gl.viewport(0, 0, st.canvas.width, st.canvas.height);
};

/** Upload custom triangle-list geometry into a VAO and return its handle. */
export const createMeshData = (data: MeshData): Handle => {
  const st = requireState();
  const { gl } = st;
  if (data.positions.length !== data.normals.length) {
    throw new Error(
      `renderer: createMeshData positions (${data.positions.length}) and normals (${data.normals.length}) differ`,
    );
  }
  const count = data.positions.length;
  const positions = new Float32Array(count * 3);
  const normals = new Float32Array(count * 3);
  for (let i = 0; i < count; i += 1) {
    const p = data.positions[i]!;
    const n = data.normals[i]!;
    positions[i * 3] = p.x;
    positions[i * 3 + 1] = p.y;
    positions[i * 3 + 2] = p.z;
    normals[i * 3] = n.x;
    normals[i * 3 + 1] = n.y;
    normals[i * 3 + 2] = n.z;
  }
  const indices = new Uint32Array(data.indices);

  const vao = gl.createVertexArray();
  if (vao === null) {
    throw new Error("renderer: createVertexArray failed");
  }
  gl.bindVertexArray(vao);

  const makeBuffer = (target: number, contents: Float32Array | Uint32Array): WebGLBuffer => {
    const buffer = gl.createBuffer();
    if (buffer === null) {
      throw new Error("renderer: createBuffer failed");
    }
    gl.bindBuffer(target, buffer);
    gl.bufferData(target, contents, gl.STATIC_DRAW);
    return buffer;
  };
  const posBuf = makeBuffer(gl.ARRAY_BUFFER, positions);
  gl.enableVertexAttribArray(0);
  gl.vertexAttribPointer(0, 3, gl.FLOAT, false, 0, 0);
  const nrmBuf = makeBuffer(gl.ARRAY_BUFFER, normals);
  gl.enableVertexAttribArray(1);
  gl.vertexAttribPointer(1, 3, gl.FLOAT, false, 0, 0);
  const idxBuf = makeBuffer(gl.ELEMENT_ARRAY_BUFFER, indices);
  gl.bindVertexArray(null);

  const handle = nextHandle;
  nextHandle += 1;
  st.meshes.set(handle, { vao, buffers: [posBuf, nrmBuf, idxBuf], indexCount: indices.length });
  return handle;
};

const KIND_BUILDERS: Record<MeshKind, () => MeshData> = {
  box: unitBox,
  sphere: () => unitSphere(),
  cylinder: () => unitCylinderY(),
};

/** Get (or lazily build + cache) the shared geometry for a primitive kind. */
export const createMesh = (kind: MeshKind): Handle => {
  const st = requireState();
  const cached = st.meshKindCache.get(kind);
  if (cached !== undefined) {
    return cached;
  }
  const handle = createMeshData(KIND_BUILDERS[kind]());
  st.meshKindCache.set(kind, handle);
  return handle;
};

/** Register a Lambert material (diffuse base + additive emissive + opacity). */
export const createMaterial = (spec: MaterialSpec): Handle => {
  const st = requireState();
  const emissive = spec.emissive ?? [0, 0, 0, 1];
  const handle = nextHandle;
  nextHandle += 1;
  st.materials.set(handle, {
    baseColor: [spec.baseColor[0], spec.baseColor[1], spec.baseColor[2], spec.baseColor[3]],
    emissive: [emissive[0], emissive[1], emissive[2]],
    opacity: spec.opacity ?? 1,
  });
  return handle;
};

/** Add a scene node drawing `mesh` with `material` at `transform`. */
export const spawnRenderable = (mesh: Handle, material: Handle, transform: Transform): Entity => {
  const st = requireState();
  if (!st.meshes.has(mesh)) {
    throw new Error(`renderer: spawnRenderable got unknown mesh handle ${mesh}`);
  }
  if (!st.materials.has(material)) {
    throw new Error(`renderer: spawnRenderable got unknown material handle ${material}`);
  }
  const entity = nextEntity;
  nextEntity += 1;
  st.nodes.set(entity, { mesh, material, transform });
  return entity;
};

/** Re-pose an existing node. */
export const setNodeTransform = (entity: Entity, t: Transform): void => {
  const node = requireState().nodes.get(entity);
  if (node === undefined) {
    throw new Error(`renderer: setNodeTransform got unknown entity ${entity}`);
  }
  node.transform = t;
};

/** Set the look-at perspective camera used by the next `renderScene`. */
export const setCamera3D = (cam: Camera3D): void => {
  requireState().camera = cam;
};

/** Add a directional or point light. Lights beyond the shader's capacity
 * (8 directional + 8 point) are accepted but do not contribute. */
export const addLight = (light: Light): Entity => {
  const st = requireState();
  const entity = nextEntity;
  nextEntity += 1;
  const color: readonly [number, number, number] = [
    light.color[0] * light.intensity,
    light.color[1] * light.intensity,
    light.color[2] * light.intensity,
  ];
  if (light.kind === "directional") {
    const d = light.direction;
    const len = Math.sqrt(d.x * d.x + d.y * d.y + d.z * d.z);
    const inv = len < 1e-9 ? 0 : 1 / len;
    if (st.dirLights.length < MAX_DIR_LIGHTS) {
      st.dirLights.push({ direction: [d.x * inv, (len < 1e-9 ? -1 : d.y * inv), d.z * inv], color });
    }
  } else if (st.pointLights.length < MAX_POINT_LIGHTS) {
    st.pointLights.push({ position: [light.position.x, light.position.y, light.position.z], color });
  }
  return entity;
};

/** Drop every node, light, mesh, and material (GPU buffers included). */
export const clearScene = (): void => {
  const st = requireState();
  const { gl } = st;
  for (const mesh of st.meshes.values()) {
    gl.deleteVertexArray(mesh.vao);
    for (const buffer of mesh.buffers) {
      gl.deleteBuffer(buffer);
    }
  }
  st.meshes.clear();
  st.meshKindCache.clear();
  st.materials.clear();
  st.nodes.clear();
  st.dirLights.length = 0;
  st.pointLights.length = 0;
};

// ── frame rendering ───────────────────────────────────────────────────────────

const packVec3s = (items: readonly (readonly [number, number, number])[]): Float32Array => {
  const out = new Float32Array(items.length * 3);
  items.forEach((item, i) => {
    out[i * 3] = item[0];
    out[i * 3 + 1] = item[1];
    out[i * 3 + 2] = item[2];
  });
  return out;
};

const drawNode = (st: RendererState, node: SceneNode): void => {
  const { gl, uniforms } = st;
  const mesh = st.meshes.get(node.mesh);
  const material = st.materials.get(node.material);
  if (mesh === undefined || material === undefined) {
    return; // dropped by clearScene between spawn and draw — nothing to render
  }
  const model: Mat4 = fromTrs(node.transform.position, node.transform.rotation, node.transform.scale);
  gl.uniformMatrix4fv(uniforms.model, false, model);
  gl.uniform4f(uniforms.baseColor, material.baseColor[0], material.baseColor[1], material.baseColor[2], material.baseColor[3]);
  gl.uniform3f(uniforms.emissive, material.emissive[0], material.emissive[1], material.emissive[2]);
  gl.uniform1f(uniforms.opacity, material.opacity);
  gl.bindVertexArray(mesh.vao);
  gl.drawElements(gl.TRIANGLES, mesh.indexCount, gl.UNSIGNED_INT, 0);
};

/** Clear and draw the retained scene: opaque nodes first, then translucent
 * nodes back-to-front with blending on and depth writes off. */
export const renderScene = (): void => {
  const st = requireState();
  const { gl, uniforms, camera } = st;

  gl.depthMask(true);
  gl.clearColor(CLEAR[0], CLEAR[1], CLEAR[2], 1);
  gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
  gl.useProgram(st.program);

  const aspect = st.canvas.width / Math.max(1, st.canvas.height);
  const proj = perspective(camera.fovY, aspect, camera.near, camera.far);
  const view = lookAt(camera.position, camera.target, { x: 0, y: 1, z: 0 });
  gl.uniformMatrix4fv(uniforms.viewProj, false, multiply(proj, view));

  gl.uniform1i(uniforms.dirCount, st.dirLights.length);
  if (st.dirLights.length > 0) {
    gl.uniform3fv(uniforms.dirDir, packVec3s(st.dirLights.map((l) => l.direction)));
    gl.uniform3fv(uniforms.dirColor, packVec3s(st.dirLights.map((l) => l.color)));
  }
  gl.uniform1i(uniforms.pointCount, st.pointLights.length);
  if (st.pointLights.length > 0) {
    gl.uniform3fv(uniforms.pointPos, packVec3s(st.pointLights.map((l) => l.position)));
    gl.uniform3fv(uniforms.pointColor, packVec3s(st.pointLights.map((l) => l.color)));
  }

  const opaque: SceneNode[] = [];
  const translucent: SceneNode[] = [];
  for (const node of st.nodes.values()) {
    const material = st.materials.get(node.material);
    ((material?.opacity ?? 1) < 1 ? translucent : opaque).push(node);
  }

  gl.disable(gl.BLEND);
  for (const node of opaque) {
    drawNode(st, node);
  }

  if (translucent.length > 0) {
    const eye = camera.position;
    const viewDist = (node: SceneNode): number => {
      const p = node.transform.position;
      const dx = p.x - eye.x;
      const dy = p.y - eye.y;
      const dz = p.z - eye.z;
      return dx * dx + dy * dy + dz * dz;
    };
    translucent.sort((a, b) => viewDist(b) - viewDist(a));
    gl.enable(gl.BLEND);
    gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);
    gl.depthMask(false);
    for (const node of translucent) {
      drawNode(st, node);
    }
    gl.depthMask(true);
    gl.disable(gl.BLEND);
  }

  gl.bindVertexArray(null);
};
