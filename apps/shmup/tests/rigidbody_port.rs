//! Golden verification for `physics::rigidbody`, against captures taken from
//! the original `Claude-of-Duty/src/physics/rigidbody.js` running under Node.

use axiom_shmup::physics::bvh::StaticWorld;
use axiom_shmup::physics::rigidbody::{RigidBody, RigidBodyWorld, Shape};
use axiom_shmup::physics::surfaces::mask;
use axiom_shmup::world::palette::Surface;
use serde_json::Value;
use std::rc::Rc;

/// Build the exact world the capture built, from the geometry it recorded.
fn world_from(geo: &Value) -> Rc<StaticWorld> {
    let mut world = StaticWorld::new();
    for obj in geo.as_array().expect("geo array") {
        let tris: Vec<f64> = obj["tris"]
            .as_array()
            .expect("tris")
            .iter()
            .map(|v| v.as_f64().expect("number"))
            .collect();
        let idx = obj["surface"].as_u64().expect("surface index") as usize;
        world.add_triangles(
            &tris,
            obj["count"].as_u64().expect("count") as usize,
            Surface::ALL[idx],
            obj["mask"].as_u64().expect("mask") as u16,
            obj["name"].as_str().expect("name"),
        );
    }
    world.build();
    Rc::new(world)
}

fn shape_from_str(s: &str) -> Shape {
    match s {
        "sphere" => Shape::Sphere,
        "capsule" => Shape::Capsule,
        _ => Shape::Box,
    }
}

fn golden() -> Value {
    let raw = include_str!("rigidbody/golden.json");
    serde_json::from_str(raw).expect("golden.json parses")
}

fn f(v: &Value) -> f64 {
    v.as_f64().expect("number")
}

fn f_opt(v: &Value) -> Option<f64> {
    v.as_f64()
}

fn vec3(v: &Value) -> [f64; 3] {
    [f(&v[0]), f(&v[1]), f(&v[2])]
}

fn quat(v: &Value) -> [f64; 4] {
    [f(&v[0]), f(&v[1]), f(&v[2]), f(&v[3])]
}

/// Positions and velocities involve `+ - * /`, `sqrt`, `exp`, `sin`, `cos`.
/// Accumulated error over many steps can exceed 1e-9; we use 1e-6.
const TOL: f64 = 1e-6;

fn close(a: f64, b: f64, what: &str, case: &str, step: usize, body: usize) {
    assert!(
        (a - b).abs() <= TOL,
        "{case}: step[{step}] body[{body}] {what}: got {a}, golden {b} (delta {})",
        (a - b).abs()
    );
}

#[test]
fn every_golden_case_replays_against_the_original() {
    let g = golden();
    let tests = g["tests"].as_array().expect("tests array");
    assert_eq!(tests.len(), 10, "the capture recorded ten cases");

    let mut non_vacuous = 0;

    for case in tests {
        let name = case["name"].as_str().expect("name");
        let world = world_from(&case["geo"]);

        let initial = case["initialBodies"].as_array().expect("initialBodies");
        let steps = case["steps"].as_array().expect("steps");

        let mut rbw = RigidBodyWorld::new(world, -20.6);

        for init in initial {
            let shape = shape_from_str(init["shape"].as_str().expect("shape"));
            
            // Extract construction parameters
            let (hx, hy, hz) = if shape == Shape::Box {
                let he = &init["halfExtents"];
                (f(&he["x"]), f(&he["y"]), f(&he["z"]))
            } else {
                (0.1, 0.1, 0.1) // dummy values, not used for non-box
            };
            
            let radius = if shape == Shape::Sphere || shape == Shape::Capsule {
                f_opt(&init["radius"]).unwrap_or(0.1)
            } else {
                0.1
            };
            
            let half_height = if shape == Shape::Capsule {
                f_opt(&init["halfHeight"]).unwrap_or(0.0)
            } else {
                0.0
            };
            
            let mass = f(&init["mass"]);
            let position = vec3(&init["position"]);
            let quaternion = quat(&init["quaternion"]);
            let linear_velocity = vec3(&init["linearVelocity"]);
            let angular_velocity = vec3(&init["angularVelocity"]);
            let restitution = f(&init["restitution"]);
            let friction = f(&init["friction"]);
            let linear_damping = f(&init["linearDamping"]);
            let angular_damping = f(&init["angularDamping"]);
            let gravity_scale = f(&init["gravityScale"]);
            let ccd = init["ccd"].as_bool().unwrap_or(false);
            let lifetime = f_opt(&init["lifetime"]).unwrap_or(f64::INFINITY);

            let body = RigidBody::new(
                0, // id will be assigned by add()
                shape,
                hx, hy, hz,
                radius,
                half_height,
                mass,
                position,
                quaternion,
                linear_velocity,
                angular_velocity,
                restitution,
                friction,
                linear_damping,
                angular_damping,
                gravity_scale,
                0, // surface (not used in this port)
                mask::DEBRIS, // mask for debris collision
                0, // layer (not used in this port)
                ccd,
                lifetime,
            );
            rbw.add(body);
        }

        // The bodies are now in rbw.bodies() with assigned IDs
        // We need to track them by their position in the array
        // since the golden captures them in order
        
        for (step_idx, step) in steps.iter().enumerate() {
            let dt = f(&step["dt"]);
            rbw.step(dt);
            
            let step_bodies = step["bodies"].as_array().expect("bodies array");
            let rbw_bodies = rbw.bodies();
            
            for (body_idx, expected) in step_bodies.iter().enumerate() {
                if body_idx >= rbw_bodies.len() {
                    continue; // body may have been removed
                }
                let got = &rbw_bodies[body_idx];
                
                // Compare state
                let exp_pos = vec3(&expected["position"]);
                let exp_quat = quat(&expected["quaternion"]);
                let exp_linvel = vec3(&expected["linearVelocity"]);
                let exp_angvel = vec3(&expected["angularVelocity"]);
                
                for axis in 0..3 {
                    close(got.position[axis], exp_pos[axis], "position", name, step_idx, body_idx);
                    close(got.quaternion[axis], exp_quat[axis], "quaternion", name, step_idx, body_idx);
                    close(got.linear_velocity[axis], exp_linvel[axis], "linear_velocity", name, step_idx, body_idx);
                    close(got.angular_velocity[axis], exp_angvel[axis], "angular_velocity", name, step_idx, body_idx);
                }
                close(got.quaternion[3], exp_quat[3], "quaternion.w", name, step_idx, body_idx);
                
                assert_eq!(
                    got.sleeping,
                    expected["sleeping"].as_bool().expect("sleeping"),
                    "{name}: step[{step_idx}] body[{body_idx}] sleeping flag"
                );
                
                close(
                    got.sleep_timer,
                    f(&expected["sleepTimer"]),
                    "sleep_timer",
                    name,
                    step_idx,
                    body_idx,
                );
            }
        }

        non_vacuous += 1;
    }

    assert!(
        non_vacuous >= 8,
        "only {non_vacuous} of 10 golden cases carry impacts — the goldens have gone vacuous"
    );
}