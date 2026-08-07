//! The **semantic road vocabulary**: what a section's road *is*, and what is
//! layered on top of it.
//!
//! Every primitive is stated in course distance and road semantics — a length,
//! a radius, a height, a lane count — and never as a world-space control point.
//! That is the whole reason this layer exists: a control point is a *result*,
//! and an author who edits results cannot be told that their turn is now too
//! tight, because "too tight" is a property of a radius and there is no radius
//! anywhere in a list of points.
//!
//! Each primitive compiles into a continuous **road profile** over its own
//! length — a heading rate, a grade, a bank and a half-width, all functions of
//! the fraction along the section — which the geometry compiler concatenates and
//! integrates once for the whole course. Continuity between sections is
//! therefore not something anybody has to remember: there is one integration and
//! it never restarts.

use crate::course::error::{
    finite, positive, CourseError, CourseErrorCode, CourseResult,
};

/// Which way a turn goes, in **road** terms.
///
/// `Right` is the driver's right, which is `+right` on a [`TrackSample`] and a
/// *positive* curvature. (Which way that ends up on screen is a separate
/// question the app answers once, in `sim::controller` — see the world-chirality
/// note in `ARCHITECTURE.md`.)
///
/// [`TrackSample`]: crate::track::TrackSample
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnDirection {
    /// Toward the driver's left: negative curvature.
    Left,
    /// Toward the driver's right: positive curvature.
    Right,
}

impl TurnDirection {
    /// `+1.0` for a right-hander, `-1.0` for a left-hander.
    pub const fn sign(self) -> f32 {
        match self {
            TurnDirection::Left => -1.0,
            TurnDirection::Right => 1.0,
        }
    }

    /// The other way round.
    pub const fn flipped(self) -> TurnDirection {
        match self {
            TurnDirection::Left => TurnDirection::Right,
            TurnDirection::Right => TurnDirection::Left,
        }
    }

    /// The DSL token.
    pub const fn token(self) -> &'static str {
        match self {
            TurnDirection::Left => "left",
            TurnDirection::Right => "right",
        }
    }

    /// Resolve a DSL token.
    pub fn parse(token: &str) -> Option<TurnDirection> {
        match token {
            "left" => Some(TurnDirection::Left),
            "right" => Some(TurnDirection::Right),
            _ => None,
        }
    }
}

/// One semantic piece of road.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RoadPrimitiveSpec {
    /// Level, straight road.
    Straight {
        /// Arc length (m).
        length_m: f32,
    },
    /// A constant-radius turn.
    ///
    /// The heading rate is `direction / radius_m`, eased in and out over
    /// [`TURN_EASE_FRACTION`] of the length at each end so the curvature is
    /// continuous with whatever the section is joined to.
    Turn {
        /// Arc length (m).
        length_m: f32,
        /// Turn radius (m).
        radius_m: f32,
        /// Which way it goes.
        direction: TurnDirection,
    },
    /// A pair of opposite turns, joined without a straight between them.
    ///
    /// The heading rate is a full sine over the length, so the curvature passes
    /// through zero exactly at the midpoint — which is what makes it one S-bend
    /// rather than two turns that happen to be adjacent.
    SBend {
        /// Arc length (m).
        length_m: f32,
        /// Radius of each half (m).
        radius_m: f32,
        /// Which way the first half goes.
        first: TurnDirection,
    },
    /// A hill that rises and returns to level, cresting at the midpoint.
    Crest {
        /// Arc length (m).
        length_m: f32,
        /// Height gained at the crest (m).
        height_m: f32,
    },
    /// A hollow that falls and returns to level.
    Dip {
        /// Arc length (m).
        length_m: f32,
        /// Depth at the bottom (m).
        depth_m: f32,
    },
    /// Straight road whose banking rolls from one angle to another.
    BankTransition {
        /// Arc length (m).
        length_m: f32,
        /// Bank at the start (rad).
        from_rad: f32,
        /// Bank at the end (rad).
        to_rad: f32,
    },
    /// Straight road that gains or loses lanes.
    ///
    /// The road's *width* is what actually changes; the lane count follows from
    /// it, because there is exactly one definition of where the lanes are
    /// (`Track::lane_reach`) and it reads the width.
    LaneTransition {
        /// Arc length (m).
        length_m: f32,
        /// Lanes at the start.
        from_lanes: u32,
        /// Lanes at the end.
        to_lanes: u32,
    },
    /// Straight road whose tarmac widens or narrows without changing lanes.
    WidthTransition {
        /// Arc length (m).
        length_m: f32,
        /// Half-width at the start (m).
        from_half_width_m: f32,
        /// Half-width at the end (m).
        to_half_width_m: f32,
    },
}

