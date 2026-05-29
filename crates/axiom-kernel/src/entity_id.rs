//! Stable identifier for an entity.

use crate::id_macro::define_id;

define_id! {
    /// A stable, strongly-typed identifier for an entity.
    ///
    /// This is purely an ID primitive: the kernel implements no ECS. Higher
    /// layers decide what an entity *is*; the kernel only guarantees a stable,
    /// deterministically (de)serializable identity.
    EntityId
}
