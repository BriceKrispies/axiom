//! **Inspecting a compiled course**: the deterministic text dump, and the live
//! authoring rows the debug overlay shows.
//!
//! Both exist for the same reason: a compiled plan is a few hundred kilobytes of
//! numbers, and "the traffic feels wrong here" is not a bug report anybody can
//! act on. The dump is what a test diffs and what an agent reads to answer
//! questions about a course without running it; the rows are what a developer
//! reads while driving.
//!
//! The dump is **stable text**: same plan in, byte-identical string out, with
//! every float printed at a fixed precision so a re-run cannot produce a
//! one-in-the-last-digit diff.

use super::CoursePlan;

/// The deterministic textual form of a whole compiled course.
pub fn dump(plan: &CoursePlan) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "course {} seed {:#018x}\n{}",
        plan.name(),
        plan.seed(),
        plan.report().dump()
    ));
    out.push_str("--- traffic ---\n");
    plan.traffic().iter().for_each(|p| {
        out.push_str(&format!(
            "{:<6} {:>7.0}m..{:<7.0}m lane {:>2} {:>5.1} m/s {:<7} section {:<3}{}\n",
            p.id.to_string(),
            p.spawn_m,
            p.despawn_m,
            p.lane,
            p.speed_mps,
            p.archetype.token(),
            p.section,
            p.encounter
                .map(|e| format!(" [{e}]"))
                .unwrap_or_default(),
        ));
    });
    out.push_str("--- encounters ---\n");
    plan.encounters().iter().for_each(|e| {
        out.push_str(&format!(
            "{:<5} {:<13} {:>7.0}m..{:<7.0}m {:>3} vehicles, {} chances, {:.2} m clearance, \
             {:.2} s reaction{}\n",
            e.id.to_string(),
            e.kind,
            e.start_m,
            e.end_m,
            e.vehicles.len(),
            e.target_near_misses,
            e.lateral_clearance_m,
            e.minimum_reaction_time_s,
            e.requires_route.then_some(", route required").unwrap_or(""),
        ));
    });
    out.push_str("--- pickups ---\n");
    plan.pickups().iter().for_each(|p| {
        out.push_str(&format!(
            "{:<6} {:>7.0}m lane {:>2} {:<6} section {}\n",
            p.id.to_string(),
            p.at_m,
            p.lane,
            p.tier.token(),
            p.section,
        ));
    });
    out.push_str("--- near-miss windows ---\n");
    plan.near_miss_windows().iter().for_each(|w| {
        out.push_str(&format!(
            "{:>7.0}m..{:<7.0}m {:>3} vehicles, {} chances, {:.2}..{:.2} m, {} side, \
             weight {:.2}\n",
            w.start_m,
            w.end_m,
            w.vehicles.len(),
            w.intended_opportunities,
            w.clearance_m.lo,
            w.clearance_m.hi,
            w.side.token(),
            w.difficulty_weight,
        ));
    });
    out
}

