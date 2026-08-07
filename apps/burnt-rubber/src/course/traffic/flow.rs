//! **Ambient flow compilation**: a density description becomes concrete
//! vehicles.
//!
//! The whole generator is a single bounded walk along the zone, placing one
//! vehicle at a time and stepping forward by a drawn headway. Everything the
//! flow spec offers — platoons, bursts, recovery stretches, open corridors — is
//! a modification of that step, which is why they compose rather than fight:
//! there is one cursor and one rule for moving it.
//!
//! Density is expressed **per kilometre of course**, and the walk is in metres,
//! so the traffic a player meets is a property of the road and not of how fast
//! they drove up to it.

use crate::course::compiler::seeds::{section_draw, SeedDomain};
use crate::course::error::CourseResult;
use crate::course::specification::{
    LaneWeight, SectionId, TrafficFlowSpec, VehicleArchetype, VehicleId,
};
use crate::draw::Draw;
use crate::track::Track;

use super::{TrafficPlan, PLAN_LIFETIME_M};

/// The most vehicles one zone may compile.
///
/// A bound, not a target: the walk always steps forward by at least
/// `min_headway_m`, so it terminates on its own — but a zone authored with a
/// tiny headway over a long course could otherwise produce tens of thousands of
/// plans, and a course that dense is a mistake rather than a style.
pub const MAX_VEHICLES_PER_ZONE: usize = 512;

/// The expected player speed the `speed_relative_to_expected` blend is measured
/// against — the speed the shipping car actually carries.
const REFERENCE_EXPECTED_SPEED_MPS: f32 = 80.0;

/// Compile the ambient traffic for one zone.
///
/// `next_id` is advanced as vehicles are minted, so identities are dense and
/// ordered across the whole course.
#[allow(clippy::too_many_arguments)]
pub fn compile(
    course_seed: u64,
    zone_id: &SectionId,
    spec: &TrafficFlowSpec,
    track: &Track,
    start_m: f32,
    end_m: f32,
    expected_speed_mps: f32,
    section_of: &dyn Fn(f32) -> u16,
    next_id: &mut u32,
) -> CourseResult<Vec<TrafficPlan>> {
    let mut draw = section_draw(course_seed, zone_id, SeedDomain::TrafficFlow);
    let mut cosmetic = section_draw(course_seed, zone_id, SeedDomain::Cosmetic);
    // Drawn once for the whole zone: where in the speed wave this stretch of
    // road starts.
    let speed_phase = draw.range(0.0, std::f32::consts::TAU);
    let mut plans: Vec<TrafficPlan> = Vec::new();
    let mut cursor = start_m;
    let mut corridor_at = start_m + spec.open_corridor_every_m.sample(&mut draw);
    // The burst/recovery cycle: dense stretch, then relaxed stretch, repeating.
    // A zone with no burst length simply never enters one.
    let cycle_m = spec.burst_length_m + spec.recovery_length_m;

    for _ in 0..MAX_VEHICLES_PER_ZONE {
        if cursor >= end_m {
            break;
        }
        // An open corridor: deliberately empty road, so the player gets a
        // breath and a place to spend boost.
        if (spec.open_corridor_length_m > 0.0) & (cursor >= corridor_at) {
            cursor += spec.open_corridor_length_m;
            corridor_at = cursor + spec.open_corridor_every_m.sample(&mut draw);
            continue;
        }

        let dense = in_burst(cursor - start_m, spec, cycle_m);
        let leader = place(
            spec,
            speed_phase,
            track,
            cursor,
            expected_speed_mps,
            section_of,
            next_id,
            &mut draw,
            &mut cosmetic,
            dense,
        );
        plans.push(leader);

        // A platoon: a knot of cars travelling together, which reads as a much
        // harder gap than the same cars spread out would.
        let platoon = draw
            .chance(spec.platoon_probability)
            .then(|| spec.platoon_size.sample(&mut draw).max(1))
            .unwrap_or(1);
        for _ in 1..platoon {
            cursor += spec.platoon_gap_m.max(spec.min_headway_m);
            (cursor < end_m).then(|| {
                plans.push(place(
                    spec,
                    speed_phase,
                    track,
                    cursor,
                    expected_speed_mps,
                    section_of,
                    next_id,
                    &mut draw,
                    &mut cosmetic,
                    dense,
                ));
            });
        }
        // A platoon is followed by the widest legal gap, so it reads as a group
        // rather than as the traffic simply getting denser.
        let after_platoon = (platoon > 1).then_some(spec.max_headway_m).unwrap_or(0.0);
        cursor += headway(spec, dense, &mut draw).max(after_platoon);
    }

    Ok(plans)
}

