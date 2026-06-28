//! The neutral, ordered, layer-sorted 2D draw-list — the 2D peer of
//! `axiom-render`'s `RenderCommandList`.

use axiom_kernel::Meters;
use axiom_math::Vec2;

use crate::camera2d::Camera2d;
use crate::draw2d_command::Draw2dCommand;
use crate::handles::PaintId;
use crate::paint::{GradientStop, Paint2d, PaintTable};

/// A frame's 2D draw commands after the `(layer, submission)` sort, plus the
/// per-frame paint table and the resolved camera.
///
/// Primitives only — no GPU/DOM/font/scene types — so it is hashable and
/// byte-comparable for golden tests. Inspected by the app/runtime through
/// indexed accessors, the `KIND_*` codes on each [`Draw2dCommand`], and the
/// paint/camera accessors here.
#[derive(Debug, Clone, PartialEq)]
pub struct Draw2dList {
    commands: Vec<Draw2dCommand>,
    paints: PaintTable,
    camera: Option<Camera2d>,
}

impl Draw2dList {
    pub(crate) fn new(
        commands: Vec<Draw2dCommand>,
        paints: PaintTable,
        camera: Option<Camera2d>,
    ) -> Self {
        Draw2dList {
            commands,
            paints,
            camera,
        }
    }

    /// The number of draw commands.
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// Whether the list holds no commands.
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// The command at `idx`, or `None` if out of range.
    pub fn at(&self, idx: usize) -> Option<&Draw2dCommand> {
        self.commands.get(idx)
    }

    /// All commands, in final `(layer, submission)` order.
    pub fn commands(&self) -> &[Draw2dCommand] {
        &self.commands
    }

    /// The resolved camera, or `None` if the author set none this frame.
    pub fn camera(&self) -> Option<Camera2d> {
        self.camera
    }

    /// The number of registered paints.
    pub fn paint_count(&self) -> usize {
        self.paints.len()
    }

    /// The linear-gradient `(from, to)` for `paint`, or `None` if the id is
    /// unknown or the paint is radial.
    pub fn paint_linear(&self, paint: PaintId) -> Option<(Vec2, Vec2)> {
        self.paints.get(paint).and_then(Paint2d::as_linear)
    }

    /// The radial-gradient `(center, radius)` for `paint`, or `None` if the id
    /// is unknown or the paint is linear.
    pub fn paint_radial(&self, paint: PaintId) -> Option<(Vec2, Meters)> {
        self.paints.get(paint).and_then(Paint2d::as_radial)
    }

    /// The stops of `paint`, or `None` if the id is unknown.
    pub fn paint_stops(&self, paint: PaintId) -> Option<Vec<GradientStop>> {
        self.paints.get(paint).map(|p| p.stops().to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_list_is_empty() {
        let list = Draw2dList::new(Vec::new(), PaintTable::default(), None);
        assert!(list.is_empty());
        assert_eq!(list.len(), 0);
        assert_eq!(list.at(0), None);
        assert_eq!(list.commands(), &[]);
        assert_eq!(list.camera(), None);
        assert_eq!(list.paint_count(), 0);
    }

    #[test]
    fn unknown_paint_id_yields_none_on_every_accessor() {
        let list = Draw2dList::new(Vec::new(), PaintTable::default(), None);
        assert_eq!(list.paint_linear(PaintId::from_raw(0)), None);
        assert_eq!(list.paint_radial(PaintId::from_raw(0)), None);
        assert_eq!(list.paint_stops(PaintId::from_raw(0)), None);
    }
}
