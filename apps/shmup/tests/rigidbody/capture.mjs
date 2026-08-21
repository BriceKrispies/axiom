// Golden capture script for rigidbody.js
// Run with: node apps/shmup/tests/rigidbody/capture.mjs
// Writes golden.json to the same directory

import { StaticWorld } from 'file:///C:/dev/Claude-of-Duty/src/physics/bvh.js';
import { RigidBody, RigidBodyWorld } from 'file:///C:/dev/Claude-of-Duty/src/physics/rigidbody.js';
import { SURFACE, LAYER, MASK } from 'file:///C:/dev/Claude-of-Duty/src/physics/surfaces.js';
import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

const __origAdd = StaticWorld.prototype.addTriangles;
StaticWorld.prototype.addTriangles = function (pos, count, surface, mask, name) {
    if (!this.__geo) this.__geo = [];
    this.__geo.push({ tris: Array.from(pos), count, surface, mask, name });
    return __origAdd.call(this, pos, count, surface, mask, name);
};

function buildFloorWorld() {
    const world = new StaticWorld();
    const floor = new Float32Array([
        -20.0, 0.0, 20.0,  20.0, 0.0, 20.0,  20.0, 0.0, -20.0,
        -20.0, 0.0, 20.0,  20.0, 0.0, -20.0,  -20.0, 0.0, -20.0,
    ]);
    world.addTriangles(floor, 2, SURFACE.concrete, LAYER.STATIC, "floor");
    world.build();
    world.__tag = 'floor';
    return world;
}

function buildFloorAndWallWorld() {
    const world = new StaticWorld();
    const floor = new Float32Array([
        -20.0, 0.0, 20.0,  20.0, 0.0, 20.0,  20.0, 0.0, -20.0,
        -20.0, 0.0, 20.0,  20.0, 0.0, -20.0,  -20.0, 0.0, -20.0,
    ]);
    world.addTriangles(floor, 2, SURFACE.concrete, LAYER.STATIC, "floor");
    const wall = new Float32Array([
        10.0, 0.0, 20.0,  10.0, 5.0, 20.0,  10.0, 5.0, -20.0,
        10.0, 0.0, 20.0,  10.0, 5.0, -20.0,  10.0, 0.0, -20.0,
    ]);
    world.addTriangles(wall, 2, SURFACE.metal, LAYER.STATIC, "wall");
    world.build();
    world.__tag = 'floor_wall';
    return world;
}

function buildStackWorld() {
    const world = new StaticWorld();
    const floor = new Float32Array([
        -20.0, 0.0, 20.0,  20.0, 0.0, 20.0,  20.0, 0.0, -20.0,
        -20.0, 0.0, 20.0,  20.0, 0.0, -20.0,  -20.0, 0.0, -20.0,
    ]);
    world.addTriangles(floor, 2, SURFACE.concrete, LAYER.STATIC, "floor");
    world.build();
    world.__tag = 'stack_floor';
    return world;
}

function recordBodyState(body) {
    return {
        id: body.id,
        shape: body.shape,
        // Construction parameters (needed to reconstruct the body)
        halfExtents: body.shape === 'box' ? { x: body.hx, y: body.hy, z: body.hz } : undefined,
        radius: body.shape === 'sphere' ? body.radius : (body.shape === 'capsule' ? body.radius : undefined),
        halfHeight: body.shape === 'capsule' ? body.halfHeight : undefined,
        mass: body.mass,
        restitution: body.restitution,
        friction: body.friction,
        linearDamping: body.linearDamping,
        angularDamping: body.angularDamping,
        gravityScale: body.gravityScale,
        ccd: body.ccd,
        lifetime: body.lifetime,
        // State
        position: [body.position.x, body.position.y, body.position.z],
        quaternion: [body.quaternion.x, body.quaternion.y, body.quaternion.z, body.quaternion.w],
        linearVelocity: [body.linearVelocity.x, body.linearVelocity.y, body.linearVelocity.z],
        angularVelocity: [body.angularVelocity.x, body.angularVelocity.y, body.angularVelocity.z],
        sleeping: body.sleeping,
        sleepTimer: body.sleepTimer,
    };
}

function runTest(name, world, setupBodies, steps) {
    const physWorld = new RigidBodyWorld(world, -20.6);
    const bodies = setupBodies(physWorld);
    
    const initialBodies = bodies.map(recordBodyState);
    const perStep = [];
    
    for (const step of steps) {
        physWorld.step(step.dt);
        perStep.push({
            dt: step.dt,
            bodies: bodies.map(recordBodyState),
        });
    }
    
    return {
        name,
        geo: world.__geo,
        initialBodies,
        steps: perStep,
    };
}

