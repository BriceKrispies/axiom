//! **Motif expansion**: parameterized figures that become ordinary sections.
//!
//! A motif is a *shorthand for road an author would otherwise write out*, and it
//! stops existing the moment it is expanded. `high_speed_sweeps { count = 4 }`
//! becomes four `SectionSpec`s named `coastal_sweeps/0..3`, each an ordinary
//! turn with ordinary modifiers, and everything downstream — geometry, traffic,
//! validation, the runtime — sees only those. Nothing in the compiled plan
//! records that a motif was involved.
//!
//! That is what makes motifs safe to add: a new motif is a new function here and
//! a new variant in [`MotifKind`], and it cannot introduce a new *concept* into
//! the compiler, because its whole output is sections that already existed.
//!
//! Expansion is **deterministic and independently seeded**
//! ([`SeedDomain::Motif`]): a motif draws from a stream derived from the course
//! seed and its own stable id, so re-tuning one motif cannot re-roll another.

use crate::course::compiler::seeds::{section_draw, SeedDomain};
use crate::course::error::CourseResult;
use crate::course::specification::{
    BankingMode, MotifInvocation, MotifKind, MotifParams, RoadModifierSpec, RoadPrimitiveSpec,
    SectionId, SectionSpec, TurnDirection,
};
use crate::draw::Draw;

/// Expand `invocation` into the ordinary sections it stands for.
///
/// The returned sections are in course order, named `<id>/<n>`, and carry the
/// invocation's environment, lane count and expected speed. The invocation's
/// traffic zone is *not* attached here — it spans the whole expansion and is
/// resolved by the compiler once the sections have distances.
pub fn expand(course_seed: u64, invocation: &MotifInvocation) -> CourseResult<Vec<SectionSpec>> {
    invocation.params.validate()?;
    let mut draw = section_draw(course_seed, &invocation.id, SeedDomain::Motif);
    let params = &invocation.params;
    let sections = match invocation.kind {
        MotifKind::HighSpeedSweeps => high_speed_sweeps(&invocation.id, params, &mut draw),
        MotifKind::AlternatingSlalom => alternating_slalom(&invocation.id, params, &mut draw),
        MotifKind::RollingFreeway => rolling_freeway(&invocation.id, params, &mut draw),
        MotifKind::TunnelSqueeze => tunnel_squeeze(&invocation.id, params, &mut draw),
        MotifKind::BlindCrest => blind_crest(&invocation.id, params, &mut draw),
        MotifKind::LaneCollapse => lane_collapse(&invocation.id, params, &mut draw),
        MotifKind::Corkscrew => corkscrew(&invocation.id, params, &mut draw),
    };
    Ok(sections
        .into_iter()
        .map(|section| SectionSpec {
            lanes: section.lanes.or(Some(params.lanes.lo)),
            environment: section.environment.or(invocation.environment),
            expected_speed_mps: section
                .expected_speed_mps
                .or(invocation.expected_speed_mps),
            ..section
        })
        .collect())
}

/// **High-speed sweeps** — `count` long alternating bends, each with a short
/// straight to breathe on, banked into the corner.
///
/// The alternation is the figure: a run of same-way bends is one long corner,
/// and the thing that makes a sweeper section read as a sequence is that each
/// one loads the car the other way.
fn high_speed_sweeps(id: &SectionId, params: &MotifParams, draw: &mut Draw) -> Vec<SectionSpec> {
    let per_bend = params.length_m / params.count.max(1) as f32;
    let bend_length = per_bend * 0.78;
    let link_length = (per_bend - bend_length).max(20.0);
    let opening = draw.sign();
    let elevation_phase = draw.range(0.0, std::f32::consts::TAU);
    let mut travelled = 0.0f32;
    (0..params.count)
        .flat_map(|i| {
            let radius_m = params.radius_m.sample(draw);
            let bank = params.bank_rad.sample(draw);
            let direction = ((opening * (-1.0f32).powi(i as i32)) > 0.0)
                .then_some(TurnDirection::Right)
                .unwrap_or(TurnDirection::Left);
            let bend_phase = wave_phase(elevation_phase, travelled, params.wavelength_m);
            travelled += bend_length;
            let link_phase = wave_phase(elevation_phase, travelled, params.wavelength_m);
            travelled += link_length;
            [
                rolling(
                    SectionSpec::new(
                        id.child(format!("bend{i}")),
                        RoadPrimitiveSpec::Turn {
                            length_m: bend_length,
                            radius_m,
                            direction,
                        },
                    )
                    .with_modifier(RoadModifierSpec::Banking {
                        mode: BankingMode::FollowCurvature,
                        strength: 1.0,
                        maximum_rad: bank,
                    }),
                    params,
                    bend_phase,
                ),
                rolling(
                    SectionSpec::new(
                        id.child(format!("link{i}")),
                        RoadPrimitiveSpec::Straight {
                            length_m: link_length,
                        },
                    ),
                    params,
                    link_phase,
                ),
            ]
        })
        .collect()
}

