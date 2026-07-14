//! The frontend view model: typed, styled widget descriptions the screens
//! produce and the platform presenter renders. These are app-local styled
//! components over the interface layer's value vocabulary (`UiRect` rects,
//! packed colors) — placement, focus, and content are all decided HERE, so
//! the presenter is a dumb renderer and the whole view is native-testable.

pub mod arcade_button;
pub mod arcade_panel;
pub mod navigation_hint;
pub mod setting_row;
pub mod team_card;
pub mod value_selector;

pub use arcade_button::{ArcadeButton, ButtonStyle};
pub use arcade_panel::ArcadePanel;
pub use navigation_hint::{hints_for, Hint, HintSet};
pub use setting_row::{CategoryTabs, RowControl, SettingRow};
pub use team_card::{EmblemView, RatingBars, Side, TeamCard};
pub use value_selector::ValueSelector;

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
    /// Optional CSS accent color (team tints, warnings).
    pub accent: Option<String>,
    /// Italicized display where supported (the arcade slant).
    pub italic: bool,
}

/// The oversized END ZONE title mark (procedural).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TitleLogo {
    pub small: bool,
    /// Blinking PRESS START line under the logo.
    pub press_start: bool,
}

/// One typed widget.
#[derive(Debug, Clone, PartialEq)]
pub enum Widget {
    Panel(ArcadePanel),
    Button(ArcadeButton),
    TeamCard(TeamCard),
    SettingRow(SettingRow),
    Selector(ValueSelector),
    Tabs(CategoryTabs),
    Label(Label),
    Emblem(EmblemView),
    Ratings(RatingBars),
    Logo(TitleLogo),
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
}

/// A modal dialog option.
#[derive(Debug, Clone, PartialEq)]
pub struct ModalOption {
    pub id: WidgetId,
    pub label: String,
    pub focused: bool,
    pub danger: bool,
}

/// An app-styled modal dialog (focus-confined; never a browser dialog).
#[derive(Debug, Clone, PartialEq)]
pub struct ModalView {
    pub title: String,
    pub body: String,
    pub options: Vec<ModalOption>,
}

/// What sits behind the interface: the live procedural field presentation,
/// dimmed for readability, optionally team-tinted at its edges.
#[derive(Debug, Clone, PartialEq)]
pub struct BackgroundView {
    pub show_field: bool,
    /// 0 = fully visible field, 1 = fully covered.
    pub dim: f32,
    /// CSS team tints applied to the left/right screen regions.
    pub tint: Option<(String, String)>,
    /// Whether continuous decorative motion is allowed (reduced motion off).
    pub animated: bool,
}

/// The complete per-tick frontend view.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneView {
    pub screen: Screen,
    pub widgets: Vec<Placed>,
    pub modal: Option<ModalView>,
    pub hints: Vec<Hint>,
    pub background: BackgroundView,
    pub transition: Option<TransitionView>,
    /// Attract-mode feature phrase currently displayed, if any.
    pub ticker: Option<String>,
}
