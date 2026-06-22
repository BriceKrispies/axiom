//! A generic wound/damage record substrate — records, not combat or healing.

use std::collections::BTreeMap;

use crate::cause::CauseRef;
use crate::ids::{BodyId, BodyPartId, DefinitionId, ResidueId, TissueId, WoundId};

/// How damage was inflicted. Opaque to sim-core behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DamageMode {
    /// Blunt impact.
    Blunt,
    /// A cut.
    Cut,
    /// A pierce.
    Pierce,
    /// A burn.
    Burn,
    /// A crush.
    Crush,
    /// A tear.
    Tear,
    /// Chemical damage.
    Chemical,
    /// An unclassified mode.
    Generic,
}

const DAMAGE_MODES: [DamageMode; 8] = [
    DamageMode::Blunt,
    DamageMode::Cut,
    DamageMode::Pierce,
    DamageMode::Burn,
    DamageMode::Crush,
    DamageMode::Tear,
    DamageMode::Chemical,
    DamageMode::Generic,
];

impl DamageMode {
    /// Construct from a code, `None` if out of range.
    pub fn from_code(code: u8) -> Option<DamageMode> {
        DAMAGE_MODES.get(code as usize).copied()
    }

    /// The mode's deterministic code.
    pub fn code(self) -> u8 {
        self as u8
    }
}

/// A comparable damage severity (higher = more severe). sim-core assigns no
/// threshold meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DamageSeverity(u32);

impl DamageSeverity {
    /// A severity from a deterministic level.
    pub const fn new(level: u32) -> Self {
        DamageSeverity(level)
    }

    /// The raw level.
    pub const fn level(self) -> u32 {
        self.0
    }
}

/// Damage to one tissue within a wound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TissueDamage {
    tissue: TissueId,
    severity: DamageSeverity,
}

impl TissueDamage {
    /// Damage of `severity` to `tissue`.
    pub const fn new(tissue: TissueId, severity: DamageSeverity) -> Self {
        TissueDamage { tissue, severity }
    }

    /// The damaged tissue.
    pub const fn tissue(self) -> TissueId {
        self.tissue
    }

    /// The severity to that tissue.
    pub const fn severity(self) -> DamageSeverity {
        self.severity
    }
}

/// An opaque, domain-defined wound state code (e.g. fresh vs scarred — later).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WoundState(u32);

impl WoundState {
    /// A wound state from a deterministic code.
    pub const fn new(code: u32) -> Self {
        WoundState(code)
    }

    /// The raw code.
    pub const fn code(self) -> u32 {
        self.0
    }
}

/// The inputs for a wound record (grouped to keep the create call boring).
#[derive(Debug, Clone)]
pub struct WoundSpec {
    /// The wounded body.
    pub body: BodyId,
    /// The wounded part.
    pub part: BodyPartId,
    /// The specific tissue, if known.
    pub tissue: Option<TissueId>,
    /// How the damage was inflicted.
    pub mode: DamageMode,
    /// How severe the damage is.
    pub severity: DamageSeverity,
    /// The material/substance involved, if any.
    pub material: Option<DefinitionId>,
    /// The residue involved, if any.
    pub residue: Option<ResidueId>,
    /// Per-tissue damage detail.
    pub tissue_damage: Vec<TissueDamage>,
    /// What caused the wound, if recorded.
    pub cause: Option<CauseRef>,
    /// The logical tick the wound was created at.
    pub tick: u64,
}

/// A recorded wound on a body part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WoundRecord {
    id: WoundId,
    body: BodyId,
    part: BodyPartId,
    tissue: Option<TissueId>,
    mode: DamageMode,
    severity: DamageSeverity,
    material: Option<DefinitionId>,
    residue: Option<ResidueId>,
    tissue_damage: Vec<TissueDamage>,
    cause: Option<CauseRef>,
    tick: u64,
    state: WoundState,
}

