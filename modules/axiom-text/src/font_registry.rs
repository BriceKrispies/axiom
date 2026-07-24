//! The font registry: generational [`FontHandle`]s over registered compiled
//! fonts, with reference counting so a font in use cannot be unregistered.

use crate::compiled_font::CompiledFont;
use crate::text_error::{TextError, TextResult};

/// A stable, generation-checked handle to a registered font. A handle to a slot
/// whose font was unregistered (and possibly replaced) fails validation instead
/// of silently addressing the new font.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FontHandle {
    /// Slot index in the registry.
    pub index: u32,
    /// Generation the slot held when this handle was issued.
    pub generation: u32,
}

/// One registry slot: a generation counter, an optional font, and how many live
/// text objects reference it.
#[derive(Debug, Clone, Default)]
struct FontSlot {
    generation: u32,
    font: Option<CompiledFont>,
    refs: u32,
}

/// A generational store of compiled fonts.
#[derive(Debug, Clone, Default)]
pub(crate) struct FontRegistry {
    slots: Vec<FontSlot>,
}

impl FontRegistry {
    /// The number of live (registered) fonts.
    pub(crate) fn live_count(&self) -> u32 {
        self.slots.iter().filter(|slot| slot.font.is_some()).count() as u32
    }

    /// Register a font, reusing a freed slot when one exists. Fails
    /// `CapacityExceeded` past `limit`.
    pub(crate) fn register(&mut self, font: CompiledFont, limit: u32) -> TextResult<FontHandle> {
        (self.live_count() < limit)
            .then_some(())
            .ok_or(TextError::CapacityExceeded)
            .map(|()| {
                let free = self.slots.iter().position(|slot| slot.font.is_none());
                let index = free.unwrap_or(self.slots.len());
                free.is_none().then(|| self.slots.push(FontSlot::default()));
                let slot = &mut self.slots[index];
                slot.font = Some(font);
                slot.refs = 0;
                FontHandle {
                    index: index as u32,
                    generation: slot.generation,
                }
            })
    }

    /// Borrow a font by handle, checking the generation.
    pub(crate) fn get(&self, handle: FontHandle) -> Option<&CompiledFont> {
        self.slots
            .get(handle.index as usize)
            .filter(|slot| slot.generation == handle.generation)
            .and_then(|slot| slot.font.as_ref())
    }

    /// Validate a handle, returning `StaleFontHandle` if it no longer resolves.
    pub(crate) fn require(&self, handle: FontHandle) -> TextResult<()> {
        self.get(handle)
            .map(|_| ())
            .ok_or(TextError::StaleFontHandle)
    }

    /// The handle of the first registered font advertising `family`, if any.
    pub(crate) fn find_by_family(&self, family: &str) -> Option<FontHandle> {
        self.slots.iter().enumerate().find_map(|(index, slot)| {
            slot.font
                .as_ref()
                .filter(|font| font.family() == family)
                .map(|_| FontHandle {
                    index: index as u32,
                    generation: slot.generation,
                })
        })
    }

    /// Increment a font's reference count (a text now uses it).
    pub(crate) fn retain(&mut self, handle: FontHandle) {
        self.slots
            .get_mut(handle.index as usize)
            .filter(|slot| slot.generation == handle.generation)
            .into_iter()
            .for_each(|slot| slot.refs += 1);
    }

    /// Decrement a font's reference count (a text stopped using it).
    pub(crate) fn release(&mut self, handle: FontHandle) {
        self.slots
            .get_mut(handle.index as usize)
            .filter(|slot| slot.generation == handle.generation)
            .into_iter()
            .for_each(|slot| slot.refs = slot.refs.saturating_sub(1));
    }

    /// Unregister a font. Fails `StaleFontHandle` if the handle is dead, or
    /// `FontStillReferenced` if a live text still uses it.
    pub(crate) fn unregister(&mut self, handle: FontHandle) -> TextResult<()> {
        self.slots
            .get_mut(handle.index as usize)
            .filter(|slot| (slot.generation == handle.generation) & slot.font.is_some())
            .ok_or(TextError::StaleFontHandle)
            .and_then(|slot| {
                (slot.refs == 0)
                    .then_some(slot)
                    .ok_or(TextError::FontStillReferenced)
            })
            .map(|slot| {
                slot.font = None;
                slot.generation += 1;
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fallback_font::default_font;

    #[test]
    fn register_get_and_generation_check() {
        let mut reg = FontRegistry::default();
        let h = reg.register(default_font(), 8).unwrap();
        assert_eq!(reg.live_count(), 1);
        assert!(reg.get(h).is_some());
        assert_eq!(reg.require(h), Ok(()));
        reg.unregister(h).unwrap();
        assert_eq!(reg.require(h), Err(TextError::StaleFontHandle));
        assert!(reg.get(h).is_none());
    }

    #[test]
    fn slot_reuse_bumps_generation_and_invalidates_old_handle() {
        let mut reg = FontRegistry::default();
        let h1 = reg.register(default_font(), 8).unwrap();
        reg.unregister(h1).unwrap();
        let h2 = reg.register(default_font(), 8).unwrap();
        assert_eq!(h2.index, h1.index, "slot reused");
        assert_ne!(h2.generation, h1.generation, "generation advanced");
        assert_eq!(reg.require(h1), Err(TextError::StaleFontHandle));
        assert!(reg.get(h2).is_some());
    }

    #[test]
    fn capacity_is_enforced() {
        let mut reg = FontRegistry::default();
        reg.register(default_font(), 1).unwrap();
        assert_eq!(
            reg.register(default_font(), 1),
            Err(TextError::CapacityExceeded)
        );
    }

    #[test]
    fn referenced_font_cannot_be_unregistered() {
        let mut reg = FontRegistry::default();
        let h = reg.register(default_font(), 8).unwrap();
        reg.retain(h);
        assert_eq!(reg.unregister(h), Err(TextError::FontStillReferenced));
        reg.release(h);
        assert_eq!(reg.unregister(h), Ok(()));
    }

    #[test]
    fn unregister_twice_is_stale() {
        let mut reg = FontRegistry::default();
        let h = reg.register(default_font(), 8).unwrap();
        reg.unregister(h).unwrap();
        assert_eq!(reg.unregister(h), Err(TextError::StaleFontHandle));
    }
}