/// Add the motif's elevation wave to a section, if it asked for one.
///
/// Hills and bends are separate knobs in the pacing plan they came from — a
/// section can be curvy and flat, or straight and hilly — so every motif that
/// makes a *shape* can also carry a *profile*, rather than there being one motif
/// for corners and another for hills.
fn rolling(section: SectionSpec, params: &MotifParams, phase_rad: f32) -> SectionSpec {
    (params.elevation_amplitude_m.abs() > 1.0e-4)
        .then(|| {
            section.clone().with_modifier(RoadModifierSpec::ElevationWave {
                amplitude_m: params.elevation_amplitude_m,
                wavelength_m: params.wavelength_m,
                phase_rad,
            })
        })
        .unwrap_or(section)
}

/// The phase a wave has reached `travelled_m` into a motif — what keeps a wave
/// continuous across the sections a motif is cut into instead of restarting at
/// every join.
fn wave_phase(base_rad: f32, travelled_m: f32, wavelength_m: f32) -> f32 {
    base_rad + std::f32::consts::TAU * travelled_m / wavelength_m.max(1.0)
}

/// **Alternating slalom** — `count` S-bends butted together, no straights.
fn alternating_slalom(id: &SectionId, params: &MotifParams, draw: &mut Draw) -> Vec<SectionSpec> {
    let per_bend = params.length_m / params.count.max(1) as f32;
    let opening = draw.sign();
    let elevation_phase = draw.range(0.0, std::f32::consts::TAU);
    (0..params.count)
        .map(|i| {
            let radius_m = params.radius_m.sample(draw);
            let bank = params.bank_rad.sample(draw);
            let first = ((opening * (-1.0f32).powi(i as i32)) > 0.0)
                .then_some(TurnDirection::Right)
                .unwrap_or(TurnDirection::Left);
            rolling(
                SectionSpec::new(
                    id.child(i),
                    RoadPrimitiveSpec::SBend {
                        length_m: per_bend,
                        radius_m,
                        first,
                    },
                )
                .with_modifier(RoadModifierSpec::Banking {
                    mode: BankingMode::FollowCurvature,
                    strength: 1.0,
                    maximum_rad: bank,
                }),
                params,
                wave_phase(elevation_phase, i as f32 * per_bend, params.wavelength_m),
            )
        })
        .collect()
}

/// **Rolling freeway** — straight road carrying an elevation wave and a lateral
/// wave, split into `count` pieces so traffic and scenery have somewhere to
/// change character.
///
/// This is the motif the example course's closing run uses, and the one that
/// shows what modifiers are for: the road is *authored* as a straight and is
/// nothing of the kind by the time the waves are on it.
fn rolling_freeway(id: &SectionId, params: &MotifParams, draw: &mut Draw) -> Vec<SectionSpec> {
    let per_piece = params.length_m / params.count.max(1) as f32;
    let lateral_phase = draw.range(0.0, std::f32::consts::TAU);
    let elevation_phase = draw.range(0.0, std::f32::consts::TAU);
    (0..params.count)
        .map(|i| {
            // The phase advances with the piece so the wave is continuous
            // *across* the pieces rather than restarting at every join.
            let travelled = i as f32 * per_piece;
            SectionSpec::new(
                id.child(i),
                RoadPrimitiveSpec::Straight {
                    length_m: per_piece,
                },
            )
            .with_modifier(RoadModifierSpec::LateralWave {
                amplitude_m: params.lateral_amplitude_m,
                wavelength_m: params.wavelength_m,
                phase_rad: lateral_phase
                    + std::f32::consts::TAU * travelled / params.wavelength_m.max(1.0),
            })
            .with_modifier(RoadModifierSpec::ElevationWave {
                amplitude_m: params.elevation_amplitude_m,
                wavelength_m: params.wavelength_m * 0.75,
                phase_rad: elevation_phase
                    + std::f32::consts::TAU * travelled / (params.wavelength_m * 0.75).max(1.0),
            })
        })
        .collect()
}

