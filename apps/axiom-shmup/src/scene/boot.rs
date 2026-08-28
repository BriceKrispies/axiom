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
    // Two separate factors, multiplied together.
    //
    // FIRST, the scale. `SkyDriver::key_light` divides the sun by
    // `KEY_INTENSITY_FULL_SCALE` because the engine types a light's intensity as
    // a `Ratio`, and that constant's own doc says what it is: *"a stand-in for
    // the source's exposure path, not a replacement for it."* Multiplying it
    // back here is what restores the scene to the scale it was shaded at. Every
    // other radiance the app authors — the sky's two gradient stops, the sun
    // disc, the hemisphere ambient, the fog colour — now carries the same
    // divisor (`look::SCENE_RADIANCE_SCALE`), so this one multiply restores all
    // of them together and the sun-to-shadow ratio survives it.
    //
    // SECOND, the metering. The source does not carry a fixed exposure at all:
    // `render/index.js:249` runs an `AutoExposure` — a GPU log-luminance
    // reduction to EV100 (`render/exposure.js:107-118`:
    // `ev = log2(L * 100/12.5)`, `exposure = keyScale / (1.2 * 2^ev)`, i.e.
    // `exposure = 1 / (9.6 * L)` for a log-average scene luminance `L`) —
    // re-metered every frame. This port has no metering pass, so this factor
    // stands in for one.
    //
    // ---- MEASURED, on matched framing ------------------------------------
    //
    // `uv run scripts/parity_shot.py hero`, 1280x720, camera PINNED and clock
    // PINNED against the original's own `hero` shot (`apps/shmup/src/dev/shots.js`,
    // which also carries `time: 16.5` — the hour `look::HOUR` now matches).
    //
    // The fit is not a mean-of-the-frame: the two towns are not byte-identical
    // (the port draws 1.64x the instances), so a whole-frame mean mixes the
    // exposure gap with a dressing gap. It is taken on the two regions least
    // sensitive to dressing — `skyHi`, which is the sky pass and nothing else,
    // and `sunlit`, a large facade dominated by the key — inverted through AgX's
    // own curve rather than compared as bytes:
    //
    //     contrast(t) = byte / 255      (the composite's `pow(.,2.2)` and the
    //                                    sRGB encode cancel exactly)
    //     scene       = 2^(t * 16.5 - 12.47393)
    //
    //     region    original -> port      needs
    //     skyHi      188.89     93.57     +3.101 stops   (x8.578)
    //     sunlit     175.03     71.98     +3.385 stops   (x10.446)
    //
    // Those two agree to 0.28 stop, and that agreement is the load-bearing
    // result: it says the sky and the key light are on ONE scale to within a
    // third of a stop, which is what `look::SCENE_RADIANCE_SCALE` exists to
    // guarantee and what the old double-tone-map destroyed. A missing `PI` there
    // would show up here as a 1.65-stop disagreement between these two rows.
    //
    // So the remaining error is a single global exposure, and the fit is their
    // geometric mean: `x9.466` on an authored `1.1301`.
    const METERING_FIT: f64 = 1.348;
    let exposure = (crate::scene::wiring::look::KEY_INTENSITY_FULL_SCALE * METERING_FIT) as f32;
    windowing.set_tonemap(FrameTonemap::blended(
        Ratio::new(1.0).expect("an authored tone-map strength is finite"),
        Ratio::new(exposure).expect("the restored photometric scale is finite"),
    ));
    windowing.set_surfaces(scene.app.surfaces().to_vec());
    windowing.set_material_programs(scene.app.material_surface_programs());

    let meshes = scene.app.mesh_set();
    // The per-mesh triangle table the frame census needs to turn a draw list into
    // a triangle count. Once, at bind, before the set is handed to the backend.
    scene.console.borrow_mut().observe_meshes(&meshes);
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
        // Through the console, not around it: `frame_dt` applies the `dt` pin when
        // one is installed and, either way, RECORDS what this frame advanced by,
        // which is what lets `stats` answer `dt_used=` instead of leaving a
        // harness to assume its pin took. See `DevConsole::frame_dt`.
        let dt = scene.console.borrow_mut().frame_dt((now - last) / 1000.0);
        last = now;

        let pad = crate::input::dom::poll_pad();
        let pose = {
            let mut input = input.borrow_mut();
            input.poll_gamepad(pad);
            // The console's input pin (`freeze on`). `Input::frozen` was ported
            // faithfully and then had no writer outside its own tests, so capture
            // mode existed in the type and could not be entered.
            input.frozen = scene.console.borrow().frozen();
            scene.game.frame(dt, &mut input)
        };
        // The scripted camera, for the same reason and in the same shape. Without
        // this line `cam` is accepted, reported as installed, and moves no pixel —
        // and a parity harness comparing two different framings is measuring
        // nothing. `DevConsole::resolve_camera` is the identity when no override
        // is set, so an ordinary run is unchanged apart from the recorded pose.
        let pose = scene.console.borrow_mut().resolve_camera(pose);
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
        // The frame census, and with it the only readiness signal this side has:
        // `__ax_console` is installed before the GPU binds, so a harness waiting
        // on the global is waiting for nothing, while a non-zero `observed=` is
        // the first fact that means a frame was actually rendered.
        scene.console.borrow_mut().observe_frame(&outcome);
        // `window.__READY__` once three engine frames have run -- the same
        // BOOT_FRAMES the original uses (`apps/shmup/src/main.js`), so "ready"
        // means the same thing on both sides of a parity capture. `__ax_console`
        // is installed BEFORE the GPU binds, so its existence is not a ready
        // signal and a harness that waits on it is waiting for nothing.
        (scene.console.borrow().frames_observed() >= 3).then(|| {
            web_sys::window().map(|w| {
                js_sys::Reflect::set(
                    &w,
                    &wasm_bindgen::JsValue::from_str("__READY__"),
                    &wasm_bindgen::JsValue::TRUE,
                )
            })
        });
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

