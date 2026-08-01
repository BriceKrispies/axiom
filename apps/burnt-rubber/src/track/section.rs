//! The course's **pacing curve**: which kind of road comes in what order, and
//! with what generation parameters.
//!
//! This is the piece that stops a procedural course from being uniform noise. A
//! nine-kilometre road built from one set of parameters is nine kilometres of
//! the same road; a driver learns it in ten seconds and is bored by the second
//! minute. So the course is authored as an *ordered list of section profiles* —
//! open the run with a straight so acceleration reads, then long sweepers, then
//! crests, then something technical, then the enclosed tunnel, then the wide
//! traffic straight, then the canyon squeeze, then a closing sweep — and each
//! section's numbers are what the generator draws its random values *within*.
//!
//! The randomness lives inside a section, never across it: the same seed always
//! produces the same bends, and every seed produces the same *shape of run*.

/// A stretch of road with its own character.
///
/// The order of the variants is the order they appear in the course.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SectionKind {
    /// Flat and straight: learn the throttle and the steering.
    StartStraight,
    /// Long, fast, open bends with posts marching past.
    SweepingBends,
    /// Rolling hills into a visible crest that unloads the car.
    RollingHills,
    /// Faster, tighter, alternating bends — the first place the handbrake helps.
    TechnicalBends,
    /// Enclosed: barriers both sides, ceiling, repeated lights.
    Tunnel,
    /// Wide, flat, flat-out — and full of traffic to thread.
    HighSpeedStraight,
    /// Narrow with rock walls close on both sides: peripheral motion at its
    /// most violent.
    Canyon,
    /// A closing sequence of sweepers to finish on.
    FinalSweeps,
    /// The finish area: straight, wide, unmistakable.
    Finish,
}

impl SectionKind {
    /// Every section, in course order.
    pub const ALL: [SectionKind; 9] = [
        SectionKind::StartStraight,
        SectionKind::SweepingBends,
        SectionKind::RollingHills,
        SectionKind::TechnicalBends,
        SectionKind::Tunnel,
        SectionKind::HighSpeedStraight,
        SectionKind::Canyon,
        SectionKind::FinalSweeps,
        SectionKind::Finish,
    ];

