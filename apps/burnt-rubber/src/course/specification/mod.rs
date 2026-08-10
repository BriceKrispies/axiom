//! **The authored course**, as a typed value.
//!
//! This is the one representation a human (or a generator, or the DSL parser)
//! writes, and everything downstream is derived from it. It is deliberately
//! *semantic*: a section is a length and a radius and a lane count, never a list
//! of world-space points, because the point of authoring a road is to be able to
//! say what the road is and be told when that is impossible.
//!
//! There is **no untyped property map** anywhere in it. An unknown field is a
//! parse error ([`CourseErrorCode::UnknownField`]), not a silently-ignored key,
//! which is what makes a typo in a course source a build failure rather than a
//! section that quietly does nothing.
//!
//! ```text
//! CourseSpec
//!  ├── CourseDefaults            lanes, lane width, shoulder, expected speed
//!  ├── ValidationThresholds      what the validator judges against
//!  └── [CourseItem]              in course order
//!        ├── Section(SectionSpec)         one primitive + modifiers
//!        ├── Group(SectionGroupSpec)      several primitives + one traffic zone
//!        └── Motif(MotifInvocation)       expands into the above
//! ```
//!
//! Each of those three carries the same two placement lists over its own span:
//! a traffic zone ([`TrafficZoneSpec`]) and a set of boost pickups
//! ([`BoostPickupSpec`]). They are deliberately separate lists rather than one:
//! traffic is a *density description* the compiler generates vehicles from, and
//! a pickup is a *placement* the author wrote out. Folding a pickup into the
//! traffic zone would also put it in the list the collision resolver scans and
//! the traversability grid treats as blocking, which is the one thing a pickup
//! must never be.
//!
//! [`CourseErrorCode::UnknownField`]: crate::course::error::CourseErrorCode::UnknownField

pub mod builder;
pub mod environment;
pub mod ids;
pub mod motif;
pub mod pickup;
pub mod road;
pub mod thresholds;
pub mod traffic;
pub mod units;

use crate::course::error::{finite, positive, CourseError, CourseErrorCode, CourseResult};

pub use builder::CourseBuilder;
pub use environment::{SectionKind, Zone};
pub use ids::{EncounterId, PickupId, SectionId, VehicleId};
pub use motif::{MotifInvocation, MotifKind, MotifParams, MAX_MOTIF_COUNT};
pub use pickup::{BoostPickupSpec, BoostTier, MAX_PICKUP_ROW};
pub use road::{
    BankingMode, RoadModifierSpec, RoadPrimitiveSpec, TurnDirection,
};
pub use thresholds::ValidationThresholds;
pub use traffic::{
    EncounterSpec, LaneWeight, NearMissWindowSpec, PassingSide, RollingWallSpec, SlalomSpec,
    TrafficFlowSpec, TrafficZoneSpec, VehicleArchetype, ZipperSpec,
};
pub use units::{CountRange, Dimension, ScalarRange, Unit};

/// The values every section inherits unless it says otherwise.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CourseDefaults {
    /// Lanes a section carries when it does not author its own count.
    pub lanes: u32,
    /// The width of one lane (m).
    ///
    /// **Constant for the whole course**, by construction: `Track::lane_lateral`
    /// puts lane `n` at `n · lane_width` for every metre of the course, which is
    /// what makes a lane a durable identity rather than an ordinal into a list
    /// that changes length. A per-section lane width would break that, so the
    /// specification does not offer one.
    pub lane_width_m: f32,
    /// The paved margin between the outermost lane and the edge of the tarmac
    /// (m).
    pub shoulder_width_m: f32,
    /// The speed a competent player is expected to be carrying (m/s). Traffic
    /// closing speeds, reaction times and the boost budget are all measured
    /// against it.
    pub expected_speed_mps: f32,
    /// The environment a section inherits when it does not name one.
    pub environment: SectionKind,
}

