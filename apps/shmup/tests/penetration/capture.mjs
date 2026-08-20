// Golden capture script for penetration.js
// Run with: node apps/shmup/tests/penetration/capture.mjs
// Writes golden.json to the same directory

import { StaticWorld } from 'file:///C:/dev/Claude-of-Duty/src/physics/bvh.js';
import { Ballistics } from 'file:///C:/dev/Claude-of-Duty/src/physics/penetration.js';
import { SURFACE, LAYER, MASK } from 'file:///C:/dev/Claude-of-Duty/src/physics/surfaces.js';
import { makeHitRecord } from 'file:///C:/dev/Claude-of-Duty/src/physics/math.js';
import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

/**
 * Wrapper that mimics PhysicsSystem.raycast using StaticWorld directly.
 * The penetration.js expects a hit object with:
 * - hit: boolean
 * - point: {x, y, z}
 * - normal: {x, y, z}
 * - surfaceIndex: number
 * - distance: number
 * - frontFace: boolean
 * - object: number
 * - collider: null (not in static world)
 * - body: null (not in static world)
 * - ragdoll: null (not in static world)
 * - triangle: number
 */
class PhysicsWrapper {
    constructor(staticWorld) {
        this.staticWorld = staticWorld;
        this.raw = makeHitRecord();
    }

    raycast(ox, oy, oz, dx, dy, dz, maxDist, mask) {
        const l = Math.hypot(dx, dy, dz);
        if (l < 1e-9) {
            return {
                hit: false,
                point: { x: ox, y: oy, z: oz },
                normal: { x: 0, y: 1, z: 0 },
                surfaceIndex: 0,
                distance: 0,
                frontFace: true,
                object: -1,
                collider: null,
                body: null,
                ragdoll: null,
                triangle: -1,
            };
        }
        dx /= l; dy /= l; dz /= l;

        const hit = this.staticWorld.raycast(ox, oy, oz, dx, dy, dz, maxDist, mask, this.raw, -1);
        if (!hit) {
            return {
                hit: false,
                point: { x: 0, y: 0, z: 0 },
                normal: { x: 0, y: 1, z: 0 },
                surfaceIndex: 0,
                distance: 0,
                frontFace: true,
                object: -1,
                collider: null,
                body: null,
                ragdoll: null,
                triangle: -1,
            };
        }

        // Convert StaticWorld hit record to penetration.js expected format
        // Note: raw is mutated by raycast
        return {
            hit: this.raw.hit,
            point: { x: this.raw.px, y: this.raw.py, z: this.raw.pz },
            normal: { x: this.raw.nx, y: this.raw.ny, z: this.raw.nz },
            surfaceIndex: this.raw.surface,
            distance: this.raw.t,
            frontFace: this.raw.frontFace,
            object: this.raw.object,
            collider: null,
            body: null,
            ragdoll: null,
            triangle: this.raw.tri,
        };
    }

    emitImpact() {
        // No-op for capture
    }
}


// --- verification harness: record world geometry into the golden ---
const __origAdd = StaticWorld.prototype.addTriangles;
StaticWorld.prototype.addTriangles = function (pos, count, surface, mask, name) {
    if (!this.__geo) this.__geo = [];
    this.__geo.push({ tris: Array.from(pos), count, surface, mask, name });
    return __origAdd.call(this, pos, count, surface, mask, name);
};

function buildTestWorld() {
    const world = new StaticWorld();
    
    // Floor: two triangles at y=0, spanning [-10,10] in x and z
    // CCW from above: (p00, p10, p11) then (p00, p11, p01)
    const floor = new Float32Array([
        -10.0, 0.0, 10.0,  10.0, 0.0, 10.0,  10.0, 0.0, -10.0,
        -10.0, 0.0, 10.0,  10.0, 0.0, -10.0,  -10.0, 0.0, -10.0,
    ]);
    world.addTriangles(floor, 2, SURFACE.concrete, LAYER.STATIC, "floor");
    
    // Wall: two triangles at x=2, spanning y=[0,3], z=[-10,10], facing -X
    const wall = new Float32Array([
        2.0, 0.0, 10.0,  2.0, 3.0, 10.0,  2.0, 3.0, -10.0,
        2.0, 0.0, 10.0,  2.0, 3.0, -10.0,  2.0, 0.0, -10.0,
    ]);
    world.addTriangles(wall, 2, SURFACE.metal, LAYER.STATIC, "wall");
    
    world.build();
    world.__tag = 'main';
    return world;
}

