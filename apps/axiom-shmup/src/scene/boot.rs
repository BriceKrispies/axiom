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
    // Keys are wired on every device; the pointer arm depends on which one this
    // is, so it is chosen below once `is_mobile` has answered.
    crate::input::dom::attach_keyboard(&input);
    // The touch overlay, only where the primary input really is a finger on a
    // small screen. On anything else this is `None` and costs one branch at boot.
    //
    // It produces a synthetic gamepad quad and the same `Mouse0`/`Mouse2` codes a
    // mouse produces, so nothing downstream knows a phone is involved: the dead
    // zone, the look curve, the fire gate and the ADS blend are the ones the port
    // already has. A second control path would have been a second place for "what
    // does aiming feel like" to live.
    let touch = crate::touch::is_mobile()
        .then(|| crate::touch::attach(&input));
    // Exactly one pointer path. Attaching the mouse arm as well would fire the
    // weapon on every joystick press and take a pointer lock that freezes the
    // client coordinates the overlay measures from - see `input::dom::attach_mouse`.
    touch
        .is_none()
        .then(|| crate::input::dom::attach_mouse(&input, &canvas));

    // The surface follows the VIEWPORT, not a fixed 16:9 plate.
    //
    // 1280x720 was hardcoded and the canvas is `width:100vw;height:100vh`, so on a
    // 414x896 phone the browser stretched a 16:9 render into a 0.46 portrait and
    // the street visibly sheared. A backbuffer that does not match the element it
    // is presented into is a distortion no camera setting can undo.
    //
    // Capped on the long axis: past 1280 the extra pixels cost fill rate — this
    // frame is fill-bound, see `RenderScaleController` below — and buy nothing a
    // phone panel can show. The adaptive scaler trims from there.
    const MAX_LONG_EDGE: f64 = 1280.0;
    let vw = window
        .inner_width()
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(MAX_LONG_EDGE)
        .max(1.0);
    let vh = window
        .inner_height()
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(720.0)
        .max(1.0);
    let fit = (MAX_LONG_EDGE / vw.max(vh)).min(1.0);
    let (width, height) = (
        (vw * fit).round().max(1.0) as u32,
        (vh * fit).round().max(1.0) as u32,
    );
    // The camera has to be told, or the projection keeps the 16:9 it was built
    // with and shears in the other direction.
    scene.game.aspect = f64::from(width) / f64::from(height);
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
    // The quality preset's render scale, which the port has always carried and
    // never handed to the engine. `config.js:53/68` authors 0.72 at low and 0.85
    // at medium; `config.rs` ports both faithfully, and nothing read them — which
    // is why `?quality=low` measured identically to `?quality=ultra` (29.2 ms
    // against 29.4 ms) despite the preset table saying it should not.
    //
    // It is the lever that matters here. Measured on the `hero` pose with vsync
    // off, the frame is FILL-RATE bound: dropping the backbuffer from 1280x720 to
    // 640x360 — a 4x pixel cut — took the frame from 29.2 ms to 7.00 ms, a 4.2x
    // speedup almost exactly proportional to pixel count, while draws (140),
    // instances (~503) and triangles (799,288) stayed IDENTICAL in every view.
    //
    // `render_scale_control` is a setter the engine has always exposed
    // (`windowing_api.rs:646`), read once per frame by the driver.
    //
    // `for_display()` rather than a fixed rung, and rather than the quality
    // preset's authored `render_scale`. The engine has no public `RenderScale`
    // constructor on purpose: the scale is an OUTPUT of a controller that
    // watches measured frame time, not a value an app picks. Its own doc calls
    // this "the constructor an app should use", and warns that handing it the
    // simulation's fixed step bakes in the assumption that the display refreshes
    // at the tick rate -- on a 120 Hz panel the loop would hold 16 ms frames and
    // call it a success while the display asked for 8.
    //
    // The controller straddles the budget with a dead band (drop above 1.08x,
    // raise below 0.78x) so the resolution cannot visibly breathe at the exact
    // rate it was asked to hold.
    // A HARD 60 FLOOR, so the budget is not 60.
    //
    // `for_display()` defends 16.667 ms, and the controller does exactly that:
    // it parks AT the budget. Measured, it settled on 16.8 ms — nominally 59.5
    // fps and over the line on every single spike. A controller told to defend
    // 60 delivers a median of 60 and a tail well under it, which is not a floor.
    //
    // The floor is set by the TAIL, so the budget has to be sized from it.
    // Measured under camera motion (`scripts/frame_motion.py`): worst/median was
    // 33.0/16.8 = 1.96, and p99/median 25.1/16.8 = 1.49. To keep the WORST frame
    // inside 16.667 ms at that spread the median must sit near 8.5 ms, so the
    // budget is 8 ms and the controller is left to find the rung that holds it.
    //
    // That is deliberately an aggressive cut: it will drive the ladder
    // (0.50/0.62/0.75/0.87/1.0) down to a low rung and the image will be softer.
    // The trade is stated rather than hidden, `scripts/quality_guard.py` measures
    // what it costs, and the honest fix that would buy the quality back is
    // cutting per-fragment work — at scale 1.0 this frame is 29.2 ms and the
    // material shader composes twelve layers per fragment.
    //
    // `retarget` clamps to at most `SLOWEST_BUDGET_NANOS` (16.667 ms), so a
    // budget can only ever be made tighter than 60, never looser.
    const FRAME_BUDGET_NANOS: u64 = 8_000_000;
    // `holding_floor`, not `new`: the optimistic constructors start at full scale
    // and walk DOWN, and that descent costs ~4 x (DROP_RUN + CHANGE_COOLDOWN)
    // frames. Measured here, a fresh load with a 60 s settle still read 16.80 ms
    // while a long-running session read 7.5 ms — about a minute of play spent
    // under the target, which no budget value can fix because the cost is in the
    // starting position. Starting at the coarsest rung makes frame one already
    // safe; quality climbs back on evidence.
    let mut render_scale = axiom_host::RenderScaleController::holding_floor(FRAME_BUDGET_NANOS);
    let set_render_scale = windowing.render_scale_control();
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
    // `uv run scripts/parity_shot.py hero`, 1280x720, every axis PINNED (camera,
    // clock, input, resolution, frame index) against the original's own `hero`
    // shot (`apps/shmup/src/dev/shots.js`, which also carries `time: 16.5` — the
    // hour `look::HOUR` now matches).
    //
    // The fit is not a mean-of-the-frame: the two towns are not the same town
    // (the port draws 1.64x the instances), so a whole-frame mean mixes the
    // exposure gap with a dressing gap. It is taken on the regions least
    // sensitive to dressing — `skyHi`, which is the sky pass and nothing else;
    // `sunlit`, a large facade dominated by the key; and `fg` — inverted through
    // AgX's own curve rather than compared as bytes:
    //
    //     contrast(t) = byte / 255      (the composite's `pow(.,2.2)` and the
    //                                    sRGB encode cancel exactly)
    //     scene       = 2^(t * 16.5 - 12.47393)
    //
    // Two rounds. The first, against an authored `1.1301`, read `skyHi +3.101`
    // and `sunlit +3.385` stops short — agreeing to 0.28 stop, which is the
    // load-bearing result: the sky and the key are on ONE scale to within a
    // third of a stop, exactly what `look::SCENE_RADIANCE_SCALE` exists to
    // guarantee. A missing `PI` there would have shown up here as a 1.65-stop
    // disagreement between those two rows, and did not.
    //
    // The second round, after `look::dome_shoulder` restored the sky's own
    // published roll-off, reads:
    //
    //     region    original -> port      residual
    //     skyHi      188.9      191.4     -0.09 stops
    //     sunlit     175.0      181.0     -0.20 stops
    //     fg          70.7       76.8     -0.23 stops
    //
    // so this trims by their mean, -0.17 stops (x0.889), from 1.348.
    //
    // `street` is deliberately excluded and still reads +0.90: the original's
    // road is grey asphalt and the port's is sand. That is an albedo, not an
    // exposure, and metering on it would drag the whole frame to hide it.
    const METERING_FIT: f64 = 1.199;
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
        // Feed the measured frame to the adaptive scaler and hand the driver the
        // rung it picked. Measured on the `hero` pose with vsync off, this frame
        // is FILL-RATE bound: cutting the backbuffer from 1280x720 to 640x360 --
        // a 4x pixel cut -- took it from 29.2 ms to 7.00 ms, a 4.2x speedup
        // almost exactly proportional to pixel count, while draws (140),
        // instances (~503) and triangles (799,288) stayed IDENTICAL in every
        // view. Resolution is therefore the only lever that moves this frame,
        // and quality presets demonstrably were not one: low measured 29.2 ms
        // against ultra's 29.4 ms.
        set_render_scale(render_scale.observe((dt * 1.0e9) as u64));
        last = now;

        // Touch first, then a real gamepad. They cannot both be live in
        // practice, and `TouchControls::pad` reports `None` while the stick is
        // idle, so a phone with a controller attached still works.
        let pad = touch
            .as_ref()
            .and_then(crate::touch::TouchControls::pad)
            .or_else(crate::input::dom::poll_pad);
        let pose = {
            let mut input = input.borrow_mut();
            // INSTALL it, do not apply it. `Game::frame` calls `begin_frame`,
            // which polls the pad itself; applying it here as well meant the
            // game's own call reset the stick a line later and no stick input
            // ever reached the player.
            input.set_pad(pad);
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
            // The pad and the move vector are reported for the same reason the
            // lock state is: a stick that deflects on screen and moves nobody is
            // indistinguishable, from outside, from a stick that is not being
            // read. `pad` is what the frame HANDED the input; `move` is what the
            // movement machine ASKED for afterwards. When those two disagree the
            // fault is between them, and when they agree and the player stands
            // still it is downstream of both.
            let (mvx, mvy) = inp.move_vector();
            let pad_text = pad.map_or_else(
                || "none".to_owned(),
                |a| format!("{:.3},{:.3},{:.3},{:.3}", a[0], a[1], a[2], a[3]),
            );
            scene.console.borrow_mut().set_status(format!(
                "pointer_locked={} enabled={} frozen={} pointerLockElement={} lock_error={} \
                 pad={pad_text} move={mvx:.3},{mvy:.3}",
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

