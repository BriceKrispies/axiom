//! The **programmatic** way to write a [`CourseSpec`].
//!
//! There is exactly one authored representation, and this builds the same value
//! the DSL parser builds. That equivalence is not a nicety — it is what lets the
//! shipping course be generated in Rust
//! ([`crate::course::procedural`]) and the demo course be written in text, and
//! have both go through the identical expansion, compilation and validation. A
//! second "programmatic course" type would be a second thing to keep correct.
//!
//! The test `parse_and_builder_agree` in [`crate::course::authoring`] pins the
//! equivalence directly, by building the same course both ways and comparing the
//! specs.

use super::{
    CourseDefaults, CourseItem, CourseSpec, MotifInvocation, MotifKind, MotifParams,
    RoadPrimitiveSpec, SectionGroupSpec, SectionId, SectionKind, SectionSpec, TrafficZoneSpec,
    ValidationThresholds,
};

/// Accumulates a [`CourseSpec`] in course order.
#[derive(Debug, Clone)]
pub struct CourseBuilder {
    spec: CourseSpec,
}

impl CourseBuilder {
    /// Start a course named `name`, seeded with `seed`.
    pub fn new(name: impl Into<String>, seed: u64) -> CourseBuilder {
        CourseBuilder {
            spec: CourseSpec::new(name, seed),
        }
    }

    /// Replace the defaults sections inherit.
    pub fn defaults(mut self, defaults: CourseDefaults) -> CourseBuilder {
        self.spec.defaults = defaults;
        self
    }

    /// Replace the thresholds validation judges against.
    pub fn thresholds(mut self, thresholds: ValidationThresholds) -> CourseBuilder {
        self.spec.thresholds = thresholds;
        self
    }

    /// Append a bare primitive under `id`.
    pub fn section(self, id: &str, primitive: RoadPrimitiveSpec) -> CourseBuilder {
        self.push_section(SectionSpec::new(SectionId::new(id), primitive))
    }

    /// Append a fully-configured section.
    pub fn push_section(mut self, section: SectionSpec) -> CourseBuilder {
        self.spec.items.push(CourseItem::Section(section));
        self
    }

    /// Append a group of primitives sharing one id and one traffic zone.
    pub fn group(mut self, group: SectionGroupSpec) -> CourseBuilder {
        self.spec.items.push(CourseItem::Group(group));
        self
    }

    /// Append a motif invocation.
    pub fn motif(mut self, invocation: MotifInvocation) -> CourseBuilder {
        self.spec.items.push(CourseItem::Motif(invocation));
        self
    }

    /// Append a motif of `kind` under `id`, configured by `params`.
    pub fn motif_with(
        self,
        id: &str,
        kind: MotifKind,
        params: MotifParams,
        environment: Option<SectionKind>,
        traffic: Option<TrafficZoneSpec>,
    ) -> CourseBuilder {
        self.motif(MotifInvocation {
            id: SectionId::new(id),
            kind,
            params,
            environment,
            expected_speed_mps: None,
            traffic,
        })
    }

    /// The finished specification.
    pub fn build(self) -> CourseSpec {
        self.spec
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::course::specification::{CountRange, ScalarRange};

    #[test]
    fn the_builder_appends_in_course_order() {
        let spec = CourseBuilder::new("demo", 42)
            .section("a", RoadPrimitiveSpec::Straight { length_m: 100.0 })
            .motif(MotifInvocation::new(
                SectionId::new("m"),
                MotifKind::BlindCrest,
            ))
            .section("b", RoadPrimitiveSpec::Straight { length_m: 200.0 })
            .build();
        assert_eq!(spec.name, "demo");
        assert_eq!(spec.seed, 42);
        assert_eq!(spec.items.len(), 3);
        let names: Vec<String> = spec
            .items
            .iter()
            .map(|item| match item {
                CourseItem::Section(s) => s.id.to_string(),
                CourseItem::Group(g) => g.id.to_string(),
                CourseItem::Motif(m) => m.id.to_string(),
            })
            .collect();
        assert_eq!(names, vec!["a", "m", "b"]);
    }

    #[test]
    fn the_builder_carries_defaults_thresholds_groups_and_motif_parameters() {
        let defaults = CourseDefaults {
            lanes: 3,
            ..CourseDefaults::DEFAULT
        };
        let thresholds = ValidationThresholds {
            starved_ratio: 1.2,
            ..ValidationThresholds::DEFAULT
        };
        let params = MotifParams {
            count: 4,
            radius_m: ScalarRange::new(90.0, 150.0),
            lanes: CountRange::exact(3),
            ..MotifParams::DEFAULT
        };
        let mut group = SectionGroupSpec::new(SectionId::new("g"));
        group.parts.push(SectionSpec::new(
            SectionId::new("part"),
            RoadPrimitiveSpec::Straight { length_m: 300.0 },
        ));
        let spec = CourseBuilder::new("demo", 1)
            .defaults(defaults)
            .thresholds(thresholds)
            .group(group)
            .motif_with(
                "sweeps",
                MotifKind::HighSpeedSweeps,
                params.clone(),
                Some(SectionKind::SweepingBends),
                Some(TrafficZoneSpec::default()),
            )
            .push_section(
                SectionSpec::new(
                    SectionId::new("tail"),
                    RoadPrimitiveSpec::Straight { length_m: 50.0 },
                )
                .with_lanes(3),
            )
            .build();

        assert_eq!(spec.defaults, defaults);
        assert_eq!(spec.thresholds, thresholds);
        match &spec.items[1] {
            CourseItem::Motif(m) => {
                assert_eq!(m.params, params);
                assert_eq!(m.environment, Some(SectionKind::SweepingBends));
                assert!(m.traffic.is_some());
            }
            other => panic!("expected a motif, got {other:?}"),
        }
        match &spec.items[0] {
            CourseItem::Group(g) => assert_eq!(g.parts.len(), 1),
            other => panic!("expected a group, got {other:?}"),
        }
        assert!(spec.validate().is_ok(), "{:?}", spec.validate());
    }
}
