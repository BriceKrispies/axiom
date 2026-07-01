//! Generic body plan definitions: reusable anatomical structures (not creatures).
//!
//! A body plan is built incrementally as a *draft* (add parts, connect them, set
//! symmetry), then finished into an immutable, name-keyed plan. sim-core hardcodes
//! no creature, organ, or limb — plans are pure data built by the consumer.

use std::collections::{BTreeMap, BTreeSet};

use crate::body_surface::{BodySurfaceKind, SurfaceExposure};
use crate::ids::BodyPlanId;
use crate::tissue::TissueLayer;

/// The category of a body part in a plan. Opaque to sim-core behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BodyPlanPartKind {
    /// A central/torso part.
    Core,
    /// A head part.
    Head,
    /// A limb part.
    Limb,
    /// An extremity part.
    Extremity,
    /// An organ part.
    Organ,
    /// A mouth part.
    Mouth,
    /// An eye part.
    Eye,
    /// An uncategorized part.
    Generic,
}

const PART_KINDS: [BodyPlanPartKind; 8] = [
    BodyPlanPartKind::Core,
    BodyPlanPartKind::Head,
    BodyPlanPartKind::Limb,
    BodyPlanPartKind::Extremity,
    BodyPlanPartKind::Organ,
    BodyPlanPartKind::Mouth,
    BodyPlanPartKind::Eye,
    BodyPlanPartKind::Generic,
];

impl BodyPlanPartKind {
    /// Construct from a code, `None` if out of range.
    pub fn from_code(code: u8) -> Option<BodyPlanPartKind> {
        PART_KINDS.get(code as usize).copied()
    }

    /// The kind's deterministic code.
    pub fn code(self) -> u8 {
        self as u8
    }
}

/// Optional symmetry metadata for a part.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BodyPlanSymmetry {
    /// No symmetry.
    None,
    /// One of a bilateral (left/right) pair.
    Bilateral,
    /// One of a radial group.
    Radial,
}

const SYMMETRIES: [BodyPlanSymmetry; 3] = [
    BodyPlanSymmetry::None,
    BodyPlanSymmetry::Bilateral,
    BodyPlanSymmetry::Radial,
];

impl BodyPlanSymmetry {
    /// Construct from a code, `None` if out of range.
    pub fn from_code(code: u8) -> Option<BodyPlanSymmetry> {
        SYMMETRIES.get(code as usize).copied()
    }

    /// The symmetry's deterministic code.
    pub fn code(self) -> u8 {
        self as u8
    }
}

/// A capability a part provides, as an opaque deterministic code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BodyPlanCapability(u32);

impl BodyPlanCapability {
    /// Construct from a deterministic code.
    pub const fn new(code: u32) -> Self {
        BodyPlanCapability(code)
    }

    /// The raw code.
    pub const fn code(self) -> u32 {
        self.0
    }
}

/// A surface a part exposes, as `(kind, exposure)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceSpec {
    kind: BodySurfaceKind,
    exposure: SurfaceExposure,
}

impl SurfaceSpec {
    /// A surface spec.
    pub const fn new(kind: BodySurfaceKind, exposure: SurfaceExposure) -> Self {
        SurfaceSpec { kind, exposure }
    }

    /// The surface kind.
    pub const fn kind(self) -> BodySurfaceKind {
        self.kind
    }

    /// The surface exposure.
    pub const fn exposure(self) -> SurfaceExposure {
        self.exposure
    }
}

/// One part of a body plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyPlanPart {
    index: u32,
    name: String,
    kind: BodyPlanPartKind,
    symmetry: BodyPlanSymmetry,
    group: u32,
    capabilities: BTreeSet<u32>,
    tissue_layers: Vec<TissueLayer>,
    surfaces: Vec<SurfaceSpec>,
}

