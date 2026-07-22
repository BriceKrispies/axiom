//! Pre-snap route chalk: the selected play's routes are published on the
//! snapshot while the offense is set, drawn as dotted chalk just above the turf,
//! with exactly one primary read — and they clear the moment the ball is snapped.

use axiom_end_zone::launch::RunConfig;
use axiom_end_zone::presentation::chalk::{chalk_instances, ChalkMaterial};
use axiom_end_zone::presentation::snapshot::capture;
use axiom_end_zone::showcase::ShowcaseRun;
use axiom_end_zone::state::PlayPhase;

#[test]
fn presnap_chalk_draws_the_offense_routes_with_one_primary() {
    // A fresh run is set pre-snap in the default play, so the routes are chalked.
    let run = ShowcaseRun::new_run(&RunConfig::new(0xC1A1_0001));
    let snap = capture(&run.sim);

    assert!(!snap.pre_snap_routes.is_empty(), "routes are chalked pre-snap");
    assert_eq!(
        snap.pre_snap_routes.iter().filter(|r| r.primary).count(),
        1,
        "exactly one primary read is highlighted"
    );
    for route in &snap.pre_snap_routes {
        assert!(
            route.points.len() >= 2,
            "a route runs from the alignment through its waypoints"
        );
    }

    let mut dots = Vec::new();
    chalk_instances(&snap, &mut dots);
    assert!(!dots.is_empty(), "the routes produce chalk dots");
    assert!(
        dots.iter().any(|d| d.material == ChalkMaterial::Primary),
        "the primary read is chalked in its own color"
    );
    assert!(
        dots.iter()
            .all(|d| d.transform.translation.y > 0.0 && d.transform.translation.y < 0.3),
        "chalk sits just above the turf, not floating"
    );
}

#[test]
fn chalk_only_shows_before_the_snap() {
    let mut run = ShowcaseRun::new_run(&RunConfig::new(0xC1A1_0002));
    let (mut saw_presnap, mut saw_snapped) = (false, false);
    for _ in 0..1200 {
        run.step(&[]);
        let snap = capture(&run.sim);
        match snap.phase {
            PlayPhase::PreSnap => saw_presnap |= !snap.pre_snap_routes.is_empty(),
            _ => {
                assert!(
                    snap.pre_snap_routes.is_empty(),
                    "chalk clears once the ball is live"
                );
                saw_snapped = true;
            }
        }
    }
    assert!(saw_presnap, "the run shows chalk pre-snap");
    assert!(saw_snapped, "and the run leaves the pre-snap phase");
}