/// **Tunnel squeeze** — approach, collapse to the narrow count, hold, and open
/// back out.
fn tunnel_squeeze(id: &SectionId, params: &MotifParams, draw: &mut Draw) -> Vec<SectionSpec> {
    let wide = params.lanes.lo;
    let narrow = params.narrow_lanes.min(wide);
    let transition = (params.length_m * 0.16).max(60.0);
    let corridor = (params.length_m - transition * 2.0).max(80.0);
    // A corridor is more claustrophobic if it is not dead straight.
    let radius_m = params.radius_m.sample(draw).max(params.radius_m.lo);
    let bend = draw
        .chance(0.5)
        .then_some(TurnDirection::Right)
        .unwrap_or(TurnDirection::Left);
    vec![
        SectionSpec::new(
            id.child("collapse"),
            RoadPrimitiveSpec::LaneTransition {
                length_m: transition,
                from_lanes: wide,
                to_lanes: narrow,
            },
        )
        .with_lanes(narrow),
        SectionSpec::new(
            id.child("corridor"),
            RoadPrimitiveSpec::Turn {
                length_m: corridor,
                radius_m,
                direction: bend,
            },
        )
        .with_lanes(narrow)
        .with_modifier(RoadModifierSpec::Banking {
            mode: BankingMode::Flat,
            strength: 0.0,
            maximum_rad: 0.0,
        }),
        SectionSpec::new(
            id.child("release"),
            RoadPrimitiveSpec::LaneTransition {
                length_m: transition,
                from_lanes: narrow,
                to_lanes: wide,
            },
        )
        .with_lanes(wide),
    ]
}

/// **Blind crest** — a rise steep enough to hide the road beyond it, with a run
/// up and a landing.
fn blind_crest(id: &SectionId, params: &MotifParams, draw: &mut Draw) -> Vec<SectionSpec> {
    let approach = (params.length_m * 0.3).max(60.0);
    let crest = (params.length_m * 0.4).max(80.0);
    // Which way the road goes on the far side is the whole joke of a blind
    // crest, so it is drawn rather than authored.
    let radius_m = params.radius_m.sample(draw);
    let away = draw
        .chance(0.5)
        .then_some(TurnDirection::Right)
        .unwrap_or(TurnDirection::Left);
    vec![
        SectionSpec::new(
            id.child("approach"),
            RoadPrimitiveSpec::Straight {
                length_m: approach,
            },
        ),
        SectionSpec::new(
            id.child("crest"),
            RoadPrimitiveSpec::Crest {
                length_m: crest,
                height_m: params.height_m,
            },
        ),
        SectionSpec::new(
            id.child("landing"),
            RoadPrimitiveSpec::Turn {
                length_m: (params.length_m - approach - crest).max(60.0),
                radius_m,
                direction: away,
            },
        ),
    ]
}

