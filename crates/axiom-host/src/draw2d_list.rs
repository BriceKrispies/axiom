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
///
/// A `Default` list is empty (no commands, no paints, no camera). The
/// `axiom-draw2d` builder accumulates a frame onto one of these through the
/// producer methods below (`set_camera`, `register_linear`/`register_radial`,
/// `push_command`) and finalizes it with [`Draw2dList::sort_commands`]; the
/// builder owns the authoring ergonomics (transform stack, submit counter), the
/// host owns the contract.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Draw2dList {
    commands: Vec<Draw2dCommand>,
    paints: PaintTable,
    camera: Option<Camera2d>,
}

impl Draw2dList {
    /// Set the resolved camera for this frame (producer side).
    pub fn set_camera(&mut self, camera: Camera2d) {
        self.camera = Some(camera);
    }

    /// Register a linear gradient, returning its [`PaintId`] (producer side).
    pub fn register_linear(&mut self, from: Vec2, to: Vec2, stops: Vec<GradientStop>) -> PaintId {
        self.paints.register(Paint2d::linear(from, to, stops))
    }

    /// Register a radial gradient, returning its [`PaintId`] (producer side).
    pub fn register_radial(
        &mut self,
        center: Vec2,
        radius: Meters,
        stops: Vec<GradientStop>,
    ) -> PaintId {
        self.paints.register(Paint2d::radial(center, radius, stops))
    }

    /// Append a built [`Draw2dCommand`] in submit order (producer side).
    pub fn push_command(&mut self, command: Draw2dCommand) {
        self.commands.push(command);
    }

    /// Stable-sort the accumulated commands by `(layer, submission)` so equal
    /// layers keep submit order — the one finalize step the builder triggers.
    pub fn sort_commands(&mut self) {
        self.commands
            .sort_by_key(|c| (c.layer(), c.submission_index()));
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
    use crate::common2d::Common2d;
    use crate::fill2d::Fill2d;
    use crate::rect::Rect;
    use crate::rgba::Rgba;
    use axiom_kernel::Ratio;
    use axiom_math::Mat3;

    fn ratio(v: f32) -> Ratio {
        Ratio::new(v).unwrap()
    }

    fn meters(v: f32) -> Meters {
        Meters::new(v).unwrap()
    }

    fn red() -> Rgba {
        Rgba::new(ratio(1.0), ratio(0.0), ratio(0.0), ratio(1.0))
    }

    fn rect_cmd(submission: u32, layer: i32) -> Draw2dCommand {
        Draw2dCommand::rect(
            (submission, Mat3::IDENTITY, Common2d::new(layer, ratio(1.0))),
            Rect::new(Vec2::ZERO, Vec2::ONE),
            Fill2d::color(red()),
        )
    }

    #[test]
    fn default_list_is_empty() {
        let list = Draw2dList::default();
        assert!(list.is_empty());
        assert_eq!(list.len(), 0);
        assert_eq!(list.at(0), None);
        assert_eq!(list.commands(), &[]);
        assert_eq!(list.camera(), None);
        assert_eq!(list.paint_count(), 0);
    }

    #[test]
    fn unknown_paint_id_yields_none_on_every_accessor() {
        let list = Draw2dList::default();
        assert_eq!(list.paint_linear(PaintId::from_raw(0)), None);
        assert_eq!(list.paint_radial(PaintId::from_raw(0)), None);
        assert_eq!(list.paint_stops(PaintId::from_raw(0)), None);
    }

    #[test]
    fn set_camera_resolves_onto_the_list() {
        let mut list = Draw2dList::default();
        let cam = Camera2d::new(Vec2::new(3.0, 4.0), ratio(2.0));
        list.set_camera(cam);
        assert_eq!(list.camera(), Some(cam));
    }

    #[test]
    fn register_linear_and_radial_round_trip_through_accessors() {
        let mut list = Draw2dList::default();
        let stops = vec![
            GradientStop::new(ratio(0.0), red()),
            GradientStop::new(ratio(1.0), red()),
        ];
        let lin = list.register_linear(Vec2::ZERO, Vec2::new(1.0, 0.0), stops.clone());
        let rad = list.register_radial(Vec2::ONE, meters(4.0), stops);
        assert_eq!(lin, PaintId::from_raw(0));
        assert_eq!(rad, PaintId::from_raw(1));
        assert_eq!(list.paint_count(), 2);
        assert_eq!(list.paint_linear(lin), Some((Vec2::ZERO, Vec2::new(1.0, 0.0))));
        assert_eq!(list.paint_radial(rad), Some((Vec2::ONE, meters(4.0))));
        assert_eq!(list.paint_stops(lin).map(|s| s.len()), Some(2));
    }

    #[test]
    fn push_then_sort_orders_by_layer_then_submission() {
        let mut list = Draw2dList::default();
        // Submit out of layer order, with a tie on layer 0.
        list.push_command(rect_cmd(0, 2));
        list.push_command(rect_cmd(1, 0));
        list.push_command(rect_cmd(2, 0));
        list.sort_commands();
        let ordered: Vec<(i32, u32)> = list
            .commands()
            .iter()
            .map(|c| (c.layer(), c.submission_index()))
            .collect();
        assert_eq!(ordered, vec![(0, 1), (0, 2), (2, 0)]);
    }
}
