//! The presentation asset seam (SPEC-04 §10 `loadTexture` / `loadFont`): the
//! deterministic handle-minting half of texture/font loading.
//!
//! ## Determinism class — presentation, app-side fetch
//! Textures and fonts are **presentation** assets: their pixels are only ever
//! sampled by `onRender`, never read into sim, so they do **not** ride the
//! sim-tick scheduler (`axiom-assets`, which stays the path for sim-class streamed
//! content). The contract is "fetch in the app; handle stable for life": this core
//! mints a stable `u64` handle for a url **synchronously** and remembers the
//! url→handle map; the actual `fetch`/decode/upload happens in the browser arm
//! (`web/src/harness.ts`), which reads the url back via [`GameBridge::texture_url`]
//! and binds the decoded pixels under the same handle. Nothing here touches the
//! network, a wall clock, or a browser symbol — it is pure, native-testable
//! bookkeeping.
//!
//! Font loading is a degenerate case in Tier-0: there is one built-in monospace
//! font ([`crate::font`]), so [`GameBridge::load_font`] ignores the url and returns
//! the built-in handle; the harness bakes the matching atlas.

use crate::font;
use crate::GameBridge;

/// Loaded-texture handles count from this base so they never collide with
/// render-target texture ids (which count from 0) or the reserved font atlas id.
const TEXTURE_ID_BASE: u64 = 0x1000_0000;

/// The url→handle registry for presentation textures: a url's handle is its
/// (stable, dedup'd) index into this list, offset by [`TEXTURE_ID_BASE`].
#[derive(Debug, Default)]
pub struct AssetRegistry {
    texture_urls: Vec<String>,
}

impl AssetRegistry {
    /// A fresh, empty registry.
    pub fn new() -> Self {
        AssetRegistry::default()
    }
}

impl GameBridge {
    /// Mint (or recall) the stable [`crate::font::FONT_ATLAS_TEXTURE`]-style handle
    /// for `url` (`loadTexture`): the same url always returns the same handle, so
    /// an author can load once and draw every frame. The browser arm fetches and
    /// decodes the pixels under this handle; until they arrive, a sprite naming it
    /// simply isn't drawn (no error).
    pub fn load_texture(&mut self, url: &str) -> u64 {
        let existing = self.assets.texture_urls.iter().position(|u| u == url);
        let index = existing.unwrap_or_else(|| {
            self.assets.texture_urls.push(url.to_string());
            self.assets.texture_urls.len() - 1
        });
        index as u64 + TEXTURE_ID_BASE
    }

    /// The url a `loadTexture` handle names, or the empty string for an unknown id
    /// (`textureUrl`). The browser arm calls this once per id to learn what to
    /// fetch.
    pub fn texture_url(&self, id: u64) -> String {
        id.checked_sub(TEXTURE_ID_BASE)
            .and_then(|i| self.assets.texture_urls.get(i as usize))
            .cloned()
            .unwrap_or_default()
    }

    /// Every `loadTexture` handle minted so far, in mint order (`textureIds`). The
    /// browser arm polls this to discover which textures to fetch/decode/upload to
    /// the engine's 2D presenter — it pairs each id with [`Self::texture_url`]. The
    /// reserved font-atlas id is not included (the harness bakes that one itself).
    pub fn texture_ids(&self) -> Vec<u64> {
        (0..self.assets.texture_urls.len())
            .map(|i| i as u64 + TEXTURE_ID_BASE)
            .collect()
    }

    /// The built-in monospace font handle (`loadFont`). Tier-0 ships exactly one
    /// font, so the `url` is ignored and the built-in handle is returned; the
    /// harness bakes the matching atlas under [`crate::font::FONT_ATLAS_TEXTURE`].
    pub fn load_font(&mut self, _url: &str) -> u64 {
        font::BUILTIN_FONT.raw()
    }
}

#[cfg(target_arch = "wasm32")]
mod wasm_exports {
    use wasm_bindgen::prelude::*;

    use crate::wasm::WasmGame;

    #[wasm_bindgen]
    impl WasmGame {
        /// Mint/recall a presentation texture handle for `url` (`loadTexture`).
        #[wasm_bindgen(js_name = loadTexture)]
        pub fn load_texture(&mut self, url: String) -> f64 {
            self.bridge.load_texture(&url) as f64
        }

        /// The url a texture handle names, or `""` for an unknown id (`textureUrl`).
        #[wasm_bindgen(js_name = textureUrl)]
        pub fn texture_url(&self, id: f64) -> String {
            self.bridge.texture_url(id as u64)
        }

        /// Every `loadTexture` handle minted so far (`textureIds`), so the browser
        /// arm can fetch/decode/upload each one's pixels to the engine's 2D
        /// presenter (pairing it with [`Self::texture_url`]).
        #[wasm_bindgen(js_name = textureIds)]
        pub fn texture_ids(&self) -> Vec<f64> {
            self.bridge
                .texture_ids()
                .into_iter()
                .map(|id| id as f64)
                .collect()
        }

        /// The built-in monospace font handle (`loadFont`).
        #[wasm_bindgen(js_name = loadFont)]
        pub fn load_font(&mut self, url: String) -> f64 {
            self.bridge.load_font(&url) as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{demo_app, GameBridge};

    const STEP: u64 = 1_000_000;

    fn bridge() -> GameBridge {
        GameBridge::new(demo_app().build(), 0, STEP, 1)
    }

    #[test]
    fn load_texture_mints_stable_deduped_handles() {
        let mut b = bridge();
        let a = b.load_texture("player.png");
        let c = b.load_texture("enemy.png");
        // Distinct urls get distinct handles; the same url recalls its handle.
        assert_ne!(a, c);
        assert_eq!(b.load_texture("player.png"), a);
        assert_eq!(b.load_texture("enemy.png"), c);
    }

    #[test]
    fn texture_url_round_trips_a_handle_and_rejects_unknowns() {
        let mut b = bridge();
        let h = b.load_texture("hero.png");
        assert_eq!(b.texture_url(h), "hero.png");
        // An id below the base, or past the end, is the empty string.
        assert_eq!(b.texture_url(0), "");
        assert_eq!(b.texture_url(h + 1), "");
    }

    #[test]
    fn load_font_returns_the_builtin_handle_for_any_url() {
        use crate::font::BUILTIN_FONT;
        let mut b = bridge();
        assert_eq!(b.load_font("anything.ttf"), BUILTIN_FONT.raw());
        assert_eq!(b.load_font(""), BUILTIN_FONT.raw());
    }
}