/// **Lane collapse** — a staged narrowing with no recovery, each stage holding
/// long enough for the loss to register before the next one.
fn lane_collapse(id: &SectionId, params: &MotifParams, draw: &mut Draw) -> Vec<SectionSpec> {
    let wide = params.lanes.lo;
    let narrow = params.narrow_lanes.min(wide);
    // Each stage drops one lane *pair*, because the lattice only has odd counts.
    let stages = ((wide.saturating_sub(narrow)) / 2).max(1);
    let per_stage = params.length_m / stages as f32;
    let hold = per_stage * 0.55;
    let drop = per_stage - hold;
    let wobble = draw.range(0.9, 1.1);
    (0..stages)
        .flat_map(|i| {
            let from = wide - i * 2;
            let to = from.saturating_sub(2).max(narrow);
            [
                SectionSpec::new(
                    id.child(format!("hold{i}")),
                    RoadPrimitiveSpec::Straight {
                        length_m: hold * wobble,
                    },
                )
                .with_lanes(from),
                SectionSpec::new(
                    id.child(format!("drop{i}")),
                    RoadPrimitiveSpec::LaneTransition {
                        length_m: drop.max(40.0),
                        from_lanes: from,
                        to_lanes: to,
                    },
                )
                .with_lanes(to),
            ]
        })
        .collect()
}

/// **Corkscrew** — a run in, one continuous banked turn that descends far
/// enough to pass under itself, and a run out.
///
/// It is a *single* turn section rather than a string of them, and that is the
/// whole trick. A `Turn` eases its curvature in and out over
/// [`TURN_EASE_FRACTION`](crate::course::specification::road::TURN_EASE_FRACTION)
/// at each end, so a helix built from several of them would relax to straight
/// between every coil — a sequence of corners, not a screw. One section eases
/// once, at the lip and at the exit, which is exactly where a corkscrew should
/// ease.
///
/// The radius is **derived**, not authored: the motif is told how much road it
/// has and how many revolutions to spend it on, and the radius falls out. That
/// is the right way round — "one and a bit turns down a ridge in twelve hundred
/// metres" is the design, and the radius is its consequence. If the consequence
/// is tighter than the course allows, `RoadPrimitiveSpec::validate` rejects it
/// by name rather than this quietly opening the figure out.
fn corkscrew(id: &SectionId, params: &MotifParams, draw: &mut Draw) -> Vec<SectionSpec> {
    let entry = (params.length_m * CORKSCREW_ENTRY_SHARE).max(60.0);
    let runout = (params.length_m * CORKSCREW_RUNOUT_SHARE).max(80.0);
    let coil = (params.length_m - entry - runout).max(200.0);
    // The eased ends of the turn spend their arc at less than full curvature, so
    // the constant-radius middle has to make up the difference for the figure to
    // come round as many times as it was asked to.
    let turning_arc = coil * (1.0 - crate::course::specification::road::TURN_EASE_FRACTION);
    let revolutions = params.count.max(1) as f32;
    let radius_m = (turning_arc / (revolutions * std::f32::consts::TAU))
        .max(params.radius_m.lo);
    let direction = draw
        .chance(0.5)
        .then_some(TurnDirection::Right)
        .unwrap_or(TurnDirection::Left);
    vec![
        SectionSpec::new(
            id.child("entry"),
            RoadPrimitiveSpec::Straight { length_m: entry },
        ),
        SectionSpec::new(
            id.child("coil"),
            RoadPrimitiveSpec::Turn {
                length_m: coil,
                radius_m,
                direction,
            },
        )
        .with_modifier(RoadModifierSpec::Banking {
            mode: BankingMode::FollowCurvature,
            strength: 1.0,
            maximum_rad: params.bank_rad.hi,
        })
        .with_modifier(RoadModifierSpec::GradeProfile {
            drop_m: params.height_m,
        }),
        SectionSpec::new(
            id.child("runout"),
            RoadPrimitiveSpec::Straight { length_m: runout },
        ),
    ]
}

