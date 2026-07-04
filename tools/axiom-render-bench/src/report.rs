//! Telemetry parsing + FPS / phase statistics for the render benchmark.
//!
//! The Canvas2D backend logs one `axiom-canvas2d:` line per frame (with
//! `raster=…ms blit=…ms`), one `axiom-canvas2d PROFILE:` line per frame (the
//! coarse `convert`/`rasterize`/`post` split), and — in a debug wasm build — one
//! `axiom-canvas2d DEEP:` line (the `project`/`shade` split of `convert`). This
//! module scrapes those out of the Playwright controller's `console` JSON and
//! reduces them to an FPS distribution + phase averages.

use serde_json::Value;

/// One frame's coarse timing (`raster` + `blit`, the frame's wall-clock cost).
#[derive(Clone, Copy, Default)]
pub struct Frame {
    pub raster_ms: f64,
    pub blit_ms: f64,
}

impl Frame {
    /// Total presented-frame cost (what FPS is derived from).
    pub fn total_ms(&self) -> f64 {
        self.raster_ms + self.blit_ms
    }
}

/// One frame's coarse phase split.
#[derive(Clone, Copy, Default)]
pub struct Phase {
    pub convert_ms: f64,
    pub rasterize_ms: f64,
    pub post_ms: f64,
}

/// One frame's deep `convert` split (debug wasm build only).
#[derive(Clone, Copy, Default)]
pub struct Deep {
    pub project_ms: f64,
    pub shade_ms: f64,
    pub draws: f64,
    pub tris: f64,
}

/// The three telemetry populations scraped from one `console` capture.
#[derive(Default)]
pub struct Telemetry {
    pub frames: Vec<Frame>,
    pub phases: Vec<Phase>,
    pub deeps: Vec<Deep>,
}