/// How much of a [`RoadPrimitiveSpec::Turn`]'s length is spent easing the
/// curvature in, and the same again easing it out.
///
/// Without it a constant-radius turn is a step change in curvature at both ends,
/// which the geometry compiler's continuity check would (rightly) reject.
pub const TURN_EASE_FRACTION: f32 = 0.22;

impl RoadPrimitiveSpec {
    /// The arc length this primitive occupies (m).
    pub const fn length_m(&self) -> f32 {
        match *self {
            RoadPrimitiveSpec::Straight { length_m }
            | RoadPrimitiveSpec::Turn { length_m, .. }
            | RoadPrimitiveSpec::SBend { length_m, .. }
            | RoadPrimitiveSpec::Crest { length_m, .. }
            | RoadPrimitiveSpec::Dip { length_m, .. }
            | RoadPrimitiveSpec::BankTransition { length_m, .. }
            | RoadPrimitiveSpec::LaneTransition { length_m, .. }
            | RoadPrimitiveSpec::WidthTransition { length_m, .. } => length_m,
        }
    }

    /// The name used in dumps and diagnostics — also the DSL block keyword.
    pub const fn token(&self) -> &'static str {
        match *self {
            RoadPrimitiveSpec::Straight { .. } => "straight",
            RoadPrimitiveSpec::Turn { .. } => "turn",
            RoadPrimitiveSpec::SBend { .. } => "s_bend",
            RoadPrimitiveSpec::Crest { .. } => "crest",
            RoadPrimitiveSpec::Dip { .. } => "dip",
            RoadPrimitiveSpec::BankTransition { .. } => "bank_transition",
            RoadPrimitiveSpec::LaneTransition { .. } => "lane_transition",
            RoadPrimitiveSpec::WidthTransition { .. } => "width_transition",
        }
    }

    /// The signed heading rate (rad/m) at fraction `t` of the way along.
    ///
    /// This is the primitive's whole contribution to the course's *shape*: the
    /// compiler integrates it once, across every section, so a value returned
    /// here is curvature and the integral of it is heading. Nothing ever writes
    /// a position.
    pub fn heading_rate(&self, t: f32) -> f32 {
        match *self {
            RoadPrimitiveSpec::Turn {
                radius_m,
                direction,
                ..
            } => direction.sign() / radius_m.max(1.0e-3) * ease_plateau(t),
            RoadPrimitiveSpec::SBend {
                radius_m, first, ..
            } => first.sign() / radius_m.max(1.0e-3) * (std::f32::consts::TAU * t).sin(),
            _ => 0.0,
        }
    }

    /// The grade (rise over run) at fraction `t` of the way along.
    ///
    /// A crest is `h(t) = height · (1 − cos 2πt) / 2`, whose slope is zero at
    /// both ends — so the hill joins level road without a kink and returns to
    /// the elevation it started at. A dip is its negative.
    pub fn grade(&self, t: f32) -> f32 {
        match *self {
            RoadPrimitiveSpec::Crest { length_m, height_m } => {
                height_m * std::f32::consts::PI / length_m.max(1.0e-3)
                    * (std::f32::consts::TAU * t).sin()
            }
            RoadPrimitiveSpec::Dip { length_m, depth_m } => {
                -depth_m * std::f32::consts::PI / length_m.max(1.0e-3)
                    * (std::f32::consts::TAU * t).sin()
            }
            _ => 0.0,
        }
    }

    /// The banking this primitive asks for at `t`, or `None` if it has no
    /// opinion and the section's banking modifier (or the default
    /// follow-the-curvature rule) decides.
    pub fn bank_rad(&self, t: f32) -> Option<f32> {
        match *self {
            RoadPrimitiveSpec::BankTransition {
                from_rad, to_rad, ..
            } => Some(from_rad + (to_rad - from_rad) * smoothstep(t)),
            _ => None,
        }
    }

    /// The lane count this primitive asks for at `t`, or `None` if it inherits
    /// the section's.
    pub fn lanes(&self, t: f32) -> Option<f32> {
        match *self {
            RoadPrimitiveSpec::LaneTransition {
                from_lanes, to_lanes, ..
            } => Some(from_lanes as f32 + (to_lanes as f32 - from_lanes as f32) * smoothstep(t)),
            _ => None,
        }
    }

    /// The explicit half-width this primitive asks for at `t`, or `None` if the
    /// width follows from the lane count.
    pub fn half_width_m(&self, t: f32) -> Option<f32> {
        match *self {
            RoadPrimitiveSpec::WidthTransition {
                from_half_width_m,
                to_half_width_m,
                ..
            } => Some(
                from_half_width_m + (to_half_width_m - from_half_width_m) * smoothstep(t),
            ),
            _ => None,
        }
    }

    /// Reject an unbuildable primitive. Called once per section before any
    /// geometry is produced, so a bad number can never reach the integrator.
    pub fn validate(&self, min_radius_m: f32) -> CourseResult<()> {
        positive(
            self.length_m(),
            "length_m",
            CourseErrorCode::InvalidSectionLength,
        )?;
        match *self {
            RoadPrimitiveSpec::Turn { radius_m, .. }
            | RoadPrimitiveSpec::SBend { radius_m, .. } => {
                positive(radius_m, "radius_m", CourseErrorCode::InvalidRadius)?;
                (radius_m >= min_radius_m).then_some(()).ok_or_else(|| {
                    CourseError::new(
                        CourseErrorCode::InvalidRadius,
                        format!(
                            "a {radius_m} m radius is tighter than the course minimum of \
                             {min_radius_m} m"
                        ),
                    )
                    .in_field("radius_m")
                })?;
            }
            RoadPrimitiveSpec::Crest { height_m, .. } => {
                positive(height_m, "height_m", CourseErrorCode::InvalidSectionLength)?;
            }
            RoadPrimitiveSpec::Dip { depth_m, .. } => {
                positive(depth_m, "depth_m", CourseErrorCode::InvalidSectionLength)?;
            }
            RoadPrimitiveSpec::BankTransition {
                from_rad, to_rad, ..
            } => {
                finite(from_rad, "from_rad")?;
                finite(to_rad, "to_rad")?;
            }
            RoadPrimitiveSpec::LaneTransition {
                from_lanes,
                to_lanes,
                ..
            } => {
                validate_lane_count(from_lanes, "from")?;
                validate_lane_count(to_lanes, "to")?;
            }
            RoadPrimitiveSpec::WidthTransition {
                from_half_width_m,
                to_half_width_m,
                ..
            } => {
                positive(
                    from_half_width_m,
                    "from_half_width_m",
                    CourseErrorCode::InvalidRoadWidth,
                )?;
                positive(
                    to_half_width_m,
                    "to_half_width_m",
                    CourseErrorCode::InvalidRoadWidth,
                )?;
            }
            RoadPrimitiveSpec::Straight { .. } => {}
        }
        Ok(())
    }
}