impl CourseDefaults {
    /// The shipping defaults — the road the demo course is built on.
    pub const DEFAULT: CourseDefaults = CourseDefaults {
        lanes: 5,
        lane_width_m: 3.5,
        shoulder_width_m: 0.75,
        expected_speed_mps: 80.0,
        environment: SectionKind::StartStraight,
    };

    /// Reject defaults the road cannot be built on.
    pub fn validate(&self) -> CourseResult<()> {
        road::validate_lane_count(self.lanes, "lanes")?;
        positive(
            self.lane_width_m,
            "lane_width",
            CourseErrorCode::InvalidLaneWidth,
        )?;
        finite(self.shoulder_width_m, "shoulder_width")?;
        (self.shoulder_width_m >= 0.0).then_some(()).ok_or_else(|| {
            CourseError::new(
                CourseErrorCode::InvalidRoadWidth,
                format!(
                    "`shoulder_width` must not be negative, got {}",
                    self.shoulder_width_m
                ),
            )
            .in_field("shoulder_width")
        })?;
        positive(
            self.expected_speed_mps,
            "expected_speed",
            CourseErrorCode::InvalidSpeedRange,
        )?;
        Ok(())
    }
}

impl Default for CourseDefaults {
    fn default() -> Self {
        CourseDefaults::DEFAULT
    }
}

/// One authored piece of road: a primitive, plus whatever is layered on it.
#[derive(Debug, Clone, PartialEq)]
pub struct SectionSpec {
    /// The stable name every seed and diagnostic for this road is anchored on.
    pub id: SectionId,
    /// What the road is.
    pub primitive: RoadPrimitiveSpec,
    /// Deterministic signals layered on top, in order.
    pub modifiers: Vec<RoadModifierSpec>,
    /// The lane count, or the course default.
    pub lanes: Option<u32>,
    /// The expected player speed here (m/s), or the course default.
    pub expected_speed_mps: Option<f32>,
    /// The environment/scenery profile, or the course default.
    pub environment: Option<SectionKind>,
    /// Traffic authored directly on this section. A section inside a group
    /// leaves this empty and the group's zone covers it.
    pub traffic: Option<TrafficZoneSpec>,
    /// Boost pickups placed on this section, at offsets from its start.
    pub pickups: Vec<BoostPickupSpec>,
}

impl SectionSpec {
    /// A section that is just `primitive`, named `id`.
    pub fn new(id: SectionId, primitive: RoadPrimitiveSpec) -> SectionSpec {
        SectionSpec {
            id,
            primitive,
            modifiers: Vec::new(),
            lanes: None,
            expected_speed_mps: None,
            environment: None,
            traffic: None,
            pickups: Vec::new(),
        }
    }

    /// Add a modifier.
    pub fn with_modifier(mut self, modifier: RoadModifierSpec) -> SectionSpec {
        self.modifiers.push(modifier);
        self
    }

    /// Set the lane count.
    pub fn with_lanes(mut self, lanes: u32) -> SectionSpec {
        self.lanes = Some(lanes);
        self
    }

    /// Set the environment.
    pub fn with_environment(mut self, environment: SectionKind) -> SectionSpec {
        self.environment = Some(environment);
        self
    }

    /// Set the expected player speed (m/s).
    pub fn with_expected_speed(mut self, speed_mps: f32) -> SectionSpec {
        self.expected_speed_mps = Some(speed_mps);
        self
    }

    /// Attach traffic.
    pub fn with_traffic(mut self, traffic: TrafficZoneSpec) -> SectionSpec {
        self.traffic = Some(traffic);
        self
    }

    /// Place a boost pickup, or a row of them.
    pub fn with_pickup(mut self, pickup: BoostPickupSpec) -> SectionSpec {
        self.pickups.push(pickup);
        self
    }
}

