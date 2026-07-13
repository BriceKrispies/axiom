/*
 * api.test.ts — the co-located test for the contract module. api.ts is pure type
 * vocabulary (no runtime code), so this exists to place api.ts in the test graph
 * (the co-location gate) and to pin the shapes: a value constructed against each
 * public type must type-check and round-trip through a plain read. If a field is
 * renamed or dropped, this stops compiling.
 */

import { strict as assert } from "node:assert";
import { test } from "node:test";
import type { Camera3D, Light, MaterialSpec, MeshData, Rgba, ToneSpec, Transform } from "./api.ts";

test("contract shapes construct and read back", () => {
  const color: Rgba = [1, 0.5, 0.25, 1];
  const transform: Transform = {
    position: { x: 1, y: 2, z: 3 },
    rotation: [0, 0, 0, 1],
    scale: { x: 1, y: 1, z: 1 },
  };
  const mesh: MeshData = {
    positions: [{ x: 0, y: 0, z: 0 }],
    normals: [{ x: 0, y: 1, z: 0 }],
    indices: [0],
  };
  const material: MaterialSpec = { baseColor: color, emissive: color, roughness: 0.5, opacity: 1 };
  const light: Light = { kind: "directional", direction: { x: 0, y: -1, z: 0 }, color, intensity: 1 };
  const camera: Camera3D = {
    position: { x: 0, y: 0, z: 5 },
    target: { x: 0, y: 0, z: 0 },
    fovY: Math.PI / 3,
    near: 0.1,
    far: 100,
  };
  const tone: ToneSpec = { wave: "sine", freq: 440, duration: 0.1 };

  assert.equal(transform.position.x, 1);
  assert.equal(mesh.indices[0], 0);
  assert.equal(material.baseColor[3], 1);
  assert.equal(light.kind, "directional");
  assert.equal(camera.far, 100);
  assert.equal(tone.wave, "sine");
});
