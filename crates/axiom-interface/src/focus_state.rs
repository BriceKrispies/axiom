//! [`FocusState`] — which panel currently owns input focus.
//!
//! Focus is a single-owner concept across the whole interface: focusing one panel
//! transfers focus away from any other. Branchless.

use crate::panel_id::PanelId;

/// The panel that currently owns input focus, if any.
#[derive(Debug, Default, Clone)]
pub(crate) struct FocusState {
    owner: Option<PanelId>,
}

impl FocusState {
    pub(crate) fn new() -> Self {
        FocusState::default()
    }

    /// Give `panel` focus, transferring it away from any previous owner.
    pub(crate) fn focus(&mut self, panel: PanelId) {
        self.owner = Some(panel);
    }

    /// Release focus, but only if `panel` is the current owner (branchless filter).
    pub(crate) fn blur(&mut self, panel: PanelId) {
        self.owner = self.owner.filter(|&owner| owner != panel);
    }

    pub(crate) fn is_focused(&self, panel: PanelId) -> bool {
        self.owner == Some(panel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::HandleId;

    fn panel(raw: u64) -> PanelId {
        PanelId::from_handle(HandleId::from_raw(raw))
    }

    #[test]
    fn focus_transfers_between_panels() {
        let (a, b) = (panel(1), panel(2));
        let mut focus = FocusState::new();
        assert!(!focus.is_focused(a));
        focus.focus(a);
        assert!(focus.is_focused(a));
        focus.focus(b);
        assert!(focus.is_focused(b) && !focus.is_focused(a));
    }

    #[test]
    fn blur_only_releases_the_current_owner() {
        let (a, b) = (panel(1), panel(2));
        let mut focus = FocusState::new();
        focus.focus(a);
        focus.blur(b); // not the owner → no-op
        assert!(focus.is_focused(a));
        focus.blur(a);
        assert!(!focus.is_focused(a));
        focus.blur(a); // already released → no-op
        assert!(!focus.is_focused(a));
    }
}
