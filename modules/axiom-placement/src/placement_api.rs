//! [`PlacementApi`] — deterministic object scatter on the proc substrate.
//!
//! `scatter` evaluates a draw-only [`Recipe`] of `count` nodes at a content
//! [`Address`] and reduces each artifact word into an integer grid position. The
//! same `(seed, address, count, bounds)` always yields the same [`Placement`].
//! Branchless and integer-only — no naked floats cross the boundary.

use axiom_proc::{ProcApi, Recipe};
use axiom_space::Address;

use crate::placement::Placement;

/// The scatter recipe version. Bump to deliberately re-key generation (+ regolden);
/// versioning is a first-class input.
const SCATTER_VERSION: u32 = 1;

/// The deterministic object-placement facade.
#[derive(Debug)]
pub struct PlacementApi;

impl PlacementApi {
    /// Scatter `count` objects across a `width × height` integer grid at `address`
    /// under `seed`. Deterministic: identical inputs always yield the same
    /// placement. Degenerate bounds (a `0` width or height) collapse to the origin
    /// rather than panic.
    pub fn scatter(seed: u64, address: &Address, count: u32, width: u32, height: u32) -> Placement {
        let (artifact, _trace) = ProcApi::evaluate(&scatter_recipe(count), seed, address)
            .expect("the draw-only scatter recipe is a valid DAG");
        let positions = artifact
            .words()
            .iter()
            .map(|&word| position(word, width, height))
            .collect();
        Placement::new(positions)
    }
}

/// A draw-only recipe of `count` nodes — one entropy draw per object.
fn scatter_recipe(count: u32) -> Recipe {
    let mut recipe = Recipe::new(SCATTER_VERSION);
    (0..count).for_each(|_| {
        recipe.draw();
    });
    recipe
}

/// Reduce one drawn word into a grid position. A `0` width/height clamps to `1`
/// (so the position collapses to the origin axis) — branchless and panic-free.
fn position(word: u64, width: u32, height: u32) -> (u32, u32) {
    let w = u64::from(width).max(1);
    let h = u64::from(height).max(1);
    ((word % w) as u32, ((word / w) % h) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_space::SpaceApi;

    fn site(segments: &[u64]) -> Address {
        segments
            .iter()
            .fold(SpaceApi::root(), |a, &s| SpaceApi::child(&a, s))
    }

    #[test]
    fn scatter_is_deterministic_and_within_bounds() {
        let a = site(&[3, 9]);
        let p1 = PlacementApi::scatter(7, &a, 12, 16, 16);
        let p2 = PlacementApi::scatter(7, &a, 12, 16, 16);
        assert_eq!(p1, p2);
        assert_eq!(p1.to_bytes(), p2.to_bytes());
        assert_eq!(p1.len(), 12);
        assert!(!p1.is_empty());
        assert!(p1.positions().iter().all(|&(x, y)| x < 16 && y < 16));
    }

    #[test]
    fn distinct_seeds_or_sites_scatter_differently() {
        let base = PlacementApi::scatter(7, &site(&[3, 9]), 12, 16, 16);
        assert_ne!(base, PlacementApi::scatter(8, &site(&[3, 9]), 12, 16, 16));
        assert_ne!(base, PlacementApi::scatter(7, &site(&[3, 10]), 12, 16, 16));
    }

    #[test]
    fn scatter_reproduces_across_a_sweep() {
        let a = site(&[1]);
        for count in 0..20u32 {
            assert_eq!(
                PlacementApi::scatter(1, &a, count, 8, 8),
                PlacementApi::scatter(1, &a, count, 8, 8)
            );
        }
    }

    #[test]
    fn zero_count_is_empty_and_zero_bounds_are_safe() {
        let a = site(&[0]);
        let empty = PlacementApi::scatter(1, &a, 0, 8, 8);
        assert!(empty.is_empty());
        assert_eq!(empty.len(), 0);
        // Degenerate bounds never panic; everything collapses to the origin.
        let degenerate = PlacementApi::scatter(1, &a, 4, 0, 0);
        assert_eq!(degenerate.len(), 4);
        assert!(degenerate.positions().iter().all(|&p| p == (0, 0)));
    }

    #[test]
    fn golden_scatter_digest_is_stable() {
        let p = PlacementApi::scatter(7, &site(&[3, 9]), 12, 16, 16);
        assert_eq!(p.digest().raw(), 16_830_903_468_323_819_069);
    }

    #[test]
    fn types_are_debug() {
        let p = PlacementApi::scatter(7, &site(&[3, 9]), 2, 8, 8);
        assert!(!format!("{p:?}").is_empty());
        assert!(!format!("{:?}", PlacementApi).is_empty());
    }
}
