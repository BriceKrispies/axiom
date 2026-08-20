//! Golden verification for `physics::penetration`, against captures taken from
//! the original `Claude-of-Duty/src/physics/penetration.js` running under Node.
//!
//! The port and its capture harness were written by an external agent
//! (`openrouter/nvidia/nemotron-3-ultra-550b-a55b:free` driven through
//! `opencode`); the harness was never run and no Rust test existed, so this
//! file is the verification half.
//!
//! Every case's *inputs* as well as its expected impacts come out of
//! `tests/penetration/golden.json`, so nothing here is hand-transcribed — the
//! same rule `docs/work-manifests/shmup-port/02-port-recipe.md` sets for the
//! rest of the port.

use axiom_shmup::physics::bvh::StaticWorld;
use axiom_shmup::physics::penetration::Ballistics;
use axiom_shmup::world::palette::Surface;
use serde_json::Value;
use std::rc::Rc;

/// Build the exact world the capture built, from the geometry it recorded.
///
/// Nothing here is hand-transcribed: `capture.mjs` records every
/// `addTriangles` call the original made, so the Rust side replays the same
/// world rather than a re-typed approximation of it.
fn world_from(geo: &Value) -> Rc<StaticWorld> {
    let mut world = StaticWorld::default();
    for obj in geo.as_array().expect("geo array") {
        let tris: Vec<f64> = obj["tris"]
            .as_array()
            .expect("tris")
            .iter()
            .map(f)
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

fn golden() -> Value {
    let raw = include_str!("penetration/golden.json");
    serde_json::from_str(raw).expect("golden.json parses")
}

fn f(v: &Value) -> f64 {
    v.as_f64().expect("number")
}

fn vec3(v: &Value) -> [f64; 3] {
    [f(&v[0]), f(&v[1]), f(&v[2])]
}

/// Positions and damage are built from `+ - * /` and a `hypot`, so they are
/// compared with a tight tolerance rather than bit-exactly; `sqrt` is not
/// bit-guaranteed across libm implementations. Counts, surfaces, exit flags and
/// object ids are compared exactly — a differing count is a different
/// algorithm, never rounding.
const TOL: f64 = 1e-9;

fn close(a: f64, b: f64, what: &str, case: &str, i: usize) {
    assert!(
        (a - b).abs() <= TOL,
        "{case}: impact[{i}] {what}: got {a}, golden {b} (delta {})",
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

        let p = &case["params"];
        let origin = [f(&p["origin"]["x"]), f(&p["origin"]["y"]), f(&p["origin"]["z"])];
        let dir = [f(&p["dir"]["x"]), f(&p["dir"]["y"]), f(&p["dir"]["z"])];

        let mut bal = Ballistics::new(world);
        let count = bal.fire(
            origin,
            dir,
            f(&p["maxDist"]),
            f(&p["damage"]),
            f(&p["penetration"]),
            p["mask"].as_u64().expect("mask") as u16,
            f(&p["dropoff"]),
            None,
            false,
        );

        let want = case["impacts"].as_array().expect("impacts array");
        assert_eq!(
            count,
            case["count"].as_u64().expect("count") as usize,
            "{name}: impact COUNT differs — that is a different algorithm, not rounding"
        );
        assert_eq!(count, want.len(), "{name}: count disagrees with the recorded impacts");

        non_vacuous += usize::from(count > 0);

        for (i, expected) in want.iter().enumerate() {
            let got = &bal.impacts()[i];
            let wp = vec3(&expected["point"]);
            let wn = vec3(&expected["normal"]);

            for axis in 0..3 {
                close(got.point[axis], wp[axis], "point", name, i);
                close(got.normal[axis], wn[axis], "normal", name, i);
            }
            close(got.damage, f(&expected["damage"]), "damage", name, i);
            close(got.distance, f(&expected["distance"]), "distance", name, i);

            assert_eq!(
                got.surface.name(),
                expected["surface"].as_str().expect("surface"),
                "{name}: impact[{i}] surface"
            );
            assert_eq!(
                got.exit,
                expected["exit"].as_bool().expect("exit"),
                "{name}: impact[{i}] exit flag"
            );
            assert_eq!(
                i64::from(got.object),
                expected["object"].as_i64().expect("object"),
                "{name}: impact[{i}] object id"
            );
        }
    }

    // Two of the recorded cases produce no impacts at all. They are kept
    // because the capture recorded them, but they discriminate nothing, so the
    // suite asserts that most cases carry real data.
    assert!(
        non_vacuous >= 8,
        "only {non_vacuous} of 10 golden cases carry impacts — the goldens have gone vacuous"
    );
}
