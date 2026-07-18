//! The frontend view model: typed, styled widget descriptions the screens
//! produce and the platform presenter renders. These are app-local styled
//! components over the interface layer's value vocabulary (`UiRect` rects,
//! packed colors) — placement, focus, and content are all decided HERE, so the
//! presenter is a dumb renderer and the whole view is native-testable.

pub mod arcade_button;
pub mod arcade_panel;
pub mod navigation_hint;

pub use arcade_button::{ArcadeButton, ButtonStyle};
pub use arcade_panel::ArcadePanel;
pub use navigation_hint::{hints_for, Hint, HintSet};

use axiom_interface::UiRect;

use super::navigation::WidgetId;
use super::state::Screen;
use super::transitions::TransitionView;

/// Text scale steps for labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelSize {
    Huge,
    Heading,
    Body,
    Small,
}

/// A styled text label.
#[derive(Debug, Clone, PartialEq)]
pub struct Label {
    pub text: String,
    pub size: LabelSize,
    /// Optional CSS accent color.
    pub accent: Option<String>,
    /// Italicized display where supported (the arcade slant).
    pub italic: bool,
}

impl Label {
    pub fn new(text: &str, size: LabelSize) -> Self {
        Label {
            text: text.to_string(),
            size,
            accent: None,
            italic: false,
        }
    }
}

/// The oversized END ZONE title mark (procedural).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TitleLogo {
    pub small: bool,
    /// Blinking PRESS START line under the logo.
    pub press_start: bool,
}

/// One settings row: a label and its current value string, optionally a value
/// the player adjusts left/right (a slider or a cycled enum).
#[derive(Debug, Clone, PartialEq)]
pub struct SettingRow {
    pub label: String,
    pub value: String,
    /// A `0.0..=1.0` fill for slider-style rows (volume); `None` for toggles.
    pub fill: Option<f32>,
}

/// One typed widget.
#[derive(Debug, Clone, PartialEq)]
pub enum Widget {
    Panel(ArcadePanel),
    Button(ArcadeButton),
    Label(Label),
    Logo(TitleLogo),
    Setting(SettingRow),
}

/// A widget placed on the logical viewport.
#[derive(Debug, Clone, PartialEq)]
pub struct Placed {
    pub id: WidgetId,
    pub rect: UiRect,
    pub focused: bool,
    pub enabled: bool,
    pub widget: Widget,
}

impl Placed {
    pub fn new(id: WidgetId, rect: UiRect, widget: Widget) -> Self {
        Placed {
            id,
            rect,
            focused: false,
            enabled: true,
            widget,
        }
    }

    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }
}

/// What sits behind the interface: the live procedural field presentation,
/// dimmed for readability.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BackgroundView {
    pub show_field: bool,
    /// 0 = fully visible field, 1 = fully covered.
    pub dim: f32,
}

/// The complete per-tick frontend view.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneView {
    pub screen: Screen,
    pub widgets: Vec<Placed>,
    pub hints: Vec<Hint>,
    pub background: BackgroundView,
    pub transition: Option<TransitionView>,
}
