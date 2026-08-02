//! The real Canvas 2D presentation arm — wasm32 only.
//!
//! This is the thin platform binding: it owns the browser's
//! `CanvasRenderingContext2d` and blits the pure rasterizer's finished RGBA
//! framebuffer to it with `putImageData`. It contains **no** rendering logic —
//! all projection, triangle rasterization, depth testing, terrain LOD, material
//! fallback, fog, and budgeting live in the pure, native-tested core
//! (`software_rasterizer` and friends), so this arm only constructs an
//! `ImageData` and uploads it. None of it compiles on native, so the engine's
//! default build, `cargo test`, and the coverage gate never pull in `web-sys`.
//!
//! The canvas backing store is set to the low internal resolution and CSS-scaled
//! to the display size with `image-rendering: pixelated`, so the browser does a
//! crisp nearest-neighbour upscale of the low-poly image.

use wasm_bindgen::{Clamped, JsCast, JsValue};
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, HtmlElement, ImageData};

/// The browser canvas + its 2D context, sized to the low framebuffer resolution.
#[derive(Debug)]
pub(crate) struct LiveCanvasBinding {
    canvas: HtmlCanvasElement,
    ctx: CanvasRenderingContext2d,
}

impl LiveCanvasBinding {
    /// Acquire the canvas's 2D context and switch its backing store to the
    /// internal `fb_width`×`fb_height` resolution. Errors (no context, wrong
    /// type) surface as `JsValue` so the caller can fall through to
    /// "unsupported".
    ///
    /// ## This binding owns the backing store, and nothing else
    ///
    /// The **backing store** (`canvas.width`/`height`) is the backend's: it is
    /// the resolution the software rasterizer writes, and no page can know it.
    /// The **CSS box** — how large that image is drawn on screen — belongs to
    /// the page, exactly as it does for the GPU binding, which sets no style at
    /// all.
    ///
    /// This used to set `width`/`height`/`max-width` inline, and that was the
    /// defect. Inline styles beat an author stylesheet, so a page's responsive
    /// rule could not win: an app whose CSS said `width: 100vw; height: 100vh`
    /// on a phone got a proportional-height box from this backend and a
    /// full-screen one from the GPU backend. The same app laid out differently
    /// depending on which backend it happened to bind — the exact disagreement
    /// the old comment here called out as something "a backend has no business
    /// doing", while doing it.
    ///
    /// The overflow that inline `max-width: 100%` was defending against does not
    /// return: it could only happen because a *pixel* width was being forced
    /// here too. With no inline layout at all, an unstyled canvas falls back to
    /// its intrinsic backing-store size — standard replaced-element behaviour,
    /// and the same thing the GPU arm has always given those pages.
    ///
    /// `image-rendering` stays: it is a paint hint for how to filter the
    /// upscale, not a layout decision, and a page that wanted smooth scaling of
    /// a deliberately low-res framebuffer would be asking for a blurrier
    /// version of this backend's whole point.
    pub(crate) fn attach(
        canvas: &HtmlCanvasElement,
        fb_width: u32,
        fb_height: u32,
    ) -> Result<Self, JsValue> {
        let ctx = canvas
            .get_context("2d")?
            .ok_or_else(|| JsValue::from_str("no 2d context"))?
            .dyn_into::<CanvasRenderingContext2d>()?;

        canvas.set_width(fb_width);
        canvas.set_height(fb_height);
        // `style()` lives on HtmlElement; a canvas *is* one. Paint hint only —
        // see the note above on why no layout property is set here.
        let style = canvas.unchecked_ref::<HtmlElement>().style();
        let _ = style.set_property("image-rendering", "pixelated");
        // putImageData ignores smoothing, but keep any future drawImage crisp too.
        ctx.set_image_smoothing_enabled(false);

        Ok(LiveCanvasBinding {
            canvas: canvas.clone(),
            ctx,
        })
    }

    /// Upload one finished frame's RGBA bytes to the canvas. `rgba` is
    /// `width*height*4` bytes, row-major, top-left origin — exactly what
    /// `ImageData` expects.
    pub(crate) fn blit(&self, width: u32, height: u32, rgba: &[u8]) {
        // Keep the backing store in lockstep with the framebuffer size.
        if self.canvas.width() != width || self.canvas.height() != height {
            self.canvas.set_width(width);
            self.canvas.set_height(height);
        }
        if let Ok(image) = ImageData::new_with_u8_clamped_array_and_sh(Clamped(rgba), width, height)
        {
            let _ = self.ctx.put_image_data(&image, 0.0, 0.0);
        }
    }
}