    /// The name the HUD shows.
    pub const fn name(self) -> &'static str {
        match self {
            SectionKind::StartStraight => "OPENING STRAIGHT",
            SectionKind::SweepingBends => "COASTAL SWEEPERS",
            SectionKind::RollingHills => "RIDGE CRESTS",
            SectionKind::TechnicalBends => "THE ESSES",
            SectionKind::Tunnel => "TUNNEL",
            SectionKind::HighSpeedStraight => "THE LONG HAUL",
            SectionKind::Canyon => "CANYON RUN",
            SectionKind::FinalSweeps => "FINAL SWEEP",
            SectionKind::Finish => "FINISH",
        }
    }

    /// The scenery vocabulary this section's roadside is drawn from.
    pub const fn zone(self) -> Zone {
        match self {
            SectionKind::StartStraight => Zone::Meadow,
            SectionKind::SweepingBends => Zone::Coast,
            SectionKind::RollingHills => Zone::Forest,
            SectionKind::TechnicalBends => Zone::Forest,
            SectionKind::Tunnel => Zone::Tunnel,
            SectionKind::HighSpeedStraight => Zone::Industrial,
            SectionKind::Canyon => Zone::Canyon,
            SectionKind::FinalSweeps => Zone::Coast,
            SectionKind::Finish => Zone::Meadow,
        }
    }

    /// Whether this section demands a continuous guardrail on both sides. The
    /// enclosed sections do, because their whole job is peripheral motion right
    /// next to the camera.
    pub const fn walled(self) -> bool {
        matches!(self, SectionKind::Tunnel | SectionKind::Canyon)
    }

    /// The generation profile for this section.
    pub const fn profile(self) -> SectionProfile {
        match self {
            SectionKind::StartStraight => SectionProfile {
                length: 620.0,
                curviness: 0.10,
                bend_points: (2, 3),
                straight_points: (6, 9),
                hilliness: 0.0,
                hill_points: (3, 4),
                hill_gap: (8, 12),
                half_width: 9.0,
                width_jitter: 0.0,
            },
            SectionKind::SweepingBends => SectionProfile {
                length: 1700.0,
                curviness: 0.78,
                bend_points: (5, 9),
                straight_points: (2, 5),
                hilliness: 0.28,
                hill_points: (4, 7),
                hill_gap: (5, 10),
                half_width: 8.5,
                width_jitter: 0.8,
            },
            SectionKind::RollingHills => SectionProfile {
                length: 1250.0,
                curviness: 0.42,
                bend_points: (4, 7),
                straight_points: (3, 6),
                hilliness: 1.0,
                hill_points: (2, 4),
                hill_gap: (2, 4),
                half_width: 8.0,
                width_jitter: 0.6,
            },
            SectionKind::TechnicalBends => SectionProfile {
                length: 1150.0,
                curviness: 1.0,
                bend_points: (3, 5),
                straight_points: (1, 3),
                hilliness: 0.35,
                hill_points: (3, 5),
                hill_gap: (4, 7),
                half_width: 7.0,
                width_jitter: 0.5,
            },
            SectionKind::Tunnel => SectionProfile {
                length: 780.0,
                curviness: 0.34,
                bend_points: (4, 7),
                straight_points: (2, 4),
                hilliness: 0.12,
                hill_points: (3, 5),
                hill_gap: (6, 10),
                half_width: 7.5,
                width_jitter: 0.0,
            },
            SectionKind::HighSpeedStraight => SectionProfile {
                length: 1500.0,
                curviness: 0.22,
                bend_points: (6, 10),
                straight_points: (5, 9),
                hilliness: 0.15,
                hill_points: (4, 7),
                hill_gap: (7, 12),
                half_width: 10.5,
                width_jitter: 0.4,
            },
            SectionKind::Canyon => SectionProfile {
                length: 1100.0,
                curviness: 0.86,
                bend_points: (3, 6),
                straight_points: (1, 3),
                hilliness: 0.45,
                hill_points: (3, 5),
                hill_gap: (3, 6),
                half_width: 6.5,
                width_jitter: 0.4,
            },
            SectionKind::FinalSweeps => SectionProfile {
                length: 850.0,
                curviness: 0.70,
                bend_points: (5, 8),
                straight_points: (2, 4),
                hilliness: 0.30,
                hill_points: (3, 6),
                hill_gap: (5, 9),
                half_width: 9.0,
                width_jitter: 0.5,
            },
            SectionKind::Finish => SectionProfile {
                length: 320.0,
                curviness: 0.0,
                bend_points: (2, 3),
                straight_points: (8, 12),
                hilliness: 0.0,
                hill_points: (3, 4),
                hill_gap: (8, 12),
                half_width: 11.0,
                width_jitter: 0.0,
            },
        }
    }

    /// The section covering `fraction` (0..1) of the course by *authored*
    /// length. The generator resolves the exact sample ranges; this is the
    /// coarse plan.
    pub fn at_fraction(fraction: f32) -> SectionKind {
        let total = Self::total_length();
        let target = fraction.clamp(0.0, 1.0) * total;
        let mut walked = 0.0;
        for kind in SectionKind::ALL {
            walked += kind.profile().length;
            if target <= walked {
                return kind;
            }
        }
        SectionKind::Finish
    }

    /// The authored total course length in metres.
    pub fn total_length() -> f32 {
        SectionKind::ALL
            .iter()
            .map(|k| k.profile().length)
            .sum::<f32>()
    }
}

/// The scenery vocabulary a section's roadside draws from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Zone {
    Meadow,
    Coast,
    Forest,
    Tunnel,
    Industrial,
    Canyon,
}

impl Zone {
    /// Every zone, in a stable order (the scenery pool is grouped by it).
    pub const ALL: [Zone; 6] = [
        Zone::Meadow,
        Zone::Coast,
        Zone::Forest,
        Zone::Tunnel,
        Zone::Industrial,
        Zone::Canyon,
    ];