function main() {
    const floorWorld = buildFloorWorld();
    const floorWallWorld = buildFloorAndWallWorld();
    const stackWorld = buildStackWorld();
    
    const results = [];
    
    // Test 1: Box coming to rest on floor (gravity, contacts, sleep)
    results.push(runTest("box_rest_on_floor", floorWorld, (w) => {
        const box = new RigidBody({
            shape: 'box',
            halfExtents: { x: 0.5, y: 0.5, z: 0.5 },
            mass: 1.0,
            position: { x: 0, y: 5, z: 0 },
            restitution: 0.2,
            friction: 0.6,
            ccd: false,
        });
        w.add(box);
        return [box];
    }, [
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
    ]));
    
    // Test 2: Bouncing ball (restitution)
    results.push(runTest("sphere_bounce_restitution", floorWorld, (w) => {
        const sphere = new RigidBody({
            shape: 'sphere',
            radius: 0.3,
            mass: 0.5,
            position: { x: 0, y: 3, z: 0 },
            restitution: 0.8,
            friction: 0.1,
            ccd: false,
        });
        w.add(sphere);
        return [sphere];
    }, [
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
    ]));
    
    // Test 3: Sliding box (friction)
    results.push(runTest("box_slide_friction", floorWorld, (w) => {
        const box = new RigidBody({
            shape: 'box',
            halfExtents: { x: 0.5, y: 0.25, z: 0.5 },
            mass: 2.0,
            position: { x: 0, y: 0.5, z: 0 },
            linearVelocity: { x: 5, y: 0, z: 0 },
            restitution: 0.1,
            friction: 0.8,
            ccd: false,
        });
        w.add(box);
        return [box];
    }, [
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
    ]));
    
    // Test 4: Fast body with CCD (tunneling prevention)
    results.push(runTest("fast_box_ccd_prevents_tunnel", floorWallWorld, (w) => {
        const box = new RigidBody({
            shape: 'box',
            halfExtents: { x: 0.2, y: 0.2, z: 0.2 },
            mass: 0.1,
            position: { x: -5, y: 1, z: 0 },
            linearVelocity: { x: 50, y: 0, z: 0 },
            restitution: 0.3,
            friction: 0.2,
            ccd: true,
        });
        w.add(box);
        return [box];
    }, [
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
    ]));
    
    // Test 5: Body going to sleep
    results.push(runTest("box_sleeps_after_resting", floorWorld, (w) => {
        const box = new RigidBody({
            shape: 'box',
            halfExtents: { x: 0.4, y: 0.4, z: 0.4 },
            mass: 1.0,
            position: { x: 2, y: 3, z: 0 },
            restitution: 0.1,
            friction: 0.9,
            ccd: false,
        });
        w.add(box);
        return [box];
    }, [
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
    ]));
    
    // Test 6: Stack of boxes settling
    results.push(runTest("stack_of_boxes_settling", stackWorld, (w) => {
        const boxes = [];
        for (let i = 0; i < 4; i++) {
            const box = new RigidBody({
                shape: 'box',
                halfExtents: { x: 0.5, y: 0.5, z: 0.5 },
                mass: 1.0,
                position: { x: 0, y: 1.5 + i * 1.1, z: 0 },
                restitution: 0.1,
                friction: 0.8,
                ccd: false,
            });
            w.add(box);
            boxes.push(box);
        }
        return boxes;
    }, [
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
    ]));
    
    // Test 7: Capsule rolling
    results.push(runTest("capsule_rolls_on_floor", floorWorld, (w) => {
        const capsule = new RigidBody({
            shape: 'capsule',
            radius: 0.3,
            halfHeight: 0.5,
            mass: 1.5,
            position: { x: -3, y: 1.5, z: 0 },
            linearVelocity: { x: 3, y: 0, z: 0 },
            angularVelocity: { x: 0, y: 0, z: -10 },
            restitution: 0.15,
            friction: 0.7,
            ccd: false,
        });
        w.add(capsule);
        return [capsule];
    }, [
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
    ]));
    
    // Test 8: Multiple bodies with different masses
    results.push(runTest("different_masses_fall_same_rate", floorWorld, (w) => {
        const light = new RigidBody({
            shape: 'box',
            halfExtents: { x: 0.3, y: 0.3, z: 0.3 },
            mass: 0.1,
            position: { x: -2, y: 4, z: 0 },
            restitution: 0.0,
            friction: 0.9,
            ccd: false,
        });
        const heavy = new RigidBody({
            shape: 'box',
            halfExtents: { x: 0.3, y: 0.3, z: 0.3 },
            mass: 10.0,
            position: { x: 2, y: 4, z: 0 },
            restitution: 0.0,
            friction: 0.9,
            ccd: false,
        });
        w.add(light);
        w.add(heavy);
        return [light, heavy];
    }, [
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
    ]));
    
    // Test 9: Angular velocity integration
    results.push(runTest("spinning_box_angular_integration", floorWorld, (w) => {
        const box = new RigidBody({
            shape: 'box',
            halfExtents: { x: 0.5, y: 0.3, z: 0.8 },
            mass: 1.0,
            position: { x: 0, y: 3, z: 0 },
            angularVelocity: { x: 5, y: 3, z: 2 },
            restitution: 0.2,
            friction: 0.5,
            ccd: false,
        });
        w.add(box);
        return [box];
    }, [
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
    ]));
    
    // Test 10: Apply radial impulse (explosion)
    results.push(runTest("radial_impulse_scatter", floorWorld, (w) => {
        const boxes = [];
        for (let i = 0; i < 5; i++) {
            const angle = (i / 5) * Math.PI * 2;
            const box = new RigidBody({
                shape: 'box',
                halfExtents: { x: 0.2, y: 0.2, z: 0.2 },
                mass: 0.5,
                position: { x: Math.cos(angle) * 2, y: 2, z: Math.sin(angle) * 2 },
                restitution: 0.3,
                friction: 0.4,
                ccd: false,
            });
            w.add(box);
            boxes.push(box);
        }
        w.applyRadialImpulse(0, 2, 0, 5.0, 15.0);
        return boxes;
    }, [
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
        { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 }, { dt: 1/60 },
    ]));
    
    const output = {
        captureDate: new Date().toISOString(),
        sourceFile: "src/physics/rigidbody.js",
        tests: results,
    };
    
    const scriptDir = path.dirname(fileURLToPath(import.meta.url));
    const outPath = path.join(scriptDir, 'golden.json');
    fs.writeFileSync(outPath, JSON.stringify(output, null, 2));
    console.log(`Written ${results.length} test cases to ${outPath}`);
}

main();