//! Scene authoring expressed as a **startup preparation task**.
//!
//! This is the structural half of the fix for the ordering defect
//! [`RunningApp::realize`] used to carry: it called `Runtime::start()` before it
//! authored the scene, so the runtime reported `Running` for an application
//! whose meshes did not yet exist. Moving the `start()` call further down would
//! have corrected that one line and left the next agent free to move it back.
//!
//! Instead, authoring **is** preparation. [`AuthorTask`] is the first task
//! pushed onto the [`axiom_runtime::PreparationSchedule`] that `realize` hands
//! to `Runtime::prepare`, and `Runtime::start` accepts only `Prepared`. The
//! ordering is therefore no longer a convention a reader has to notice — a
//! `realize` that started before authoring simply could not reach `Running`.
//!
//! The task writes its product into an `Rc<RefCell<Option<_>>>` its constructor
//! captured, because [`axiom_runtime::PreparationTask::prepare`] takes no
//! arguments and returns no data: the runtime owns the *fact* that preparation
//! completed, the caller owns the *data*.

use std::cell::RefCell;
use std::rc::Rc;

use axiom_runtime::{PreparationTask, RuntimeResult};

use super::{AuthoredScene, RunningApp, SetupFn};

/// The slot an [`AuthorTask`] deposits its realized scene into, shared with the
/// [`RunningApp::realize`] that will build itself from it.
///
/// `Option` rather than a defaultable value is deliberate: a caller that reads
/// the cell before the phase ran finds `None` — an unmistakable absence — rather
/// than a plausible-looking empty scene that would render as a blank world.
pub(super) type AuthoredCell = Rc<RefCell<Option<AuthoredScene>>>;

/// The engine's own preparation task: run the app's setup closure and realize it
/// into a scene, resolved geometry, material colours, and the renderable count.
///
/// Held by the schedule as a `Box<dyn PreparationTask>`, so this type never
/// escapes the umbrella.
pub(super) struct AuthorTask {
    setup: Option<SetupFn>,
    aspect: f32,
    out: AuthoredCell,
}

impl AuthorTask {
    /// A task that will author `setup` at `aspect` and deposit the result in `out`.
    pub(super) fn new(setup: Option<SetupFn>, aspect: f32, out: AuthoredCell) -> Self {
        AuthorTask { setup, aspect, out }
    }
}

impl PreparationTask for AuthorTask {
    /// Author the scene. Infallible today — [`RunningApp::author`] resolves an
    /// absent camera and an absent light to engine defaults rather than
    /// erroring — so this always reports success and the phase advances.
    fn prepare(&mut self) -> RuntimeResult<()> {
        let authored = RunningApp::author(self.setup.take(), self.aspect);
        self.out.borrow_mut().replace(authored);
        Ok(())
    }
}