/// Several primitives that share an id, an environment and one traffic zone.
///
/// This is what `section "tunnel_squeeze" { straight {…} lane_transition {…}
/// traffic {…} }` produces: three pieces of road under one name, and one traffic
/// zone spanning all of them. The compiled course sees only ordinary sections
/// (`tunnel_squeeze/0`, `tunnel_squeeze/1`) and one zone over their combined
/// span.
#[derive(Debug, Clone, PartialEq)]
pub struct SectionGroupSpec {
    /// The stable name the parts are minted under.
    pub id: SectionId,
    /// The parts, in course order. Each carries its own primitive and modifiers;
    /// their `id` is ignored and re-minted from the group's.
    pub parts: Vec<SectionSpec>,
    /// The lane count the parts inherit.
    pub lanes: Option<u32>,
    /// The expected player speed the parts inherit (m/s).
    pub expected_speed_mps: Option<f32>,
    /// The environment the parts inherit.
    pub environment: Option<SectionKind>,
    /// Traffic across the whole group.
    pub traffic: Option<TrafficZoneSpec>,
    /// Boost pickups across the whole group, at offsets from the *group's*
    /// start — the parts are one span as far as placement is concerned.
    pub pickups: Vec<BoostPickupSpec>,
}

impl SectionGroupSpec {
    /// An empty group named `id`.
    pub fn new(id: SectionId) -> SectionGroupSpec {
        SectionGroupSpec {
            id,
            parts: Vec::new(),
            lanes: None,
            expected_speed_mps: None,
            environment: None,
            traffic: None,
            pickups: Vec::new(),
        }
    }
}

/// One entry in a course, in course order.
#[derive(Debug, Clone, PartialEq)]
pub enum CourseItem {
    /// A single piece of road.
    Section(SectionSpec),
    /// Several pieces sharing a name and a traffic zone.
    Group(SectionGroupSpec),
    /// A motif, which expands into the two above.
    Motif(MotifInvocation),
}

/// The whole authored course.
#[derive(Debug, Clone, PartialEq)]
pub struct CourseSpec {
    /// The course's name — what a dump and the HUD call it.
    pub name: String,
    /// The one seed every generated value on this course is derived from.
    ///
    /// **Explicit, never a clock reading.** Two compilations of the same source
    /// with the same seed produce byte-identical plans, which is the property
    /// every replay, test and capture in this app leans on.
    pub seed: u64,
    /// What sections inherit.
    pub defaults: CourseDefaults,
    /// What validation judges the compiled course against.
    pub thresholds: ValidationThresholds,
    /// The course, in order.
    pub items: Vec<CourseItem>,
}

impl CourseSpec {
    /// An empty course named `name`, seeded with `seed`.
    pub fn new(name: impl Into<String>, seed: u64) -> CourseSpec {
        CourseSpec {
            name: name.into(),
            seed,
            defaults: CourseDefaults::DEFAULT,
            thresholds: ValidationThresholds::DEFAULT,
            items: Vec::new(),
        }
    }

    /// The lane reach the road can offer under these defaults — how far out from
    /// the centreline an authored lane index may go.
    pub fn lane_reach(&self) -> i32 {
        (self.defaults.lanes.max(1) as i32 - 1) / 2
    }

