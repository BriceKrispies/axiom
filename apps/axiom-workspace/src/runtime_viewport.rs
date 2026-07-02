//! [`RuntimeViewportState`] — the state of the Runtime Viewport panel.
//!
//! This panel is an explicit **placeholder**. Real runtime embedding — attaching
//! a live session's presentation surface into the viewport — is a future
//! integration point that does not exist yet: the workspace app touches no GPU,
//! no windowing, and no browser surface.
//!
//! The viewport is toggleable between two placeholder views
//! ([`RuntimeViewportView`]): the plain viewport placeholder, and a
//! **backend-comparison** placeholder that names the same three render backends
//! the demo gallery compares side by side (WebGPU · WebGL2 · Canvas2D). The live
//! comparison — one deterministic demo rendered through all three backends at
//! once — runs in the demo gallery (`apps/axiom-gallery`), which owns the real
//! render surfaces. The workspace only names the comparison as data; it embeds no
//! live surface and no nested browsing context here.

/// Which placeholder view the runtime viewport shows. Pure shell state — a toggle
/// between two placeholders, never a live renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RuntimeViewportView {
    /// The plain "Runtime Viewport Placeholder" surface.
    #[default]
    Placeholder,
    /// The backend-comparison placeholder (WebGPU · WebGL2 · Canvas2D), mirroring
    /// the gallery's live three-backend comparison.
    BackendTriptych,
}

/// One backend in the runtime-viewport comparison, as canonical data. It mirrors a
/// backend the gallery compares; it carries no render surface of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComparisonBackend {
    id: &'static str,
    name: &'static str,
    note: &'static str,
}

impl ComparisonBackend {
    /// The backend's URL/pin id (e.g. `webgpu`) — the same token the gallery pins
    /// a pane to.
    #[must_use]
    pub fn id(&self) -> &'static str {
        self.id
    }

    /// The backend's display name (e.g. `WebGPU`).
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// A short note on the backend's role (e.g. `GPU · primary`).
    #[must_use]
    pub fn note(&self) -> &'static str {
        self.note
    }
}

/// The three comparison backends, in the order the engine's own backend cascade
/// prefers them. Canonical, fixed data — the same trio the gallery compares.
const COMPARISON_BACKENDS: [ComparisonBackend; 3] = [
    ComparisonBackend {
        id: "webgpu",
        name: "WebGPU",
        note: "GPU · primary",
    },
    ComparisonBackend {
        id: "webgl2",
        name: "WebGL2",
        note: "GPU · fallback",
    },
    ComparisonBackend {
        id: "canvas2d",
        name: "Canvas2D",
        note: "software rasterizer",
    },
];

/// The Runtime Viewport panel state. A placeholder: no runtime is attached and no
/// presentation surface is embedded yet. It tracks only which placeholder view is
/// selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RuntimeViewportState {
    attached: bool,
    view: RuntimeViewportView,
}

impl RuntimeViewportState {
    /// Whether a live runtime is attached to the viewport. Always `false` today —
    /// real runtime embedding is a future integration point.
    #[must_use]
    pub fn attached(&self) -> bool {
        self.attached
    }

    /// The placeholder label the shell shows while no runtime is embedded.
    #[must_use]
    pub fn placeholder_label(&self) -> &'static str {
        "Runtime Viewport Placeholder"
    }

    /// Which placeholder view is selected (plain viewport vs backend comparison).
    #[must_use]
    pub fn view(&self) -> RuntimeViewportView {
        self.view
    }

    /// Select a placeholder view. Pure data — toggles the tab, embeds nothing.
    #[must_use]
    pub fn with_view(mut self, view: RuntimeViewportView) -> Self {
        self.view = view;
        self
    }

    /// The backends the comparison view names, in the engine's preference order.
    /// Fixed, canonical data — the same three the gallery compares.
    #[must_use]
    pub fn comparison_backends(&self) -> &'static [ComparisonBackend] {
        &COMPARISON_BACKENDS
    }
}
