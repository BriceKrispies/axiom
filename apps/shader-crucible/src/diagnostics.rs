//! **The frame diagnostics panel — and the numbers it refuses to invent.**
//!
//! This app was, until commit `ba4024dd`, roughly twice as slow on a phone as it
//! had any right to be: `Surface::flatten` ran once per surface per *frame*
//! inside `present_packet_with_surfaces`, and a throttled profile put ~67% of the
//! frame's CPU inside it. Nothing on the page said so. The only way to find it
//! was to attach Chrome DevTools to a phone and read a flame chart.
//!
//! That fix landed and the app is **still slow on a phone**, so this panel is
//! not insurance against a future regression — it is the instrument for an open
//! one. What it has already established, by A/B rather than by argument, is in
//! [`Verdict`] and in `crate::web::Levers`: the frame's cost on the WebGL2
//! fallback is per-draw *submission*, invariant to resolution and to the number
//! of generated surface programs.
//!
//! It is the *portable* half: it owns the arithmetic (windowed
//! percentiles, the CPU split, the residual, the verdict) and the HTML the panel
//! is drawn from, and it names no browser API, so `tests` below drive every
//! reading natively. `crate::web` owns the platform half — reading
//! `performance.now()` around the three spans and writing the flushed HTML into
//! the page.
//!
//! ## What is measured, what is derived, and what this will not say
//!
//! Every number the panel prints carries a provenance tag, because a diagnostics
//! panel that mixes measurements with plausible-looking guesses is worse than no
//! panel — it sends the next person optimising the wrong thing.
//!
//! * **`m` — measured.** A real `performance.now()` delta: the frame gap, and
//!   the three spans the app's frame loop actually consists of
//!   (`app.render(tick)`, [`crate::frame::packet_of`],
//!   `GpuBackendApi::present_packet_with_surfaces`). The panel's own cost is
//!   measured the same way and printed beside them.
//! * **`d` — derived.** Arithmetic over measurements or over the frame's own
//!   packet: percentiles, the frame's batch count (grouped on exactly the key
//!   `frame_packet_adapter::frame_packet_to_batches` sorts on), and the
//!   **residual** — `frame gap − main-thread time`.
//! * **`s` — static.** A fact about the workload that does not change per
//!   frame: triangle count, backbuffer size, the barrier's program count.
//!
//! **The GPU time is the engine's, or it is absent — it is never this app's
//! guess.** Every render pass in `axiom-gpu-backend` used to pass
//! `timestamp_writes: None`, so the panel could print nothing but the residual.
//! The backend now resolves real per-pass timestamps and reports them through
//! `GpuBackendApi::gpu_pass_timing`, and this panel prints them **as measured**.
//! Where the adapter cannot do it — notably the browser's WebGL2 downlevel, the
//! path this app actually runs on for most visitors, which has no timestamp
//! queries at all — the card prints the engine's own reason and no number.
//!
//! The residual stays either way, because it answers a different question: frame
//! gap minus main-thread time is GPU **plus** compositor **plus** whatever the
//! browser waited before the next vsync. With a GPU number beside it, the *gap*
//! between the two is the compositor and the wait — which is a thing this panel
//! could not previously separate at all.

/// How many frames the rolling window holds — two seconds at 60 Hz.
///
/// Long enough that a p95 means something, short enough that the panel reacts to
/// a change (a CPU throttle, a hitch, an orbit drag) within a couple of seconds
/// rather than averaging it away.
pub const WINDOW: usize = 120;

/// How many of the window's most recent frames the sparkline draws.
pub const SPARK: usize = 60;

/// How often the DOM is written, in milliseconds — 5 Hz.
///
/// **The panel must not distort what it measures.** Writing `innerHTML` every
/// frame would put string formatting and a DOM parse inside the very frame whose
/// cost is being reported. Samples accumulate in Rust at frame rate and the page
/// is written five times a second; the write's cost lands in `panel_ms` on the
/// frame that pays it, so the spike is visible rather than hidden.
pub const FLUSH_MS: f64 = 200.0;

/// One frame's measurements, in milliseconds.
///
/// Every field is a `performance.now()` delta taken by [`crate::web`] — there is
/// no modelled or estimated field on this struct.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrameSample {
    /// Wall time from the previous frame callback's entry to this one's — the
    /// browser's actual frame cadence, vsync included.
    pub gap_ms: f64,
    /// `orbit.apply` + `app.render(tick)`: the simulation step and the scene
    /// walk that produces the frame's draw list.
    pub render_ms: f64,
    /// [`crate::frame::packet_of`]: the `FrameOutcome` → `FramePacket`
    /// translation, including the caption billboarding.
    pub packet_ms: f64,
    /// `GpuBackendApi::present_packet_with_surfaces`: batching, instance
    /// packing, and command submission. This is the span that carried the
    /// per-frame `flatten` before `ba4024dd` hoisted it to the barrier.
    pub present_ms: f64,
    /// What the diagnostics themselves cost this frame: the bookkeeping every
    /// frame pays, plus the DOM write on the frames that flush.
    pub panel_ms: f64,
}

impl FrameSample {
    /// The app's own main-thread frame cost — the three spans, without the
    /// panel's overhead, so the number does not flatter itself.
    pub fn cpu_ms(&self) -> f64 {
        self.render_ms + self.packet_ms + self.present_ms
    }

    /// Everything this frame spent on the main thread, panel included.
    pub fn main_ms(&self) -> f64 {
        self.cpu_ms() + self.panel_ms
    }

    /// Frame gap minus main-thread time, floored at zero. **Not GPU time** — see
    /// the module docs.
    pub fn residual_ms(&self) -> f64 {
        (self.gap_ms - self.main_ms()).max(0.0)
    }
}

/// A rolling window of the last [`WINDOW`] frames.
#[derive(Clone, Debug, Default)]
pub struct FrameHistory {
    samples: std::collections::VecDeque<FrameSample>,
}

impl FrameHistory {
    /// An empty window.
    pub fn new() -> Self {
        FrameHistory {
            samples: std::collections::VecDeque::with_capacity(WINDOW),
        }
    }