    /// The zone's stable index — used to address per-zone pools by arithmetic
    /// rather than by search.
    pub const fn index(self) -> usize {
        match self {
            Zone::Meadow => 0,
            Zone::Coast => 1,
            Zone::Forest => 2,
            Zone::Tunnel => 3,
            Zone::Industrial => 4,
            Zone::Canyon => 5,
        }
    }
}

/// The numeric envelope the generator draws a section's road inside.
///
/// `*_points` pairs are inclusive ranges measured in **control points** (see
/// [`crate::tuning::CourseTuning::control_spacing`]), so a `(5, 9)` bend is
/// 200–360 m of arc — a genuine sweeper, not a kink.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SectionProfile {
    /// Authored length of the section (m).
    pub length: f32,
    /// Peak bend severity as a fraction of the maximum legal heading step.
    pub curviness: f32,
    /// Inclusive range of control points one bend occupies.
    pub bend_points: (u32, u32),
    /// Inclusive range of control points between bends.
    pub straight_points: (u32, u32),
    /// Peak hill severity as a fraction of the maximum legal grade.
    pub hilliness: f32,
    /// Inclusive range of control points one hill occupies.
    pub hill_points: (u32, u32),
    /// Inclusive range of control points between hills.
    pub hill_gap: (u32, u32),
    /// The section's nominal road half-width (m).
    pub half_width: f32,
    /// How much the half-width is allowed to wander within the section (m).
    pub width_jitter: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_authored_course_is_the_advertised_length() {
        let total = SectionKind::total_length();
        assert!(
            (8_000.0..=10_000.0).contains(&total),
            "the course is 8-10 km, got {total} m"
        );
    }

    #[test]
    fn every_section_names_itself_and_picks_a_zone() {
        for kind in SectionKind::ALL {
            assert!(!kind.name().is_empty());
            assert!(Zone::ALL.contains(&kind.zone()));
            assert!(kind.profile().length > 0.0);
        }
    }

    #[test]
    fn the_enclosed_sections_are_the_walled_ones() {
        assert!(SectionKind::Tunnel.walled());
        assert!(SectionKind::Canyon.walled());
        assert!(!SectionKind::StartStraight.walled());
        assert!(!SectionKind::HighSpeedStraight.walled());
    }

    #[test]
    fn fractional_lookup_walks_the_sections_in_order() {
        assert_eq!(SectionKind::at_fraction(0.0), SectionKind::StartStraight);
        assert_eq!(SectionKind::at_fraction(-1.0), SectionKind::StartStraight);
        assert_eq!(SectionKind::at_fraction(1.0), SectionKind::Finish);
        assert_eq!(SectionKind::at_fraction(2.0), SectionKind::Finish);
        // The order the fractions resolve in is the declared order.
        let walked: Vec<SectionKind> = (0..200)
            .map(|i| SectionKind::at_fraction(i as f32 / 199.0))
            .collect();
        let mut seen: Vec<SectionKind> = Vec::new();
        for kind in walked {
            if seen.last() != Some(&kind) {
                seen.push(kind);
            }
        }
        assert_eq!(seen, SectionKind::ALL.to_vec());
    }

    #[test]
    fn zone_indices_are_dense_and_unique() {
        let indices: Vec<usize> = Zone::ALL.iter().map(|z| z.index()).collect();
        assert_eq!(indices, (0..Zone::ALL.len()).collect::<Vec<_>>());
    }

    /// The profiles have to be orderable by character or the pacing curve is a
    /// fiction: the straights must be straighter than the technical bends, and
    /// the hills must be hillier than everything else.
    #[test]
    fn the_pacing_curve_actually_varies() {
        let straight = SectionKind::StartStraight.profile();
        let esses = SectionKind::TechnicalBends.profile();
        let hills = SectionKind::RollingHills.profile();
        assert!(straight.curviness < esses.curviness);
        assert!(hills.hilliness > esses.hilliness);
        assert!(
            SectionKind::HighSpeedStraight.profile().half_width
                > SectionKind::Canyon.profile().half_width,
            "the canyon squeezes and the long haul opens up"
        );
    }
}