/// Reject a lane count the lane lattice cannot represent.
///
/// The count must be **odd** and at least [`crate::track::MIN_LANES`]: one lane
/// sits on the centreline and the rest are paired off either side of it, which
/// is what makes lane 0 the same piece of road for the whole course.
pub fn validate_lane_count(lanes: u32, field: &str) -> CourseResult<()> {
    let min = crate::track::MIN_LANES as u32;
    let max = crate::track::MAX_LANES as u32;
    ((lanes >= min) & (lanes <= max) & (lanes % 2 == 1))
        .then_some(())
        .ok_or_else(|| {
            CourseError::new(
                CourseErrorCode::InvalidLaneCount,
                format!(
                    "`{field}` is {lanes} lanes; the lattice carries an odd count between \
                     {min} and {max} so that lane 0 is always the centreline"
                ),
            )
            .in_field(field)
        })
}

/// A deterministic signal layered on top of a section's base primitive.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RoadModifierSpec {
    /// A sideways weave. Realised as *curvature*, so the road genuinely bends
    /// rather than the centreline being displaced after the fact — a displaced
    /// centreline would leave the tangents pointing where they were before.
    LateralWave {
        /// Peak displacement (m).
        amplitude_m: f32,
        /// Distance of one full cycle (m).
        wavelength_m: f32,
        /// Phase offset (rad).
        phase_rad: f32,
    },
    /// A rolling elevation wave, added to the base grade.
    ElevationWave {
        /// Peak displacement (m).
        amplitude_m: f32,
        /// Distance of one full cycle (m).
        wavelength_m: f32,
        /// Phase offset (rad).
        phase_rad: f32,
    },
    /// How the road is banked through this section.
    Banking {
        /// Where the banking comes from.
        mode: BankingMode,
        /// Scales the mode's output, `0..1`-ish.
        strength: f32,
        /// Hard ceiling on the magnitude (rad).
        maximum_rad: f32,
    },
    /// A sustained change of elevation across the section.
    ///
    /// **The only way to author a net elevation change**, and the reason it is a
    /// modifier rather than a primitive: elevation is orthogonal to what the
    /// road does in plan. A crest and a dip both return to the level they
    /// started at by construction, and an elevation wave is periodic, so before
    /// this existed the only way to get a road that ended lower than it began
    /// was to ride a quarter of a wave whose wavelength you had worked out by
    /// hand. Now a descending turn is a turn with a drop on it.
    ///
    /// The grade is **constant** across the section (`drop / length`) rather
    /// than eased at its ends. That is what lets a figure cut into several
    /// sections descend *continuously* through the joins instead of levelling
    /// off at each one; the compiler's rate limiter is what smooths the ends of
    /// the whole figure, so a section actually falls a little short of the drop
    /// it asks for wherever it meets level road.
    GradeProfile {
        /// How much lower the section ends than it began (m). Negative climbs.
        drop_m: f32,
    },
    /// A width ramp across the section, independent of its lane count.
    WidthProfile {
        /// Half-width at the start (m).
        start_half_width_m: f32,
        /// Half-width at the end (m).
        end_half_width_m: f32,
    },
    /// A lane-count ramp across the section.
    LaneProfile {
        /// Lanes at the start.
        start_lanes: u32,
        /// Lanes at the end.
        end_lanes: u32,
    },
}