    /// Record one frame, evicting the oldest once the window is full.
    pub fn push(&mut self, sample: FrameSample) {
        (self.samples.len() >= WINDOW).then(|| self.samples.pop_front());
        self.samples.push_back(sample);
    }

    /// How many frames the window currently holds.
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// Whether no frame has been recorded yet.
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// The window's samples, oldest first.
    pub fn samples(&self) -> impl Iterator<Item = &FrameSample> {
        self.samples.iter()
    }

    /// The most recently recorded frame, if there is one.
    pub fn newest(&self) -> Option<&FrameSample> {
        self.samples.back()
    }

    /// The distribution of one field over the window.
    pub fn stat(&self, field: fn(&FrameSample) -> f64) -> Stat {
        let mut values: Vec<f64> = self.samples.iter().map(field).collect();
        values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        Stat::of(&values)
    }
}

/// A windowed distribution.
///
/// **Percentiles rather than a mean**, deliberately: a mean frame time hides
/// exactly the thing worth finding. Sixty smooth frames and one 100 ms hitch
/// average to a healthy-looking number; the p95 and the max do not.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stat {
    /// The fastest 5% boundary — with a frame gap, the closest thing to the
    /// display's own interval that can be observed without asking it.
    pub p05: f64,
    /// The median.
    pub p50: f64,
    /// The 95th percentile: what a bad frame costs.
    pub p95: f64,
    /// The worst frame in the window.
    pub max: f64,
}

impl Stat {
    /// The distribution of an **ascending-sorted** slice. An empty slice is all
    /// zeroes, which is what the panel shows before the first frame lands.
    fn of(sorted: &[f64]) -> Self {
        Stat {
            p05: percentile(sorted, 0.05),
            p50: percentile(sorted, 0.50),
            p95: percentile(sorted, 0.95),
            max: sorted.last().copied().unwrap_or(0.0),
        }
    }
}

/// The `q`-quantile of an ascending-sorted slice, by nearest rank.
fn percentile(sorted: &[f64], q: f64) -> f64 {
    sorted
        .len()
        .checked_sub(1)
        .map(|last| {
            let rank = (q * last as f64).round() as usize;
            sorted[rank.min(last)]
        })
        .unwrap_or(0.0)
}

/// The frame's workload, as the packet and the backend actually describe it.
///
/// Everything here is a count or a size — nothing is a timing. These are the
/// facts that tell you *what* the GPU was asked to do on a frame whose duration
/// cannot be measured.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Workload {
    /// Draws in the packet, captions included.
    pub draws: usize,
    /// Instance batches the backend will group these draws into — computed on
    /// exactly the key `frame_packet_adapter::frame_packet_to_batches` sorts on,
    /// `(surface_program, mesh_id, material_id)`, so this is the count the
    /// renderer really issues rather than an app-side guess at it.
    pub batches: usize,
    /// Distinct non-zero surface programs in the frame. Because the backend
    /// sorts draws by program, this is also the number of pipeline switches the
    /// surfaced half of the frame costs.
    pub programs_used: usize,
    /// Triangles submitted this frame: each draw's mesh index count / 3, summed.
    pub triangles: u64,
    /// Lights in the packet.
    pub lights: usize,
    /// The backbuffer the app renders into, in device pixels.
    pub backbuffer: (u32, u32),
    /// **The device tier's** render size — `GpuBackendApi::render_width/height`.
    ///
    /// Not the live render scale: the tier size is decided once, at
    /// initialisation, and the backend exposes no accessor for the size the live
    /// binding is currently rendering at (`LiveGpuBinding::render_size` exists
    /// and is unreachable from the facade). So [`Workload::render_scale`] below
    /// carries what the app *asked* for, and the two are printed as the separate
    /// facts they are.
    pub render_target: (u32, u32),
    /// The render scale the app last handed the backend — 1.0 at full, 0.50 at
    /// the ladder's floor, or whatever the adaptive controller chose. Labelled
    /// "asked" on the panel, because the engine will not report back what it
    /// actually rendered at.
    pub render_scale: f64,
    /// The canvas's laid-out CSS size, and the page's device pixel ratio — the
    /// pair that says whether the backbuffer is being up- or down-scaled to
    /// reach the screen.
    pub css: (f64, f64),
    /// `window.devicePixelRatio`.
    pub dpr: f64,
    /// **Which GPU backend `wgpu` actually bound**, as the engine's own
    /// initialisation reported it (`BrowserWebGpu` or `Gl`).
    ///
    /// This is not cosmetic. The WebGL2 arm is a different cost profile
    /// entirely — this repository's own profiling records ~52 GL calls per draw
    /// on it, and it has no timestamp queries at all — so a page that silently
    /// fell back to it is a different app from the one that bound WebGPU, and
    /// "the phone is slow" may have no other cause. The engine knows this at
    /// `modules/axiom-gpu-backend/src/live_gpu_binding.rs:302` and only
    /// **logs** it; see the module docs for the accessor that ought to exist.
    pub backend: String,
    /// Programs the barrier compiled.
    pub prepared_programs: u32,
    /// Authored surfaces the barrier saw.
    pub prepared_surfaces: u32,
    /// The backend's capability profile, named.
    pub profile: String,
    /// What the first frame could not honour — empty is the healthy answer.
    pub degraded: String,
    /// **Whether the engine resolved real per-pass GPU times for a recent
    /// frame.**
    ///
    /// `false` is the normal answer on the WebGL2 fallback, which has no
    /// timestamp queries at all, and on any adapter that does not offer
    /// `TIMESTAMP_QUERY`.
    pub gpu_available: bool,
    /// Why there is no GPU timing, when there is none. Printed verbatim: a
    /// missing measurement is reported with its reason, never as a zero.
    pub gpu_reason: String,
    /// The measured passes of the last frame that finished resolving, in
    /// milliseconds — `("shadow", 19.2)`, `("main", 8.4)`, and so on. A pass the
    /// frame did not run is **absent**, not zero.
    pub gpu_passes: Vec<(String, f64)>,
    /// Every measured pass, summed.
    pub gpu_total_ms: f64,
    /// Which frame those passes belong to. Never the frame on screen: resolving
    /// a timestamp query set completes on a later task, so this lags, and saying
    /// which frame it is is the difference between a measurement and a number.
    pub gpu_frame: u64,
}