    /// Reject a specification that cannot be expanded, before any generation
    /// runs. Structural only: this checks the *authored* numbers, and the
    /// compiled geometry and traffic get their own pass afterwards.
    pub fn validate(&self) -> CourseResult<()> {
        self.defaults.validate()?;
        self.thresholds.validate()?;
        (!self.items.is_empty()).then_some(()).ok_or_else(|| {
            CourseError::new(
                CourseErrorCode::EmptyCourse,
                format!("course `{}` has no sections", self.name),
            )
        })?;
        // A traffic lane may reach as far as the widest road the course can
        // author, not merely as far as the default: a section that widens to
        // five lanes really does have lane ±2 in it.
        let reach = crate::track::MAX_LANE_REACH;
        self.items.iter().try_for_each(|item| match item {
            CourseItem::Section(section) => validate_section(section, &self.thresholds, reach),
            CourseItem::Group(group) => {
                (!group.parts.is_empty()).then_some(()).ok_or_else(|| {
                    CourseError::new(
                        CourseErrorCode::EmptyCourse,
                        format!("section group `{}` has no parts", group.id),
                    )
                    .in_section(group.id.as_str())
                })?;
                group
                    .parts
                    .iter()
                    .try_for_each(|part| validate_section(part, &self.thresholds, reach))?;
                group
                    .traffic
                    .as_ref()
                    .map(|t| t.validate(reach))
                    .transpose()
                    .map(|_| ())
                    .and_then(|()| validate_pickups(&group.pickups, reach))
                    .map_err(|e| e.in_section(group.id.as_str()))
            }
            CourseItem::Motif(motif) => motif
                .params
                .validate()
                .and_then(|()| {
                    motif
                        .traffic
                        .as_ref()
                        .map(|t| t.validate(reach))
                        .transpose()
                        .map(|_| ())
                })
                .and_then(|()| validate_pickups(&motif.pickups, reach))
                .map_err(|e| e.in_section(motif.id.as_str())),
        })
    }
}

fn validate_section(
    section: &SectionSpec,
    thresholds: &ValidationThresholds,
    lane_reach: i32,
) -> CourseResult<()> {
    let name = section.id.as_str();
    section
        .primitive
        .validate(thresholds.min_turn_radius_m)
        .map_err(|e| e.in_section(name))?;
    section
        .modifiers
        .iter()
        .try_for_each(|m| m.validate())
        .map_err(|e| e.in_section(name))?;
    section
        .lanes
        .map(|lanes| road::validate_lane_count(lanes, "lanes"))
        .transpose()
        .map_err(|e| e.in_section(name))?;
    section
        .expected_speed_mps
        .map(|s| positive(s, "expected_speed", CourseErrorCode::InvalidSpeedRange))
        .transpose()
        .map_err(|e| e.in_section(name))?;
    section
        .traffic
        .as_ref()
        .map(|t| t.validate(lane_reach))
        .transpose()
        .map_err(|e| e.in_section(name))?;
    validate_pickups(&section.pickups, lane_reach).map_err(|e| e.in_section(name))?;
    Ok(())
}

