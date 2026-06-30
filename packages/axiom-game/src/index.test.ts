import assert from "node:assert/strict";
import { test } from "node:test";

import * as game from "./index.ts";

// The public authoring surface (SPEC-00 §4.2 / SPEC-14). This test pins every
// documented runtime export of the barrel: its presence and its kind. Type-only
// re-exports are erased at runtime and are verified by vocabulary.test.ts /
// native-bridge.test.ts instead.

const FUNCTION_EXPORTS = [
  // game lifecycle + registry
  "createGame",
  "GameLoop",
  "onFixedUpdate",
  "onRender",
  "GameRegistry",
  "activeRegistry",
  "useRegistry",
  // scene + loop core
  "mountScene",
  "stepFrame",
  "makeFrame",
  "makeSim",
  "interpolationAlpha",
  "Scene",
  // ui + layout
  "makeUi",
  "solveLayout",
  // rng / world / input
  "makeRng",
  "StreamRng",
  "makeWorld",
  "BridgeWorld",
  "makeInput",
  "SnapshotInput",
  "bindAction",
  // math + query
  "aabbOverlap",
  "circleOverlap",
  "clamp",
  "lerp",
  "normalizeAngle",
  "pointInRect",
  "overlapBox",
  "overlapCircle",
  "raycast",
  // host
  "getSessionConfig",
  "notifyReady",
  "reportOutcome",
  "reportOutcomes",
  "bindNative",
  // time / state machine / tweens / pump
  "makeTime",
  "BridgeStateMachine",
  "makeTweens",
  "TickPump",
  // game objects / physics
  "makeAdd",
  "GameObject",
  "makePhysics",
  // sound
  "loadSound",
  "playMusic",
  "playSound",
  "playTone",
  "scheduleSound",
  "setMasterVolume",
  "setMuted",
  "stopVoice",
  // 2d flip-book sampler (SPEC-04 §10.2)
  "sampleAnimation",
  // control flow vocabulary + sample
  "orElse",
  "whenPresent",
  "Arena",
  // grid / path
  "BridgeGrid",
  "createGrid",
  "gridDistanceField",
  "gridPath",
  "gridReachable",
  "stepToward",
  "tileSpace",
  // 3d scene
  "addLight",
  "createMaterial",
  "createMesh",
  "createMeshData",
  "setCamera3D",
  // netcode
  "bindNetTransport",
  "boundNetConfig",
  "boundNetRestore",
  "boundNetSnapshot",
  "configureNet",
  "joinRoom",
  "makeNetSim",
  "onRestore",
  "onSnapshot",
  "bindMatchmaker",
  "bindRoomHost",
  "hostRoom",
  "matchmake",
  "makeNetParticipants",
  "makeSnapshotIntake",
  "reconstructSnapshot",
  "axiomNetFactory",
  "decodeIntent",
  "encodeIntent",
  "netTransportFromClient",
] as const satisfies readonly (keyof typeof game)[];

const OBJECT_EXPORTS = ["EASES", "mat4", "quat", "v2", "v3"] as const satisfies readonly (keyof typeof game)[];

test("every documented function/class export is present and callable", () => {
  for (const name of FUNCTION_EXPORTS) {
    assert.equal(typeof game[name], "function", `${name} should be a function`);
  }
});

test("the math namespaces and the eases table are exported as objects", () => {
  for (const name of OBJECT_EXPORTS) {
    assert.equal(typeof game[name], "object", `${name} should be an object`);
  }
});

test("ROOT_STREAM is exported as the numeric root stream id", () => {
  assert.equal(typeof game.ROOT_STREAM, "number");
  assert.equal(game.ROOT_STREAM, 0);
});
