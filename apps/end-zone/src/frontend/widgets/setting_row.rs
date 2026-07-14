//! Settings rows and category tabs: one typed row per setting, with the
//! control kind (selector / toggle / volume slider / key binding / action)
//! the presenter renders and the screen adjusts.

/// The interactive control a settings row carries.
#[derive(Debug, Clone, PartialEq)]
pub enum RowControl {
    /// Left/right steps through named values.
    Selector { has_prev: bool, has_next: bool },
    /// A boolean toggle.
    Toggle { on: bool },
    /// A stepped volume slider (0..=max).
    Volume { value: u8, max: u8 },
    /// A control binding: token labels, capture + conflict state.
    Binding {
        tokens: Vec<String>,
        capturing: bool,
        conflict: bool,
    },
    /// A plain activatable row (RESTORE DEFAULTS…).
    Action,
    /// A read-only diagnostic row (seed display).
    ReadOnly,
}

/// One settings row.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingRow {
    pub label: String,
    pub value: String,
    pub control: RowControl,
    /// Explains what the setting really affects (small print).
    pub detail: Option<String>,
}

impl SettingRow {
    pub fn selector(label: &str, value: &str, has_prev: bool, has_next: bool) -> Self {
        SettingRow {
            label: label.to_string(),
            value: value.to_string(),
            control: RowControl::Selector { has_prev, has_next },
            detail: None,
        }
    }

    pub fn toggle(label: &str, on: bool) -> Self {
        SettingRow {
            label: label.to_string(),
            value: if on { "ON" } else { "OFF" }.to_string(),
            control: RowControl::Toggle { on },
            detail: None,
        }
    }

    pub fn volume(label: &str, value: u8, max: u8) -> Self {
        SettingRow {
            label: label.to_string(),
            value: format!("{value}/{max}"),
            control: RowControl::Volume { value, max },
            detail: None,
        }
    }

    pub fn with_detail(mut self, detail: &str) -> Self {
        self.detail = Some(detail.to_string());
        self
    }
}

/// The settings category tab strip.
#[derive(Debug, Clone, PartialEq)]
pub struct CategoryTabs {
    pub labels: Vec<String>,
    pub active: usize,
}
