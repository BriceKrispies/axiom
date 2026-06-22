//! Deterministic identity newtypes for the simulation substrate.
//!
//! Every sim-core entity (fact, relation, definition, rule, process, causal
//! event) is named by a `u64`-backed newtype. The values are minted
//! deterministically by the owning store (monotonic counters / order-independent
//! hashes — never random, never wall-clock), are totally ordered and hashable,
//! and round-trip through the kernel binary format via [`Reflect`].

use axiom_kernel::{BinaryReader, BinaryWriter, FieldSchema, KernelResult, Reflect, TypeSchema};

/// Define a `u64`-backed deterministic id newtype with `Reflect` support.
macro_rules! sim_id {
    ($name:ident, $schema:literal, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(u64);

        impl $name {
            /// Construct this id from a raw value.
            pub const fn from_raw(raw: u64) -> Self {
                $name(raw)
            }

            /// The raw value backing this id.
            pub const fn raw(self) -> u64 {
                self.0
            }
        }

        impl Reflect for $name {
            const SCHEMA: TypeSchema = TypeSchema::new($schema, &[FieldSchema::new("raw", "u64")]);

            fn reflect_write(&self, writer: &mut BinaryWriter) {
                self.0.reflect_write(writer);
            }

            fn reflect_read(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
                u64::reflect_read(reader).map(Self)
            }
        }
    };
}

sim_id!(FactId, "FactId", "A deterministic identity for a fact.");
sim_id!(
    RelationId,
    "RelationId",
    "A deterministic identity for a relation."
);
sim_id!(
    DefinitionId,
    "DefinitionId",
    "A deterministic identity for a definition."
);
sim_id!(RuleId, "RuleId", "A deterministic identity for a rule.");
sim_id!(
    ProcessId,
    "ProcessId",
    "A deterministic identity for a process."
);
sim_id!(
    CausalEventId,
    "CausalEventId",
    "A deterministic identity for a causal event."
);
sim_id!(
    ResidueId,
    "ResidueId",
    "A deterministic identity for a residue."
);
sim_id!(
    InteractionId,
    "InteractionId",
    "A deterministic identity for an interaction record."
);
sim_id!(
    TransferRuleId,
    "TransferRuleId",
    "A deterministic identity for a transfer rule."
);
sim_id!(
    MaterialEffectRuleId,
    "MaterialEffectRuleId",
    "A deterministic identity for a material effect rule."
);
sim_id!(
    BodyId,
    "BodyId",
    "A deterministic identity for a body instance."
);
sim_id!(
    BodyPlanId,
    "BodyPlanId",
    "A deterministic identity for a body plan definition."
);
sim_id!(
    BodyPartId,
    "BodyPartId",
    "A deterministic identity for an instantiated body part."
);
sim_id!(
    TissueId,
    "TissueId",
    "A deterministic identity for a tissue definition."
);
sim_id!(
    BodySurfaceId,
    "BodySurfaceId",
    "A deterministic identity for an instantiated body surface."
);
sim_id!(
    WoundId,
    "WoundId",
    "A deterministic identity for a wound record."
);

#[cfg(test)]
mod tests {
    use super::*;

    /// Exercise the full generated surface (ordering, equality, raw round-trip,
    /// hashing, binary round-trip, truncation) for one id type.
    macro_rules! id_behaviour {
        ($name:ident, $test:ident) => {
            #[test]
            fn $test() {
                use std::collections::HashSet;
                // raw round-trip
                assert_eq!($name::from_raw(7).raw(), 7);
                // ordering + equality
                assert!($name::from_raw(1) < $name::from_raw(2));
                assert_eq!($name::from_raw(5), $name::from_raw(5));
                assert_ne!($name::from_raw(5), $name::from_raw(6));
                // hashing
                let mut set = HashSet::new();
                set.insert($name::from_raw(3));
                set.insert($name::from_raw(3));
                set.insert($name::from_raw(4));
                assert_eq!(set.len(), 2);
                // binary round-trip
                let id = $name::from_raw(0x0102_0304_0506_0708);
                let mut writer = BinaryWriter::new();
                id.reflect_write(&mut writer);
                let bytes = writer.into_bytes();
                assert_eq!(
                    $name::reflect_read(&mut BinaryReader::new(&bytes)).unwrap(),
                    id
                );
                assert_eq!($name::SCHEMA.name(), stringify!($name));
                // truncation fails cleanly at every prefix
                for len in 0..bytes.len() {
                    assert!($name::reflect_read(&mut BinaryReader::new(&bytes[..len])).is_err());
                }
            }
        };
    }

    id_behaviour!(FactId, fact_id_behaviour);
    id_behaviour!(RelationId, relation_id_behaviour);
    id_behaviour!(DefinitionId, definition_id_behaviour);
    id_behaviour!(RuleId, rule_id_behaviour);
    id_behaviour!(ProcessId, process_id_behaviour);
    id_behaviour!(CausalEventId, causal_event_id_behaviour);
    id_behaviour!(ResidueId, residue_id_behaviour);
    id_behaviour!(InteractionId, interaction_id_behaviour);
    id_behaviour!(TransferRuleId, transfer_rule_id_behaviour);
    id_behaviour!(MaterialEffectRuleId, material_effect_rule_id_behaviour);
    id_behaviour!(BodyId, body_id_behaviour);
    id_behaviour!(BodyPlanId, body_plan_id_behaviour);
    id_behaviour!(BodyPartId, body_part_id_behaviour);
    id_behaviour!(TissueId, tissue_id_behaviour);
    id_behaviour!(BodySurfaceId, body_surface_id_behaviour);
    id_behaviour!(WoundId, wound_id_behaviour);
}