/// Whether the cursor is inside the dense half of the burst/recovery cycle.
fn in_burst(travelled_m: f32, spec: &TrafficFlowSpec, cycle_m: f32) -> bool {
    (cycle_m > 0.0) & (travelled_m.rem_euclid(cycle_m.max(1.0e-3)) < spec.burst_length_m)
}

/// Scaling applied to the headway inside a dense burst.
const BURST_HEADWAY_SCALE: f32 = 0.62;
/// Scaling applied to the headway in a recovery stretch.
const RECOVERY_HEADWAY_SCALE: f32 = 1.35;
/// How much slower a vehicle inside a burst cruises — bunched traffic is slow
/// traffic, which is what makes a burst something to get *through*.
///
/// Gentle on purpose: see [`SPEED_WAVE_CYCLE_M`]. A large step at a burst
/// boundary is a concertina waiting to happen.
const BURST_SPEED_SCALE: f32 = 0.9;

/// How much course one full cycle of the speed wave covers (m).
///
/// **Traffic near other traffic travels at about the same speed**, and this is
/// what enforces it. The speed band is walked along the course by a slow wave
/// rather than drawn independently per vehicle, plus [`SPEED_JITTER_MPS`] of
/// variation on top.
///
/// This is not decoration; it is what stops the flow walling the road off
/// without anybody authoring a wall. Independent draws put a 22 m/s car in front
/// of a 38 m/s one, and over the ~13 s both are inside the player's horizon that
/// 16 m/s differential closes 200 m — far more than the minimum headway — so
/// cars generated a comfortable distance apart arrive abreast. A slow wave keeps
/// any two vehicles within a few headways of each other under about 3 m/s apart,
/// which closes far less than the gap between them, while the course as a whole
/// still runs through the whole band.
const SPEED_WAVE_CYCLE_M: f32 = 4_000.0;

/// How much a single vehicle may differ from its neighbourhood's speed (m/s).
const SPEED_JITTER_MPS: f32 = 0.5;

/// Draw the gap to the next vehicle.
///
/// Two-sided around the **preferred** headway rather than uniform across the
/// band: the preferred value is what the flow actually looks like, and the
/// min/max are how far it is allowed to stray. A uniform draw would make
/// `preferred` decorative.
fn headway(spec: &TrafficFlowSpec, dense: bool, draw: &mut Draw) -> f32 {
    // The average of two uniforms is triangular about its centre, which is the
    // cheapest way to get a distribution that clusters rather than spreads.
    let t = (draw.unit() + draw.unit()) * 0.5 - 0.5;
    let spread = (t < 0.0)
        .then(|| spec.preferred_headway_m - spec.min_headway_m)
        .unwrap_or(spec.max_headway_m - spec.preferred_headway_m);
    let raw = spec.preferred_headway_m + 2.0 * t * spread;
    let scale = dense
        .then_some(BURST_HEADWAY_SCALE)
        .unwrap_or_else(|| (spec.burst_length_m > 0.0).then_some(RECOVERY_HEADWAY_SCALE).unwrap_or(1.0));
    (raw * scale).clamp(spec.min_headway_m, spec.max_headway_m)
}