impl WoundRecord {
    /// This wound's id.
    pub const fn id(&self) -> WoundId {
        self.id
    }
    /// The wounded body.
    pub const fn body(&self) -> BodyId {
        self.body
    }
    /// The wounded part.
    pub const fn part(&self) -> BodyPartId {
        self.part
    }
    /// The specific tissue, if known.
    pub const fn tissue(&self) -> Option<TissueId> {
        self.tissue
    }
    /// The damage mode.
    pub const fn mode(&self) -> DamageMode {
        self.mode
    }
    /// The severity.
    pub const fn severity(&self) -> DamageSeverity {
        self.severity
    }
    /// The material/substance involved, if any.
    pub const fn material(&self) -> Option<DefinitionId> {
        self.material
    }
    /// The residue involved, if any.
    pub const fn residue(&self) -> Option<ResidueId> {
        self.residue
    }
    /// The per-tissue damage detail.
    pub fn tissue_damage(&self) -> &[TissueDamage] {
        &self.tissue_damage
    }
    /// What caused the wound, if recorded.
    pub const fn cause(&self) -> Option<CauseRef> {
        self.cause
    }
    /// The logical tick the wound was created at.
    pub const fn tick(&self) -> u64 {
        self.tick
    }
    /// The wound state.
    pub const fn state(&self) -> WoundState {
        self.state
    }
}

/// A deterministic store of wound records, keyed/iterated by ascending id.
#[derive(Debug, Clone, Default)]
pub struct WoundStore {
    wounds: BTreeMap<WoundId, WoundRecord>,
    next: u64,
}

impl WoundStore {
    /// Create an empty store. The first wound has id 1.
    pub fn new() -> Self {
        WoundStore {
            wounds: BTreeMap::new(),
            next: 1,
        }
    }

    /// Create a wound record, minting and returning its deterministic id.
    pub fn create(&mut self, spec: WoundSpec) -> WoundId {
        let id = WoundId::from_raw(self.next);
        self.next += 1;
        self.wounds.insert(
            id,
            WoundRecord {
                id,
                body: spec.body,
                part: spec.part,
                tissue: spec.tissue,
                mode: spec.mode,
                severity: spec.severity,
                material: spec.material,
                residue: spec.residue,
                tissue_damage: spec.tissue_damage,
                cause: spec.cause,
                tick: spec.tick,
                state: WoundState::new(0),
            },
        );
        id
    }

    /// Borrow a wound by id.
    pub fn get(&self, id: WoundId) -> Option<&WoundRecord> {
        self.wounds.get(&id)
    }

    /// Update a wound's state. Returns whether it existed.
    pub fn set_state(&mut self, id: WoundId, state: WoundState) -> bool {
        self.wounds
            .get_mut(&id)
            .map(|wound| wound.state = state)
            .is_some()
    }

    /// Wounds on a body, ascending by id.
    pub fn by_body(&self, body: BodyId) -> impl Iterator<Item = &WoundRecord> {
        self.wounds.values().filter(move |wound| wound.body == body)
    }

    /// Wounds on a part, ascending by id.
    pub fn by_part(&self, part: BodyPartId) -> impl Iterator<Item = &WoundRecord> {
        self.wounds.values().filter(move |wound| wound.part == part)
    }

    /// Wounds of a damage mode, ascending by id.
    pub fn by_mode(&self, mode: DamageMode) -> impl Iterator<Item = &WoundRecord> {
        self.wounds.values().filter(move |wound| wound.mode == mode)
    }

    /// Wounds of an exact severity, ascending by id.
    pub fn by_severity(&self, severity: DamageSeverity) -> impl Iterator<Item = &WoundRecord> {
        self.wounds
            .values()
            .filter(move |wound| wound.severity == severity)
    }

    /// All wounds, ascending by id.
    pub fn iter(&self) -> impl Iterator<Item = &WoundRecord> {
        self.wounds.values()
    }

    /// The number of wounds.
    pub fn len(&self) -> usize {
        self.wounds.len()
    }

