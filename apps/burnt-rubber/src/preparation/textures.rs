//! **Texture preparation** — synthesising the three procedural albedos before
//! the race starts.
//!
//! The road's asphalt, the verge's grass and the palm foliage are generated
//! pixel by pixel on the CPU. All three are argument-free deterministic
//! constants: `asphalt_albedo()` and its siblings take nothing and return the
//! same bytes on every call, on every machine. That is exactly the shape that
//! belongs in a startup phase — there is no gameplay state to wait for, and
//! nothing about them can change once the race is running.
//!
//! # What this task does NOT do
//!
//! It produces pixels; it does not register them. Uploading a texture needs
//! `&mut RunningApp`, and a [`PreparationTask`] is handed nothing —
//! deliberately, because that is what stops startup work from reaching into the
//! frame path. So the split is:
//!
//! ```text
//! TextureTask::prepare   ->  Vec<u8> pixels          (inside the barrier)
//! ScenePalette::install_prepared  ->  add_texture_data + materials  (after it)
//! ```
//!
//! The expensive half moves; the cheap half stays where the app builds its
//! scene. Registration order is untouched, which matters more than it sounds:
//! `add_texture_data` mints `id = custom_textures.len() + 1`, those ids are
//! baked into material contents via `with_custom_texture`, and both are encoded
//! in the committed golden artifacts.

use std::cell::RefCell;
use std::rc::Rc;

use axiom_runtime::{PreparationTask, RuntimeResult};

use crate::render::asphalt_texture::asphalt_albedo;
use crate::render::foliage_texture::foliage_albedo;
use crate::render::verge_texture::verge_albedo;

/// The three procedural albedos, synthesised once.
///
/// Deliberately not `Default`: an empty pixel buffer is not a texture, and a
/// silently-empty albedo is exactly the plausible-looking wrong value the
/// `Option` product cell exists to prevent.
#[derive(Debug, Clone)]
pub struct PreparedTextures {
    asphalt: Vec<u8>,
    verge: Vec<u8>,
    foliage: Vec<u8>,
}

impl PreparedTextures {
    /// Run all three generators. This is the expensive half of texture setup,
    /// and the only thing [`TextureTask`] does.
    pub fn generate() -> PreparedTextures {
        PreparedTextures {
            asphalt: asphalt_albedo(),
            verge: verge_albedo(),
            foliage: foliage_albedo(),
        }
    }

    /// The road surface albedo (`ASPHALT_RES` square, RGBA8).
    pub fn asphalt(&self) -> &[u8] {
        &self.asphalt
    }

    /// The verge albedo (`VERGE_RES` square, RGBA8).
    pub fn verge(&self) -> &[u8] {
        &self.verge
    }

    /// The palm-crown albedo (`FOLIAGE_RES` square, RGBA8).
    pub fn foliage(&self) -> &[u8] {
        &self.foliage
    }
}

/// Synthesises the three albedos at startup into [`PreparedTextures`].
#[derive(Debug)]
pub struct TextureTask {
    /// The cell this task writes its product into.
    pub out: Rc<RefCell<Option<PreparedTextures>>>,
}

impl PreparationTask for TextureTask {
    fn prepare(&mut self) -> RuntimeResult<()> {
        // Infallible: all three generators are argument-free and allocate their
        // own buffer. There is no failure mode to invent here.
        *self.out.borrow_mut() = Some(PreparedTextures::generate());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::asphalt_texture::RES as ASPHALT_RES;
    use crate::render::foliage_texture::RES as FOLIAGE_RES;
    use crate::render::verge_texture::RES as VERGE_RES;

    fn prepared() -> PreparedTextures {
        let out = Rc::new(RefCell::new(None));
        let mut task = TextureTask {
            out: Rc::clone(&out),
        };
        task.prepare().expect("the generators are infallible");
        let product = out.borrow_mut().take();
        product.expect("the task wrote its product")
    }

    /// Each albedo is a full RGBA8 buffer at its own resolution.
    #[test]
    fn preparing_produces_the_three_albedos() {
        let t = prepared();
        assert_eq!(t.asphalt().len(), (ASPHALT_RES * ASPHALT_RES * 4) as usize);
        assert_eq!(t.verge().len(), (VERGE_RES * VERGE_RES * 4) as usize);
        assert_eq!(t.foliage().len(), (FOLIAGE_RES * FOLIAGE_RES * 4) as usize);
    }

    /// The move changed nothing: prepared pixels equal what the generator
    /// produces when called directly, which is what keeps the goldens still.
    #[test]
    fn the_prepared_albedos_match_the_generators() {
        let t = prepared();
        assert_eq!(t.asphalt(), asphalt_albedo().as_slice());
        assert_eq!(t.verge(), verge_albedo().as_slice());
        assert_eq!(t.foliage(), foliage_albedo().as_slice());
    }

    /// Deterministic, as the generators' own `f() == f()` tests already imply —
    /// asserted here because the whole phase rests on it.
    #[test]
    fn two_preparations_produce_identical_pixels() {
        let a = prepared();
        let b = prepared();
        assert_eq!(a.asphalt(), b.asphalt());
        assert_eq!(a.verge(), b.verge());
        assert_eq!(a.foliage(), b.foliage());
    }
}
