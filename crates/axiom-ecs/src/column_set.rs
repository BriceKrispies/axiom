//! A storage's columns, exposed generically for whole-world operations.

use axiom_kernel::EntityId;

use crate::erased_column::ErasedColumn;

/// Exposes a component storage's columns as ordered, type-erased views.
///
/// A consumer's storage struct implements this by listing its columns (each a
/// [`crate::ComponentColumn`]) in a fixed order, paired with a stable role name.
/// That is all the `World` needs to serialize, deserialize, and describe the
/// whole world generically — the columns themselves know how to (de)serialize
/// and describe via [`ErasedColumn`]. `columns()` and `columns_mut()` must yield
/// the same columns in the same order.
pub trait ColumnSet {
    /// The columns, each paired with a stable role name, in a fixed order.
    fn columns(&self) -> Vec<(&'static str, &dyn ErasedColumn)>;

    /// The same columns, mutably, in the same order.
    fn columns_mut(&mut self) -> Vec<(&'static str, &mut dyn ErasedColumn)>;

    /// Remove `entity` from every component column. The default walks
    /// [`Self::columns_mut`], so any storage gets correct despawn cleanup for free;
    /// it is what [`crate::World::despawn`] calls. Non-column fields a storage may
    /// hold are not touched — only its registered component columns.
    fn remove_entity(&mut self, entity: EntityId) {
        self.columns_mut()
            .into_iter()
            .for_each(|(_, column)| column.remove_entity(entity));
    }
}
