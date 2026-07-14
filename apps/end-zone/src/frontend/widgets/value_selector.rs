//! The standalone value selector: a big left/right stepper plate used on the
//! match setup screen (difficulty, game speed).

/// One value selector.
#[derive(Debug, Clone, PartialEq)]
pub struct ValueSelector {
    pub label: String,
    pub value: String,
    pub has_prev: bool,
    pub has_next: bool,
}

impl ValueSelector {
    pub fn new(label: &str, value: &str, has_prev: bool, has_next: bool) -> Self {
        ValueSelector {
            label: label.to_string(),
            value: value.to_string(),
            has_prev,
            has_next,
        }
    }
}
