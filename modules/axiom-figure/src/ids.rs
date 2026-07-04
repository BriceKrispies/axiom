//! The module's value vocabulary — the pure data types [`FigureApi`] traffics
//! in, gathered here so `lib.rs` re-exports them alongside the one behavioral
//! facade via a single `pub use ids::{…}` (Module Law #8).
//!
//! [`FigureApi`]: crate::FigureApi

pub use crate::definition::FigureDefinition;
pub use crate::figure_error::{FigureError, FigureResult};
pub use crate::part::FigurePart;
pub use crate::posed_part::PosedPart;