impl BodyPlanPart {
    /// The part's ordinal index within the plan.
    pub const fn index(&self) -> u32 {
        self.index
    }
    /// The durable part name (unique within the plan).
    pub fn name(&self) -> &str {
        &self.name
    }
    /// The part kind.
    pub const fn kind(&self) -> BodyPlanPartKind {
        self.kind
    }
    /// The part symmetry.
    pub const fn symmetry(&self) -> BodyPlanSymmetry {
        self.symmetry
    }
    /// The symmetry/group id (0 if ungrouped).
    pub const fn group(&self) -> u32 {
        self.group
    }
    /// Whether the part provides `capability`.
    pub fn has_capability(&self, capability: BodyPlanCapability) -> bool {
        self.capabilities.contains(&capability.code())
    }
    /// The tissue layers, outermost first as authored.
    pub fn tissue_layers(&self) -> &[TissueLayer] {
        &self.tissue_layers
    }
    /// The surface specs.
    pub fn surfaces(&self) -> &[SurfaceSpec] {
        &self.surfaces
    }
}

/// A connection between two parts of a plan, by index (`from` → `to`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BodyPlanConnection {
    from: u32,
    to: u32,
}

impl BodyPlanConnection {
    /// The source part index.
    pub const fn from(self) -> u32 {
        self.from
    }
    /// The destination part index.
    pub const fn to(self) -> u32 {
        self.to
    }
}

/// The fields shared by a draft-in-progress and a finished plan.
#[derive(Debug, Clone, Default)]
struct PlanParts {
    parts: Vec<BodyPlanPart>,
    connections: Vec<BodyPlanConnection>,
    name_to_index: BTreeMap<String, u32>,
}

/// Per-part authoring inputs (grouped to keep the add call boring).
#[derive(Debug, Clone)]
pub struct PlanPartSpec {
    /// Durable part name (unique within the plan).
    pub name: String,
    /// Part kind.
    pub kind: BodyPlanPartKind,
    /// Symmetry metadata.
    pub symmetry: BodyPlanSymmetry,
    /// Symmetry/group id (0 if ungrouped).
    pub group: u32,
    /// Capability codes the part provides.
    pub capabilities: Vec<u32>,
    /// Tissue layers (outermost first).
    pub tissue_layers: Vec<TissueLayer>,
    /// Surfaces the part exposes.
    pub surfaces: Vec<SurfaceSpec>,
}

impl PlanParts {
    /// Add a part, rejecting a duplicate durable name. Returns its index.
    fn add(&mut self, spec: PlanPartSpec) -> Option<u32> {
        let free = !self.name_to_index.contains_key(&spec.name);
        let index = self.parts.len() as u32;
        free.then(|| {
            self.name_to_index.insert(spec.name.clone(), index);
            self.parts.push(BodyPlanPart {
                index,
                name: spec.name,
                kind: spec.kind,
                symmetry: spec.symmetry,
                group: spec.group,
                capabilities: spec.capabilities.into_iter().collect(),
                tissue_layers: spec.tissue_layers,
                surfaces: spec.surfaces,
            });
        });
        free.then_some(index)
    }

    /// Connect two existing part indices, rejecting out-of-range references.
    fn connect(&mut self, from: u32, to: u32) -> bool {
        let len = self.parts.len() as u32;
        let valid = (from < len) & (to < len) & (from != to);
        valid.then(|| self.connections.push(BodyPlanConnection { from, to }));
        valid
    }
}

/// A finished, immutable body plan.
#[derive(Debug, Clone)]
pub struct BodyPlan {
    id: BodyPlanId,
    name: String,
    inner: PlanParts,
}