/// The live authoring rows: what the course system knows about where the player
/// is *now*.
///
/// Ordered and labelled, so the overlay never reflows between frames — the same
/// contract [`crate::diagnostics::Diagnostics::rows`] keeps.
pub fn rows(plan: &CoursePlan, distance_m: f32, ahead_m: f32) -> Vec<(String, String)> {
    let section = plan.section_at(distance_m);
    let sample = plan.track().interpolated_at(distance_m);
    let verdict = plan
        .report()
        .sections
        .iter()
        .find(|v| v.index == section.index);
    let next = plan
        .traffic()
        .get(plan.first_vehicle_at(distance_m))
        .map(|p| p.spawn_m - distance_m);
    let upcoming = plan
        .traffic()
        .iter()
        .skip(plan.first_vehicle_at(distance_m))
        .take_while(|p| p.spawn_m <= distance_m + ahead_m)
        .count();
    let chances: u32 = plan
        .windows_ahead(distance_m, ahead_m)
        .map(|w| w.intended_opportunities)
        .sum();

    vec![
        ("course".into(), plan.name().to_string()),
        ("course seed".into(), format!("{:#018x}", plan.seed())),
        (
            "course distance".into(),
            format!("{distance_m:.0} m / {:.0} m", plan.length()),
        ),
        (
            "section".into(),
            format!("{} ({})", section.id, section.primitive),
        ),
        (
            "curvature / grade / bank".into(),
            format!(
                "{:+.4} rad/m  {:+.3}  {:+.1}°",
                sample.curvature,
                sample.grade,
                sample.bank.to_degrees()
            ),
        ),
        (
            "lanes / expected".into(),
            format!(
                "{} lanes, {:.0} km/h",
                plan.track().lane_count(&sample),
                section.expected_speed_mps * 3.6
            ),
        ),
        (
            "traffic zone".into(),
            format!(
                "{} vehicles in this section",
                plan.traffic()
                    .iter()
                    .filter(|p| p.section == section.index)
                    .count()
            ),
        ),
        (
            "encounter".into(),
            plan.encounter_at(distance_m)
                .map(|e| format!("{} {} ({} cars)", e.id, e.kind, e.vehicles.len()))
                .unwrap_or_else(|| "none".to_string()),
        ),
        (
            "upcoming plans".into(),
            format!("{upcoming} within {ahead_m:.0} m"),
        ),
        (
            "nearest headway".into(),
            next.map(|d| format!("{d:.0} m"))
                .unwrap_or_else(|| "clear".to_string()),
        ),
        (
            "traversability".into(),
            verdict
                .map(|v| {
                    format!(
                        "{} ({} lane corridor)",
                        v.traversable
                            .then_some("route exists")
                            .unwrap_or("BLOCKED"),
                        v.narrowest_corridor_lanes
                    )
                })
                .unwrap_or_else(|| "unmeasured".to_string()),
        ),
        (
            "near-miss chances".into(),
            format!("{chances} within {ahead_m:.0} m"),
        ),
        (
            "pickups ahead".into(),
            {
                let ahead: Vec<&crate::course::pickups::BoostPickup> =
                    plan.pickups_ahead(distance_m, ahead_m).collect();
                ahead
                    .first()
                    .map(|next| {
                        format!(
                            "{} within {ahead_m:.0} m, next {} in lane {} at {:.0} m",
                            ahead.len(),
                            next.tier.token(),
                            next.lane,
                            next.at_m - distance_m
                        )
                    })
                    .unwrap_or_else(|| "none".to_string())
            },
        ),
        (
            "boost economy".into(),
            verdict
                .map(|v| {
                    format!(
                        "{} (earn {:.2} / spend {:.2})",
                        v.status.token(),
                        v.boost_earned,
                        v.boost_spent
                    )
                })
                .unwrap_or_else(|| plan.report().status.token().to_string()),
        ),
        (
            "validation".into(),
            format!(
                "{} errors, {} warnings",
                plan.report().errors().count(),
                plan.report().warnings().count()
            ),
        ),
    ]
}