/// Read the numeric value immediately after `key` in `text` (e.g. `raster=` →
/// `12.34`), stopping at the first non-numeric character (so a trailing `ms` or
/// space is fine). Returns `None` if the key is absent or the value doesn't parse.
pub fn num_after(text: &str, key: &str) -> Option<f64> {
    let start = text.find(key)? + key.len();
    let rest = &text[start..];
    let end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

/// Parse a Playwright-controller `console` JSON blob into the telemetry
/// populations. The controller returns `{"ok":true,"messages":[{"type","text"},…]}`.
pub fn parse(console_json: &str) -> Telemetry {
    let v: Value = serde_json::from_str(console_json).unwrap_or(Value::Null);
    let messages = v
        .get("messages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut t = Telemetry::default();
    for m in &messages {
        let text = m.get("text").and_then(Value::as_str).unwrap_or("");
        if text.contains("axiom-canvas2d PROFILE:") {
            t.phases.push(Phase {
                convert_ms: num_after(text, "convert=").unwrap_or(0.0),
                rasterize_ms: num_after(text, "rasterize=").unwrap_or(0.0),
                post_ms: num_after(text, "post=").unwrap_or(0.0),
            });
        } else if text.contains("axiom-canvas2d DEEP:") {
            t.deeps.push(Deep {
                project_ms: num_after(text, "project=").unwrap_or(0.0),
                shade_ms: num_after(text, "shade=").unwrap_or(0.0),
                draws: num_after(text, "draws=").unwrap_or(0.0),
                tris: num_after(text, "tris=").unwrap_or(0.0),
            });
        } else if text.contains("axiom-canvas2d:") {
            t.frames.push(Frame {
                raster_ms: num_after(text, "raster=").unwrap_or(0.0),
                blit_ms: num_after(text, "blit=").unwrap_or(0.0),
            });
        }
    }
    t
}

/// Count the `axiom-canvas2d:` frames in a console capture (used to size the
/// warm-up window that the measurement then skips).
pub fn frame_count(console_json: &str) -> usize {
    parse(console_json).frames.len()
}

fn mean(xs: &[f64]) -> f64 {
    (!xs.is_empty())
        .then(|| xs.iter().sum::<f64>() / xs.len() as f64)
        .unwrap_or(0.0)
}

/// The value at percentile `p` (0..1) of `sorted` (ascending). Empty → 0.
fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((p * (sorted.len() as f64 - 1.0)).round() as usize).min(sorted.len() - 1);
    sorted[idx]
}

fn fps(ms: f64) -> f64 {
    if ms > 0.0 {
        1000.0 / ms
    } else {
        0.0
    }
}

/// Render the report (human table, or `--json`). `label` names the run
/// (demo/backend/build).
pub fn render(t: &Telemetry, label: &str, json: bool) -> String {
    let mut ms: Vec<f64> = t.frames.iter().map(Frame::total_ms).collect();
    ms.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = ms.len();
    let avg_ms = mean(&ms);
    // fps percentiles come from the ms distribution: high ms = low fps, so p95-slow
    // (the 95th-percentile frame time) is the worst-case fps a player feels.
    let best = fps(percentile(&ms, 0.0));
    let median = fps(percentile(&ms, 0.5));
    let p95_slow = fps(percentile(&ms, 0.95));
    let worst = fps(percentile(&ms, 1.0));
    let avg_fps = fps(avg_ms);

    let avg_convert = mean(&t.phases.iter().map(|p| p.convert_ms).collect::<Vec<_>>());
    let avg_rasterize = mean(&t.phases.iter().map(|p| p.rasterize_ms).collect::<Vec<_>>());
    let avg_post = mean(&t.phases.iter().map(|p| p.post_ms).collect::<Vec<_>>());
    let avg_project = mean(&t.deeps.iter().map(|d| d.project_ms).collect::<Vec<_>>());
    let avg_shade = mean(&t.deeps.iter().map(|d| d.shade_ms).collect::<Vec<_>>());
    let avg_draws = mean(&t.deeps.iter().map(|d| d.draws).collect::<Vec<_>>());
    let avg_tris = mean(&t.deeps.iter().map(|d| d.tris).collect::<Vec<_>>());

    if json {
        return format!(
            "{{\"label\":\"{label}\",\"frames\":{n},\"fps\":{{\"avg\":{avg_fps:.2},\"best\":{best:.2},\"median\":{median:.2},\"p95_slow\":{p95_slow:.2},\"worst\":{worst:.2}}},\"frame_ms_avg\":{avg_ms:.2},\"phase_ms\":{{\"convert\":{avg_convert:.2},\"rasterize\":{avg_rasterize:.2},\"post\":{avg_post:.2}}},\"deep_ms\":{{\"project\":{avg_project:.2},\"shade\":{avg_shade:.2},\"draws\":{avg_draws:.0},\"tris\":{avg_tris:.0}}}}}"
        );
    }

    let phase_total = avg_convert + avg_rasterize + avg_post;
    let pct = |x: f64| {
        (phase_total > 0.0)
            .then(|| x / phase_total * 100.0)
            .unwrap_or(0.0)
    };
    let mut out = String::new();
    out.push_str(&format!("\n  render benchmark — {label}\n"));
    out.push_str(&format!("  frames measured: {n}\n"));
    out.push_str("  ── FPS ─────────────────────────────\n");
    out.push_str(&format!("    avg     {avg_fps:6.1} fps   ({avg_ms:.1} ms/frame)\n"));
    out.push_str(&format!("    best    {best:6.1} fps\n"));
    out.push_str(&format!("    median  {median:6.1} fps\n"));
    out.push_str(&format!("    p95     {p95_slow:6.1} fps   (95th-percentile-slowest frame)\n"));
    out.push_str(&format!("    worst   {worst:6.1} fps\n"));
    out.push_str("  ── phases (avg ms/frame) ───────────\n");
    out.push_str(&format!("    convert    {avg_convert:7.1} ms  ({:4.1}%)\n", pct(avg_convert)));
    out.push_str(&format!("    rasterize  {avg_rasterize:7.1} ms  ({:4.1}%)\n", pct(avg_rasterize)));
    out.push_str(&format!("    post       {avg_post:7.1} ms  ({:4.1}%)\n", pct(avg_post)));
    if !t.deeps.is_empty() {
        out.push_str("  ── convert deep-split (debug build) ─\n");
        out.push_str(&format!("    project     {avg_project:7.1} ms\n"));
        out.push_str(&format!("    shade       {avg_shade:7.1} ms\n"));
        out.push_str(&format!("    draws/frame {avg_draws:7.0}\n"));
        out.push_str(&format!("    tris/frame  {avg_tris:7.0}\n"));
    }
    out
}