/// **What the frame loop is doing, and whether the readings beside it are live.**
///
/// The crucible submits a frame only when its pixels would differ from the pixels
/// already on the screen (the rule and its derivation are in [`crate::redraw`]), so
/// on a still camera looking at a static station it draws **nothing** — which is
/// correct, and which also means the FRAME and MAIN THREAD cards are showing the
/// last run of frames rather than the current one.
///
/// This is the panel saying so. An instrument whose needle has stopped is only
/// honest if it can tell you the difference between "nothing is happening" and "I
/// am broken", and the alternative — manufacturing frames so the number keeps
/// moving — would be an instrument reporting on work it created itself. The
/// `FORCE` lever is the sanctioned way to get a live steady-state reading back;
/// see [`crate::levers`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LoopState {
    /// Why the most recent callback drew a frame, or did not —
    /// `crate::redraw::Redraw::reason`, printed verbatim.
    pub reason: &'static str,
    /// Whether that callback drew. `false` means every number in the cards above
    /// is held from an earlier frame.
    pub drawing: bool,
    /// How long ago the last frame was actually submitted, in milliseconds — the
    /// age of the image on the screen, and therefore of the readings.
    pub held_ms: f64,
    /// Frames submitted since the page loaded.
    pub drawn: u64,
    /// Callbacks that decided the screen was already correct and drew nothing.
    /// **This is the number the fix produced**: on a still crucible it climbs at
    /// the display's refresh rate while `drawn` does not move at all.
    pub skipped: u64,
}

impl Default for LoopState {
    /// The state before the device has bound and the loop has run once.
    fn default() -> Self {
        LoopState {
            reason: "warming up — no frame has been drawn yet",
            drawing: false,
            held_ms: 0.0,
            drawn: 0,
            skipped: 0,
        }
    }
}

/// Which resource the frame is up against, stated only when the measurements
/// support it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Not enough frames yet.
    Warming,
    /// The main thread fills the frame: the app's own CPU is the ceiling.
    CpuBound,
    /// The cadence is pinned by the display and the main thread has real
    /// headroom — a 16.7 ms frame that costs 2 ms of CPU.
    VsyncCapped,
    /// Frames are longer than the main thread explains, and the cadence is not
    /// pinned. The extra is off-main-thread — GPU, compositor, or the browser
    /// choosing not to run us — and this panel cannot separate those three.
    OffMainThread,
}

/// The slowest cadence that can honestly be called "the display's".
///
/// **A steady frame is not a capped frame.** A GPU that takes 65 ms per frame
/// delivers a beautifully regular 15 fps, and a verdict that reads only
/// steadiness and CPU share calls that "vsync-capped — the main thread has
/// headroom", which is true and utterly misleading. Measured on a software
/// rasterizer standing in for a weak GPU: 65.2 ms frames, 1.1 ms of CPU,
/// perfectly steady. No display refreshes at 15 Hz. Past this bound the cadence
/// is something else's doing and the panel says so.
const SLOWEST_DISPLAY_MS: f64 = 17.5;

impl Verdict {
    /// The verdict for a window: CPU-bound when the main thread fills ≥85% of
    /// the frame; vsync-capped when the cadence is steady (p95 within 25% of
    /// p50), *fast enough to be a display's* ([`SLOWEST_DISPLAY_MS`]), and the
    /// main thread uses under half of it; otherwise the residual is doing the
    /// work and is named, not attributed.
    pub fn of(history: &FrameHistory) -> Self {
        let gap = history.stat(|s| s.gap_ms);
        let main = history.stat(FrameSample::main_ms);
        let warming = history.len() < 10 || gap.p50 <= 0.0;
        let cpu_share = main.p50 / gap.p50.max(f64::MIN_POSITIVE);
        let capped = (gap.p95 <= gap.p50 * 1.25)
            & (gap.p50 <= SLOWEST_DISPLAY_MS)
            & (cpu_share < 0.5);
        [
            [
                [Verdict::OffMainThread, Verdict::VsyncCapped][usize::from(capped)],
                Verdict::CpuBound,
            ][usize::from(cpu_share >= 0.85)],
            Verdict::Warming,
        ][usize::from(warming)]
    }

    /// The sentence the panel prints.
    pub fn headline(self) -> &'static str {
        match self {
            Verdict::Warming => "warming up — not enough frames yet",
            Verdict::CpuBound => "CPU-BOUND — the main thread fills the frame",
            Verdict::VsyncCapped => {
                "VSYNC-CAPPED — the display sets the cadence; the main thread has headroom"
            }
            Verdict::OffMainThread => {
                "NOT CPU-BOUND — the frame is longer than the main thread explains"
            }
        }
    }
}

