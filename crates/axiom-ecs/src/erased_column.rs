//! Type-erased access to a component column for whole-world operations.

use axiom_kernel::{BinaryReader, BinaryWriter, EntityId, KernelResult, Reflect, TypeSchema};

use crate::component_column::ComponentColumn;

/// A component column viewed without its element type.
///
/// This is the seam that lets the world serialize and describe *all* its
/// columns generically — through `&dyn ErasedColumn` — without knowing the
/// component types. Crucially it exposes no typed accessor (no `get<T>`), so
/// there is no `downcast` and no unreachable arm: every operation works through
/// the column's [`Reflect`] element. Method names are distinct from
/// [`ComponentColumn`]'s inherent ones so the blanket impl never shadows them.
pub trait ErasedColumn {
    /// The schema of the column's component type.
    fn describe(&self) -> TypeSchema;

    /// How many entities have this component.
    fn entry_count(&self) -> usize;

    /// Serialize the whole column to the writer.
    fn write(&self, writer: &mut BinaryWriter);

    /// Replace the column's contents with a column read from the reader.
    fn read_replace(&mut self, reader: &mut BinaryReader<'_>) -> KernelResult<()>;

    /// Remove `entity`'s component from this column, if present. The seam that
    /// lets the world clean every column on despawn without knowing the type.
    fn remove_entity(&mut self, entity: EntityId);
}

impl<T: Reflect> ErasedColumn for ComponentColumn<T> {
    fn describe(&self) -> TypeSchema {
        T::SCHEMA
    }

    fn entry_count(&self) -> usize {
        self.len()
    }

    fn write(&self, writer: &mut BinaryWriter) {
        self.reflect_write(writer);
    }

    fn read_replace(&mut self, reader: &mut BinaryReader<'_>) -> KernelResult<()> {
        ComponentColumn::<T>::reflect_read(reader).map(|column| *self = column)
    }

    fn remove_entity(&mut self, entity: EntityId) {
        let _ = self.remove(entity);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::EntityId;

    fn e(raw: u64) -> EntityId {
        EntityId::from_raw(raw)
    }

    #[test]
    fn erased_column_describes_writes_and_reads_back() {
        let mut col: ComponentColumn<u32> = ComponentColumn::new();
        col.insert(e(1), 10);
        col.insert(e(2), 20);

        let erased: &dyn ErasedColumn = &col;
        assert_eq!(erased.describe().name(), "u32");
        assert_eq!(erased.entry_count(), 2);

        let mut w = BinaryWriter::new();
        erased.write(&mut w);
        let bytes = w.into_bytes();

        let mut target: ComponentColumn<u32> = ComponentColumn::new();
        {
            let erased_mut: &mut dyn ErasedColumn = &mut target;
            erased_mut
                .read_replace(&mut BinaryReader::new(&bytes))
                .unwrap();
        }
        assert_eq!(target.entry_count(), 2);
        assert_eq!(target.get(e(1)), Some(&10));
        assert_eq!(target.get(e(2)), Some(&20));
    }

    #[test]
    fn read_replace_rejects_truncation() {
        let mut col: ComponentColumn<u32> = ComponentColumn::new();
        let erased: &mut dyn ErasedColumn = &mut col;
        assert!(erased.read_replace(&mut BinaryReader::new(&[])).is_err());
    }

    #[test]
    fn remove_entity_drops_the_row() {
        let mut col: ComponentColumn<u32> = ComponentColumn::new();
        col.insert(e(1), 10);
        col.insert(e(2), 20);
        {
            let erased: &mut dyn ErasedColumn = &mut col;
            erased.remove_entity(e(1));
            // Removing an absent entity is a clean no-op.
            erased.remove_entity(e(9));
        }
        assert_eq!(col.entry_count(), 1);
        assert!(col.get(e(1)).is_none());
        assert_eq!(col.get(e(2)), Some(&20));
    }
}