/// Mint one vehicle at `distance_m`.
#[allow(clippy::too_many_arguments)]
fn place(
    spec: &TrafficFlowSpec,
    speed_phase: f32,
    track: &Track,
    distance_m: f32,
    expected_speed_mps: f32,
    section_of: &dyn Fn(f32) -> u16,
    next_id: &mut u32,
    draw: &mut Draw,
    cosmetic: &mut Draw,
    dense: bool,
) -> TrafficPlan {
    let sample = track.sample_at(distance_m);
    let reach = track.lane_reach(&sample);
    // **Ambient traffic always leaves a lane open.** One lane is protected at
    // any point on the course, and the protected lane walks along it.
    //
    // Without this the flow can wall the road off without anybody authoring a
    // wall — not at generation time, where the minimum headway holds, but a
    // dozen seconds later, once a 22 m/s car has been caught by a 38 m/s one
    // two lanes over and a third has closed on both. Blocking every lane is a
    // thing only an authored encounter may do, because only an encounter is
    // checked for leaving a route.
    let protected = protected_lane(distance_m, reach);
    let all = spec.resolved_lane_weights(reach);
    let open: Vec<LaneWeight> = all.iter().copied().filter(|w| w.lane != protected).collect();
    let weights = (!open.is_empty()).then_some(open).unwrap_or(all);
    let lane = pick_lane(&weights, draw);

    // The speed band, walked along the course by a slow wave so neighbouring
    // traffic travels at neighbouring speeds (see `SPEED_WAVE_CYCLE_M`), with a
    // little per-vehicle variation on top. Optionally rescaled so a fast section
    // presents the same *relative* closing speed as a slow one.
    let wave = ((distance_m * std::f32::consts::TAU / SPEED_WAVE_CYCLE_M) + speed_phase).sin()
        * 0.5
        + 0.5;
    let base = (spec.speed_mps.lo + (spec.speed_mps.hi - spec.speed_mps.lo) * wave
        + draw.range(-SPEED_JITTER_MPS, SPEED_JITTER_MPS))
    .clamp(spec.speed_mps.lo, spec.speed_mps.hi);
    let relative = base * expected_speed_mps / REFERENCE_EXPECTED_SPEED_MPS;
    let blend = spec.speed_relative_to_expected.clamp(0.0, 1.0);
    let speed = (base + (relative - base) * blend)
        * dense.then_some(BURST_SPEED_SCALE).unwrap_or(1.0);

    let archetypes = spec.resolved_archetype_weights();
    let archetype = pick_archetype(&archetypes, cosmetic);

    let id = VehicleId(*next_id);
    *next_id += 1;
    TrafficPlan {
        id,
        spawn_m: distance_m,
        despawn_m: (distance_m + PLAN_LIFETIME_M).min(track.length()),
        lane,
        speed_mps: speed.max(1.0),
        archetype,
        lane_changes: Vec::new(),
        speed_changes: Vec::new(),
        encounter: None,
        section: section_of(distance_m),
        // Derived from the vehicle's own identity, so a car's cosmetic wander is
        // stable even if the flow ahead of it changes.
        variation_seed: cosmetic.fork(u64::from(id.0)).seed(),
    }
}

/// Which lane ambient traffic leaves alone at `distance_m`.
///
/// It walks by one lane every [`OPEN_LANE_HOLD_M`], so the guaranteed corridor
/// is somewhere different every few hundred metres rather than being a permanent
/// empty lane the player can simply sit in.
pub fn protected_lane(distance_m: f32, lane_reach: i32) -> i32 {
    let lanes = lane_reach * 2 + 1;
    let step = (distance_m / OPEN_LANE_HOLD_M).floor().max(0.0) as i32;
    -lane_reach + step.rem_euclid(lanes)
}

/// How much course the protected lane holds before moving on (m).
pub const OPEN_LANE_HOLD_M: f32 = 240.0;

/// Draw a lane from the weights.
fn pick_lane(weights: &[LaneWeight], draw: &mut Draw) -> i32 {
    let values: Vec<f32> = weights.iter().map(|w| w.weight.max(0.0)).collect();
    weights
        .get(weighted_index(&values, draw))
        .map(|w| w.lane)
        .unwrap_or(0)
}

/// Draw a vehicle shape from the weights.
fn pick_archetype(weights: &[(VehicleArchetype, f32)], draw: &mut Draw) -> VehicleArchetype {
    let values: Vec<f32> = weights.iter().map(|(_, w)| w.max(0.0)).collect();
    weights
        .get(weighted_index(&values, draw))
        .map(|(a, _)| *a)
        .unwrap_or(VehicleArchetype::Saloon)
}