/// One reading the page displays: the id of the element that shows it, and the
/// text that goes in it.
///
/// **The panel updates values, never markup.** It rebuilt its whole body from an
/// `innerHTML` string until a straight A/B measured what that cost: with the
/// panel on, the page's own `requestAnimationFrame` cadence read p50 16.7 ms /
/// **p95 33.3 ms / max 66.7 ms**; with it off, p50 16.7 / p95 16.8 / max 16.8.
/// Parsing seven kilobytes of HTML and rebuilding two hundred nodes five times a
/// second was dropping a frame every time it happened — a diagnostics panel
/// manufacturing exactly the hitches it exists to find.
///
/// So the page owns the skeleton (see `web/index.html`), every field has a
/// stable id, and a flush writes text into elements that already exist. The
/// sparkline, which was 120 of those nodes, is now two SVG polygons and two
/// attribute writes.
pub type Reading = (&'static str, String);

/// **The prose the panel states once**, written on the first flush and never
/// again — it does not change, so it must not cost anything per frame.
///
/// It lives here rather than in the page because these three sentences are the
/// honesty contract of the whole panel, and the code that generates the numbers
/// should be the code that states what they are and are not. `tests` below
/// assert on them.
pub fn static_readings() -> Vec<Reading> {
    vec![
        (
            "d-note-gap",
            "gap = frame callback entry to entry. p05 is the fastest 5% of \
             frames — the closest observable stand-in for the display's own \
             interval."
                .to_string(),
        ),
        (
            "d-note-spans",
            "app.render = sim + scene walk · packet_of = packet build + caption \
             billboarding · present = batch, pack, submit. Those three are the \
             whole of this app's frame."
                .to_string(),
        ),
        (
            "d-note-residual",
            "This is not GPU time. It is frame gap − measured main-thread time: \
             GPU work plus compositor plus the wait for the next vsync, and \
             nothing here can separate the three."
                .to_string(),
        ),
        (
            "d-note-loop",
            "A frame is submitted only when its pixels would differ from the ones \
             already on screen: the camera or a lever moved, or a clock-reading \
             station (5) is in shot. A still camera on a static station draws \
             NOTHING, and the numbers above are then the last run of frames, not \
             the current one — the loop row says which. FORCE holds the loop open \
             so a steady-state cost can still be measured; it changes no pixel and \
             no draw, only how often the identical frame is submitted."
                .to_string(),
        ),
        (
            "d-note-workload",
            "batches and pipelines are the backend's OWN counts, read off the \
             packet through GpuBackendApi::packet_batch_counts — the app used to \
             re-derive them by duplicating the adapter's sort key by hand. \
             sample is backbuffer pixels per device pixel: above 1.0 the frame \
             is shaded larger than the screen and the compositor throws the rest \
             away. render target is the DEVICE TIER's size, fixed at \
             initialisation; render scale is what the app last asked the live \
             binding for, and the backend exposes no way to read back what it \
             rendered at — so the GPU pass times are the evidence that a scale \
             took effect."
                .to_string(),
        ),
    ]
}

/// Every value the panel shows this flush.
///
/// `loop_state` is read first and printed at the top of the FRAME card, because it
/// is the qualifier on everything under it: a cadence percentile taken from a
/// window the loop stopped filling two seconds ago is a real measurement of a run
/// that has ended, and the reader has to be told which of the two they are looking
/// at before they read the number.
pub fn readings(
    history: &FrameHistory,
    workload: &Workload,
    loop_state: &LoopState,
) -> Vec<Reading> {
    let gap = history.stat(|s| s.gap_ms);
    let cpu = history.stat(FrameSample::cpu_ms);
    let main = history.stat(FrameSample::main_ms);
    let render = history.stat(|s| s.render_ms);
    let packet = history.stat(|s| s.packet_ms);
    let present = history.stat(|s| s.present_ms);
    let panel = history.stat(|s| s.panel_ms);
    let residual = history.stat(FrameSample::residual_ms);
    vec![
        // THE LOOP — whether the frame numbers below it are live or held.
        ("d-loop", loop_state.reason.to_string()),
        (
            "d-frames",
            format!(
                "{} drawn / {} skipped · last {:.1} s ago",
                loop_state.drawn,
                loop_state.skipped,
                loop_state.held_ms / 1000.0
            ),
        ),
        // FRAME — the cadence the browser actually gave us.
        ("d-fps", format!("{:.1}", 1000.0 / gap.p50.max(f64::MIN_POSITIVE))),
        ("d-gap50", ms(gap.p50)),
        ("d-gap95", ms(gap.p95)),
        ("d-gapmax", ms(gap.max)),
        (
            "d-gap05",
            format!(
                "{} ({:.0} Hz)",
                ms(gap.p05),
                1000.0 / gap.p05.max(f64::MIN_POSITIVE)
            ),
        ),
        ("d-scale", format!("scale {:.0} ms", spark_ceiling(&gap))),
        // MAIN THREAD — measured spans.
        ("d-cpu50", format!("{:.2}", cpu.p50)),
        ("d-cpu95", ms(cpu.p95)),
        (
            "d-share",
            format!("{:.0}%", 100.0 * main.p50 / gap.p50.max(f64::MIN_POSITIVE)),
        ),
        ("d-render", ms(render.p50)),
        ("d-packet", ms(packet.p50)),
        ("d-present", ms(present.p50)),
        ("d-panel50", ms(panel.p50)),
        ("d-panelmax", ms(panel.max)),
        ("d-panelnote", panel_cost_note(&panel).to_string()),
        // NOT ON THE MAIN THREAD — derived, and named for what it is.
        ("d-res50", format!("{:.2}", residual.p50)),
        ("d-res95", ms(residual.p95)),
        ("d-vgap", ms(gap.p50)),
        ("d-vcpu", ms(cpu.p50)),
        ("d-verdict", Verdict::of(history).headline().to_string()),
        // WORKLOAD — what the GPU was asked to do on a frame whose duration
        // cannot be measured.
        ("d-backend", workload.backend.clone()),
        ("d-draws", workload.draws.to_string()),
        ("d-batches", workload.batches.to_string()),
        ("d-pipes", workload.programs_used.to_string()),
        ("d-tris", workload.triangles.to_string()),
        ("d-lights", workload.lights.to_string()),
        (
            "d-back",
            format!("{}x{}", workload.backbuffer.0, workload.backbuffer.1),
        ),
        (
            "d-target",
            format!("{}x{}", workload.render_target.0, workload.render_target.1),
        ),
        ("d-rscale", format!("{:.2}x asked", workload.render_scale)),
        (
            "d-css",
            format!(
                "{:.0}x{:.0} @{:.2}",
                workload.css.0, workload.css.1, workload.dpr
            ),
        ),
        ("d-sample", format!("{:.2}x", sample_ratio(workload))),
        (
            "d-barrier",
            format!(
                "{} prog / {} surf",
                workload.prepared_programs, workload.prepared_surfaces
            ),
        ),
        ("d-profile", workload.profile.clone()),
        ("d-degraded", workload.degraded.clone()),
        // The header's backend badge, written by Rust now that
        // `GpuBackendApi::bound_backend()` exists. The page used to wrap
        // `console.log` and scrape the engine's own initialisation line for it.
        ("diag-backend", workload.backend.clone()),
        // GPU — a real measurement when the adapter offers timestamp queries,
        // and a named absence when it does not.
        ("d-gpu-total", gpu_total(workload)),
        ("d-gpu-shadow", gpu_pass(workload, "shadow")),
        ("d-gpu-main", gpu_pass(workload, "main")),
        ("d-gpu-sdf", gpu_pass(workload, "sdf")),
        ("d-gpu-post", gpu_pass(workload, "post")),
        ("d-gpu-draw2d", gpu_pass(workload, "draw2d")),
        ("d-gpu-frame", gpu_frame(workload)),
        ("d-note-gpu", gpu_note(workload)),
    ]
}

/// One GPU pass's measured time, or an em dash when this frame did not run it —
/// **never a zero**, which would read as "it ran and cost nothing".
fn gpu_pass(workload: &Workload, name: &str) -> String {
    workload
        .gpu_passes
        .iter()
        .find(|(pass, _)| pass == name)
        .map(|(_, measured)| ms(*measured))
        .unwrap_or_else(|| "—".to_string())
}

/// Every measured pass, summed — or the absence, stated.
fn gpu_total(workload: &Workload) -> String {
    workload
        .gpu_available
        .then(|| format!("{:.2}", workload.gpu_total_ms))
        .unwrap_or_else(|| "—".to_string())
}

/// Which frame the GPU reading belongs to. It is never the one on screen.
fn gpu_frame(workload: &Workload) -> String {
    workload
        .gpu_available
        .then(|| format!("frame {}", workload.gpu_frame))
        .unwrap_or_else(|| "no frame".to_string())
}

/// **The GPU card's prose**, which is the honesty contract in one sentence.
///
/// The panel could not print a GPU time at all until `axiom-gpu-backend` grew
/// timestamp queries; before that this note named the exact lines that passed
/// `timestamp_writes: None`, because a panel that leaves a gap where a number
/// should be teaches the reader nothing. Now the number exists on adapters that
/// support it — and where it does not, the *engine's own reason* is printed
/// rather than an app-side guess at one.
fn gpu_note(workload: &Workload) -> String {
    workload
        .gpu_available
        .then(|| {
            "Measured by the engine's timestamp queries, resolved off the frame — \
             so this is the last frame that FINISHED resolving, not the one on \
             screen. A pass this frame did not run is absent, never zero."
                .to_string()
        })
        .unwrap_or_else(|| {
            format!(
                "GPU time: unavailable — {}. The browser's WebGL2 fallback has no \
                 timestamp queries at all. A number here would be invented.",
                workload.gpu_reason
            )
        })
}

/// The panel's own honesty line: it states its cost, and says so out loud when
/// that cost has grown past the budget it was written to.
fn panel_cost_note(panel: &Stat) -> &'static str {
    [
        "The panel's own cost is inside its 0.2 ms/frame budget, and it is \
         excluded from the spans above.",
        "WARNING: the panel now costs more than its 0.2 ms/frame budget — read \
         the spans above knowing the page is paying for the reading.",
    ][usize::from(panel.p50 > 0.2)]
}

