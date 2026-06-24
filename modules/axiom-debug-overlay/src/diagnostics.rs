//! The debug diagnostics the overlay reads, and its row/text formatters.
//!
//! This is the **debug-specific** data the overlay displays (frame/fps counters,
//! which backend is live, fallback status). It produces neutral `(label, value)`
//! rows + text that the overlay feeds into its `axiom-interface` panel; the panel
//! and the draw list are the layer's job, not this file's. All branchless,
//! integer/string (no naked floats cross any boundary).

/// A read-out of the live browser/WASM engine surface for one frame. Host-filled;
/// the overlay only reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Diagnostics {
    pub frame_index: u64,
    pub tick: u64,
    pub sim_ticks_this_frame: u32,
    pub fps_milli: u32,
    pub frame_time_micros: u32,
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

impl Diagnostics {
    /// The honest "no data yet" read-out: dashes and zeroes, not invented values.
    pub(crate) fn placeholder() -> Self {
        let dash = || "—".to_string();
        Diagnostics {
            frame_index: 0,
            tick: 0,
            sim_ticks_this_frame: 0,
            fps_milli: 0,
            frame_time_micros: 0,
            renderer_backend: dash(),
            canvas_owner: dash(),
            simulation_owner: dash(),
            storage_backend: dash(),
            audio_backend: dash(),
            network_backend: dash(),
            webgpu_submissions: 0,
            canvas2d_frames: 0,
            worker_messages_in: 0,
            worker_messages_out: 0,
            fallback_count: 0,
            fallback_reason: dash(),
            visibility_state: dash(),
        }
    }

    /// `fps` formatted for display (one decimal), from the integer `fps_milli`.
    pub(crate) fn fps_text(&self) -> String {
        format!("{}.{}", self.fps_milli / 1000, (self.fps_milli % 1000) / 100)
    }

    /// `frame_time` formatted for display (two decimals), from microseconds.
    pub(crate) fn frame_ms_text(&self) -> String {
        format!(
            "{}.{:02}",
            self.frame_time_micros / 1000,
            (self.frame_time_micros % 1000) / 10
        )
    }

    /// Worker message counters as `in / out`.
    pub(crate) fn worker_text(&self) -> String {
        format!("{} / {}", self.worker_messages_in, self.worker_messages_out)
    }

    /// The core diagnostics rows shown at `normal` density — the host fields, in
    /// display order, as neutral `(label, value)` pairs.
    pub(crate) fn core_rows(&self) -> Vec<(String, String)> {
        vec![
            row("frame", self.frame_index.to_string()),
            row("tick", self.tick.to_string()),
            row("fps", self.fps_text()),
            row("frame ms", self.frame_ms_text()),
            row("sim ticks", self.sim_ticks_this_frame.to_string()),
            row("renderer", self.renderer_backend.clone()),
            row("canvas owner", self.canvas_owner.clone()),
            row("sim owner", self.simulation_owner.clone()),
            row("storage", self.storage_backend.clone()),
            row("audio", self.audio_backend.clone()),
            row("network", self.network_backend.clone()),
            row("webgpu subs", self.webgpu_submissions.to_string()),
            row("canvas2d frames", self.canvas2d_frames.to_string()),
            row("worker msgs", self.worker_text()),
            row("fallbacks", self.fallback_count.to_string()),
            row("fallback reason", self.fallback_reason.clone()),
            row("visibility", self.visibility_state.clone()),
        ]
    }

    /// The four rows shown at `compact` density.
    pub(crate) fn compact_rows(&self) -> Vec<(String, String)> {
        vec![
            row("fps", self.fps_text()),
            row("frame ms", self.frame_ms_text()),
            row("renderer", self.renderer_backend.clone()),
            row("fallbacks", self.fallback_count.to_string()),
        ]
    }

    /// A one-line raw backend-selection summary (verbose density + `backend.report`).
    pub(crate) fn backend_select_text(&self) -> String {
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

    /// A multi-line text snapshot of every field (echoed by `diagnostics.snapshot`).
    pub(crate) fn snapshot_text(&self) -> String {
        [
            "diagnostics snapshot:".to_string(),
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
        ]
        .join("\n")
    }

    /// A labelled per-backend report (echoed by `backend.report`).
    pub(crate) fn backend_report_text(&self) -> String {
        [
            "backend report:".to_string(),
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

/// A neutral `(label, value)` row.
fn row(label: &str, value: String) -> (String, String) {
    (label.to_string(), value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Diagnostics {
        let mut d = Diagnostics::placeholder();
        d.frame_index = 42;
        d.fps_milli = 59_940;
        d.frame_time_micros = 16_680;
        d.renderer_backend = "webgl2".to_string();
        d.worker_messages_in = 3;
        d.worker_messages_out = 4;
        d.fallback_count = 1;
        d.fallback_reason = "webgpu device failed".to_string();
        d
    }

    #[test]
    fn placeholder_is_honest() {
        let d = Diagnostics::placeholder();
        assert_eq!(d.frame_index, 0);
        assert_eq!(d.renderer_backend, "—");
    }

    #[test]
    fn timing_and_worker_format() {
        let d = sample();
        assert_eq!(d.fps_text(), "59.9");
        assert_eq!(d.frame_ms_text(), "16.68");
        assert_eq!(d.worker_text(), "3 / 4");
        assert_eq!(Diagnostics::placeholder().fps_text(), "0.0");
        assert_eq!(Diagnostics::placeholder().frame_ms_text(), "0.00");
    }

    #[test]
    fn core_rows_cover_every_field_in_order() {
        let labels: Vec<String> = sample().core_rows().into_iter().map(|(l, _)| l).collect();
        assert_eq!(
            labels,
            vec![
                "frame", "tick", "fps", "frame ms", "sim ticks", "renderer",
                "canvas owner", "sim owner", "storage", "audio", "network",
                "webgpu subs", "canvas2d frames", "worker msgs", "fallbacks",
                "fallback reason", "visibility",
            ]
        );
    }

    #[test]
    fn compact_rows_are_the_four_at_a_glance_fields() {
        let labels: Vec<String> = sample().compact_rows().into_iter().map(|(l, _)| l).collect();
        assert_eq!(labels, vec!["fps", "frame ms", "renderer", "fallbacks"]);
    }

    #[test]
    fn select_snapshot_and_report_texts_are_complete() {
        let d = sample();
        assert!(d.backend_select_text().contains("webgl2"));
        assert!(d.snapshot_text().contains("renderer=webgl2"));
        assert!(d.snapshot_text().contains("fallbacks=1 (webgpu device failed)"));
        assert!(d.backend_report_text().contains("renderer: webgl2"));
    }
}