/// How far ahead the authoring overlay looks by default (m).
pub const DEFAULT_LOOKAHEAD_M: f32 = 600.0;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::course::procedural;

    fn plan() -> CoursePlan {
        procedural::shipping_plan(crate::DEFAULT_SEED).expect("the shipping course compiles")
    }

    #[test]
    fn the_dump_is_deterministic_and_carries_every_compiled_thing() {
        let plan = plan();
        let dump = plan.dump();
        assert_eq!(dump, plan.dump(), "the dump is not stable");
        assert!(dump.contains("--- traffic ---"));
        assert!(dump.contains("--- encounters ---"));
        assert!(dump.contains("--- pickups ---"));
        assert!(dump.contains("--- near-miss windows ---"));
        // Every pickup appears exactly once, by its own identity.
        assert!(!plan.pickups().is_empty(), "the shipping course has pickups");
        plan.pickups().iter().for_each(|p| {
            assert_eq!(
                dump.matches(&format!("{:<6} ", p.id.to_string())).count(),
                1,
                "pickup {} is not in the dump exactly once",
                p.id
            );
        });
        assert!(dump.contains(&format!("{:#018x}", crate::DEFAULT_SEED)));
        // Every vehicle appears exactly once.
        plan.traffic().iter().take(8).for_each(|p| {
            assert_eq!(
                dump.matches(&format!("{:<6} ", p.id.to_string())).count(),
                1,
                "vehicle {} is not in the dump exactly once",
                p.id
            );
        });
        // Two plans from the same seed dump identically; a different seed does
        // not.
        assert_eq!(
            procedural::shipping_plan(crate::DEFAULT_SEED).unwrap().dump(),
            dump
        );
        assert_ne!(procedural::shipping_plan(99).unwrap().dump(), dump);
    }

    #[test]
    fn the_authoring_rows_are_labelled_stable_and_complete() {
        let plan = plan();
        let first = rows(&plan, 2_000.0, DEFAULT_LOOKAHEAD_M);
        assert!(first.len() >= 13, "only {} rows", first.len());
        first.iter().for_each(|(label, value)| {
            assert!(!label.is_empty());
            assert!(!value.is_empty(), "{label} has no value");
        });
        let labels: Vec<&str> = first.iter().map(|(l, _)| l.as_str()).collect();
        for wanted in [
            "course seed",
            "course distance",
            "section",
            "curvature / grade / bank",
            "traffic zone",
            "encounter",
            "upcoming plans",
            "nearest headway",
            "traversability",
            "near-miss chances",
            "pickups ahead",
            "boost economy",
            "validation",
        ] {
            assert!(labels.contains(&wanted), "no `{wanted}` row: {labels:?}");
        }
        // The order does not change with the player's position, so the overlay
        // never reflows.
        let later: Vec<String> = rows(&plan, 6_000.0, DEFAULT_LOOKAHEAD_M)
            .into_iter()
            .map(|(l, _)| l)
            .collect();
        assert_eq!(
            labels,
            later.iter().map(|s| s.as_str()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn the_rows_report_the_section_and_the_traffic_actually_there() {
        let plan = plan();
        let section = plan.sections()[3].clone();
        let at = (section.start_m + section.end_m) * 0.5;
        let here = rows(&plan, at, DEFAULT_LOOKAHEAD_M);
        let value = |label: &str| {
            here.iter()
                .find(|(l, _)| l == label)
                .map(|(_, v)| v.clone())
                .expect("the row exists")
        };
        assert!(value("section").contains(section.id.as_str()));
        assert!(value("course distance").contains(&format!("{at:.0}")));
        assert!(value("validation").contains("errors"));
        // Past the end of the course there is nothing ahead, and that is a
        // value rather than a panic.
        let past_the_end = rows(&plan, plan.length(), DEFAULT_LOOKAHEAD_M);
        assert!(past_the_end.iter().any(|(l, v)| l == "nearest headway" && v == "clear"));
    }

    #[test]
    fn an_encounter_is_named_where_the_player_is_inside_one() {
        let plan = plan();
        let encounter = plan
            .encounters()
            .first()
            .cloned()
            .expect("the shipping course has an encounter");
        let inside = (encounter.start_m + encounter.end_m) * 0.5;
        let here = rows(&plan, inside, DEFAULT_LOOKAHEAD_M);
        let value = here
            .iter()
            .find(|(l, _)| l == "encounter")
            .map(|(_, v)| v.clone())
            .unwrap();
        assert!(value.contains(encounter.kind), "{value}");
        // And "none" well away from it.
        let away = rows(&plan, 100.0, DEFAULT_LOOKAHEAD_M);
        assert!(away.iter().any(|(l, v)| l == "encounter" && v == "none"));
    }
}
