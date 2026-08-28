//! The HUD's one injected stylesheet.
//!
//! Ported from `C:/dev/Claude-of-Duty/src/ui/style.js:1-716`.
//!
//! ## Design system
//!
//!  - **scale** every dimension is `calc(N * var(--k))` where `--k` is set from
//!    the viewport height (1080p == 1.0). The HUD therefore holds its
//!    proportions from 720p to 4K without re-authoring.
//!  - **spacing** 4px grid: `--u`. Screen margins are 6u (24px @1080p), the
//!    same margin CoD uses (~2.2% of height).
//!  - **type** one condensed system stack, uppercase, tabular figures, three
//!    ink levels (94% / 58% / 30%) and one accent per semantic: amber =
//!    caution, red = threat, cyan = friendly/objective.
//!  - **contrast** every text run carries a two-stop shadow (tight dark + wide
//!    dark bloom) so it survives a blown-out sky *and* a black interior without
//!    a scrim behind it.
//!
//! `--k` (see [`scale_factor`]) is the one piece of this file with real logic
//! attached, so it is kept as an ordinary, natively-testable function rather
//! than buried in the injected CSS; every widget that needs to know the
//! current scale reads it (`ui/index.js:587`'s `this.k = clamp(h / 1080, 0.62,
//! 2.4)`).
//!
//! The CSS and SVG-defs text is byte-for-byte the source's template literals,
//! with `FONT_STACK`/`FONT_DISPLAY`/`FONT_MONO` substituted at call time
//! exactly where the source's `${...}` interpolations sit — see
//! [`super::util::FONT_STACK`] and neighbours. Installing/removing the
//! `<style>`/`<defs>` elements is a `wasm32`-only DOM operation; the CSS text
//! itself is plain, native-testable data.

use super::util::{FONT_DISPLAY, FONT_MONO, FONT_STACK};

/// `ui/index.js:587`: `this.k = clamp(h / 1080, 0.62, 2.4)`. 1080p maps to
/// `1.0`; the range keeps the HUD legible from a tiny embedded viewport up to
/// a 4K canvas without the type going illegibly small or comically large.
pub fn scale_factor(viewport_height_px: f64) -> f64 {
    super::util::clamp(viewport_height_px / 1080.0, 0.62, 2.4)
}

/// The stylesheet body, with the three font stacks substituted where the
/// source writes `${FONT_STACK}` / `${FONT_DISPLAY}` / `${FONT_MONO}`
/// (`style.js:69-71`). Plain runtime substitution, not `format!`: the
/// template is loaded with `include_str!` and `format!` requires its
/// template argument to be a literal token, which a macro-included file is
/// not.
pub fn css() -> String {
    include_str!("style.css.tpl")
        .replace("__FONT_STACK__", FONT_STACK)
        .replace("__FONT_DISPLAY__", FONT_DISPLAY)
        .replace("__FONT_MONO__", FONT_MONO)
}

/// The one-off SVG `<defs>` block (`style.js:684-694`): the organic-edge
/// turbulence filter the blood vignette (`health.rs`) references by url.
pub const DEFS: &str = r##"<svg width="0" height="0" style="position:absolute" aria-hidden="true">
  <defs>
    <!-- organic edge for the blood vignette: banded turbulence displacing the
         gradient so the hurt overlay never reads as a clean radial ramp -->
    <filter id="ow-warp" x="-12%" y="-12%" width="124%" height="124%" color-interpolation-filters="sRGB">
      <feTurbulence type="fractalNoise" baseFrequency="0.006 0.011" numOctaves="4" seed="17" result="n"/>
      <feDisplacementMap in="SourceGraphic" in2="n" scale="34" xChannelSelector="R" yChannelSelector="G"/>
    </filter>
  </defs>
</svg>"##;

/// `installStyles()` / `removeStyles()` — `wasm32` only. Idempotent, exactly
/// as the source (`installed` guard).
#[cfg(target_arch = "wasm32")]
pub mod install {
    use std::cell::Cell;

    use wasm_bindgen::JsCast;

    thread_local! {
        static INSTALLED: Cell<bool> = const { Cell::new(false) };
    }

    fn document() -> web_sys::Document {
        web_sys::window().expect("no window").document().expect("no document")
    }

    pub fn install_styles() {
        let installed = INSTALLED.with(|c| c.get());
        if installed && document().get_element_by_id("ow-ui-style").is_some() {
            return;
        }
        let doc = document();
        let style: web_sys::HtmlStyleElement = doc
            .create_element("style")
            .expect("create style")
            .unchecked_into();
        style.set_id("ow-ui-style");
        style.set_text_content(Some(&super::css()));
        doc.head().expect("no head").append_child(&style).expect("append style");

        let defs = doc.create_element("div").expect("create div");
        defs.set_id("ow-ui-defs");
        defs.set_inner_html(super::DEFS);
        doc.body().expect("no body").append_child(&defs).expect("append defs");

        INSTALLED.with(|c| c.set(true));
    }

    pub fn remove_styles() {
        let doc = document();
        if let Some(s) = doc.get_element_by_id("ow-ui-style") {
            s.remove();
        }
        if let Some(d) = doc.get_element_by_id("ow-ui-defs") {
            d.remove();
        }
        INSTALLED.with(|c| c.set(false));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_factor_is_one_at_1080p() {
        assert_eq!(scale_factor(1080.0), 1.0);
    }

    #[test]
    fn scale_factor_clamps_at_both_ends() {
        assert_eq!(scale_factor(240.0), 0.62);
        assert_eq!(scale_factor(10_000.0), 2.4);
    }

    #[test]
    fn scale_factor_scales_linearly_between_bounds() {
        assert!((scale_factor(2160.0) - 2.0).abs() < 1e-9); // 4K height
        assert!((scale_factor(720.0) - (720.0 / 1080.0)).abs() < 1e-9);
    }

    #[test]
    fn css_substitutes_every_font_stack_exactly_once() {
        let text = css();
        assert_eq!(text.matches(FONT_STACK).count(), 1);
        assert_eq!(text.matches(FONT_DISPLAY).count(), 1);
        assert_eq!(text.matches(FONT_MONO).count(), 1);
        // The design-system constants the doc comment names.
        assert!(text.contains("--k: 1"));
        assert!(text.contains("#ffb02a")); // amber
        assert!(text.contains("#ff3f31")); // red/threat
        assert!(text.contains("#79d2ff")); // cyan/friendly
    }

    #[test]
    fn defs_declares_the_warp_filter_health_references() {
        assert!(DEFS.contains(r#"id="ow-warp""#));
    }
}
