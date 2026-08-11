//! **Texture preparation** — synthesizing the road, verge and foliage albedos
//! once, at startup, instead of inside scene installation.
//!
//! These bakes depend on nothing but their own authored parameters, which is
//! precisely why they belong in the preparation phase: they are expensive, they
//! need no gameplay state at all, and nothing about them can change once the
//! race is running.
//!
//! The task body is currently an inert placeholder — the scaffold is in place
//! and the generation still runs where it always did, so the game's behaviour
//! is unchanged.

use std::cell::RefCell;
use std::rc::Rc;

use axiom_runtime::{PreparationTask, RuntimeResult};

/// The synthesized-texture product of the preparation phase.
#[derive(Debug, Clone, Default)]
pub struct PreparedTextures {}

/// Bakes the procedural albedo textures into [`PreparedTextures`].
///
/// It carries no inputs beyond its output cell: the generators read authored
/// constants, not the seed and not the tuning.
#[derive(Debug)]
pub struct TextureTask {
    /// The cell this task writes its product into.
    pub out: Rc<RefCell<Option<PreparedTextures>>>,
}

impl PreparationTask for TextureTask {
    fn prepare(&mut self) -> RuntimeResult<()> {
        *self.out.borrow_mut() = Some(PreparedTextures::default());
        Ok(())
    }
}