/// Where a section's banking comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BankingMode {
    /// Bank into the corner, proportional to the compiled curvature. The
    /// default, and what a road really does.
    FollowCurvature,
    /// A fixed bank across the whole section, whatever the road is doing.
    Fixed,
    /// No banking at all.
    Flat,
}

impl BankingMode {
    /// The DSL token.
    pub const fn token(self) -> &'static str {
        match self {
            BankingMode::FollowCurvature => "follow_curvature",
            BankingMode::Fixed => "fixed",
            BankingMode::Flat => "flat",
        }
    }

    /// Resolve a DSL token.
    pub fn parse(token: &str) -> Option<BankingMode> {
        match token {
            "follow_curvature" => Some(BankingMode::FollowCurvature),
            "fixed" => Some(BankingMode::Fixed),
            "flat" => Some(BankingMode::Flat),
            _ => None,
        }
    }
}

impl RoadModifierSpec {
    /// The name used in dumps and as the DSL block keyword.
    pub const fn token(&self) -> &'static str {
        match *self {
            RoadModifierSpec::LateralWave { .. } => "lateral_wave",
            RoadModifierSpec::ElevationWave { .. } => "elevation_wave",
            RoadModifierSpec::Banking { .. } => "banking",
            RoadModifierSpec::GradeProfile { .. } => "grade_profile",
            RoadModifierSpec::WidthProfile { .. } => "width_profile",
            RoadModifierSpec::LaneProfile { .. } => "lane_profile",
        }
    }

    /// Reject an unbuildable modifier.
    pub fn validate(&self) -> CourseResult<()> {
        match *self {
            RoadModifierSpec::LateralWave {
                amplitude_m,
                wavelength_m,
                phase_rad,
            }
            | RoadModifierSpec::ElevationWave {
                amplitude_m,
                wavelength_m,
                phase_rad,
            } => {
                finite(amplitude_m, "amplitude_m")?;
                positive(
                    wavelength_m,
                    "wavelength_m",
                    CourseErrorCode::InvalidSectionLength,
                )?;
                finite(phase_rad, "phase_rad")?;
            }
            RoadModifierSpec::Banking {
                strength,
                maximum_rad,
                ..
            } => {
                finite(strength, "strength")?;
                finite(maximum_rad, "maximum_rad")?;
                (maximum_rad >= 0.0).then_some(()).ok_or_else(|| {
                    CourseError::new(
                        CourseErrorCode::InvalidFiniteScalar,
                        format!("`maximum_rad` must not be negative, got {maximum_rad}"),
                    )
                    .in_field("maximum_rad")
                })?;
            }
            RoadModifierSpec::GradeProfile { drop_m } => {
                finite(drop_m, "drop_m")?;
            }
            RoadModifierSpec::WidthProfile {
                start_half_width_m,
                end_half_width_m,
            } => {
                positive(
                    start_half_width_m,
                    "start_half_width_m",
                    CourseErrorCode::InvalidRoadWidth,
                )?;
                positive(
                    end_half_width_m,
                    "end_half_width_m",
                    CourseErrorCode::InvalidRoadWidth,
                )?;
            }
            RoadModifierSpec::LaneProfile {
                start_lanes,
                end_lanes,
            } => {
                validate_lane_count(start_lanes, "start_lanes")?;
                validate_lane_count(end_lanes, "end_lanes")?;
            }
        }
        Ok(())
    }
}