    /// Whether the store holds no wounds.
    pub fn is_empty(&self) -> bool {
        self.wounds.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(body: u64, part: u64, mode: DamageMode, severity: u32) -> WoundSpec {
        WoundSpec {
            body: BodyId::from_raw(body),
            part: BodyPartId::from_raw(part),
            tissue: None,
            mode,
            severity: DamageSeverity::new(severity),
            material: None,
            residue: None,
            tissue_damage: Vec::new(),
            cause: None,
            tick: 0,
        }
    }

    #[test]
    fn damage_mode_codes_round_trip() {
        assert_eq!(DamageMode::from_code(0), Some(DamageMode::Blunt));
        assert_eq!(DamageMode::from_code(7), Some(DamageMode::Generic));
        assert_eq!(DamageMode::from_code(8), None);
        assert_eq!(DamageMode::Pierce.code(), 2);
    }

    #[test]
    fn severity_orders_and_tissue_damage_pairs() {
        assert!(DamageSeverity::new(1) < DamageSeverity::new(2));
        let damage = TissueDamage::new(TissueId::from_raw(3), DamageSeverity::new(4));
        assert_eq!(damage.tissue(), TissueId::from_raw(3));
        assert_eq!(damage.severity(), DamageSeverity::new(4));
        assert_eq!(damage.severity().level(), 4);
    }

    #[test]
    fn create_get_update_and_fields() {
        let mut store = WoundStore::new();
        assert!(store.is_empty());
        let mut s = spec(1, 2, DamageMode::Cut, 5);
        s.tissue = Some(TissueId::from_raw(7));
        s.material = Some(DefinitionId::from_raw(8));
        s.residue = Some(ResidueId::from_raw(9));
        s.tissue_damage = vec![TissueDamage::new(
            TissueId::from_raw(7),
            DamageSeverity::new(5),
        )];
        s.cause = Some(CauseRef::Command);
        let id = store.create(s);
        assert_eq!(id.raw(), 1);
        let wound = store.get(id).unwrap();
        assert_eq!(wound.body(), BodyId::from_raw(1));
        assert_eq!(wound.part(), BodyPartId::from_raw(2));
        assert_eq!(wound.tissue(), Some(TissueId::from_raw(7)));
        assert_eq!(wound.mode(), DamageMode::Cut);
        assert_eq!(wound.severity(), DamageSeverity::new(5));
        assert_eq!(wound.material(), Some(DefinitionId::from_raw(8)));
        assert_eq!(wound.residue(), Some(ResidueId::from_raw(9)));
        assert_eq!(wound.tissue_damage().len(), 1);
        assert_eq!(wound.cause(), Some(CauseRef::Command));
        assert_eq!(wound.tick(), 0);
        assert_eq!(wound.state(), WoundState::new(0));
        assert_eq!(wound.state().code(), 0);
        assert!(store.set_state(id, WoundState::new(2)));
        assert_eq!(store.get(id).unwrap().state(), WoundState::new(2));
        assert!(!store.set_state(WoundId::from_raw(99), WoundState::new(1)));
    }

    #[test]
    fn queries_by_body_part_mode_severity_are_ascending() {
        let mut store = WoundStore::new();
        let w1 = store.create(spec(1, 10, DamageMode::Cut, 3));
        let _w2 = store.create(spec(2, 20, DamageMode::Burn, 9));
        let w3 = store.create(spec(1, 10, DamageMode::Cut, 3));
        assert_eq!(
            store
                .by_body(BodyId::from_raw(1))
                .map(WoundRecord::id)
                .collect::<Vec<_>>(),
            vec![w1, w3]
        );
        assert_eq!(
            store
                .by_part(BodyPartId::from_raw(10))
                .map(WoundRecord::id)
                .collect::<Vec<_>>(),
            vec![w1, w3]
        );
        assert_eq!(
            store
                .by_mode(DamageMode::Cut)
                .map(WoundRecord::id)
                .collect::<Vec<_>>(),
            vec![w1, w3]
        );
        assert_eq!(
            store
                .by_severity(DamageSeverity::new(3))
                .map(WoundRecord::id)
                .collect::<Vec<_>>(),
            vec![w1, w3]
        );
        assert_eq!(store.by_mode(DamageMode::Pierce).count(), 0);
        assert_eq!(store.iter().count(), 3);
    }
}