/// A deterministic weighted pick: walk the cumulative weights and take the
/// first bucket the draw lands in. Fixed iteration order, so it replays.
pub fn weighted_index(weights: &[f32], draw: &mut Draw) -> usize {
    let total: f32 = weights.iter().map(|w| w.max(0.0)).sum();
    let target = draw.range(0.0, total);
    let mut accumulated = 0.0f32;
    for (i, w) in weights.iter().enumerate() {
        accumulated += w.max(0.0);
        if target < accumulated {
            return i;
        }
    }
    weights.len().saturating_sub(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::course::specification::{CountRange, ScalarRange};

    fn track() -> Track {
        crate::course::procedural::shipping_plan(crate::DEFAULT_SEED)
            .expect("the shipping course compiles")
            .track()
            .clone()
    }

    fn compile_zone(spec: &TrafficFlowSpec, track: &Track, span: (f32, f32)) -> Vec<TrafficPlan> {
        let mut next = 0u32;
        compile(
            7,
            &SectionId::new("zone"),
            spec,
            track,
            span.0,
            span.1,
            80.0,
            &|_| 0,
            &mut next,
        )
        .expect("compiles")
    }

    #[test]
    fn a_density_produces_the_density_it_asked_for() {
        let track = track();
        for vehicles_per_km in [8.0f32, 12.0, 24.0] {
            let spec = TrafficFlowSpec::at_density(vehicles_per_km);
            let plans = compile_zone(&spec, &track, (300.0, 5_300.0));
            let measured = plans.len() as f32 / 5.0;
            assert!(
                (measured - vehicles_per_km).abs() < vehicles_per_km * 0.25,
                "asked for {vehicles_per_km}/km, got {measured}/km ({} cars)",
                plans.len()
            );
        }
    }

    #[test]
    fn the_minimum_headway_is_never_violated() {
        let track = track();
        let spec = TrafficFlowSpec {
            platoon_probability: 0.5,
            platoon_size: CountRange::new(2, 4),
            platoon_gap_m: 10.0,
            ..TrafficFlowSpec::at_density(20.0)
        };
        let plans = compile_zone(&spec, &track, (300.0, 5_300.0));
        assert!(plans.len() > 40);
        for pair in plans.windows(2) {
            assert!(
                pair[1].spawn_m - pair[0].spawn_m >= spec.min_headway_m - 1.0e-3,
                "{} m apart, below the {} m minimum",
                pair[1].spawn_m - pair[0].spawn_m,
                spec.min_headway_m
            );
        }
    }

    #[test]
    fn preferred_and_maximum_headway_both_move_the_placement() {
        let track = track();
        let base = TrafficFlowSpec {
            min_headway_m: 20.0,
            preferred_headway_m: 40.0,
            max_headway_m: 120.0,
            ..TrafficFlowSpec::at_density(25.0)
        };
        let tight = compile_zone(&base, &track, (300.0, 4_300.0));
        let loose = compile_zone(
            &TrafficFlowSpec {
                preferred_headway_m: 100.0,
                ..base.clone()
            },
            &track,
            (300.0, 4_300.0),
        );
        assert!(
            tight.len() as f32 > loose.len() as f32 * 1.5,
            "the preferred headway did not drive the spacing: {} vs {}",
            tight.len(),
            loose.len()
        );
        // And the maximum really is a ceiling.
        let capped = compile_zone(
            &TrafficFlowSpec {
                preferred_headway_m: 100.0,
                max_headway_m: 60.0,
                min_headway_m: 20.0,
                ..base
            },
            &track,
            (300.0, 4_300.0),
        );
        for pair in capped.windows(2) {
            assert!(
                pair[1].spawn_m - pair[0].spawn_m <= 60.0 + 1.0e-3,
                "gap {} exceeds the 60 m maximum",
                pair[1].spawn_m - pair[0].spawn_m
            );
        }
    }

    #[test]
    fn lane_weights_are_respected_over_a_large_sample() {
        let track = track();
        let spec = TrafficFlowSpec {
            lane_weights: vec![
                LaneWeight { lane: -1, weight: 1.0 },
                LaneWeight { lane: 0, weight: 3.0 },
                LaneWeight { lane: 1, weight: 1.0 },
            ],
            ..TrafficFlowSpec::at_density(24.0)
        };
        let plans = compile_zone(&spec, &track, (300.0, 8_300.0));
        assert!(plans.len() > 150, "a big enough sample: {}", plans.len());
        let centre = plans.iter().filter(|p| p.lane == 0).count() as f32;
        let fraction = centre / plans.len() as f32;
        assert!(
            (0.45..=0.75).contains(&fraction),
            "lane 0 has 3/5 of the weight but took {:.0}% of the traffic",
            fraction * 100.0
        );
        assert!(
            plans.iter().all(|p| p.lane.abs() <= 1),
            "a lane outside the authored weights was used"
        );
    }

    #[test]
    fn platoons_form_and_are_followed_by_a_real_gap() {
        let track = track();
        let spec = TrafficFlowSpec {
            platoon_probability: 1.0,
            platoon_size: CountRange::exact(3),
            platoon_gap_m: 24.0,
            min_headway_m: 20.0,
            preferred_headway_m: 60.0,
            max_headway_m: 90.0,
            ..TrafficFlowSpec::at_density(16.0)
        };
        let plans = compile_zone(&spec, &track, (300.0, 3_300.0));
        assert!(plans.len() >= 9);
        // Every third gap is the platoon's recovery, the two before it are the
        // platoon's own spacing.
        let gaps: Vec<f32> = plans
            .windows(2)
            .map(|w| w[1].spawn_m - w[0].spawn_m)
            .collect();
        let inside = gaps.chunks(3).filter(|c| c.len() == 3).filter(|c| {
            (c[0] - 24.0).abs() < 1.0e-2 && (c[1] - 24.0).abs() < 1.0e-2 && c[2] >= 90.0 - 1.0e-2
        });
        assert!(
            inside.count() > 5,
            "no platoon-then-gap rhythm in {gaps:?}"
        );
    }

    #[test]
    fn a_dense_burst_bunches_the_traffic_and_the_recovery_relaxes_it() {
        let track = track();
        let spec = TrafficFlowSpec {
            burst_length_m: 400.0,
            recovery_length_m: 400.0,
            ..TrafficFlowSpec::at_density(18.0)
        };
        let plans = compile_zone(&spec, &track, (0.0, 6_000.0));
        let (bursty, relaxed): (Vec<&TrafficPlan>, Vec<&TrafficPlan>) = plans
            .iter()
            .partition(|p| p.spawn_m.rem_euclid(800.0) < 400.0);
        assert!(
            bursty.len() > relaxed.len(),
            "the burst is not denser: {} vs {}",
            bursty.len(),
            relaxed.len()
        );
        // And a burst's traffic is slower, which is what makes it a wall.
        let mean = |v: &[&TrafficPlan]| {
            v.iter().map(|p| p.speed_mps).sum::<f32>() / v.len().max(1) as f32
        };
        assert!(
            mean(&bursty) < mean(&relaxed),
            "burst traffic is not slower: {} vs {}",
            mean(&bursty),
            mean(&relaxed)
        );
    }

    #[test]
    fn open_corridors_leave_real_gaps_at_the_authored_cadence() {
        let track = track();
        let spec = TrafficFlowSpec {
            open_corridor_every_m: ScalarRange::new(400.0, 500.0),
            open_corridor_length_m: 260.0,
            ..TrafficFlowSpec::at_density(24.0)
        };
        let plans = compile_zone(&spec, &track, (300.0, 5_300.0));
        let corridors = plans
            .windows(2)
            .filter(|w| w[1].spawn_m - w[0].spawn_m > 240.0)
            .count();
        // A 260 m corridor plus a 400-500 m cadence is a ~710 m cycle, so five
        // kilometres holds about seven.
        assert!(
            corridors >= 5,
            "only {corridors} corridors in 5 km at a 400-500 m cadence"
        );
    }

    #[test]
    fn the_same_seed_compiles_identical_vehicle_plans() {
        let track = track();
        let spec = TrafficFlowSpec::at_density(20.0);
        assert_eq!(
            compile_zone(&spec, &track, (300.0, 3_300.0)),
            compile_zone(&spec, &track, (300.0, 3_300.0))
        );
        // A different course seed is a different road full of traffic.
        let mut next = 0u32;
        let other = compile(
            8,
            &SectionId::new("zone"),
            &spec,
            &track,
            300.0,
            3_300.0,
            80.0,
            &|_| 0,
            &mut next,
        )
        .unwrap();
        assert_ne!(other, compile_zone(&spec, &track, (300.0, 3_300.0)));
    }

    /// The seed partition's job, checked where it matters most: the geometry
    /// stream and the traffic stream are not the same stream, so re-rolling the
    /// road cannot consume the traffic's draws.
    #[test]
    fn changing_the_geometry_stream_does_not_move_the_traffic() {
        let track = track();
        let spec = TrafficFlowSpec::at_density(20.0);
        let before = compile_zone(&spec, &track, (300.0, 3_300.0));
        // Burn a great deal of the geometry stream.
        let mut geometry = crate::course::compiler::seeds::domain_draw(
            7,
            crate::course::compiler::seeds::SeedDomain::Geometry,
        );
        (0..100_000).for_each(|_| {
            geometry.next_u64();
        });
        assert_eq!(compile_zone(&spec, &track, (300.0, 3_300.0)), before);
    }

    #[test]
    fn every_compiled_vehicle_is_on_the_road_and_identifiable() {
        let track = track();
        let spec = TrafficFlowSpec::at_density(20.0);
        let plans = compile_zone(&spec, &track, (300.0, 8_000.0));
        let mut ids: Vec<u32> = plans.iter().map(|p| p.id.0).collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count, "two vehicles share an id");
        for p in &plans {
            let sample = track.sample_at(p.spawn_m);
            let reach = track.lane_reach(&sample);
            assert!(p.lane.abs() <= reach, "lane {} off a reach-{reach} road", p.lane);
            assert!(p.speed_mps > 0.0 && p.speed_mps.is_finite());
            assert!(p.despawn_m > p.spawn_m);
            assert!(p.encounter.is_none(), "ambient traffic owns no encounter");
            assert!(p.variation_seed != 0);
        }
        // And they are in ascending spawn order, which is what the runtime index
        // is built on.
        assert!(plans.windows(2).all(|w| w[1].spawn_m >= w[0].spawn_m));
    }

    #[test]
    fn the_zone_vehicle_bound_terminates_a_pathologically_dense_zone() {
        let track = track();
        let spec = TrafficFlowSpec {
            min_headway_m: 0.5,
            preferred_headway_m: 0.5,
            max_headway_m: 0.5,
            ..TrafficFlowSpec::at_density(2_000.0)
        };
        let plans = compile_zone(&spec, &track, (0.0, 9_000.0));
        assert_eq!(plans.len(), MAX_VEHICLES_PER_ZONE);
    }

    #[test]
    fn the_weighted_pick_is_deterministic_and_stays_in_range() {
        let mut draw = Draw::seeded(5);
        assert_eq!(weighted_index(&[1.0, 0.0, 0.0], &mut draw), 0);
        assert_eq!(weighted_index(&[0.0, 0.0, 1.0], &mut draw), 2);
        // An all-zero set cannot pick anything meaningful; it must still return
        // a valid index rather than panicking.
        assert_eq!(weighted_index(&[0.0, 0.0], &mut draw), 1);
        assert_eq!(weighted_index(&[], &mut draw), 0);
        for _ in 0..256 {
            assert!(weighted_index(&[1.0, 2.0, 3.0], &mut draw) < 3);
        }
    }

    #[test]
    fn the_speed_blend_scales_traffic_to_the_expected_player_speed() {
        let track = track();
        let spec = TrafficFlowSpec {
            speed_mps: ScalarRange::exact(30.0),
            speed_relative_to_expected: 1.0,
            ..TrafficFlowSpec::at_density(12.0)
        };
        let mut next = 0u32;
        let slow = compile(
            7,
            &SectionId::new("z"),
            &spec,
            &track,
            300.0,
            1_300.0,
            40.0,
            &|_| 0,
            &mut next,
        )
        .unwrap();
        assert!(
            slow.iter().all(|p| (p.speed_mps - 15.0).abs() < 0.1),
            "a 40 m/s section should halve a 30 m/s band: {:?}",
            slow.first().map(|p| p.speed_mps)
        );
        let absolute = TrafficFlowSpec {
            speed_relative_to_expected: 0.0,
            ..spec
        };
        let mut next = 0u32;
        let plain = compile(
            7,
            &SectionId::new("z"),
            &absolute,
            &track,
            300.0,
            1_300.0,
            40.0,
            &|_| 0,
            &mut next,
        )
        .unwrap();
        assert!(plain.iter().all(|p| (p.speed_mps - 30.0).abs() < 0.1));
    }
}
