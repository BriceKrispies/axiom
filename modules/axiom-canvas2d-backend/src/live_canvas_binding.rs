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
    /// Acquire the canvas's 2D context, switch its backing store to the internal
    /// `fb_width`×`fb_height` resolution, and CSS-scale it to `display_width` with
    /// pixelated upscaling and a **proportional** height. Errors (no context, wrong
    /// type) surface as `JsValue` so the caller can fall through to "unsupported".
    ///
    /// The display height is `auto`, NOT a fixed pixel value: the canvas is a
    /// replaced element, so `height: auto` derives the display height from the
    /// backing store's aspect ratio. Because the backing store now preserves the
    /// surface aspect (see [`CanvasQualityPreset::framebuffer_dims`]), the on-screen
    /// box tracks the surface aspect exactly — matching the GPU canvas, which is
    /// styled the same responsive way. Pinning a fixed `height` px here was the bug
    /// that let a width-constrained canvas (e.g. `width:960px` clamped to the
    /// column by `max-width:100%`) keep its full height and so distort the image.
    ///
    /// [`CanvasQualityPreset::framebuffer_dims`]: crate::canvas_policy::CanvasQualityPreset::framebuffer_dims
    pub(crate) fn attach(
        canvas: &HtmlCanvasElement,
        fb_width: u32,
        fb_height: u32,
        display_width: u32,
    ) -> Result<Self, JsValue> {
        let ctx = canvas
            .get_context("2d")?
            .ok_or_else(|| JsValue::from_str("no 2d context"))?
            .dyn_into::<CanvasRenderingContext2d>()?;

        // Backing store = low internal resolution; CSS box = display width with a
        // proportional (aspect-preserving) height, scaled nearest-neighbour
        // ("pixelated") for a crisp low-poly look.
        canvas.set_width(fb_width);
        canvas.set_height(fb_height);
        // `style()` lives on HtmlElement; a canvas *is* one.
        let style = canvas.unchecked_ref::<HtmlElement>().style();
        let _ = style.set_property("image-rendering", "pixelated");
        let _ = style.set_property("width", &format!("{display_width}px"));
        let _ = style.set_property("height", "auto");
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