/// The three CPU spans as percentages of the measured main-thread total, for
/// the bars' widths.
pub fn bars(history: &FrameHistory) -> Vec<(&'static str, f64)> {
    let total = history.stat(FrameSample::cpu_ms).p50.max(f64::MIN_POSITIVE);
    vec![
        ("d-bar-render", share(history.stat(|s| s.render_ms).p50, total)),
        ("d-bar-packet", share(history.stat(|s| s.packet_ms).p50, total)),
        ("d-bar-present", share(history.stat(|s| s.present_ms).p50, total)),
    ]
}

/// One span's share of the measured total, clamped so a coarse clock cannot
/// draw a bar past its track.
fn share(value_ms: f64, total_ms: f64) -> f64 {
    (100.0 * value_ms / total_ms.max(f64::MIN_POSITIVE)).clamp(0.0, 100.0)
}

/// The sparkline's viewBox width.
const SPARK_W: f64 = 240.0;

/// The sparkline's viewBox height.
const SPARK_H: f64 = 34.0;

/// **The frame history, as two filled areas**: the outer one is the frame gap,
/// the inner one the main-thread time inside it.
///
/// A mean cannot show a hitch and a percentile can only count one; the shape
/// shows *where* in the last second it happened, which is what makes an
/// intermittent stall findable rather than merely known about.
///
/// Two polygon point strings — two attribute writes per flush. This was 120 DOM
/// nodes rebuilt five times a second until the A/B in [`Reading`]'s docs showed
/// what that cost the very cadence it was drawing.
pub fn spark(history: &FrameHistory) -> (String, String) {
    let ceiling = spark_ceiling(&history.stat(|s| s.gap_ms));
    let recent: Vec<&FrameSample> = history
        .samples()
        .skip(history.len().saturating_sub(SPARK))
        .collect();
    let step = SPARK_W / (SPARK as f64 - 1.0);
    let area = |value: &dyn Fn(&FrameSample) -> f64| {
        let points: String = recent
            .iter()
            .enumerate()
            .map(|(index, sample)| {
                let y = SPARK_H - (SPARK_H * value(sample) / ceiling).clamp(0.0, SPARK_H);
                format!("{:.1},{y:.1} ", index as f64 * step)
            })
            .collect();
        // Closed back along the baseline, so the polygon is an area rather than
        // a line with a filled interior of the wrong shape.
        format!(
            "0,{SPARK_H} {points}{:.1},{SPARK_H}",
            recent.len().saturating_sub(1) as f64 * step
        )
    };
    (area(&|s: &FrameSample| s.gap_ms), area(&FrameSample::main_ms))
}

