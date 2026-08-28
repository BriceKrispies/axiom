//! **The browser entry point.**
//!
//! `wasm_bindgen`'s `shmup_start` and the presentation loop behind it: build the
//! scene, attach the DOM input listeners, bind the engine's live surface, and
//! drive frames until the page goes away.
//!
//! Split out of `scene::app` because it is the one part of that file with no
//! native counterpart at all — it is `#[cfg(target_arch = "wasm32")]` end to
//! end, and it never compiles in the test build. Keeping it beside code the
//! tests do exercise is how a wasm-only break gets past a green `cargo test`,
//! which happened here: an edit compiled and tested clean natively, the wasm
//! build failed, and `axiom-serve` went on serving the last good bundle while a
//! measurement was taken off it.


#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
use axiom::prelude::*;

#[cfg(target_arch = "wasm32")]
use crate::scene::app::{build, SURFACE_ID};
use crate::scene::draw::{drive_viewmodel, write_camera};

/// Browser entry: build the scene, attach the input listeners, and drive the
/// presentation loop. See the module doc comment for why this is
/// `axiom-windowing` rather than `App::run`.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn shmup_start() {
    console_error_panic_hook::set_once();

    let mut scene = build(crate::engine::CAPTURE_SEED);
    let input = std::rc::Rc::new(std::cell::RefCell::new(crate::input::Input::new()));

    let window = web_sys::window().expect("a browser window");
    let document = window.document().expect("a document");
    let canvas: web_sys::HtmlElement = document
        .get_element_by_id(SURFACE_ID)
        .expect("the page hosts the presentation element")
        .unchecked_into();
    crate::input::dom::attach(&input, &canvas);

    let (width, height) = (1280u32, 720u32);
    // `.ow-hud` is `position: fixed; inset: 0`, so the HUD sizes to the
    // VIEWPORT, not to the surface's backing store.
    let hud_w = window
        .inner_width()
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(f64::from(width));
    let hud_h = window
        .inner_height()
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(f64::from(height));
    scene.game.hud.resize(hud_w, hud_h);
    // Build the overlay, the four layers, every widget's DOM, and inject
    // `style.css.tpl`. Exactly once.
    scene.game.hud.mount();

    let mut windowing = axiom_windowing::WindowingApi::new();
    windowing
        .configure_surface(width, height)
        .expect("surface dimensions are valid");
    windowing.set_ambient(scene.app.ambient());
    scene
        .app
        .depth_fog()
        .into_iter()
        .for_each(|fog| windowing.set_depth_fog(fog));
    scene
        .app
        .sky()
        .into_iter()
        .for_each(|sky| windowing.set_sky(sky));
    scene
        .app
        .indirect()
        .into_iter()
        .for_each(|fill| windowing.set_indirect(fill));
    // AgX, at full strength.
    //
    // The source tonemaps with AgX and meters the scene to an EV100 value; the
    // reference capture in `docs/work-manifests/shmup-port/reference/` is grey
    // and filmic where an untonemapped frame is saturated and contrasty, and
    // that difference is the largest single one between the two images.
    //
    // This also switches the scene target to `Rgba16Float`. Until now nothing
    // did: `RenderCapability::HdrTargets` was granted and `scene_target_format`
    // still returned 8-bit, so every value above 1.0 was clipped at the scene
    // pass and the bloom, exposure and AgX ports were all inert. A tone map is
    // the app-side switch for the whole HDR path.
    // **The scene's photometric scale, and the metering the port does not have.**
    //
    // Two separate facts, multiplied together.
    //
    // FIRST, the scale. `SkyDriver::key_light` divides the sun by
    // `KEY_INTENSITY_FULL_SCALE` because the engine types a light's intensity as
    // a `Ratio`, and that constant's own doc says what it is: *"a stand-in for
    // the source's exposure path, not a replacement for it."* Nothing put it
    // back, so every surface reached AgX about eight times under-exposed. The
    // frame came out dark and over-saturated — the classic "untonemapped"
    // signature, produced not by a missing tone map but by feeding a correct one
    // the wrong radiance.
    //
    // SECOND, the metering. The source does not carry a fixed exposure at all:
    // `render/index.js:207` runs an `AutoExposure` — a GPU log-luminance
    // reduction to EV100, `exposure = 1/H` — re-metered every frame. This port
    // has no metering pass, so the second factor stands in for one, and it is
    // **fitted, not derived**: with the scale alone the frame metered a mean
    // luminance of 119 against the original's 95.7 at this hour, measured on
    // matched captures with the original running beside it.
    //
    // Because it is a fit and not a meter, it is correct for THIS hour. Move
    // `HOUR` and it will drift, and the honest fix at that point is to port
    // `render/exposure.js` rather than to re-fit this number.
    const METERING_FIT: f64 = 2.11;
    let exposure = (crate::scene::wiring::look::KEY_INTENSITY_FULL_SCALE * METERING_FIT) as f32;
    windowing.set_tonemap(FrameTonemap::blended(
        Ratio::new(1.0).expect("an authored tone-map strength is finite"),
        Ratio::new(exposure).expect("the restored photometric scale is finite"),
    ));
    windowing.set_surfaces(scene.app.surfaces().to_vec());
    windowing.set_material_programs(scene.app.material_surface_programs());

    let meshes = scene.app.mesh_set();
    let materials = scene.app.material_textures();
    // The bake-once soldier bodies, uploaded at bind alongside the rigid set.
    let skinned_meshes = scene.app.skinned_mesh_set();
    // The live backend sizes its skinned instance buffer from `max_instances`
    // too, so the soldiers' upper bound has to be in it.
    let max_instances =
        (scene.app.renderable_count() + scene.soldier_draw.max_draws_per_frame()) as u32;
    // The shared cell the driver reads just before each present.
    let skinned_source: std::rc::Rc<
        std::cell::RefCell<Vec<(u64, u64, [f32; 16], [f32; 16], [f32; 4], Vec<[f32; 16]>)>>,
    > = Default::default();
    let skinned_sink = std::rc::Rc::clone(&skinned_source);

    // ---- the dev console, reachable from JavaScript ------------------------
    //
    // `window.__ax_console("ids on")` returns the console's reply as a string.
    // That is the whole agent interface: a Playwright `eval` can drive it, take
    // a screenshot, and read the codebase's own identifiers off the picture —
    // no rebuild, no source edit, no keyboard.
    //
    // The overlay host is a plain absolutely-positioned div. It is NOT drawn
    // into the WebGPU surface on purpose: a label rendered by the engine would
    // be subject to the tone map, the fog and the post chain, so the one thing
    // it must stay is legible would be the first thing to go.
    let console_handle = std::rc::Rc::clone(&scene.console);
    let console_frame = std::rc::Rc::clone(&scene.console);
    let overlay = document
        .create_element("div")
        .expect("a document can create a div");
    overlay
        .set_attribute(
            "style",
            "position:fixed;inset:0;pointer-events:none;z-index:50;font:11px ui-monospace,\
             Menlo,Consolas,monospace",
        )
        .expect("an attribute can be set");
    overlay.set_id("ax-ids");
    document
        .body()
        .expect("a document body")
        .append_child(&overlay)
        .expect("the overlay can be appended");

    let bind = wasm_bindgen::closure::Closure::wrap(Box::new(
        move |command: wasm_bindgen::JsValue| -> wasm_bindgen::JsValue {
            let text = command.as_string().unwrap_or_default();
            let reply = console_handle.borrow_mut().exec(&text);
            wasm_bindgen::JsValue::from_str(&reply)
        },
    )
        as Box<dyn FnMut(wasm_bindgen::JsValue) -> wasm_bindgen::JsValue>);
    js_sys::Reflect::set(
        &window,
        &wasm_bindgen::JsValue::from_str("__ax_console"),
        bind.as_ref().unchecked_ref(),
    )
    .expect("the binding can be installed");
    // Deliberately leaked: the closure has to outlive this function for the
    // whole session, and there is no later moment that could drop it.
    bind.forget();
    // The frame loop's handle to the same div.
    let overlay_host = overlay.clone();

    let performance = window.performance().expect("a performance clock");
    let mut last = performance.now();

    // `run_web_multi_skinned`, not `run_web_multi`: the plain entry uploads no
    // skinned meshes and reads no skinned draws, so the soldiers would simulate
    // and never appear. This call had ZERO callers in the repository — the
    // skinning path was exercised only headless, by `tools/axiom-shot`.
    let _ = windowing.run_web_multi_skinned(
        SURFACE_ID,
        meshes,
        materials,
        skinned_meshes,
        max_instances,
        // Keep the ambient the app already bound.
        None,
        skinned_source,
        move |tick| {
        let now = performance.now();
        let dt = (now - last) / 1000.0;
        last = now;

        let pad = crate::input::dom::poll_pad();
        let pose = {
            let mut input = input.borrow_mut();
            input.poll_gamepad(pad);
            scene.game.frame(dt, &mut input)
        };
        write_camera(&mut scene.app, pose);
        // The same three steps `frame` runs. This loop INLINES them rather than
        // calling it, so anything added to `frame` alone silently never runs in
        // the browser — which is exactly how the viewmodel appeared wired and
        // was not. Keep the two in step.
        drive_viewmodel(&mut scene, pose);
        scene.game.hud_frame(&input.borrow());
        // Report the input state the console answers `lock` with. One format a
        // frame; the alternative is a state nothing can observe, which is how
        // the refused-lock bug stayed invisible.
        {
            let inp = input.borrow();
            let element = web_sys::window()
                .and_then(|w| w.document())
                .and_then(|d| d.pointer_lock_element())
                .map(|e| e.id())
                .unwrap_or_else(|| "null".to_owned());
            scene.console.borrow_mut().set_status(format!(
                "pointer_locked={} enabled={} frozen={} pointerLockElement={} lock_error={}",
                inp.pointer_locked,
                inp.enabled,
                inp.frozen,
                element,
                inp.lock_error.as_deref().unwrap_or("none")
            ));
        }
        scene.fx_draw.frame(
            &mut scene.app,
            &scene.game.fx_audio.fx,
            pose,
            scene.game.time.elapsed,
        );
        scene.soldier_draw.frame(&mut scene.app, &scene.game.ai);

        let outcome = scene.app.tick(tick);
        // The id overlay. Off unless `window.__ax_console("ids on")` has run, in
        // which case `labels` is empty and this is one `Vec` allocation a frame.
        let labels = console_frame.borrow().labels(
            outcome.camera_view_proj(),
            f64::from(overlay_host.client_width()),
            f64::from(overlay_host.client_height()),
            pose.eye,
        );
        overlay_host.set_inner_html(
            &labels
                .iter()
                .map(|l| {
                    format!(
                        "<div style=\"position:absolute;left:{:.0}px;top:{:.0}px;\
                         transform:translate(-50%,-50%);color:#7dff9b;\
                         text-shadow:0 0 3px #000,0 0 3px #000;white-space:nowrap\">\
                         &#9679; {} <span style=\"opacity:.55\">{:.0}m</span></div>",
                        l.x, l.y, l.name, l.depth
                    )
                })
                .collect::<String>(),
        );

        // The skinned draws ride the shared cell, not the returned tuple.
        *skinned_sink.borrow_mut() = outcome
            .skinned_draws()
            .iter()
            .map(|d| {
                (
                    d.mesh_id(),
                    d.material_id(),
                    d.mvp(),
                    d.world(),
                    d.color(),
                    d.joints().to_vec(),
                )
            })
            .collect();
        let lights = outcome
            .lights()
            .iter()
            .map(|l| (l.kind(), l.vec(), l.color(), l.intensity()))
            .collect();
        (
            outcome.clear_color(),
            lights,
            outcome.light_view_proj(),
            outcome.mesh_batches(),
            axiom_host::FrameCamera::new(
                outcome.camera_view(),
                outcome.camera_projection(),
                outcome.camera_view_proj(),
            ),
            outcome.mesh_batch_casters(),
            outcome.sdf_scene().cloned(),
        )
        },
    );
}