impl BodyPlan {
    /// The deterministic plan id.
    pub const fn id(&self) -> BodyPlanId {
        self.id
    }
    /// The durable plan name.
    pub fn name(&self) -> &str {
        &self.name
    }
    /// The plan parts, in authored order.
    pub fn parts(&self) -> &[BodyPlanPart] {
        &self.inner.parts
    }
    /// The plan connections, in authored order.
    pub fn connections(&self) -> &[BodyPlanConnection] {
        &self.inner.connections
    }
    /// Parts of a given kind, in authored order.
    pub fn parts_by_kind(&self, kind: BodyPlanPartKind) -> impl Iterator<Item = &BodyPlanPart> {
        self.inner
            .parts
            .iter()
            .filter(move |part| part.kind == kind)
    }
    /// Parts providing a capability, in authored order.
    pub fn parts_by_capability(
        &self,
        capability: BodyPlanCapability,
    ) -> impl Iterator<Item = &BodyPlanPart> {
        self.inner
            .parts
            .iter()
            .filter(move |part| part.has_capability(capability))
    }
    /// The part with a durable name, if any.
    pub fn part_by_name(&self, name: &str) -> Option<&BodyPlanPart> {
        self.inner
            .name_to_index
            .get(name)
            .and_then(|i| self.inner.parts.get(*i as usize))
    }
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;

fn id_for(name: &str) -> BodyPlanId {
    let hash = name.bytes().fold(FNV_OFFSET, |hash, byte| {
        (hash ^ byte as u64).wrapping_mul(FNV_PRIME)
    });
    BodyPlanId::from_raw(hash)
}

/// A registry of body plans plus the in-progress drafts being built.
#[derive(Debug, Clone, Default)]
pub struct BodyPlanRegistry {
    drafts: BTreeMap<u32, PlanParts>,
    next_draft: u32,
    by_id: BTreeMap<BodyPlanId, BodyPlan>,
    by_name: BTreeMap<String, BodyPlanId>,
}

impl BodyPlanRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        BodyPlanRegistry {
            drafts: BTreeMap::new(),
            next_draft: 1,
            by_id: BTreeMap::new(),
            by_name: BTreeMap::new(),
        }
    }

    /// Begin a new draft, returning its handle.
    pub fn begin(&mut self) -> u32 {
        let handle = self.next_draft;
        self.next_draft += 1;
        self.drafts.insert(handle, PlanParts::default());
        handle
    }

    /// Add a part to a draft. Returns the part index, or `None` if the draft is
    /// unknown or the name duplicates an existing part.
    pub fn add_part(&mut self, draft: u32, spec: PlanPartSpec) -> Option<u32> {
        self.drafts.get_mut(&draft).and_then(|plan| plan.add(spec))
    }

    /// Connect two parts in a draft. Returns whether the connection was valid.
    pub fn connect(&mut self, draft: u32, from: u32, to: u32) -> bool {
        self.drafts
            .get_mut(&draft)
            .map(|plan| plan.connect(from, to))
            .unwrap_or(false)
    }

    /// Finish a draft into a named plan. Returns the plan id, or `None` if the
    /// draft is unknown or the plan name is already registered.
    pub fn finish(&mut self, draft: u32, name: &str) -> Option<BodyPlanId> {
        let id = id_for(name);
        let known = self.drafts.contains_key(&draft);
        let free = !(self.by_name.contains_key(name) | self.by_id.contains_key(&id));
        (known & free)
            .then(|| self.drafts.remove(&draft))
            .flatten()
            .map(|inner| {
                self.by_name.insert(name.to_string(), id);
                self.by_id.insert(
                    id,
                    BodyPlan {
                        id,
                        name: name.to_string(),
                        inner,
                    },
                );
                id
            })
    }

    /// Look up a plan by id.
    pub fn get(&self, id: BodyPlanId) -> Option<&BodyPlan> {
        self.by_id.get(&id)
    }

    /// Look up a plan by durable name.
    pub fn by_name(&self, name: &str) -> Option<&BodyPlan> {
        self.by_name.get(name).and_then(|id| self.by_id.get(id))
    }

    /// All plans, ascending by id.
    pub fn iter(&self) -> impl Iterator<Item = &BodyPlan> {
        self.by_id.values()
    }

    /// The number of finished plans.
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// Whether the registry holds no finished plans.
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::TissueId;

    fn spec(name: &str, kind: BodyPlanPartKind) -> PlanPartSpec {
        PlanPartSpec {
            name: name.to_string(),
            kind,
            symmetry: BodyPlanSymmetry::None,
            group: 0,
            capabilities: Vec::new(),
            tissue_layers: Vec::new(),
            surfaces: Vec::new(),
        }
    }

    #[test]
    fn part_and_symmetry_kind_codes_round_trip() {
        assert_eq!(BodyPlanPartKind::from_code(0), Some(BodyPlanPartKind::Core));
        assert_eq!(
            BodyPlanPartKind::from_code(7),
            Some(BodyPlanPartKind::Generic)
        );
        assert_eq!(BodyPlanPartKind::from_code(8), None);
        assert_eq!(BodyPlanPartKind::Extremity.code(), 3);
        assert_eq!(
            BodyPlanSymmetry::from_code(1),
            Some(BodyPlanSymmetry::Bilateral)
        );
        assert_eq!(BodyPlanSymmetry::Bilateral.code(), 1);
        assert_eq!(BodyPlanSymmetry::from_code(3), None);
    }

    #[test]
    fn build_plan_with_parts_and_connections() {
        let mut registry = BodyPlanRegistry::new();
        assert!(registry.is_empty());
        let draft = registry.begin();
        let core = registry
            .add_part(draft, spec("test-core", BodyPlanPartKind::Core))
            .unwrap();
        let limb = registry
            .add_part(draft, spec("test-limb", BodyPlanPartKind::Limb))
            .unwrap();
        assert_eq!((core, limb), (0, 1));
        assert!(registry
            .add_part(draft, spec("test-core", BodyPlanPartKind::Generic))
            .is_none());
        assert!(registry.connect(draft, core, limb));
        assert!(
            !registry.connect(draft, core, 99),
            "out-of-range index rejected"
        );
        assert!(
            !registry.connect(draft, core, core),
            "self-connection rejected"
        );
        assert!(registry
            .add_part(999, spec("x", BodyPlanPartKind::Generic))
            .is_none());
        assert!(!registry.connect(999, 0, 0));

        let id = registry.finish(draft, "test-plan").unwrap();
        assert_eq!(id, id_for("test-plan"));
        let plan = registry.get(id).unwrap();
        assert_eq!(plan.name(), "test-plan");
        assert_eq!(plan.parts().len(), 2);
        assert_eq!(plan.parts()[0].name(), "test-core");
        assert_eq!(plan.connections().len(), 1);
        assert_eq!(
            (plan.connections()[0].from(), plan.connections()[0].to()),
            (0, 1)
        );
        assert_eq!(plan.part_by_name("test-limb").unwrap().index(), 1);
        assert!(plan.part_by_name("absent").is_none());
        assert_eq!(registry.by_name("test-plan").unwrap().id(), id);
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn finish_rejects_unknown_draft_and_duplicate_plan_name() {
        let mut registry = BodyPlanRegistry::new();
        let a = registry.begin();
        registry.add_part(a, spec("p", BodyPlanPartKind::Core));
        assert!(registry.finish(a, "plan").is_some());
        assert!(registry.finish(a, "plan2").is_none());
        let b = registry.begin();
        registry.add_part(b, spec("p", BodyPlanPartKind::Core));
        assert!(registry.finish(b, "plan").is_none());
    }

    #[test]
    fn query_by_kind_and_capability_and_metadata() {
        let mut registry = BodyPlanRegistry::new();
        let draft = registry.begin();
        registry.add_part(draft, spec("test-core", BodyPlanPartKind::Core));
        let mut limb = spec("test-limb-l", BodyPlanPartKind::Limb);
        limb.symmetry = BodyPlanSymmetry::Bilateral;
        limb.group = 1;
        limb.capabilities = vec![10, 20];
        limb.tissue_layers = vec![TissueLayer::new(TissueId::from_raw(5), 0)];
        registry.add_part(draft, limb);
        registry.add_part(draft, spec("test-limb-r", BodyPlanPartKind::Limb));
        let id = registry.finish(draft, "plan").unwrap();
        let plan = registry.get(id).unwrap();

        let limbs: Vec<&str> = plan
            .parts_by_kind(BodyPlanPartKind::Limb)
            .map(BodyPlanPart::name)
            .collect();
        assert_eq!(limbs, vec!["test-limb-l", "test-limb-r"]);
        let capable: Vec<&str> = plan
            .parts_by_capability(BodyPlanCapability::new(10))
            .map(BodyPlanPart::name)
            .collect();
        assert_eq!(capable, vec!["test-limb-l"]);
        let part = plan.part_by_name("test-limb-l").unwrap();
        assert_eq!(part.symmetry(), BodyPlanSymmetry::Bilateral);
        assert_eq!(part.group(), 1);
        assert!(part.has_capability(BodyPlanCapability::new(20)));
        assert!(!part.has_capability(BodyPlanCapability::new(99)));
        assert_eq!(part.tissue_layers().len(), 1);
        assert_eq!(plan.parts_by_kind(BodyPlanPartKind::Eye).count(), 0);
        assert_eq!(registry.iter().count(), 1);
    }
}
