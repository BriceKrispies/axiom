//! A typed collection of engine assets, addressed by stable handles.

use crate::handle::Handle;

/// A collection of assets of type `T` (e.g. `Assets<Mesh>`, `Assets<Material>`).
///
/// `add` appends a description and returns a stable [`Handle`]; `get` resolves a
/// handle back to its description. This is pure value storage — it deliberately
/// does **not** hold an `axiom-resources` table (that type is un-nameable behind
/// the module facade). The engine walks the collection and registers each asset
/// through `ResourcesApi` when the app runs.
#[derive(Debug, Clone)]
pub struct Assets<T> {
    items: Vec<T>,
}

impl<T> Assets<T> {
    /// An empty collection.
    pub fn new() -> Self {
        Assets { items: Vec::new() }
    }

    /// Append `asset` and return a stable handle to it.
    pub fn add(&mut self, asset: T) -> Handle<T> {
        self.items.push(asset);
        Handle::new(self.items.len() as u64)
    }

    /// The asset a handle refers to, or `None` if the handle is not from this
    /// collection (a stale or zero handle resolves to `None`, never a panic).
    pub fn get(&self, handle: Handle<T>) -> Option<&T> {
        self.items.get((handle.id() as usize).wrapping_sub(1))
    }

    /// The number of assets in the collection.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether the collection is empty.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The assets in insertion order — the order the engine registers them.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.items.iter()
    }
}

impl<T> Default for Assets<T> {
    fn default() -> Self {
        Assets::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::Mesh;

    #[test]
    fn add_returns_one_based_handles_and_get_round_trips() {
        let mut assets: Assets<Mesh> = Assets::new();
        assert!(assets.is_empty());
        let h0 = assets.add(Mesh::Cube);
        let h1 = assets.add(Mesh::Cube);
        assert_eq!(h0.id(), 1);
        assert_eq!(h1.id(), 2);
        assert_eq!(assets.len(), 2);
        assert!(!assets.is_empty());
        assert_eq!(assets.get(h0), Some(&Mesh::Cube));
        assert_eq!(assets.get(h1), Some(&Mesh::Cube));
    }

    #[test]
    fn get_with_a_foreign_or_zero_handle_is_none() {
        let assets: Assets<Mesh> = Assets::default();
        assert_eq!(assets.get(Handle::new(0)), None);
        assert_eq!(assets.get(Handle::new(99)), None);
    }

    #[test]
    fn iter_yields_insertion_order() {
        let mut assets: Assets<Mesh> = Assets::new();
        assets.add(Mesh::Cube);
        assets.add(Mesh::Cube);
        let collected: Vec<&Mesh> = assets.iter().collect();
        assert_eq!(collected, vec![&Mesh::Cube, &Mesh::Cube]);
    }
}