/// The sparkline's full-scale value in milliseconds.
///
/// **Scaled to the p95, not to the worst frame.** A single startup hitch — this
/// app's first frame compiles pipelines and measured 394 ms — sets a maximum
/// two orders of magnitude above the steady state, and a chart scaled to it
/// draws every real frame as a flat line one pixel high. The p95 keeps the
/// working range legible and lets a genuine outlier clip at the top of the box,
/// which still reads as a spike; the exact worst frame is printed as a number
/// beside it, where a single value belongs.
fn spark_ceiling(gap: &Stat) -> f64 {
    (gap.p95 * 1.5).max(gap.p50 * 2.0).max(8.0)
}

/// **Backbuffer pixels per device pixel.**
///
/// The canvas is laid out as `min(96vw, 1280px)` at a 2:1 aspect while
/// `crate::web` pins the backbuffer at `layout::WIDTH x layout::HEIGHT`, so the
/// two are only equal by coincidence. Above 1.0 the fragment stage is shading
/// more pixels than the screen can show and the compositor throws the rest away;
/// below 1.0 the image is being magnified. On a phone — small CSS box, high
/// DPR — this number is the difference between a fill-rate problem and a
/// non-problem, and it is arithmetic over two measured sizes rather than a guess.
///
/// Zero device pixels (a `display: none` canvas, a headless probe) reports
/// `0.0` rather than an infinity.
pub fn sample_ratio(w: &Workload) -> f64 {
    let device = w.css.0 * w.dpr * w.css.1 * w.dpr;
    let back = f64::from(w.backbuffer.0) * f64::from(w.backbuffer.1);
    (device > 0.0).then(|| back / device).unwrap_or(0.0)
}

