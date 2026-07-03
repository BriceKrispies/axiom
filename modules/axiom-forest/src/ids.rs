//! The pure value-type vocabulary `ForestApi` traffics in.

use axiom_kernel::Meters;
use axiom_scatter::ScatterRule;

/// How a chunk of forest is grown: the scatter rule that places the trees, and
/// the size range each seated tree is scaled into.
#[derive(Debug, Clone)]
pub struct ForestConfig {
    /// World size of one chunk's side (metres) — passed through to the scatter.
    pub cell_size: Meters,
    /// The jittered-sub-grid rule that places this chunk's trees.
    pub scatter: ScatterRule,
    /// Smallest tree size (uniform scale, metres) a placed tree takes.
    pub min_size: Meters,
    /// Largest tree size (uniform scale, metres) a placed tree takes.
    pub max_size: Meters,
}