impl RoadModifierSpec {
    /// The constant grade a [`Self::GradeProfile`] asks for over a section of
    /// `length_m`, or `None` for every other modifier.
    pub fn sustained_grade(&self, length_m: f32) -> Option<f32> {
        match *self {
            RoadModifierSpec::GradeProfile { drop_m } => Some(-drop_m / length_m.max(1.0e-3)),
            _ => None,
        }
    }
}

/// A flat-topped ease: zero at both ends, one across the middle.
///
/// The plateau is what makes a constant-radius turn *constant* rather than a
/// single sine hump — the middle of the corner really is one radius.
fn ease_plateau(t: f32) -> f32 {
    let e = TURN_EASE_FRACTION.clamp(1.0e-3, 0.49);
    let t = t.clamp(0.0, 1.0);
    let rising = smoothstep(t / e);
    let falling = smoothstep((1.0 - t) / e);
    rising.min(falling).min(1.0)
}

/// The classic C¹ smoothstep, clamped outside `0..1`.
pub fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_turn_holds_its_radius_across_the_middle_and_eases_at_both_ends() {
        let turn = RoadPrimitiveSpec::Turn {
            length_m: 400.0,
            radius_m: 200.0,
            direction: TurnDirection::Right,
        };
        assert_eq!(turn.heading_rate(0.0), 0.0, "curvature starts at zero");
        assert!(turn.heading_rate(1.0).abs() < 1.0e-6, "and ends at zero");
        assert!(
            (turn.heading_rate(0.5) - 1.0 / 200.0).abs() < 1.0e-6,
            "the middle is exactly 1/radius: {}",
            turn.heading_rate(0.5)
        );
        // Right is positive curvature, left is its mirror.
        let left = RoadPrimitiveSpec::Turn {
            length_m: 400.0,
            radius_m: 200.0,
            direction: TurnDirection::Left,
        };
        assert!((left.heading_rate(0.5) + turn.heading_rate(0.5)).abs() < 1.0e-9);
        assert_eq!(TurnDirection::Left.flipped(), TurnDirection::Right);
        assert_eq!(TurnDirection::Right.flipped(), TurnDirection::Left);
    }

    #[test]
    fn an_s_bend_reverses_exactly_at_its_midpoint() {
        let s = RoadPrimitiveSpec::SBend {
            length_m: 300.0,
            radius_m: 150.0,
            first: TurnDirection::Right,
        };
        assert!(s.heading_rate(0.25) > 0.0, "first half turns right");
        assert!(s.heading_rate(0.5).abs() < 1.0e-5, "and reverses at the middle");
        assert!(s.heading_rate(0.75) < 0.0, "second half turns left");
        assert!(s.heading_rate(0.0).abs() < 1.0e-5);
        assert!(s.heading_rate(1.0).abs() < 1.0e-5);
    }

    #[test]
    fn a_crest_rises_and_returns_to_the_level_it_started_at() {
        let crest = RoadPrimitiveSpec::Crest {
            length_m: 180.0,
            height_m: 22.0,
        };
        assert!(crest.grade(0.0).abs() < 1.0e-6, "joins level road");
        assert!(crest.grade(1.0).abs() < 1.0e-5, "and leaves level");
        assert!(crest.grade(0.25) > 0.0, "climbing");
        assert!(crest.grade(0.75) < 0.0, "descending");
        // Integrating the grade over the section recovers the authored height.
        let steps = 4_096;
        let (peak, end) = (0..=steps).fold((0.0f32, 0.0f32), |(peak, height), i| {
            let t = i as f32 / steps as f32;
            let h = height + crest.grade(t) * 180.0 / steps as f32;
            (peak.max(h), h)
        });
        assert!((peak - 22.0).abs() < 0.2, "peaked at {peak} m, wanted 22");
        assert!(end.abs() < 0.05, "and came back to level, ended at {end} m");

        let dip = RoadPrimitiveSpec::Dip {
            length_m: 180.0,
            depth_m: 22.0,
        };
        assert!((dip.grade(0.25) + crest.grade(0.25)).abs() < 1.0e-6, "a dip mirrors a crest");
    }

    #[test]
    fn transitions_ramp_smoothly_between_their_endpoints() {
        let bank = RoadPrimitiveSpec::BankTransition {
            length_m: 200.0,
            from_rad: 0.0,
            to_rad: 0.2,
        };
        assert_eq!(bank.bank_rad(0.0), Some(0.0));
        assert!((bank.bank_rad(1.0).unwrap() - 0.2).abs() < 1.0e-6);
        assert!((bank.bank_rad(0.5).unwrap() - 0.1).abs() < 1.0e-6);
        assert_eq!(bank.lanes(0.5), None);

        let lanes = RoadPrimitiveSpec::LaneTransition {
            length_m: 140.0,
            from_lanes: 5,
            to_lanes: 3,
        };
        assert_eq!(lanes.lanes(0.0), Some(5.0));
        assert!((lanes.lanes(1.0).unwrap() - 3.0).abs() < 1.0e-6);
        assert!((lanes.lanes(0.5).unwrap() - 4.0).abs() < 1.0e-6);
        assert_eq!(lanes.bank_rad(0.5), None);

        let width = RoadPrimitiveSpec::WidthTransition {
            length_m: 100.0,
            from_half_width_m: 6.0,
            to_half_width_m: 9.0,
        };
        assert_eq!(width.half_width_m(0.0), Some(6.0));
        assert!((width.half_width_m(0.5).unwrap() - 7.5).abs() < 1.0e-6);
        assert_eq!(width.half_width_m(2.0), Some(9.0), "clamped past the end");
        assert_eq!(
            RoadPrimitiveSpec::Straight { length_m: 10.0 }.half_width_m(0.5),
            None
        );
    }

    #[test]
    fn every_primitive_reports_its_length_and_a_distinct_token() {
        let all = [
            RoadPrimitiveSpec::Straight { length_m: 10.0 },
            RoadPrimitiveSpec::Turn {
                length_m: 11.0,
                radius_m: 100.0,
                direction: TurnDirection::Left,
            },
            RoadPrimitiveSpec::SBend {
                length_m: 12.0,
                radius_m: 100.0,
                first: TurnDirection::Left,
            },
            RoadPrimitiveSpec::Crest {
                length_m: 13.0,
                height_m: 2.0,
            },
            RoadPrimitiveSpec::Dip {
                length_m: 14.0,
                depth_m: 2.0,
            },
            RoadPrimitiveSpec::BankTransition {
                length_m: 15.0,
                from_rad: 0.0,
                to_rad: 0.1,
            },
            RoadPrimitiveSpec::LaneTransition {
                length_m: 16.0,
                from_lanes: 3,
                to_lanes: 5,
            },
            RoadPrimitiveSpec::WidthTransition {
                length_m: 17.0,
                from_half_width_m: 6.0,
                to_half_width_m: 7.0,
            },
        ];
        let lengths: Vec<f32> = all.iter().map(|p| p.length_m()).collect();
        assert_eq!(lengths, vec![10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0, 17.0]);
        let mut tokens: Vec<&str> = all.iter().map(|p| p.token()).collect();
        tokens.sort_unstable();
        let count = tokens.len();
        tokens.dedup();
        assert_eq!(tokens.len(), count);
        // A straight bends and climbs not at all.
        assert_eq!(all[0].heading_rate(0.5), 0.0);
        assert_eq!(all[0].grade(0.5), 0.0);
        assert_eq!(all[0].lanes(0.5), None);
        assert_eq!(all[0].bank_rad(0.5), None);
    }

    #[test]
    fn invalid_geometry_is_rejected_with_the_right_code() {
        let ok = RoadPrimitiveSpec::Turn {
            length_m: 300.0,
            radius_m: 200.0,
            direction: TurnDirection::Right,
        };
        assert!(ok.validate(90.0).is_ok());

        let zero_length = RoadPrimitiveSpec::Straight { length_m: 0.0 };
        assert_eq!(
            zero_length.validate(90.0).unwrap_err().code,
            CourseErrorCode::InvalidSectionLength
        );
        let negative = RoadPrimitiveSpec::Straight { length_m: -5.0 };
        assert_eq!(
            negative.validate(90.0).unwrap_err().code,
            CourseErrorCode::InvalidSectionLength
        );
        let hairpin = RoadPrimitiveSpec::Turn {
            length_m: 300.0,
            radius_m: 20.0,
            direction: TurnDirection::Right,
        };
        assert_eq!(
            hairpin.validate(90.0).unwrap_err().code,
            CourseErrorCode::InvalidRadius
        );
        let no_radius = RoadPrimitiveSpec::SBend {
            length_m: 300.0,
            radius_m: 0.0,
            first: TurnDirection::Right,
        };
        assert_eq!(
            no_radius.validate(90.0).unwrap_err().code,
            CourseErrorCode::InvalidRadius
        );
        let flat_crest = RoadPrimitiveSpec::Crest {
            length_m: 100.0,
            height_m: 0.0,
        };
        assert!(flat_crest.validate(90.0).is_err());
        let flat_dip = RoadPrimitiveSpec::Dip {
            length_m: 100.0,
            depth_m: -1.0,
        };
        assert!(flat_dip.validate(90.0).is_err());
        let nan_bank = RoadPrimitiveSpec::BankTransition {
            length_m: 100.0,
            from_rad: f32::NAN,
            to_rad: 0.0,
        };
        assert_eq!(
            nan_bank.validate(90.0).unwrap_err().code,
            CourseErrorCode::InvalidFiniteScalar
        );
        let even_lanes = RoadPrimitiveSpec::LaneTransition {
            length_m: 100.0,
            from_lanes: 4,
            to_lanes: 3,
        };
        assert_eq!(
            even_lanes.validate(90.0).unwrap_err().code,
            CourseErrorCode::InvalidLaneCount
        );
        let too_wide = RoadPrimitiveSpec::WidthTransition {
            length_m: 100.0,
            from_half_width_m: 0.0,
            to_half_width_m: 6.0,
        };
        assert_eq!(
            too_wide.validate(90.0).unwrap_err().code,
            CourseErrorCode::InvalidRoadWidth
        );
        let bad_end = RoadPrimitiveSpec::WidthTransition {
            length_m: 100.0,
            from_half_width_m: 6.0,
            to_half_width_m: -1.0,
        };
        assert!(bad_end.validate(90.0).is_err());
    }

    #[test]
    fn modifiers_validate_their_own_numbers() {
        assert!(RoadModifierSpec::LateralWave {
            amplitude_m: 22.0,
            wavelength_m: 260.0,
            phase_rad: 0.0,
        }
        .validate()
        .is_ok());
        assert_eq!(
            RoadModifierSpec::LateralWave {
                amplitude_m: 22.0,
                wavelength_m: 0.0,
                phase_rad: 0.0,
            }
            .validate()
            .unwrap_err()
            .code,
            CourseErrorCode::InvalidSectionLength
        );
        assert_eq!(
            RoadModifierSpec::ElevationWave {
                amplitude_m: f32::NAN,
                wavelength_m: 100.0,
                phase_rad: 0.0,
            }
            .validate()
            .unwrap_err()
            .code,
            CourseErrorCode::InvalidFiniteScalar
        );
        assert!(RoadModifierSpec::ElevationWave {
            amplitude_m: 1.0,
            wavelength_m: 100.0,
            phase_rad: f32::INFINITY,
        }
        .validate()
        .is_err());
        assert!(RoadModifierSpec::Banking {
            mode: BankingMode::FollowCurvature,
            strength: 0.8,
            maximum_rad: 0.3,
        }
        .validate()
        .is_ok());
        assert!(RoadModifierSpec::Banking {
            mode: BankingMode::Fixed,
            strength: 0.8,
            maximum_rad: -0.3,
        }
        .validate()
        .is_err());
        assert!(RoadModifierSpec::Banking {
            mode: BankingMode::Flat,
            strength: f32::NAN,
            maximum_rad: 0.3,
        }
        .validate()
        .is_err());
        assert!(RoadModifierSpec::GradeProfile { drop_m: 40.0 }.validate().is_ok());
        assert!(RoadModifierSpec::GradeProfile { drop_m: -40.0 }
            .validate()
            .is_ok(), "a negative drop climbs");
        assert!(RoadModifierSpec::GradeProfile { drop_m: f32::NAN }
            .validate()
            .is_err());
        assert!(RoadModifierSpec::WidthProfile {
            start_half_width_m: 6.0,
            end_half_width_m: 9.0,
        }
        .validate()
        .is_ok());
        assert!(RoadModifierSpec::WidthProfile {
            start_half_width_m: 6.0,
            end_half_width_m: 0.0,
        }
        .validate()
        .is_err());
        assert!(RoadModifierSpec::WidthProfile {
            start_half_width_m: -1.0,
            end_half_width_m: 4.0,
        }
        .validate()
        .is_err());
        assert!(RoadModifierSpec::LaneProfile {
            start_lanes: 3,
            end_lanes: 5,
        }
        .validate()
        .is_ok());
        assert!(RoadModifierSpec::LaneProfile {
            start_lanes: 3,
            end_lanes: 4,
        }
        .validate()
        .is_err());
        assert!(RoadModifierSpec::LaneProfile {
            start_lanes: 2,
            end_lanes: 3,
        }
        .validate()
        .is_err());
        let mut tokens: Vec<&str> = [
            RoadModifierSpec::LateralWave {
                amplitude_m: 1.0,
                wavelength_m: 1.0,
                phase_rad: 0.0,
            },
            RoadModifierSpec::ElevationWave {
                amplitude_m: 1.0,
                wavelength_m: 1.0,
                phase_rad: 0.0,
            },
            RoadModifierSpec::Banking {
                mode: BankingMode::Flat,
                strength: 1.0,
                maximum_rad: 0.1,
            },
            RoadModifierSpec::GradeProfile { drop_m: 1.0 },
            RoadModifierSpec::WidthProfile {
                start_half_width_m: 6.0,
                end_half_width_m: 6.0,
            },
            RoadModifierSpec::LaneProfile {
                start_lanes: 3,
                end_lanes: 3,
            },
        ]
        .iter()
        .map(|m| m.token())
        .collect();
        tokens.sort_unstable();
        let count = tokens.len();
        tokens.dedup();
        assert_eq!(tokens.len(), count);
    }

    #[test]
    fn banking_modes_and_turn_directions_round_trip_through_their_tokens() {
        for m in [
            BankingMode::FollowCurvature,
            BankingMode::Fixed,
            BankingMode::Flat,
        ] {
            assert_eq!(BankingMode::parse(m.token()), Some(m));
        }
        assert_eq!(BankingMode::parse("wobble"), None);
        for d in [TurnDirection::Left, TurnDirection::Right] {
            assert_eq!(TurnDirection::parse(d.token()), Some(d));
        }
        assert_eq!(TurnDirection::parse("sideways"), None);
    }

    #[test]
    fn the_lane_count_guard_matches_the_lattice() {
        assert!(validate_lane_count(3, "lanes").is_ok());
        assert!(validate_lane_count(5, "lanes").is_ok());
        assert!(validate_lane_count(7, "lanes").is_ok());
        assert!(validate_lane_count(1, "lanes").is_err());
        assert!(validate_lane_count(4, "lanes").is_err());
        assert!(validate_lane_count(9, "lanes").is_err());
    }

    /// A drop over a length is a grade, and a section that carries one really
    /// does end where it said it would.
    #[test]
    fn a_grade_profile_states_a_drop_and_yields_the_grade_that_makes_it() {
        let drop = RoadModifierSpec::GradeProfile { drop_m: 50.0 };
        let grade = drop.sustained_grade(500.0).expect("a grade");
        assert!((grade + 0.1).abs() < 1.0e-6, "50 m over 500 m is -10%: {grade}");
        // Integrating a constant grade over the length recovers the drop.
        assert!((grade * 500.0 + 50.0).abs() < 1.0e-3);
        // Negative drops climb.
        let climb = RoadModifierSpec::GradeProfile { drop_m: -50.0 };
        assert!((climb.sustained_grade(500.0).unwrap() - 0.1).abs() < 1.0e-6);
        // A degenerate length cannot divide by zero.
        assert!(drop.sustained_grade(0.0).unwrap().is_finite());
        // And nothing else claims a sustained grade.
        assert_eq!(
            RoadModifierSpec::WidthProfile {
                start_half_width_m: 6.0,
                end_half_width_m: 6.0
            }
            .sustained_grade(100.0),
            None
        );
    }

    #[test]
    fn the_easing_helpers_are_bounded_and_clamped() {
        assert_eq!(smoothstep(-1.0), 0.0);
        assert_eq!(smoothstep(0.0), 0.0);
        assert_eq!(smoothstep(1.0), 1.0);
        assert_eq!(smoothstep(2.0), 1.0);
        assert!((smoothstep(0.5) - 0.5).abs() < 1.0e-6);
        for i in 0..=100 {
            let t = i as f32 / 100.0;
            assert!((0.0..=1.0).contains(&ease_plateau(t)), "t={t}");
        }
        assert_eq!(ease_plateau(-1.0), 0.0);
        assert_eq!(ease_plateau(2.0), 0.0);
    }
}