/// How much of a corkscrew's length is spent lining the car up for it.
const CORKSCREW_ENTRY_SHARE: f32 = 0.12;
/// How much is spent letting the car settle afterwards. Larger than the entry:
/// a car leaving a long banked descent is unloaded and needs somewhere to put
/// itself straight.
const CORKSCREW_RUNOUT_SHARE: f32 = 0.16;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::course::specification::{CountRange, ScalarRange, SectionKind};

    fn invocation(kind: MotifKind) -> MotifInvocation {
        MotifInvocation::new(SectionId::new("figure"), kind)
    }

    #[test]
    fn every_motif_expands_into_ordinary_sections() {
        for kind in MotifKind::ALL {
            let sections = expand(11, &invocation(kind)).expect("expands");
            assert!(!sections.is_empty(), "{kind:?} expanded to nothing");
            for s in &sections {
                assert!(
                    s.id.as_str().starts_with("figure/"),
                    "{kind:?} minted `{}` outside its own id",
                    s.id
                );
                assert!(
                    s.primitive.length_m() > 0.0,
                    "{kind:?} produced a zero-length section"
                );
                assert!(s.primitive.validate(90.0).is_ok(), "{kind:?}: {s:?}");
                s.modifiers
                    .iter()
                    .for_each(|m| m.validate().expect("a valid modifier"));
            }
        }
    }

    #[test]
    fn expansion_is_deterministic_for_a_seed_and_varies_with_it() {
        for kind in MotifKind::ALL {
            let a = expand(7, &invocation(kind)).unwrap();
            let b = expand(7, &invocation(kind)).unwrap();
            assert_eq!(a, b, "{kind:?} is not deterministic");
        }
        // The motifs that draw anything at all must actually differ on a
        // different seed.
        for kind in [
            MotifKind::HighSpeedSweeps,
            MotifKind::AlternatingSlalom,
            MotifKind::RollingFreeway,
            MotifKind::TunnelSqueeze,
            MotifKind::BlindCrest,
            MotifKind::Corkscrew,
        ] {
            assert_ne!(
                expand(7, &invocation(kind)).unwrap(),
                expand(8, &invocation(kind)).unwrap(),
                "{kind:?} ignores the seed"
            );
        }
    }

    /// A motif's stream is anchored on its own id, so re-seeding one motif
    /// leaves every other motif exactly where it was.
    #[test]
    fn one_motifs_local_seed_does_not_disturb_another() {
        let first = MotifInvocation::new(SectionId::new("first"), MotifKind::HighSpeedSweeps);
        let second = MotifInvocation::new(SectionId::new("second"), MotifKind::HighSpeedSweeps);
        let before = expand(3, &first).unwrap();
        // Change the *second* motif entirely.
        let mut altered = second.clone();
        altered.params.count = 7;
        let _ = expand(3, &altered).unwrap();
        assert_eq!(
            expand(3, &first).unwrap(),
            before,
            "expanding another motif moved this one"
        );
        assert_ne!(
            expand(3, &first).unwrap(),
            expand(3, &second).unwrap(),
            "two motifs on one seed are two figures"
        );
    }

    #[test]
    fn the_bounded_repeat_count_is_respected_exactly() {
        for count in [1u32, 2, 5, 9] {
            let mut m = invocation(MotifKind::AlternatingSlalom);
            m.params.count = count;
            let sections = expand(5, &m).unwrap();
            assert_eq!(sections.len(), count as usize, "slalom emits one per repeat");
            let mut m = invocation(MotifKind::HighSpeedSweeps);
            m.params.count = count;
            let sections = expand(5, &m).unwrap();
            assert_eq!(
                sections.len(),
                count as usize * 2,
                "a sweep is a bend plus its link"
            );
        }
        let mut over = invocation(MotifKind::AlternatingSlalom);
        over.params.count = crate::course::specification::MAX_MOTIF_COUNT + 1;
        assert!(expand(5, &over).is_err(), "the repeat bound is enforced");
    }

    #[test]
    fn a_sweep_run_alternates_its_bends() {
        let mut m = invocation(MotifKind::HighSpeedSweeps);
        m.params.count = 6;
        let directions: Vec<TurnDirection> = expand(21, &m)
            .unwrap()
            .into_iter()
            .filter_map(|s| match s.primitive {
                RoadPrimitiveSpec::Turn { direction, .. } => Some(direction),
                _ => None,
            })
            .collect();
        assert_eq!(directions.len(), 6);
        for pair in directions.windows(2) {
            assert_ne!(pair[0], pair[1], "two sweepers in a row went the same way");
        }
    }

    #[test]
    fn a_slalom_alternates_which_way_each_s_bend_opens() {
        let mut m = invocation(MotifKind::AlternatingSlalom);
        m.params.count = 6;
        let firsts: Vec<TurnDirection> = expand(21, &m)
            .unwrap()
            .into_iter()
            .filter_map(|s| match s.primitive {
                RoadPrimitiveSpec::SBend { first, .. } => Some(first),
                _ => None,
            })
            .collect();
        assert_eq!(firsts.len(), 6);
        for pair in firsts.windows(2) {
            assert_ne!(pair[0], pair[1]);
        }
    }

    /// The figure the motif exists for: one continuous turn, banked, descending,
    /// and round far enough to pass under itself.
    #[test]
    fn a_corkscrew_is_one_continuous_descending_turn() {
        let mut m = invocation(MotifKind::Corkscrew);
        m.params.count = 1;
        m.params.length_m = 1_250.0;
        m.params.height_m = 70.0;
        m.params.radius_m = ScalarRange::exact(90.0);
        m.params.bank_rad = ScalarRange::new(0.0, 0.25);
        let sections = expand(4, &m).unwrap();

        assert_eq!(sections.len(), 3, "run in, one coil, run out");
        assert!(sections[0].id.as_str().ends_with("/entry"));
        assert!(sections[2].id.as_str().ends_with("/runout"));

        // Exactly one turn section — several would relax to straight between
        // the coils and stop being a screw.
        let turns: Vec<&SectionSpec> = sections
            .iter()
            .filter(|s| matches!(s.primitive, RoadPrimitiveSpec::Turn { .. }))
            .collect();
        assert_eq!(turns.len(), 1);
        let coil = turns[0];

        // The radius is derived from the road available and the revolutions
        // asked for, not taken from the parameter.
        let (length_m, radius_m) = match coil.primitive {
            RoadPrimitiveSpec::Turn { length_m, radius_m, .. } => (length_m, radius_m),
            ref other => panic!("expected a turn, got {other:?}"),
        };
        let revolutions = length_m * (1.0 - 0.22) / (radius_m * std::f32::consts::TAU);
        assert!(
            (revolutions - 1.0).abs() < 0.02,
            "asked for one revolution, the geometry makes {revolutions:.2}"
        );
        assert!(radius_m > 90.0, "and it is not a hairpin: {radius_m:.0} m");

        // It descends, and it leans.
        assert!(coil
            .modifiers
            .iter()
            .any(|m| matches!(m, RoadModifierSpec::GradeProfile { drop_m } if *drop_m == 70.0)));
        assert!(coil.modifiers.iter().any(|m| matches!(
            m,
            RoadModifierSpec::Banking { maximum_rad, .. } if *maximum_rad == 0.25
        )));

        // More revolutions in the same road is a tighter radius.
        let mut tighter = m.clone();
        tighter.params.count = 2;
        let tight_radius = expand(4, &tighter)
            .unwrap()
            .into_iter()
            .find_map(|s| match s.primitive {
                RoadPrimitiveSpec::Turn { radius_m, .. } => Some(radius_m),
                _ => None,
            })
            .unwrap();
        assert!(tight_radius < radius_m, "{tight_radius} vs {radius_m}");

        // And the authored radius is a floor the derivation cannot go under.
        let mut cramped = m.clone();
        cramped.params.count = 8;
        cramped.params.radius_m = ScalarRange::exact(120.0);
        let floored = expand(4, &cramped)
            .unwrap()
            .into_iter()
            .find_map(|s| match s.primitive {
                RoadPrimitiveSpec::Turn { radius_m, .. } => Some(radius_m),
                _ => None,
            })
            .unwrap();
        assert_eq!(floored, 120.0);
    }

    #[test]
    fn the_squeeze_and_the_collapse_really_narrow_the_road() {
        let mut m = invocation(MotifKind::TunnelSqueeze);
        m.params.lanes = CountRange::exact(5);
        m.params.narrow_lanes = 3;
        let sections = expand(4, &m).unwrap();
        assert_eq!(sections.len(), 3);
        assert_eq!(sections[0].lanes, Some(3), "the collapse ends narrow");
        assert_eq!(sections[1].lanes, Some(3), "the corridor holds narrow");
        assert_eq!(sections[2].lanes, Some(5), "and it opens back out");
        match sections[0].primitive {
            RoadPrimitiveSpec::LaneTransition { from_lanes, to_lanes, .. } => {
                assert_eq!((from_lanes, to_lanes), (5, 3));
            }
            ref other => panic!("expected a lane transition, got {other:?}"),
        }

        let mut m = invocation(MotifKind::LaneCollapse);
        m.params.lanes = CountRange::exact(7);
        m.params.narrow_lanes = 3;
        let sections = expand(4, &m).unwrap();
        let ends: Vec<u32> = sections.iter().filter_map(|s| s.lanes).collect();
        assert_eq!(*ends.first().unwrap(), 7);
        assert_eq!(*ends.last().unwrap(), 3, "it collapses and does not recover");
        // Monotone: a collapse never gains a lane back.
        for pair in ends.windows(2) {
            assert!(pair[1] <= pair[0], "the collapse widened: {ends:?}");
        }
    }

    #[test]
    fn a_blind_crest_climbs_and_hides_what_follows() {
        let mut m = invocation(MotifKind::BlindCrest);
        m.params.height_m = 22.0;
        let sections = expand(9, &m).unwrap();
        assert_eq!(sections.len(), 3);
        let has_crest = sections.iter().any(|s| {
            matches!(s.primitive, RoadPrimitiveSpec::Crest { height_m, .. } if height_m == 22.0)
        });
        assert!(has_crest, "the crest is the point of the motif");
        let hidden = sections
            .iter()
            .any(|s| matches!(s.primitive, RoadPrimitiveSpec::Turn { .. }));
        assert!(hidden, "and the road turns away on the far side");
    }

    #[test]
    fn a_rolling_freeway_carries_both_waves_and_keeps_them_continuous() {
        let mut m = invocation(MotifKind::RollingFreeway);
        m.params.count = 4;
        m.params.length_m = 900.0;
        m.params.lateral_amplitude_m = 22.0;
        m.params.elevation_amplitude_m = 14.0;
        m.params.wavelength_m = 260.0;
        let sections = expand(6, &m).unwrap();
        assert_eq!(sections.len(), 4);
        for s in &sections {
            assert_eq!(s.modifiers.len(), 2);
            assert!(s
                .modifiers
                .iter()
                .any(|mo| matches!(mo, RoadModifierSpec::LateralWave { amplitude_m, .. } if *amplitude_m == 22.0)));
            assert!(s
                .modifiers
                .iter()
                .any(|mo| matches!(mo, RoadModifierSpec::ElevationWave { amplitude_m, .. } if *amplitude_m == 14.0)));
        }
        // The phase advances by exactly one piece per piece, so the wave runs
        // through the joins instead of restarting at each one.
        let phase_of = |i: usize| match sections[i].modifiers[0] {
            RoadModifierSpec::LateralWave { phase_rad, .. } => phase_rad,
            ref other => panic!("expected a lateral wave, got {other:?}"),
        };
        let piece = 900.0 / 4.0;
        let expected = std::f32::consts::TAU * piece / 260.0;
        assert!(
            (phase_of(1) - phase_of(0) - expected).abs() < 1.0e-4,
            "phase stepped by {} not {expected}",
            phase_of(1) - phase_of(0)
        );
    }

    #[test]
    fn an_invocations_environment_and_lanes_reach_every_produced_section() {
        let mut m = invocation(MotifKind::HighSpeedSweeps);
        m.environment = Some(SectionKind::Canyon);
        m.expected_speed_mps = Some(64.0);
        m.params.lanes = CountRange::exact(3);
        m.params.radius_m = ScalarRange::new(120.0, 160.0);
        for s in expand(2, &m).unwrap() {
            assert_eq!(s.environment, Some(SectionKind::Canyon));
            assert_eq!(s.expected_speed_mps, Some(64.0));
            assert_eq!(s.lanes, Some(3));
        }
        // A section that set its own lane count keeps it.
        let mut squeeze = invocation(MotifKind::TunnelSqueeze);
        squeeze.params.lanes = CountRange::exact(5);
        squeeze.params.narrow_lanes = 3;
        squeeze.environment = Some(SectionKind::Tunnel);
        let sections = expand(2, &squeeze).unwrap();
        assert_eq!(sections[1].lanes, Some(3), "the corridor keeps its own count");
        assert_eq!(sections[1].environment, Some(SectionKind::Tunnel));
    }
}