function buildMultiLayerWorld() {
    const world = new StaticWorld();
    // 10 plaster layers at y = 0, 0.5, 1.0, ..., 4.5
    for (let i = 0; i < 10; i++) {
        const y = i * 0.5;
        const layer = new Float32Array([
            -1.0, y, 1.0,  1.0, y, 1.0,  1.0, y, -1.0,
            -1.0, y, 1.0,  1.0, y, -1.0,  -1.0, y, -1.0,
        ]);
        world.addTriangles(layer, 2, SURFACE.plaster, LAYER.STATIC, `layer${i}`);
    }
    world.build();
    world.__tag = 'layers';
    return world;
}

function runTest(name, world, params) {
    const phys = new PhysicsWrapper(world);
    const bal = new Ballistics(phys);
    const count = bal.fire(params);
    const impacts = bal.impacts.slice(0, count).map(imp => ({
        point: [imp.point.x, imp.point.y, imp.point.z],
        normal: [imp.normal.x, imp.normal.y, imp.normal.z],
        surface: imp.surface,
        exit: imp.exit,
        damage: imp.damage,
        distance: imp.distance,
        object: imp.object,
    }));
    return { name, geo: world.__geo, params, count, impacts };
}

function main() {
    const world = buildTestWorld();
    const multiLayerWorld = buildMultiLayerWorld();
    
    const results = [];
    
    // Test 1: Bullet stops in first layer (concrete, shallow angle)
    results.push(runTest("stops_in_first_layer_concrete_shallow", world, {
        origin: { x: 0.0, y: 5.0, z: 0.0 },
        dir: { x: 0.0, y: 0.01, z: 1.0 },
        maxDist: 10.0,
        damage: 34.0,
        penetration: 1.0,
        mask: MASK.BULLET,
        dropoff: 0.55,
        rng: null,
        emit: false,
    }));
    
    // Test 2: Bullet penetrates concrete floor then hits wall
    results.push(runTest("penetrates_concrete_then_hits_wall", world, {
        origin: { x: 2.0, y: 5.0, z: 0.0 },
        dir: { x: 0.0, y: -1.0, z: 0.0 },
        maxDist: 20.0,
        damage: 34.0,
        penetration: 1.0,
        mask: MASK.BULLET,
        dropoff: 0.55,
        rng: null,
        emit: false,
    }));
    
    // Test 3: Bullet exhausts 6-layer cap (plaster, .50 AP)
    results.push(runTest("exhausts_six_layer_cap_plaster_ap", multiLayerWorld, {
        origin: { x: 0.0, y: 10.0, z: 0.0 },
        dir: { x: 0.0, y: -1.0, z: 0.0 },
        maxDist: 20.0,
        damage: 34.0,
        penetration: 2.2,
        mask: MASK.BULLET,
        dropoff: 0.55,
        rng: null,
        emit: false,
    }));
    
    // Test 4: Grazing hit (large thickness stops round)
    results.push(runTest("grazing_hit_stops_round", world, {
        origin: { x: 0.0, y: 5.0, z: 0.0 },
        dir: { x: 0.0, y: 0.001, z: 1.0 },
        maxDist: 10.0,
        damage: 34.0,
        penetration: 1.0,
        mask: MASK.BULLET,
        dropoff: 0.55,
        rng: null,
        emit: false,
    }));
    
    // Test 5: Damage bleed curve - straight down through concrete
    results.push(runTest("damage_bleed_concrete_straight", world, {
        origin: { x: 2.0, y: 5.0, z: 0.0 },
        dir: { x: 0.0, y: -1.0, z: 0.0 },
        maxDist: 20.0,
        damage: 34.0,
        penetration: 1.0,
        mask: MASK.BULLET,
        dropoff: 0.55,
        rng: null,
        emit: false,
    }));
    
    // Test 6: Metal penetration
    results.push(runTest("metal_penetration", world, {
        origin: { x: 0.0, y: 1.5, z: 0.0 },
        dir: { x: 1.0, y: 0.0, z: 0.0 },
        maxDist: 10.0,
        damage: 34.0,
        penetration: 1.0,
        mask: MASK.BULLET,
        dropoff: 0.55,
        rng: null,
        emit: false,
    }));
    
    // Test 7: Plaster penetration (high pen_depth)
    const plasterWorld = new StaticWorld();
    const plaster = new Float32Array([
        -5.0, 0.0, 5.0,  5.0, 0.0, 5.0,  5.0, 0.0, -5.0,
        -5.0, 0.0, 5.0,  5.0, 0.0, -5.0,  -5.0, 0.0, -5.0,
    ]);
    plasterWorld.addTriangles(plaster, 2, SURFACE.plaster, LAYER.STATIC, "plaster");
    plasterWorld.build();
    
    results.push(runTest("plaster_penetration_high_power", plasterWorld, {
        origin: { x: 0.0, y: 5.0, z: 0.0 },
        dir: { x: 0.0, y: -1.0, z: 0.0 },
        maxDist: 20.0,
        damage: 34.0,
        penetration: 2.2,
        mask: MASK.BULLET,
        dropoff: 0.55,
        rng: null,
        emit: false,
    }));
    
    // Test 8: Flesh penetration (organic, low energy_loss)
    const fleshWorld = new StaticWorld();
    const flesh = new Float32Array([
        -5.0, 0.0, 5.0,  5.0, 0.0, 5.0,  5.0, 0.0, -5.0,
        -5.0, 0.0, 5.0,  5.0, 0.0, -5.0,  -5.0, 0.0, -5.0,
    ]);
    fleshWorld.addTriangles(flesh, 2, SURFACE.flesh, LAYER.STATIC, "flesh");
    fleshWorld.build();
    
    results.push(runTest("flesh_penetration", fleshWorld, {
        origin: { x: 0.0, y: 5.0, z: 0.0 },
        dir: { x: 0.0, y: -1.0, z: 0.0 },
        maxDist: 20.0,
        damage: 34.0,
        penetration: 1.0,
        mask: MASK.BULLET,
        dropoff: 0.55,
        rng: null,
        emit: false,
    }));
    
    // Test 9: Wood penetration
    const woodWorld = new StaticWorld();
    const wood = new Float32Array([
        -5.0, 0.0, 5.0,  5.0, 0.0, 5.0,  5.0, 0.0, -5.0,
        -5.0, 0.0, 5.0,  5.0, 0.0, -5.0,  -5.0, 0.0, -5.0,
    ]);
    woodWorld.addTriangles(wood, 2, SURFACE.wood, LAYER.STATIC, "wood");
    woodWorld.build();
    
    results.push(runTest("wood_penetration", woodWorld, {
        origin: { x: 0.0, y: 5.0, z: 0.0 },
        dir: { x: 0.0, y: -1.0, z: 0.0 },
        maxDist: 20.0,
        damage: 34.0,
        penetration: 1.0,
        mask: MASK.BULLET,
        dropoff: 0.55,
        rng: null,
        emit: false,
    }));
    
    // Test 10: Glass penetration (shatters)
    const glassWorld = new StaticWorld();
    const glass = new Float32Array([
        2.0, 0.0, 10.0,  2.0, 3.0, 10.0,  2.0, 3.0, -10.0,
        2.0, 0.0, 10.0,  2.0, 3.0, -10.0,  2.0, 0.0, -10.0,
    ]);
    glassWorld.addTriangles(glass, 2, SURFACE.glass, LAYER.STATIC, "glass");
    glassWorld.build();
    
    results.push(runTest("glass_penetration", glassWorld, {
        origin: { x: 0.0, y: 1.5, z: 0.0 },
        dir: { x: 1.0, y: 0.0, z: 0.0 },
        maxDist: 10.0,
        damage: 34.0,
        penetration: 1.0,
        mask: MASK.BULLET,
        dropoff: 0.55,
        rng: null,
        emit: false,
    }));
    
    // Write golden.json
    const output = {
        captureDate: new Date().toISOString(),
        sourceFile: "src/physics/penetration.js",
        tests: results,
    };
    
    const scriptDir = path.dirname(fileURLToPath(import.meta.url));
    const outPath = path.join(scriptDir, 'golden.json');
    fs.writeFileSync(outPath, JSON.stringify(output, null, 2));
    console.log(`Written ${results.length} test cases to ${outPath}`);
}

main();