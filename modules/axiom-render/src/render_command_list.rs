//! A deterministic, ordered list of [`crate::RenderCommand`]s.

use crate::render_command::RenderCommand;

/// A deterministic, ordered list of render commands.
///
/// Constructed by [`crate::RenderApi::build_command_list`]; inspected
/// by the app through `RenderApi`'s indexed accessors. The list never
/// reorders the commands `build_command_list` emitted.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderCommandList {
    commands: Vec<RenderCommand>,
}

impl RenderCommandList {
    pub const fn new() -> Self {
        RenderCommandList {
            commands: Vec::new(),
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        RenderCommandList {
            commands: Vec::with_capacity(capacity),
        }
    }

    /// Empty the list, reusing its allocated capacity — the per-frame reuse
    /// entry point for a retained command list (`clear` then refill instead of
    /// allocating a fresh one each frame).
    pub fn clear(&mut self) {
        self.commands.clear();
    }

    pub(crate) fn push(&mut self, command: RenderCommand) {
        self.commands.push(command);
    }

    pub fn len(&self) -> usize {
        self.commands.len()
    }

    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    pub fn commands(&self) -> &[RenderCommand] {
        &self.commands
    }

    pub fn at(&self, idx: usize) -> Option<&RenderCommand> {
        self.commands.get(idx)
    }
}

impl Default for RenderCommandList {
    fn default() -> Self {
        RenderCommandList::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_math::Mat4;

    #[test]
    fn new_list_is_empty() {
        let l = RenderCommandList::new();
        assert!(l.is_empty());
        assert_eq!(l.len(), 0);
    }

    #[test]
    fn populated_list_is_not_empty() {
        let mut l = RenderCommandList::new();
        l.push(RenderCommand::clear_frame([0.0, 0.0, 0.0, 1.0]));
        // Kills `is_empty -> true`: a list holding a command is not empty.
        assert!(!l.is_empty());
        assert_eq!(l.len(), 1);
    }

    #[test]
    fn with_capacity_is_empty_but_constructed() {
        // Kills `with_capacity -> Default::default()` only insofar as the
        // returned list must be a usable, empty list that accepts pushes.
        let mut l = RenderCommandList::with_capacity(4);
        assert!(l.is_empty());
        assert_eq!(l.len(), 0);
        l.push(RenderCommand::clear_frame([1.0, 1.0, 1.0, 1.0]));
        assert_eq!(l.len(), 1);
    }

    #[test]
    fn push_and_at_round_trip() {
        let mut l = RenderCommandList::new();
        l.push(RenderCommand::clear_frame([0.0, 0.0, 0.0, 1.0]));
        l.push(RenderCommand::draw_indexed(7, 0, 36, Mat4::IDENTITY));
        assert_eq!(l.len(), 2);
        assert_eq!(
            l.at(0).unwrap().kind_code(),
            RenderCommand::KIND_CLEAR_FRAME
        );
    }

    #[test]
    fn default_matches_new() {
        let a = RenderCommandList::default();
        let b = RenderCommandList::new();
        assert_eq!(a, b);
    }
}
