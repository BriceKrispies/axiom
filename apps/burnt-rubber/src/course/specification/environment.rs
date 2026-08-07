//! The **environment profile** a section names: what a stretch of road is
//! called, what its roadside is drawn from, and whether it is enclosed.
//!
//! This used to be the course's *pacing plan* — a fixed list of nine section
//! kinds, each carrying the generation envelope its road was drawn inside. That
//! is now the authored [`CourseSpec`](super::CourseSpec)'s job: a section states
//! its own geometry as primitives and modifiers, and names one of these as its
//! **scenery/environment reference**.
//!
//! What is left here is exactly the part the *renderer* consumes and the
//! generator does not: the HUD's name, the scenery vocabulary
//! ([`Zone`]), and whether the section demands a continuous wall. Several
//! sections may name the same environment, and the shipping course's nine
//! sections happen to name nine different ones.

/// The character of a stretch of road: its name, its roadside vocabulary and
/// whether it is walled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SectionKind {
    /// Flat and open: the opening straight.
    StartStraight,
    /// Long, fast, open bends with posts marching past.
    SweepingBends,
    /// Rolling hills into a visible crest that unloads the car.
    RollingHills,
    /// Faster, tighter, alternating bends — where the handbrake helps.
    TechnicalBends,
    /// Enclosed: barriers both sides, ceiling, repeated lights.
    Tunnel,
    /// Wide, flat, flat-out — and full of traffic to thread.
    HighSpeedStraight,
    /// Narrow with rock walls close on both sides.
    Canyon,
    /// A closing sequence of sweepers.
    FinalSweeps,
    /// The finish area: straight, wide, unmistakable.
    Finish,
}

impl SectionKind {
    /// Every environment, in the order the shipping course uses them.
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

    /// The token the DSL names this environment by.
    pub const fn token(self) -> &'static str {
        match self {
            SectionKind::StartStraight => "start_straight",
            SectionKind::SweepingBends => "sweeping_bends",
            SectionKind::RollingHills => "rolling_hills",
            SectionKind::TechnicalBends => "technical_bends",
            SectionKind::Tunnel => "tunnel",
            SectionKind::HighSpeedStraight => "high_speed_straight",
            SectionKind::Canyon => "canyon",
            SectionKind::FinalSweeps => "final_sweeps",
            SectionKind::Finish => "finish",
        }
    }

    /// Resolve a DSL token.
    pub fn parse(token: &str) -> Option<SectionKind> {
        SectionKind::ALL.into_iter().find(|k| k.token() == token)
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
    ///
    /// This is also the renderer's only way to express a **tunnel or a walled
    /// corridor**, which is why those are the two structures the specification
    /// offers: a section can be enclosed because the road mesh and the scenery
    /// pool already know how to draw that, and cannot be a bridge because
    /// nothing in the renderer knows how to draw one.
    pub const fn walled(self) -> bool {
        matches!(self, SectionKind::Tunnel | SectionKind::Canyon)
    }
}

/// The scenery vocabulary a section's roadside draws from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Zone {
    /// Open grassland.
    Meadow,
    /// Coastline.
    Coast,
    /// Woodland.
    Forest,
    /// Enclosed, lit.
    Tunnel,
    /// Warehouses and gantries.
    Industrial,
    /// Rock walls.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_environment_names_itself_and_picks_a_zone() {
        for kind in SectionKind::ALL {
            assert!(!kind.name().is_empty());
            assert!(!kind.token().is_empty());
            assert!(Zone::ALL.contains(&kind.zone()));
        }
    }

    #[test]
    fn every_environment_round_trips_through_its_dsl_token() {
        for kind in SectionKind::ALL {
            assert_eq!(SectionKind::parse(kind.token()), Some(kind));
        }
        assert_eq!(SectionKind::parse("swamp"), None);
        // Tokens are distinct, or two authored environments would collide.
        let mut tokens: Vec<&str> = SectionKind::ALL.iter().map(|k| k.token()).collect();
        tokens.sort_unstable();
        let count = tokens.len();
        tokens.dedup();
        assert_eq!(tokens.len(), count);
    }

    #[test]
    fn the_enclosed_environments_are_the_walled_ones() {
        assert!(SectionKind::Tunnel.walled());
        assert!(SectionKind::Canyon.walled());
        assert!(!SectionKind::StartStraight.walled());
        assert!(!SectionKind::HighSpeedStraight.walled());
        assert!(!SectionKind::Finish.walled());
    }

    #[test]
    fn zone_indices_are_dense_and_unique() {
        let indices: Vec<usize> = Zone::ALL.iter().map(|z| z.index()).collect();
        assert_eq!(indices, (0..Zone::ALL.len()).collect::<Vec<_>>());
    }
}
