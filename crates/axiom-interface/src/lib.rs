//! # Axiom Interface — the deterministic, renderer-neutral interface layer
//!
//! `axiom-interface` is a root-adjacent engine layer (`depends_on = ["kernel"]`)
//! that owns the neutral primitives behind engine-facing, UI-like surfaces:
//! panels with stable identity, integer layout rectangles, visibility, pinning,
//! focus, keyboard/text input events as data, a command-console model, and
//! **neutral interface draw descriptions**. It adapts the kernel's `HandleId`
//! into [`PanelId`] (the genuine kernel dependency) and produces an
//! [`InterfaceDrawList`] that a platform backend (DOM, canvas, native) turns into
//! pixels.
//!
//! ## Why a layer, not a module
//! The debug overlay grew its own panel/window/console/focus system; the moment a
//! second UI surface needs the same primitives, an engine **module** cannot
//! supply them (modules may not depend on modules). The Module Law's own rule —
//! "a primitive many modules need belongs in a lower layer" — makes this a layer.
//!
//! ## What it never owns
//! No DOM, browser, WebGPU/WebGL/Canvas2D, native windows, renderer submission,
//! font rasterization, debug metrics, profiler/editor/menu/settings logic. The
//! browser binding stays in the platform-facing module that *consumes* this layer
//! (the debug overlay's wasm arm renders the draw list).
//!
//! ## Public surface
//! One behavioral facade, [`InterfaceApi`], plus the neutral value vocabulary it
//! traffics in: [`PanelId`], [`InterfaceInputEvent`], [`InterfaceDrawList`] /
//! [`InterfaceDrawItem`], the keybinding primitive [`Keymap`] / [`KeyBinding`],
//! and the console-command shape a consumer composes — [`ParsedCommand`],
//! [`CommandOutcome`], [`CommandTable`] / [`CommandSpec`].

mod command_table;
mod console_model;
mod draw_list;
mod focus_state;
mod input_event;
mod interface_api;
mod interface_command;
mod interface_state;
mod keymap;
mod layout_rect;
mod panel;
mod panel_id;

pub use command_table::CommandSpec;
pub use command_table::CommandTable;
pub use draw_list::InterfaceDrawItem;
pub use draw_list::InterfaceDrawList;
pub use input_event::InterfaceInputEvent;
pub use interface_api::InterfaceApi;
pub use interface_command::CommandOutcome;
pub use interface_command::ParsedCommand;
pub use keymap::KeyBinding;
pub use keymap::Keymap;
pub use panel_id::PanelId;
