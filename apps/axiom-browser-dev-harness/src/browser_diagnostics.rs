//! The diagnostics the overlay reads, and the replaceable provider that supplies
//! them.
//!
//! [`BrowserDiagnosticsSnapshot`] is a plain value type: the host fills it in and
//! hands it to the overlay, which only ever reads it. It is **not** engine state
//! — it is a read-out of the browser surface (frame/fps counters, which backend
//! is live, fallback status). Today the values come from
//! [`StubDiagnosticsProvider`]; a real host implements [`DiagnosticsProvider`]
//! and feeds richer numbers in through the same shape, with no overlay changes.

/// One labelled key/value line in the overlay body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticRow {
    pub label: String,
    pub value: String,
}

impl DiagnosticRow {
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        DiagnosticRow {
            label: label.into(),
            value: value.into(),
        }
    }
}

/// A read-out of the live browser/WASM engine surface for one frame.
///
/// Every field is host-supplied. The overlay never writes these; it formats and
/// displays them. Replace the values' source (the provider) without touching the
/// overlay.
#[derive(Debug, Clone, PartialEq)]
pub struct BrowserDiagnosticsSnapshot {
    pub frame_index: u64,
    pub tick: u64,
    pub fps: f64,
    pub frame_time_ms: f64,
    pub sim_ticks_this_frame: u32,
    pub renderer_backend: String,
    pub canvas_owner: String,
    pub simulation_owner: String,
    pub storage_backend: String,
    pub audio_backend: String,
    pub network_backend: String,
    pub webgpu_submissions: u64,
    pub canvas2d_frames: u64,
    pub worker_messages_in: u64,
    pub worker_messages_out: u64,
    pub fallback_count: u32,
    pub fallback_reason: String,
    pub visibility_state: String,
}

impl BrowserDiagnosticsSnapshot {
    /// The canonical stub read-out: deterministic, honest about which engine
    /// pieces own which surface (per the engine's own structure), and zeroed for
    /// the live counters. The live frame/fps numbers are layered on with
    /// [`Self::with_frame`].
    pub fn stub() -> Self {
        BrowserDiagnosticsSnapshot {
            frame_index: 0,
            tick: 0,
            fps: 0.0,
            frame_time_ms: 0.0,
            sim_ticks_this_frame: 1,
            renderer_backend: "webgpu".to_string(),
            canvas_owner: "axiom-windowing".to_string(),
            simulation_owner: "axiom-runtime".to_string(),
            storage_backend: "memory".to_string(),
            audio_backend: "none".to_string(),
            network_backend: "none".to_string(),
            webgpu_submissions: 0,
            canvas2d_frames: 0,
            worker_messages_in: 0,
            worker_messages_out: 0,
            fallback_count: 0,
            fallback_reason: "none".to_string(),
            visibility_state: "visible".to_string(),
        }
    }

    /// Overlay the per-frame live counters onto a snapshot (the harness's RAF
    /// loop calls this each frame so the overlay looks alive once toggled on).
    pub fn with_frame(mut self, frame_index: u64, tick: u64, fps: f64, frame_time_ms: f64) -> Self {
        self.frame_index = frame_index;
        self.tick = tick;
        self.fps = fps;
        self.frame_time_ms = frame_time_ms;
        self
    }

    /// `fps` formatted for display (one decimal).
    pub fn fps_text(&self) -> String {
        format!("{:.1}", self.fps)
    }

    /// `frame_time_ms` formatted for display (two decimals).
    pub fn frame_ms_text(&self) -> String {
        format!("{:.2}", self.frame_time_ms)
    }

    /// Worker message counters as `in / out`.
    pub fn worker_text(&self) -> String {
        format!("{} / {}", self.worker_messages_in, self.worker_messages_out)
    }

    /// The core diagnostics rows shown at `normal` density — the host-supplied
    /// fields, in display order. (The command-history count is appended by the
    /// overlay state, since it is console-derived, not host-supplied.)
    pub fn core_rows(&self) -> Vec<DiagnosticRow> {
        vec![
            DiagnosticRow::new("frame", self.frame_index.to_string()),
            DiagnosticRow::new("tick", self.tick.to_string()),
            DiagnosticRow::new("fps", self.fps_text()),
            DiagnosticRow::new("frame ms", self.frame_ms_text()),
            DiagnosticRow::new("sim ticks", self.sim_ticks_this_frame.to_string()),
            DiagnosticRow::new("renderer", self.renderer_backend.clone()),
            DiagnosticRow::new("canvas owner", self.canvas_owner.clone()),
            DiagnosticRow::new("sim owner", self.simulation_owner.clone()),
            DiagnosticRow::new("storage", self.storage_backend.clone()),
            DiagnosticRow::new("audio", self.audio_backend.clone()),
            DiagnosticRow::new("network", self.network_backend.clone()),
            DiagnosticRow::new("webgpu subs", self.webgpu_submissions.to_string()),
            DiagnosticRow::new("canvas2d frames", self.canvas2d_frames.to_string()),
            DiagnosticRow::new("worker msgs", self.worker_text()),
            DiagnosticRow::new("fallbacks", self.fallback_count.to_string()),
            DiagnosticRow::new("fallback reason", self.fallback_reason.clone()),
            DiagnosticRow::new("visibility", self.visibility_state.clone()),
        ]
    }

    /// The four rows shown at `compact` density.
    pub fn compact_rows(&self) -> Vec<DiagnosticRow> {
        vec![
            DiagnosticRow::new("fps", self.fps_text()),
            DiagnosticRow::new("frame ms", self.frame_ms_text()),
            DiagnosticRow::new("renderer", self.renderer_backend.clone()),
            DiagnosticRow::new("fallbacks", self.fallback_count.to_string()),
        ]
    }

