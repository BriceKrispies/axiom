//! The trajectory editor: gestures in, [`EditorCommand`]s and an overlay out.
//!
//! This is the only place in the game that knows a pixel exists *and* knows what
//! a shot is, and it is deliberately the seam: it may write a command, and that
//! is the entire vocabulary it has for reaching the shot. It cannot touch the
//! trajectory, the ball, the keeper or the phase machine.
//!
//! ```text
//! Pointer (neutral, from axiom-input)
//!   → DragTracker      what the gesture is doing        drag.rs
//!   → Zone             what it took hold of             here
//!   → Grab             where on the curve, and from what sculpt.rs
//!   → EditorCommand    the only thing it may say        play::session
//!   → EditorView       what the screen should show      view.rs
//! ```

pub mod drag;
pub mod layout;
pub mod sculpt;
pub mod view;

use axiom::prelude::{Vec2, Vec3};
use axiom_input::Pointer;

use crate::play::{EditorCommand, Phase, Session};
use crate::projection::ScreenProjection;
use crate::shot::GoalTarget;
use crate::tuning::Tuning;

pub use drag::{DragEvent, DragTracker};
pub use layout::{Layout, Rect};
pub use sculpt::{Grab, SculptPanel};
pub use view::{ButtonView, EditorView, PanelView};

/// What the live gesture took hold of when it went down.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Zone {
    Nothing,
    /// Aiming inside (or near) the goal.
    Aim,
    /// Sculpting, holding the curve at this grab.
    Panel(Grab),
    /// Pressing the primary action.
    Action,
    /// Pressing the step-back chip.
    Back,
}

/// How many segments the drawn curve is built from. Enough to read as smooth at
/// phone sizes, few enough that the overlay is a handful of points.
const CURVE_SEGMENTS: usize = 28;

/// The editor.
#[derive(Debug, Clone)]
pub struct Editor {
    drag: DragTracker,
    zone: Zone,
    last_phase: Phase,
}

impl Editor {
    pub fn new() -> Editor {
        Editor {
            drag: DragTracker::new(),
            zone: Zone::Nothing,
            last_phase: Phase::Ready,
        }
    }

    /// Read one tick of pointer state against the current session, producing the
    /// commands it implies.
    pub fn update(
        &mut self,
        pointer: Option<Pointer>,
        session: &Session,
        projection: &ScreenProjection,
        tuning: &Tuning,
    ) -> Vec<EditorCommand> {
        let layout = Layout::resolve(projection.viewport(), &tuning.editor);
        // A phase change under the player's finger abandons the gesture rather
        // than letting it fire into the next stage.
        (session.phase() != self.last_phase).then(|| {
            self.drag.cancel();
            self.zone = Zone::Nothing;
        });
        self.last_phase = session.phase();

        let dead_zone = layout.scaled(tuning.editor.dead_zone);
        let event = self.drag.update(pointer, dead_zone);
        let panel = session
            .phase()
            .sculpting()
            .map(|p| sculpt_panel(&layout, p, tuning));

        match event {
            DragEvent::Idle => Vec::new(),
            DragEvent::Begin { at } => self.begin(at, session, projection, &layout, panel.as_ref()),
            DragEvent::Move { at, .. } => self.moved(at, session, projection, panel.as_ref()),
            DragEvent::End { at, moved, .. } => self.ended(at, &layout, moved, session),
        }
    }

    fn begin(
        &mut self,
        at: Vec2,
        session: &Session,
        projection: &ScreenProjection,
        layout: &Layout,
        panel: Option<&SculptPanel>,
    ) -> Vec<EditorCommand> {
        let editing = session.phase().editing();
        let has_back = session.phase().backed().is_some();
        let action_rect = [layout.wide_action(), layout.action][usize::from(has_back)];
        self.zone = match () {
            _ if !editing => Zone::Nothing,
            _ if action_rect.contains(at) => Zone::Action,
            _ if has_back & layout.back.contains(at) => Zone::Back,
            _ => match panel.filter(|p| p.rect.contains(at)) {
                Some(p) => Zone::Panel(Grab::begin(p, active_curve(session), at)),
                // Everything else on screen is the aim pad. A touch that misses
                // the goal by a mile still aims, because the alternative — a tap
                // that does nothing — is the single most common way a phone
                // control feels broken.
                None => Zone::Aim,
            },
        };
        // Aiming is live from the moment of contact, so the reticle is already
        // under the finger before it starts to drag.
        match self.zone {
            Zone::Aim => aim_command(at, projection, session).into_iter().collect(),
            _ => Vec::new(),
        }
    }

    fn moved(
        &mut self,
        at: Vec2,
        session: &Session,
        projection: &ScreenProjection,
        panel: Option<&SculptPanel>,
    ) -> Vec<EditorCommand> {
        match (self.zone, panel) {
            (Zone::Aim, _) => aim_command(at, projection, session).into_iter().collect(),
            (Zone::Panel(grab), Some(panel)) => {
                let tuning = session.tuning();
                let axis = match panel.projection {
                    crate::play::Projection::Horizontal => &tuning.bend,
                    crate::play::Projection::Vertical => &tuning.loft,
                };
                let curve = grab.curve(panel, at, axis);
                vec![match panel.projection {
                    crate::play::Projection::Horizontal => EditorCommand::SetBend(curve),
                    crate::play::Projection::Vertical => EditorCommand::SetLoft(curve),
                }]
            }
            _ => Vec::new(),
        }
    }