/// Structural checks on a span's pickups. Whether the road has the authored lane
/// *at the compiled distance* is a question about geometry, and belongs to
/// `validation::check_pickups`, not here.
fn validate_pickups(pickups: &[BoostPickupSpec], lane_reach: i32) -> CourseResult<()> {
    pickups.iter().try_for_each(|p| p.validate(lane_reach))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn straight(id: &str, length_m: f32) -> SectionSpec {
        SectionSpec::new(
            SectionId::new(id),
            RoadPrimitiveSpec::Straight { length_m },
        )
    }

    #[test]
    fn the_shipping_defaults_describe_a_buildable_road() {
        let d = CourseDefaults::DEFAULT;
        assert!(d.validate().is_ok());
        assert_eq!(CourseDefaults::default(), d);
        // The default road really is the width the lane ladder needs.
        let half_width = d.lanes as f32 * d.lane_width_m * 0.5 + d.shoulder_width_m;
        assert!((half_width - 9.5).abs() < 1.0e-4, "half width {half_width}");
    }

    #[test]
    fn bad_defaults_are_rejected_with_the_right_code() {
        let base = CourseDefaults::DEFAULT;
        assert_eq!(
            CourseDefaults { lanes: 4, ..base }.validate().unwrap_err().code,
            CourseErrorCode::InvalidLaneCount
        );
        assert_eq!(
            CourseDefaults { lane_width_m: 0.0, ..base }
                .validate()
                .unwrap_err()
                .code,
            CourseErrorCode::InvalidLaneWidth
        );
        assert_eq!(
            CourseDefaults { shoulder_width_m: -1.0, ..base }
                .validate()
                .unwrap_err()
                .code,
            CourseErrorCode::InvalidRoadWidth
        );
        assert_eq!(
            CourseDefaults { shoulder_width_m: f32::NAN, ..base }
                .validate()
                .unwrap_err()
                .code,
            CourseErrorCode::InvalidFiniteScalar
        );
        assert_eq!(
            CourseDefaults { expected_speed_mps: 0.0, ..base }
                .validate()
                .unwrap_err()
                .code,
            CourseErrorCode::InvalidSpeedRange
        );
    }

    #[test]
    fn a_section_builder_sets_exactly_what_it_names() {
        let s = straight("opening", 500.0)
            .with_lanes(3)
            .with_environment(SectionKind::Tunnel)
            .with_expected_speed(70.0)
            .with_modifier(RoadModifierSpec::Banking {
                mode: BankingMode::Flat,
                strength: 1.0,
                maximum_rad: 0.1,
            })
            .with_traffic(TrafficZoneSpec::default());
        assert_eq!(s.lanes, Some(3));
        assert_eq!(s.environment, Some(SectionKind::Tunnel));
        assert_eq!(s.expected_speed_mps, Some(70.0));
        assert_eq!(s.modifiers.len(), 1);
        assert!(s.traffic.is_some());
        assert_eq!(s.id.as_str(), "opening");

        let bare = straight("bare", 100.0);
        assert_eq!(bare.lanes, None);
        assert_eq!(bare.environment, None);
        assert!(bare.modifiers.is_empty());
        assert!(bare.traffic.is_none());
    }

    #[test]
    fn an_empty_course_is_rejected() {
        let spec = CourseSpec::new("nothing", 1);
        assert_eq!(
            spec.validate().unwrap_err().code,
            CourseErrorCode::EmptyCourse
        );
    }

    #[test]
    fn a_valid_course_of_every_item_kind_validates() {
        let mut spec = CourseSpec::new("mixed", 7);
        spec.items.push(CourseItem::Section(straight("a", 400.0)));
        let mut group = SectionGroupSpec::new(SectionId::new("g"));
        group.parts.push(straight("ignored", 300.0));
        group.parts.push(SectionSpec::new(
            SectionId::new("ignored"),
            RoadPrimitiveSpec::LaneTransition {
                length_m: 140.0,
                from_lanes: 5,
                to_lanes: 3,
            },
        ));
        group.traffic = Some(TrafficZoneSpec {
            flow: Some(TrafficFlowSpec::at_density(18.0)),
            ..TrafficZoneSpec::default()
        });
        spec.items.push(CourseItem::Group(group));
        spec.items.push(CourseItem::Motif(MotifInvocation::new(
            SectionId::new("m"),
            MotifKind::RollingFreeway,
        )));
        assert!(spec.validate().is_ok(), "{:?}", spec.validate());
        assert_eq!(spec.lane_reach(), 2);
    }

    #[test]
    fn a_section_with_bad_geometry_is_rejected_and_names_its_section() {
        let mut spec = CourseSpec::new("bad", 1);
        spec.items.push(CourseItem::Section(SectionSpec::new(
            SectionId::new("hairpin"),
            RoadPrimitiveSpec::Turn {
                length_m: 200.0,
                radius_m: 15.0,
                direction: TurnDirection::Right,
            },
        )));
        let err = spec.validate().unwrap_err();
        assert_eq!(err.code, CourseErrorCode::InvalidRadius);
        assert_eq!(err.section.as_deref(), Some("hairpin"));
    }

    #[test]
    fn every_authored_sub_record_is_reached_by_validation() {
        // A bad modifier.
        let mut spec = CourseSpec::new("bad", 1);
        spec.items.push(CourseItem::Section(
            straight("wave", 400.0).with_modifier(RoadModifierSpec::LateralWave {
                amplitude_m: 10.0,
                wavelength_m: -1.0,
                phase_rad: 0.0,
            }),
        ));
        assert_eq!(spec.validate().unwrap_err().section.as_deref(), Some("wave"));

        // A bad lane count on a section.
        let mut spec = CourseSpec::new("bad", 1);
        spec.items
            .push(CourseItem::Section(straight("lanes", 400.0).with_lanes(2)));
        assert_eq!(
            spec.validate().unwrap_err().code,
            CourseErrorCode::InvalidLaneCount
        );

        // A bad expected speed on a section.
        let mut spec = CourseSpec::new("bad", 1);
        spec.items.push(CourseItem::Section(
            straight("speed", 400.0).with_expected_speed(-3.0),
        ));
        assert_eq!(
            spec.validate().unwrap_err().code,
            CourseErrorCode::InvalidSpeedRange
        );

        // A bad traffic zone on a section.
        let mut spec = CourseSpec::new("bad", 1);
        spec.items.push(CourseItem::Section(
            straight("traffic", 400.0).with_traffic(TrafficZoneSpec {
                flow: Some(TrafficFlowSpec {
                    vehicles_per_km: -1.0,
                    ..TrafficFlowSpec::at_density(10.0)
                }),
                ..TrafficZoneSpec::default()
            }),
        ));
        assert_eq!(
            spec.validate().unwrap_err().section.as_deref(),
            Some("traffic")
        );

        // An empty group.
        let mut spec = CourseSpec::new("bad", 1);
        spec.items
            .push(CourseItem::Group(SectionGroupSpec::new(SectionId::new("g"))));
        assert_eq!(
            spec.validate().unwrap_err().code,
            CourseErrorCode::EmptyCourse
        );

        // A bad group traffic zone.
        let mut spec = CourseSpec::new("bad", 1);
        let mut group = SectionGroupSpec::new(SectionId::new("g"));
        group.parts.push(straight("p", 100.0));
        group.traffic = Some(TrafficZoneSpec {
            encounters: vec![EncounterSpec::Zipper(ZipperSpec {
                first_open_lane: 9,
                ..ZipperSpec::of_length(100.0)
            })],
            ..TrafficZoneSpec::default()
        });
        spec.items.push(CourseItem::Group(group));
        let err = spec.validate().unwrap_err();
        assert_eq!(err.code, CourseErrorCode::InvalidEncounterLane);
        assert_eq!(err.section.as_deref(), Some("g"));

        // A bad motif.
        let mut spec = CourseSpec::new("bad", 1);
        let mut motif = MotifInvocation::new(SectionId::new("m"), MotifKind::BlindCrest);
        motif.params.count = MAX_MOTIF_COUNT + 1;
        spec.items.push(CourseItem::Motif(motif));
        let err = spec.validate().unwrap_err();
        assert_eq!(err.code, CourseErrorCode::RepeatLimitExceeded);
        assert_eq!(err.section.as_deref(), Some("m"));

        // A bad motif traffic zone.
        let mut spec = CourseSpec::new("bad", 1);
        let mut motif = MotifInvocation::new(SectionId::new("m"), MotifKind::BlindCrest);
        motif.traffic = Some(TrafficZoneSpec {
            flow: Some(TrafficFlowSpec {
                min_headway_m: 900.0,
                ..TrafficFlowSpec::at_density(10.0)
            }),
            ..TrafficZoneSpec::default()
        });
        spec.items.push(CourseItem::Motif(motif));
        assert_eq!(
            spec.validate().unwrap_err().code,
            CourseErrorCode::InvalidHeadwayRange
        );

        // Bad thresholds.
        let mut spec = CourseSpec::new("bad", 1);
        spec.items.push(CourseItem::Section(straight("a", 100.0)));
        spec.thresholds.min_turn_radius_m = 0.0;
        assert!(spec.validate().is_err());
    }
}
