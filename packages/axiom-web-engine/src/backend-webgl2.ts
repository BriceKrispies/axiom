/*
 * backend-webgl2.ts — the hardware drawing backend: one Lambert forward
 * program over WebGL2. Meshes are uploaded into VAOs; a frame clears to the
 * shared dark-navy, draws opaque nodes first, then translucent nodes
 * (opacity < 1) back-to-front with blending on and depth writes off. Face
 * culling stays off — several generated/thin meshes are seen from both sides,
 * so the shader flips back-face normals instead. Model matrices are recomputed
 * per frame from stored transforms (< 500 nodes).
 */

import type { Handle, MeshData } from "./api.ts";
import { type FrameNode, type RenderBackend, type SceneFrame, AMBIENT, MAX_DIR_LIGHTS, MAX_POINT_LIGHTS } from "./backend.ts";
import { type Mat4, fromTrs, lookAt, multiply, perspective } from "./mat4.ts";

const VERT_SRC = `#version 300 es
layout(location = 0) in vec3 aPosition;
layout(location = 1) in vec3 aNormal;
layout(location = 2) in float aAo;
uniform mat4 uModel;
uniform mat4 uViewProj;
out vec3 vNormal;
out vec3 vWorldPos;
out float vAo;
void main() {
  vec4 world = uModel * vec4(aPosition, 1.0);
  vWorldPos = world.xyz;
  // Upper-3x3 of the model matrix; renormalized per-fragment (uniform enough
  // visually even under non-uniform scale).
  vNormal = mat3(uModel) * aNormal;
  vAo = aAo;
  gl_Position = uViewProj * world;
}
`;

