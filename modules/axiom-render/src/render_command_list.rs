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
    pub fn new() -> Self {
        RenderCommandList {
            commands: Vec::new(),
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        RenderCommandList {
            commands: Vec::with_capacity(capacity),
        }
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
    fn push_and_at_round_trip() {
        let mut l = RenderCommandList::new();
        l.push(RenderCommand::ClearFrame {
            color: [0.0, 0.0, 0.0, 1.0],
        });
        l.push(RenderCommand::DrawIndexed {
            index_count: 36,
            world: Mat4::IDENTITY,
        });
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
