//! Shared test fixtures: small hand-checkable region graphs and typed fields.
//!
//! Test-only (`#[cfg(test)]`): the solver tests build path/line graphs whose BFS
//! distances, receivers and flow are trivial to verify by hand.

use axiom_geosphere::RegionGraph;
use axiom_kernel::Meters;

/// A path graph `0-1-2-…-(n-1)`: each interior region borders its two neighbours,
/// the ends border one. The CSR mirrors the app's original test globes.
pub(crate) fn line_graph(n: usize) -> RegionGraph {
    let mut offsets = vec![0u32];
    let mut neighbours = Vec::new();
    (0..n).for_each(|i| {
        (i > 0).then(|| neighbours.push((i - 1) as u32));
        (i + 1 < n).then(|| neighbours.push((i + 1) as u32));
        offsets.push(neighbours.len() as u32);
    });
    RegionGraph {
        offsets,
        neighbours,
    }
}

/// Lift raw elevations into `Meters` (tests own the raw scalars; the layer API is
/// typed).
pub(crate) fn meters(values: &[f32]) -> Vec<Meters> {
    values.iter().copied().map(Meters::finite_or_zero).collect()
}