// The GLSL twin of shading.ts: the diffuse Lambert term, a WHITE Blinn-Phong
// specular lobe + Schlick Fresnel rim (both driven by uRoughness and the eye
// vector), per-vertex AO on the diffuse+ambient term, and the highlight-rolloff
// tonemap on the final composite. Kept byte-matched to shadeSurface/tonemap; the
// constants (8.0/128.0 shininess, 0.04 F0, 0.5 gain, 5.0 power, 0.9 knee, 0.08
// falloff) mirror shading.ts and shading.test.ts pins the parity.
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
uniform float uRoughness;
uniform vec3 uEye;
in vec3 vNormal;
in vec3 vWorldPos;
in float vAo;
out vec4 outColor;
float tonemap1(float c) {
  float knee = 0.9;
  float low = min(c, knee);
  float excess = max(c - knee, 0.0) / (1.0 - knee);
  return low + (1.0 - knee) * (excess / (1.0 + excess));
}
void main() {
  vec3 n = normalize(vNormal);
  // Culling is off; make back faces shade like front faces for thin meshes.
  n = gl_FrontFacing ? n : -n;
  float gloss = clamp(1.0 - uRoughness, 0.0, 1.0);
  float shininess = 8.0 + gloss * (128.0 - 8.0);
  vec3 toEye = normalize(uEye - vWorldPos);
  vec3 diffuse = vec3(${AMBIENT});
  float ndv = max(dot(n, toEye), 0.0);
  float rim = (1.0 - 0.04) * pow(1.0 - ndv, 5.0) * gloss * 0.5;
  vec3 specular = vec3(rim);
  for (int i = 0; i < MAX_DIR; i++) {
    if (i >= uDirCount) { break; }
    vec3 toLight = -uDirDir[i];
    float ndl = dot(n, toLight);
    diffuse += max(ndl, 0.0) * uDirColor[i];
    float spec = pow(max(dot(n, normalize(toLight + toEye)), 0.0), shininess) * gloss * max(sign(ndl), 0.0);
    specular += spec * uDirColor[i];
  }
  for (int i = 0; i < MAX_POINT; i++) {
    if (i >= uPointCount) { break; }
    vec3 offset = uPointPos[i] - vWorldPos;
    float d = length(offset);
    float atten = 1.0 / (1.0 + 0.08 * d * d);
    vec3 l = offset / max(d, 1e-5);
    float ndl = dot(n, l);
    diffuse += (max(ndl, 0.0) / (1.0 + 0.08 * d * d)) * uPointColor[i];
    float spec = pow(max(dot(n, normalize(l + toEye)), 0.0), shininess) * gloss * max(sign(ndl), 0.0) * atten;
    specular += spec * uPointColor[i];
  }
  vec3 lit = diffuse * vAo * uBaseColor.rgb + specular + uEmissive;
  outColor = vec4(tonemap1(lit.r), tonemap1(lit.g), tonemap1(lit.b), uOpacity);
}
`;

interface GpuMesh {
  readonly vao: WebGLVertexArrayObject;
  readonly buffers: readonly WebGLBuffer[];
  readonly indexCount: number;
}

interface Uniforms {
  readonly model: WebGLUniformLocation;
  readonly viewProj: WebGLUniformLocation;
  readonly baseColor: WebGLUniformLocation;
  readonly emissive: WebGLUniformLocation;
  readonly opacity: WebGLUniformLocation;
  readonly roughness: WebGLUniformLocation;
  readonly eye: WebGLUniformLocation;
  readonly dirCount: WebGLUniformLocation;
  readonly dirDir: WebGLUniformLocation;
  readonly dirColor: WebGLUniformLocation;
  readonly pointCount: WebGLUniformLocation;
  readonly pointPos: WebGLUniformLocation;
  readonly pointColor: WebGLUniformLocation;
}

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

const packVec3s = (items: readonly (readonly [number, number, number])[]): Float32Array => {
  const out = new Float32Array(items.length * 3);
  items.forEach((item, i) => {
    out[i * 3] = item[0];
    out[i * 3 + 1] = item[1];
    out[i * 3 + 2] = item[2];
  });
  return out;
};

/** Create the WebGL2 backend, or return null when the context is unavailable. */
export const createWebGl2Backend = (canvas: HTMLCanvasElement): RenderBackend | null => {
  const gl = canvas.getContext("webgl2", { antialias: true });
  if (gl === null) {
    return null;
  }
  const program = linkProgram(gl);
  const uniforms: Uniforms = {
    model: uniform(gl, program, "uModel"),
    viewProj: uniform(gl, program, "uViewProj"),
    baseColor: uniform(gl, program, "uBaseColor"),
    emissive: uniform(gl, program, "uEmissive"),
    opacity: uniform(gl, program, "uOpacity"),
    roughness: uniform(gl, program, "uRoughness"),
    eye: uniform(gl, program, "uEye"),
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

  const meshes = new Map<Handle, GpuMesh>();

  const drawNode = (frame: SceneFrame, node: FrameNode): void => {
    const mesh = meshes.get(node.mesh);
    const material = frame.materials.get(node.material);
    if (mesh === undefined || material === undefined) {
      return; // dropped by clearScene between spawn and draw — nothing to render
    }
    const model: Mat4 = fromTrs(node.transform.position, node.transform.rotation, node.transform.scale);
    gl.uniformMatrix4fv(uniforms.model, false, model);
    gl.uniform4f(uniforms.baseColor, material.baseColor[0], material.baseColor[1], material.baseColor[2], material.baseColor[3]);
    gl.uniform3f(uniforms.emissive, material.emissive[0], material.emissive[1], material.emissive[2]);
    gl.uniform1f(uniforms.opacity, material.opacity);
    gl.uniform1f(uniforms.roughness, material.roughness);
    gl.bindVertexArray(mesh.vao);
    gl.drawElements(gl.TRIANGLES, mesh.indexCount, gl.UNSIGNED_INT, 0);
  };

  return {
    dropMeshes: (): void => {
      for (const mesh of meshes.values()) {
        gl.deleteVertexArray(mesh.vao);
        for (const buffer of mesh.buffers) {
          gl.deleteBuffer(buffer);
        }
      }
      meshes.clear();
    },
    meshDetail: "high",
    name: "WebGL2",
    render: (frame: SceneFrame): void => {
      gl.depthMask(true);
      gl.clearColor(frame.clearColor[0], frame.clearColor[1], frame.clearColor[2], 1);
      gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
      gl.useProgram(program);

      const aspect = canvas.width / Math.max(1, canvas.height);
      const proj = perspective(frame.camera.fovY, aspect, frame.camera.near, frame.camera.far);
      const view = lookAt(frame.camera.position, frame.camera.target, { x: 0, y: 1, z: 0 });
      gl.uniformMatrix4fv(uniforms.viewProj, false, multiply(proj, view));
      gl.uniform3f(uniforms.eye, frame.camera.position.x, frame.camera.position.y, frame.camera.position.z);

      gl.uniform1i(uniforms.dirCount, frame.dirLights.length);
      if (frame.dirLights.length > 0) {
        gl.uniform3fv(uniforms.dirDir, packVec3s(frame.dirLights.map((l) => l.direction)));
        gl.uniform3fv(uniforms.dirColor, packVec3s(frame.dirLights.map((l) => l.color)));
      }
      gl.uniform1i(uniforms.pointCount, frame.pointLights.length);
      if (frame.pointLights.length > 0) {
        gl.uniform3fv(uniforms.pointPos, packVec3s(frame.pointLights.map((l) => l.position)));
        gl.uniform3fv(uniforms.pointColor, packVec3s(frame.pointLights.map((l) => l.color)));
      }

      const opaque: FrameNode[] = [];
      const translucent: FrameNode[] = [];
      for (const node of frame.nodes) {
        const material = frame.materials.get(node.material);
        ((material?.opacity ?? 1) < 1 ? translucent : opaque).push(node);
      }

      gl.disable(gl.BLEND);
      for (const node of opaque) {
        drawNode(frame, node);
      }

      if (translucent.length > 0) {
        const eye = frame.camera.position;
        const viewDist = (node: FrameNode): number => {
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
          drawNode(frame, node);
        }
        gl.depthMask(true);
        gl.disable(gl.BLEND);
      }

      gl.bindVertexArray(null);
    },
    resize: (width: number, height: number): void => {
      gl.viewport(0, 0, width, height);
    },
    uploadMesh: (handle: Handle, data: MeshData): void => {
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
      // Per-vertex ambient occlusion: absent -> 1.0 everywhere (a no-op multiply).
      const ao = new Float32Array(count).fill(1);
      const aoSrc = data.ao;
      if (aoSrc !== undefined) {
        for (let i = 0; i < count; i += 1) {
          ao[i] = aoSrc[i] ?? 1;
        }
      }

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
      const aoBuf = makeBuffer(gl.ARRAY_BUFFER, ao);
      gl.enableVertexAttribArray(2);
      gl.vertexAttribPointer(2, 1, gl.FLOAT, false, 0, 0);
      const idxBuf = makeBuffer(gl.ELEMENT_ARRAY_BUFFER, indices);
      gl.bindVertexArray(null);

      meshes.set(handle, { vao, buffers: [posBuf, nrmBuf, aoBuf, idxBuf], indexCount: indices.length });
    },
  };
};