    /// A one-line raw backend-selection summary (shown at `verbose` density and
    /// echoed by the `backend.report` command).
    pub fn backend_select_text(&self) -> String {
        format!(
            "{} · {} · {} · {} · {} · {}",
            self.renderer_backend,
            self.canvas_owner,
            self.simulation_owner,
            self.storage_backend,
            self.audio_backend,
            self.network_backend,
        )
    }

    /// A multi-line text snapshot of every field (echoed by the
    /// `diagnostics.snapshot` command). Deterministic.
    pub fn snapshot_text(&self) -> String {
        let mut lines = vec![
            format!("frame={} tick={}", self.frame_index, self.tick),
            format!("fps={} frame_ms={}", self.fps_text(), self.frame_ms_text()),
            format!("sim_ticks={}", self.sim_ticks_this_frame),
            format!("renderer={}", self.renderer_backend),
            format!("canvas_owner={}", self.canvas_owner),
            format!("sim_owner={}", self.simulation_owner),
            format!("storage={}", self.storage_backend),
            format!("audio={}", self.audio_backend),
            format!("network={}", self.network_backend),
            format!("webgpu_submissions={}", self.webgpu_submissions),
            format!("canvas2d_frames={}", self.canvas2d_frames),
            format!("worker_msgs={}", self.worker_text()),
            format!("fallbacks={} ({})", self.fallback_count, self.fallback_reason),
            format!("visibility={}", self.visibility_state),
        ];
        lines.insert(0, "diagnostics snapshot (stub):".to_string());
        lines.join("\n")
    }

    /// A labelled per-backend report (echoed by the `backend.report` command).
    /// Deterministic.
    pub fn backend_report_text(&self) -> String {
        [
            "backend report (stub):".to_string(),
            format!("renderer: {}", self.renderer_backend),
            format!("canvas:   {}", self.canvas_owner),
            format!("sim:      {}", self.simulation_owner),
            format!("storage:  {}", self.storage_backend),
            format!("audio:    {}", self.audio_backend),
            format!("network:  {}", self.network_backend),
        ]
        .join("\n")
    }
}

/// The replaceable source of [`BrowserDiagnosticsSnapshot`]s. The overlay
/// consumes snapshots; *where they come from* is this trait's job, so a real host
/// can swap its own provider in without the overlay knowing.
pub trait DiagnosticsProvider {
    fn snapshot(&self) -> BrowserDiagnosticsSnapshot;
}

/// The default provider: hands back the canonical stub read-out. Replace with a
/// host-backed provider later.
#[derive(Debug, Default, Clone, Copy)]
pub struct StubDiagnosticsProvider;

impl DiagnosticsProvider for StubDiagnosticsProvider {
    fn snapshot(&self) -> BrowserDiagnosticsSnapshot {
        BrowserDiagnosticsSnapshot::stub()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_is_deterministic() {
        // The whole point of a stub: two reads are byte-identical.
        assert_eq!(
            BrowserDiagnosticsSnapshot::stub(),
            BrowserDiagnosticsSnapshot::stub()
        );
    }

    #[test]
    fn stub_names_the_engine_surface_owners() {
        let s = BrowserDiagnosticsSnapshot::stub();
        assert_eq!(s.canvas_owner, "axiom-windowing");
        assert_eq!(s.simulation_owner, "axiom-runtime");
        assert_eq!(s.renderer_backend, "webgpu");
        assert_eq!(s.fallback_count, 0);
        assert_eq!(s.fallback_reason, "none");
    }

    #[test]
    fn with_frame_overlays_only_the_live_counters() {
        let s = BrowserDiagnosticsSnapshot::stub().with_frame(42, 41, 59.94, 16.68);
        assert_eq!(s.frame_index, 42);
        assert_eq!(s.tick, 41);
        assert_eq!(s.fps_text(), "59.9");
        assert_eq!(s.frame_ms_text(), "16.68");
        // Non-frame fields are untouched.
        assert_eq!(s.canvas_owner, "axiom-windowing");
    }

    #[test]
    fn provider_returns_the_stub_snapshot() {
        let provider = StubDiagnosticsProvider;
        assert_eq!(provider.snapshot(), BrowserDiagnosticsSnapshot::stub());
    }

    #[test]
    fn core_rows_cover_every_host_field_in_order() {
        let rows = BrowserDiagnosticsSnapshot::stub().core_rows();
        let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(
            labels,
            vec![
                "frame",
                "tick",
                "fps",
                "frame ms",
                "sim ticks",
                "renderer",
                "canvas owner",
                "sim owner",
                "storage",
                "audio",
                "network",
                "webgpu subs",
                "canvas2d frames",
                "worker msgs",
                "fallbacks",
                "fallback reason",
                "visibility",
            ]
        );
    }

    #[test]
    fn compact_rows_are_the_four_at_a_glance_fields() {
        let rows = BrowserDiagnosticsSnapshot::stub().compact_rows();
        let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(labels, vec!["fps", "frame ms", "renderer", "fallbacks"]);
    }

    #[test]
    fn snapshot_and_report_text_are_deterministic_and_nonempty() {
        let s = BrowserDiagnosticsSnapshot::stub();
        assert_eq!(s.snapshot_text(), BrowserDiagnosticsSnapshot::stub().snapshot_text());
        assert!(s.snapshot_text().contains("renderer=webgpu"));
        assert!(s.backend_report_text().contains("canvas:   axiom-windowing"));
        assert!(s.backend_select_text().contains("webgpu"));
    }
}
