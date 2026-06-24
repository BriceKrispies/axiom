//! # Axiom Layout — responsive, mobile-first placement
//!
//! The engine's single home for **how on-screen regions are ordered and placed**.
//! Given the host boundary's viewport facts — logical size, [`Orientation`], and
//! safe-area insets — and a flat flex/constraint node tree, [`solve`] computes one
//! placed [`LayoutRect`] per node, recalculated whenever the viewport changes. Apps
//! (and, later, the interface layer) declare *intent* — roles, grow weights, min
//! sizes, an aspect to preserve — and the engine decides placement, **mobile-first
//! by default**: the root is inset by the safe area, an [`Direction::Adaptive`] node
//! flips its children from a row to a stacked column on a narrow/upright screen, and
//! free space is distributed by grow weight. No per-app CSS breakpoints.
//!
//! ## What this layer is / is not
//! - It **is** a pure, deterministic solver: same viewport + tree in, same rects
//!   out, fully testable on native. No browser, no renderer, no app concepts.
//! - It is **not** an interactive UI system. `axiom-interface` owns *interactive*
//!   panels (drag/pin/console); this layer owns *responsive placement*. They
//!   compose — a panel's responsive default position can come from here.
//!
//! ## Shape
//! Build a [`LayoutTree`] with [`LayoutTreeBuilder`] (a root + children, each with a
//! [`LayoutStyle`] and a caller [`NodeId`]), call [`solve`], and read each region's
//! rect from the [`LayoutResult`].
//!
//! [`Orientation`]: axiom_host::Orientation

mod insets;
mod layout_rect;
mod layout_style;
mod node_id;
mod result;
mod solver;
mod style_enums;
mod tree;

pub use insets::Insets;
pub use layout_rect::LayoutRect;
pub use layout_style::LayoutStyle;
pub use node_id::NodeId;
pub use result::LayoutResult;
pub use solver::solve;
pub use style_enums::{Align, CrossSize, Direction, FlexWrap, Justify};
pub use tree::{LayoutTree, LayoutTreeBuilder};
