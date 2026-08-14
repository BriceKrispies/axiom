//! The DOM half of the dial panel: **the sliders are built here, from the dial
//! table**, and they write into the shared [`SceneConfig`] the frame closure
//! reads.
//!
//! ## Why the panel is generated and not authored in the page
//!
//! A slider whose range lives in HTML and whose meaning lives in Rust is two
//! sources of truth that drift the first time a dial is re-scoped. So the page
//! carries one empty container and every control in it — label, input, range,
//! step, numeric read-out — is created from [`Dial::ALL`]. A dial cannot exist
//! without a slider in front of it, and a slider cannot exist without a dial
//! behind it.
//!
//! ## The one cell, and why it is not hidden state
//!
//! The DOM is an event source and the render loop is a consumer, so exactly one
//! `Rc<RefCell<SceneConfig>>` bridges them — the same shape `pointer_input.rs`
//! already uses for the orbit camera, and it lives here at the browser edge
//! rather than anywhere the scene can see it. Everything downstream takes the
//! configuration **by value or by reference, as an argument**: hand the same
//! config and the same tick to the animation twice and you get the same pose.
//!
//! ## Live dials, and the one that is not
//!
//! Fourteen dials re-pose the running scene, so their `input` handler writes the
//! cell and returns; the next frame reads it. The fifteenth (`detail`)
//! re-tessellates geometry, which the live backend uploads once at bind, so it
//! cannot be answered by a re-pose — its handler puts the whole configuration in
//! the query string and reloads the page. Every *other* change also rewrites the
//! query string through `replaceState`, so that reload (and the one a device
//! rotation triggers, see `live.rs`) comes back to the scene the user had built
//! rather than to the defaults.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::config::{Dial, SceneConfig};

/// The live configuration, shared between the DOM's event handlers and the
/// render loop.
pub type SharedConfig = Rc<RefCell<SceneConfig>>;

/// Build the panel inside the element with id `container_id` and wire every
/// slider to `config`.
///
/// A page without that element (a harness, a screenshot host) simply gets no
/// panel — the scene still runs at whatever configuration it was handed.
pub fn install(container_id: &str, config: SharedConfig) {
    let Some(document) = web_sys::window().and_then(|window| window.document()) else {
        return;
    };
    let Some(container) = document.get_element_by_id(container_id) else {
        return;
    };
    container.set_inner_html("");
    for dial in Dial::ALL {
        let spec = dial.spec();
        let Ok(row) = document.create_element("div") else {
            continue;
        };
        row.set_class_name("dial");

        let Ok(name) = document.create_element("span") else {
            continue;
        };
        name.set_class_name("dial-name");
        name.set_text_content(Some(spec.label));

        let Ok(readout) = document.create_element("span") else {
            continue;
        };
        readout.set_class_name("dial-value");
        readout.set_id(&readout_id(dial));
        readout.set_text_content(Some(&printed(dial, config.borrow().raw(dial))));

        let Some(input) = document
            .create_element("input")
            .ok()
            .and_then(|element| element.dyn_into::<web_sys::HtmlInputElement>().ok())
        else {
            continue;
        };
        input.set_type("range");
        input.set_min(&spec.min.to_string());
        input.set_max(&spec.max.to_string());
        input.set_step(&spec.step.to_string());
        input.set_value(&config.borrow().raw(dial).to_string());
        input.set_class_name("dial-slider");
        let _ = input.set_attribute("data-dial", spec.key);
        let _ = input.set_attribute("aria-label", spec.label);

        let _ = row.append_child(&name);
        let _ = row.append_child(&readout);
        let _ = row.append_child(&input);
        let _ = container.append_child(&row);

        listen(&input, dial, config.clone());
    }
}

/// Attach the one `input` handler this slider needs.
fn listen(input: &web_sys::HtmlInputElement, dial: Dial, config: SharedConfig) {
    let element = input.clone();
    let handler = Closure::wrap(Box::new(move || {
        let Ok(value) = element.value().parse::<f32>() else {
            return;
        };
        let updated = {
            let mut current = config.borrow_mut();
            current.set(dial, value);
            *current
        };
        // The read-out shows the *taken* value, not the raw event value, so a
        // clamp or a snap is visible rather than silent.
        show(dial, updated.raw(dial));
        remember(&updated);
        // Geometry is uploaded once at bind, so the detail dial cannot be
        // answered by a re-pose. The scene has just been written into the URL,
        // so the reload comes back to exactly this configuration.
        (!dial.spec().live).then(reload);
    }) as Box<dyn FnMut()>);
    let _ = input
        .add_event_listener_with_callback("input", handler.as_ref().unchecked_ref());
    handler.forget();
}

/// Write `value` into `dial`'s numeric read-out.
fn show(dial: Dial, value: f32) {
    web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id(&readout_id(dial)))
        .into_iter()
        .for_each(|node| node.set_text_content(Some(&printed(dial, value))));
}

/// The DOM id of `dial`'s numeric read-out.
fn readout_id(dial: Dial) -> String {
    format!("dial-value-{}", dial.spec().key)
}

/// A dial's value as the panel prints it: fixed to the dial's own precision, and
/// spelled out in words where a bare number would be meaningless.
fn printed(dial: Dial, value: f32) -> String {
    match dial {
        Dial::Direction => ["clockwise", "anticlockwise"][usize::from(value > 0.0)].to_string(),
        Dial::Detail => crate::variant::SceneVariant::from_index(value as usize)
            .label()
            .to_string(),
        _ => format!("{value:.*}", dial.spec().decimals),
    }
}

/// Put the whole configuration in the address bar without navigating, so a
/// reload — whether the detail dial's or a device rotation's — restores it.
fn remember(config: &SceneConfig) {
    let query = config.to_query();
    let target = format!("?{query}");
    let url = [".", target.as_str()][usize::from(!query.is_empty())];
    let _ = web_sys::window()
        .and_then(|window| window.history().ok())
        .map(|history| history.replace_state_with_url(&JsValue::NULL, "", Some(url)));
}

/// Re-run the app against the configuration now in the address bar.
fn reload() {
    let _ = web_sys::window().map(|window| window.location().reload());
}