    fn ended(
        &mut self,
        at: Vec2,
        layout: &Layout,
        moved: bool,
        session: &Session,
    ) -> Vec<EditorCommand> {
        let has_back = session.phase().backed().is_some();
        let action_rect = [layout.wide_action(), layout.action][usize::from(has_back)];
        let zone = core::mem::replace(&mut self.zone, Zone::Nothing);
        // A button fires on release, and only if the release is still on it: a
        // press that slid off is a change of mind, not a command.
        match zone {
            Zone::Action if !moved && action_rect.contains(at) => vec![EditorCommand::Advance],
            Zone::Back if !moved && layout.back.contains(at) => vec![EditorCommand::Back],
            _ => Vec::new(),
        }
    }

    /// Whether the primary action is being held down right now (for the press
    /// highlight).
    fn pressing(&self, zone: Zone) -> bool {
        (self.zone == zone) & self.drag.active()
    }

    /// Resolve what the overlay should draw this tick.
    pub fn view(
        &self,
        session: &Session,
        projection: &ScreenProjection,
        tuning: &Tuning,
    ) -> EditorView {
        let layout = Layout::resolve(projection.viewport(), &tuning.editor);
        let phase = session.phase();
        let tally = (session.tally().goals, session.tally().attempts);
        let mut out = EditorView::quiet(phase, layout.viewport, layout.short, tally);
        out.banner = session.result().map(|r| r.banner());
        out.prompt = phase.label();

        // The goal, and the point inside it the shot finishes on.
        let quad = session.mouth().frame_corners();
        out.goal_quad = quad
            .iter()
            .map(|c| projection.project(*c))
            .collect::<Option<Vec<Vec2>>>()
            .and_then(|v| <[Vec2; 4]>::try_from(v).ok())
            .filter(|_| phase.shows_preview());
        out.target = projection
            .project(session.shot().world_target)
            .filter(|_| phase.shows_preview());
        out.aim_emphasis = [0.35f32, 1.0][usize::from(phase == Phase::TargetSelection)]
            * f32::from(u8::from(phase.shows_preview()));

        // The panel.
        out.panel = phase.sculpting().map(|p| {
            let panel = sculpt_panel(&layout, p, tuning);
            let curve = active_curve(session);
            let grabbing = matches!(self.zone, Zone::Panel(_)) & self.drag.active();
            let (hint_low, hint_high) = view::hints(p);
            PanelView {
                rect: panel.rect,
                projection: p,
                label: phase.label(),
                ball_end: panel.plot(0.0, 0.0),
                goal_end: panel.plot(1.0, 0.0),
                baseline: panel.baseline(),
                curve: panel.polyline(curve, CURVE_SEGMENTS),
                handle: match self.zone {
                    Zone::Panel(grab) if self.drag.active() => Some(grab.handle(
                        &panel,
                        curve,
                        layout.scaled(tuning.editor.handle_lift),
                    )),
                    _ => None,
                },
                active: grabbing,
                hint_low,
                hint_high,
            }
        });

        // The action row.
        let has_back = phase.backed().is_some();
        out.action = phase.editing().then(|| ButtonView {
            rect: [layout.wide_action(), layout.action][usize::from(has_back)],
            label: phase.action_label(),
            pressed: self.pressing(Zone::Action),
        });
        out.back = (phase.editing() & has_back).then(|| ButtonView {
            rect: layout.back,
            label: "BACK",
            pressed: self.pressing(Zone::Back),
        });
        out
    }
}

impl Default for Editor {
    fn default() -> Self {
        Editor::new()
    }
}

/// The panel for a projection, at this layout.
fn sculpt_panel(
    layout: &Layout,
    projection: crate::play::Projection,
    tuning: &Tuning,
) -> SculptPanel {
    let axis = match projection {
        crate::play::Projection::Horizontal => &tuning.bend,
        crate::play::Projection::Vertical => &tuning.loft,
    };
    SculptPanel::new(layout.panel, projection, axis)
}

/// The curve the current stage edits.
fn active_curve(session: &Session) -> &crate::shot::BendCurve {
    match session.phase() {
        Phase::VerticalSculpt => &session.intent().loft,
        _ => &session.intent().bend,
    }
}

/// Turn a screen point into an aim, if the phase takes one.
fn aim_command(
    at: Vec2,
    projection: &ScreenProjection,
    session: &Session,
) -> Option<EditorCommand> {
    session
        .phase()
        .accepts_aim()
        .then(|| projection.goal_plane_hit(at))
        .flatten()
        .map(|hit: Vec3| {
            let (h, v) = session.mouth().to_normalized(hit);
            EditorCommand::Aim(GoalTarget::new(h, v))
        })
}