/// Milliseconds, two decimals.
fn ms(value: f64) -> String {
    format!("{value:.2} ms")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(gap: f64, render: f64, packet: f64, present: f64) -> FrameSample {
        FrameSample {
            gap_ms: gap,
            render_ms: render,
            packet_ms: packet,
            present_ms: present,
            panel_ms: 0.05,
        }
    }

    fn filled(count: usize, gap: f64, cpu_each: f64) -> FrameHistory {
        let mut history = FrameHistory::new();
        (0..count).for_each(|_| history.push(sample(gap, cpu_each, cpu_each, cpu_each)));
        history
    }

    /// A loop that is drawing every frame — the state every reading below the
    /// loop row is only a live measurement in.
    fn drawing() -> LoopState {
        LoopState {
            reason: "drawing — a clock-reading station is on screen",
            drawing: true,
            held_ms: 16.7,
            drawn: 240,
            skipped: 0,
        }
    }

    /// The value shown by one element id, for asserting on a reading table.
    fn shown(readings: &[Reading], id: &str) -> String {
        readings
            .iter()
            .find(|(key, _)| *key == id)
            .map(|(_, value)| value.clone())
            .unwrap_or_else(|| panic!("no reading for {id}"))
    }

    #[test]
    fn a_sample_splits_into_cpu_panel_and_residual() {
        let s = sample(16.7, 1.0, 0.5, 0.5);
        assert!((s.cpu_ms() - 2.0).abs() < 1e-9);
        assert!((s.main_ms() - 2.05).abs() < 1e-9);
        assert!((s.residual_ms() - 14.65).abs() < 1e-9);
    }

    /// The residual can never go negative: a frame whose measured spans exceed
    /// its gap (a clock coarsened past the span, a first frame) reports zero
    /// rather than a negative "GPU time".
    #[test]
    fn the_residual_is_floored_at_zero() {
        assert_eq!(sample(1.0, 5.0, 5.0, 5.0).residual_ms(), 0.0);
    }

    #[test]
    fn the_window_evicts_the_oldest_frame() {
        let mut history = FrameHistory::new();
        assert!(history.is_empty());
        (0..WINDOW + 20).for_each(|i| history.push(sample(i as f64, 0.0, 0.0, 0.0)));
        assert_eq!(history.len(), WINDOW);
        // The oldest surviving frame is the 21st pushed.
        assert_eq!(history.samples().next().expect("a frame").gap_ms, 20.0);
    }

    #[test]
    fn an_empty_window_reports_zeroes_and_not_a_panic() {
        let history = FrameHistory::new();
        assert_eq!(history.stat(|s| s.gap_ms), Stat::default());
        assert_eq!(Verdict::of(&history), Verdict::Warming);
        assert_eq!(shown(&readings(&history, &Workload::default(), &LoopState::default()), "d-fps"), "inf");
    }

    /// **The p95 is the point of the panel.** Sixty smooth frames and one 100 ms
    /// hitch have a healthy-looking mean; the percentiles and the max do not
    /// hide it.
    #[test]
    fn a_single_hitch_survives_the_percentiles() {
        let mut history = filled(60, 16.7, 0.5);
        history.push(sample(100.0, 0.5, 0.5, 0.5));
        let gap = history.stat(|s| s.gap_ms);
        assert!((gap.p50 - 16.7).abs() < 1e-9, "{gap:?}");
        assert_eq!(gap.max, 100.0);
        let mean: f64 = history.samples().map(|s| s.gap_ms).sum::<f64>() / history.len() as f64;
        assert!(
            mean < 18.5,
            "the mean hides the hitch, which is why it is not shown"
        );
    }

    #[test]
    fn percentiles_are_nearest_rank_over_a_sorted_slice() {
        let values: Vec<f64> = (1..=100).map(f64::from).collect();
        // Nearest rank over 0..=99: the median rank is 50, the 51st value.
        assert_eq!(percentile(&values, 0.50), 51.0);
        assert_eq!(percentile(&values, 0.95), 95.0);
        assert_eq!(percentile(&values, 0.05), 6.0);
        assert_eq!(percentile(&[], 0.5), 0.0);
    }

    /// A 16.7 ms cadence costing 1.5 ms of CPU is the display's doing, and the
    /// panel says so instead of implying the app is fast enough by luck.
    #[test]
    fn a_steady_cadence_with_headroom_is_reported_as_vsync_capped() {
        assert_eq!(Verdict::of(&filled(60, 16.7, 0.5)), Verdict::VsyncCapped);
    }

    /// The regression this panel exists for: main-thread time grows until it
    /// fills the frame, and the verdict flips.
    #[test]
    fn a_main_thread_that_fills_the_frame_is_reported_as_cpu_bound() {
        assert_eq!(Verdict::of(&filled(60, 16.7, 5.0)), Verdict::CpuBound);
    }

    /// Long frames the main thread does not explain are named as
    /// off-main-thread and never as "GPU".
    #[test]
    fn unexplained_long_frames_are_named_off_main_thread_not_gpu() {
        let mut history = FrameHistory::new();
        // A jittery cadence — p95 well past p50 — with a cheap main thread.
        (0..60).for_each(|i| {
            history.push(sample(20.0 + (i % 30) as f64, 0.4, 0.3, 0.3));
        });
        assert_eq!(Verdict::of(&history), Verdict::OffMainThread);
        assert!(!Verdict::OffMainThread.headline().contains("GPU"));
    }

    /// **A steady 15 fps is not a capped 15 fps.** Measured on a software
    /// rasterizer: 65 ms frames, 1.1 ms of CPU, dead steady. Nothing refreshes
    /// at 15 Hz, so the honest verdict is that the time is off the main thread —
    /// not that the display is setting the pace.
    #[test]
    fn a_steady_but_slow_cadence_is_not_called_vsync_capped() {
        assert_eq!(Verdict::of(&filled(60, 65.0, 0.35)), Verdict::OffMainThread);
        assert_eq!(Verdict::of(&filled(60, 16.7, 0.35)), Verdict::VsyncCapped);
    }

    #[test]
    fn a_short_window_is_still_warming() {
        assert_eq!(Verdict::of(&filled(5, 16.7, 0.5)), Verdict::Warming);
    }

    /// **An unmeasurable GPU is reported as unmeasurable**, with the engine's own
    /// reason, and with no number anywhere near it.
    ///
    /// This is the WebGL2 case — the path most visitors are actually on — and it
    /// is the one where a plausible-looking zero would do the most damage: a
    /// shadow pass reading `0.00 ms` is an instruction to go and optimise
    /// something else.
    #[test]
    fn an_unmeasurable_gpu_prints_the_reason_and_no_number() {
        let absent = Workload {
            gpu_available: false,
            gpu_reason: "the adapter offers no timestamp query".to_string(),
            ..Workload::default()
        };
        let values = readings(&filled(60, 16.7, 0.5), &absent, &drawing());
        assert_eq!(shown(&values, "d-gpu-total"), "—");
        assert_eq!(shown(&values, "d-gpu-shadow"), "—");
        assert_eq!(shown(&values, "d-gpu-frame"), "no frame");
        let note = shown(&values, "d-note-gpu");
        assert!(note.contains("GPU time: unavailable"));
        assert!(note.contains("the adapter offers no timestamp query"));
        assert!(note.contains("would be invented"));
        // The residual is still named for what it is, whether or not there is a
        // GPU number beside it.
        let notes: String = static_readings()
            .iter()
            .map(|(_, text)| text.clone())
            .collect::<Vec<String>>()
            .join(" ");
        assert!(notes.contains("This is not GPU time"));
    }

    /// **A measured GPU is printed as measured, pass by pass** — and a pass the
    /// frame did not run is an em dash rather than a zero.
    #[test]
    fn a_measured_gpu_prints_each_pass_and_omits_the_ones_that_did_not_run() {
        let measured = Workload {
            gpu_available: true,
            gpu_passes: vec![("shadow".to_string(), 19.25), ("main".to_string(), 8.4)],
            gpu_total_ms: 27.65,
            gpu_frame: 412,
            ..Workload::default()
        };
        let values = readings(&filled(60, 33.3, 0.5), &measured, &drawing());
        assert_eq!(shown(&values, "d-gpu-shadow"), "19.25 ms");
        assert_eq!(shown(&values, "d-gpu-main"), "8.40 ms");
        assert_eq!(shown(&values, "d-gpu-sdf"), "—", "an unrun pass is not a zero");
        assert_eq!(shown(&values, "d-gpu-total"), "27.65");
        assert_eq!(shown(&values, "d-gpu-frame"), "frame 412");
        assert!(shown(&values, "d-note-gpu").contains("not the one on screen"));
    }

    /// The backend badge in the panel's header is written by Rust, from the
    /// engine's own `bound_backend()` — the page no longer wraps `console.log`
    /// to scrape it out of an initialisation line.
    #[test]
    fn the_header_badge_is_a_reading_and_not_a_scraped_console_line() {
        let values = readings(
            &filled(60, 16.7, 0.5),
            &Workload {
                backend: "GpuFallback".to_string(),
                ..Workload::default()
            },
            &drawing(),
        );
        assert_eq!(shown(&values, "diag-backend"), "GpuFallback");
    }

    /// Every reading the page has a slot for is produced, and the numbers are
    /// the ones the window holds.
    #[test]
    fn the_reading_table_carries_every_field_the_page_shows() {
        let workload = Workload {
            draws: 25,
            batches: 25,
            programs_used: 11,
            triangles: 11_182,
            lights: 2,
            backbuffer: (1280, 640),
            render_target: (1280, 640),
            css: (1231.0, 616.0),
            dpr: 1.0,
            prepared_programs: 11,
            prepared_surfaces: 11,
            profile: "gpu/all".to_string(),
            degraded: "none".to_string(),
            backend: "BrowserWebGpu".to_string(),
            ..Workload::default()
        };
        let values = readings(&filled(60, 16.7, 0.5), &workload, &drawing());
        assert_eq!(shown(&values, "d-fps"), "59.9");
        assert_eq!(shown(&values, "d-gap50"), "16.70 ms");
        assert_eq!(shown(&values, "d-cpu50"), "1.50");
        assert_eq!(shown(&values, "d-render"), "0.50 ms");
        assert_eq!(shown(&values, "d-res50"), "15.15");
        assert_eq!(shown(&values, "d-backend"), "BrowserWebGpu");
        assert_eq!(shown(&values, "d-back"), "1280x640");
        assert_eq!(shown(&values, "d-barrier"), "11 prog / 11 surf");
        assert!(shown(&values, "d-verdict").contains("VSYNC-CAPPED"));
        assert!(shown(&values, "d-gap05").contains("Hz"));
    }

    /// **An idle loop is reported as idle, with the age of what it is showing.**
    ///
    /// This is the panel's half of the fix. The frame numbers do not become wrong
    /// when the loop stops — they become *historical*, and the difference between
    /// "59.9 fps" and "59.9 fps, measured four seconds ago" is the whole value of
    /// the card. The skipped count is the saving, stated as a number.
    #[test]
    fn an_idle_loop_says_so_and_dates_the_readings_it_is_holding() {
        let idle = LoopState {
            reason: "IDLE — nothing that decides a pixel has moved",
            drawing: false,
            held_ms: 4210.0,
            drawn: 37,
            skipped: 253,
        };
        let values = readings(&filled(60, 16.7, 0.5), &Workload::default(), &idle);
        assert_eq!(shown(&values, "d-loop"), idle.reason);
        assert_eq!(
            shown(&values, "d-frames"),
            "37 drawn / 253 skipped · last 4.2 s ago"
        );
        // The cadence itself is untouched: it is a real measurement of a run that
        // has ended, and the row above is what says so.
        assert_eq!(shown(&values, "d-fps"), "59.9");
        // A live loop reads the same row without the alarm.
        let live = readings(&filled(60, 16.7, 0.5), &Workload::default(), &drawing());
        assert!(!shown(&live, "d-loop").contains("IDLE"));
        assert_eq!(shown(&live, "d-frames"), "240 drawn / 0 skipped · last 0.0 s ago");
        // ...and before the first frame there is a state to print rather than a
        // blank.
        assert!(LoopState::default().reason.contains("warming up"));
        assert!(!LoopState::default().drawing);
        // The panel states the rule itself, once, beside the row.
        let notes: String = static_readings()
            .iter()
            .map(|(_, text)| text.clone())
            .collect::<Vec<String>>()
            .join(" ");
        assert!(notes.contains("draws NOTHING"), "{notes}");
        assert!(notes.contains("FORCE holds the loop open"), "{notes}");
    }

    /// The bars are the spans' shares of the measured total, and they sum to
    /// 100% because the three spans *are* the frame.
    #[test]
    fn the_bars_are_each_spans_share_of_the_measured_total() {
        let widths = bars(&filled(60, 16.7, 0.5));
        let total: f64 = widths.iter().map(|(_, share)| share).sum();
        assert!((total - 100.0).abs() < 0.1, "{widths:?}");
        widths
            .iter()
            .for_each(|(_, share)| assert!((share - 33.33).abs() < 0.1));
    }

    /// A bar can never draw past its track, whatever the clock's granularity
    /// makes of a span.
    #[test]
    fn a_bar_is_clamped_to_its_track() {
        assert_eq!(share(9.0, 4.0), 100.0);
        assert_eq!(share(0.0, 0.0), 0.0);
    }

    /// The sparkline is two point strings, bounded by [`SPARK`] samples however
    /// long the window is, and a hitch shows up as a spike that reaches the top
    /// of the box.
    #[test]
    fn the_sparkline_is_two_bounded_areas_and_shows_the_hitch() {
        let mut history = filled(WINDOW, 16.7, 0.5);
        history.push(sample(90.0, 0.5, 0.5, 0.5));
        let (gap_area, main_area) = spark(&history);
        // One point per sample, plus the two baseline corners that close it.
        assert_eq!(gap_area.split_whitespace().count(), SPARK + 2);
        assert_eq!(main_area.split_whitespace().count(), SPARK + 2);
        // The hitch is the newest sample, so the last plotted point sits at the
        // very top of the box (y = 0) while the steady frames sit low.
        assert!(gap_area.contains(",0.0"), "{gap_area}");
        // The main-thread area stays pinned near the baseline: 1.55 ms of a
        // 33 ms full scale is a pixel and a half of a 34-pixel box.
        assert!(main_area.contains(",32.4"), "{main_area}");
        // And the ceiling follows the p95, so one 90 ms outlier clips instead
        // of flattening every real frame against the floor.
        assert!(spark_ceiling(&history.stat(|s| s.gap_ms)) < 40.0);
    }

    /// The panel reports its own cost, and escalates to a warning when it grows
    /// past the budget it was written to.
    #[test]
    fn the_panel_reports_and_polices_its_own_cost() {
        assert!(panel_cost_note(&Stat {
            p50: 0.05,
            ..Stat::default()
        })
        .contains("inside its 0.2 ms/frame budget"));
        assert!(panel_cost_note(&Stat {
            p50: 0.9,
            ..Stat::default()
        })
        .contains("WARNING"));
    }

    /// **The oversample number, which is the whole phone question in one line.**
    /// A 1280x640 backbuffer inside a 380x190 CSS box on a DPR-3 phone is
    /// shading ~1.26 pixels for every pixel the screen can show.
    #[test]
    fn the_sample_ratio_is_backbuffer_pixels_per_device_pixel() {
        let phone = Workload {
            backbuffer: (1280, 640),
            css: (380.0, 190.0),
            dpr: 3.0,
            ..Workload::default()
        };
        assert!(
            (sample_ratio(&phone) - 1.261).abs() < 0.01,
            "{}",
            sample_ratio(&phone)
        );
        let exact = Workload {
            backbuffer: (1280, 640),
            css: (1280.0, 640.0),
            dpr: 1.0,
            ..Workload::default()
        };
        assert!((sample_ratio(&exact) - 1.0).abs() < 1e-9);
        // A canvas with no laid-out box reports zero, never an infinity.
        assert_eq!(sample_ratio(&Workload::default()), 0.0);
    }
}
