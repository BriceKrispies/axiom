//! **The diagnostics export** — everything the panel knows, as one JSON object.
//!
//! The phone is the only ground truth this investigation has. Chrome's device
//! emulation does not emulate a phone GPU, so an emulated run and a real one
//! disagree, and the disagreement is exactly where the answer lives. But a phone
//! is also the one machine an engineer cannot look at: there is no DevTools
//! panel to read over somebody's shoulder, no console to scroll, and a
//! screenshot of a diagnostics panel is a picture of numbers rather than the
//! numbers.
//!
//! So the panel has an export. One tap produces this object; it goes to the
//! clipboard and to the console, and it carries **every** fact the page holds —
//! the measured distribution of the frame gap and of each CPU span, the
//! workload, the backend `wgpu` actually bound, the backbuffer against the
//! screen, the capability profile, what the frame could not honour, which levers
//! are pulled, and what each station's flattened graph costs per pixel. A run
//! that produced a surprising number can then be *compared* rather than
//! described.
//!
//! ## Why the station costs are in here
//!
//! Station 1 halves the frame rate when it fills the screen, and the reason is
//! not visible on the panel: its flattened graph is ~300 nodes per pixel, of
//! which most compute constants that were never folded. `Surface::flatten` is
//! the same call the barrier makes, so the counts below are the counts the WGSL
//! emitter really saw — not an estimate of them. They are computed **on the
//! export**, never per frame: flattening eleven surfaces once cost this app 67%
//! of its frame before `ba4024dd` hoisted it out of the loop, and a diagnostics
//! panel is not allowed to put it back.
//!
//! ## Everything here is portable
//!
//! This module names no browser API: it takes the history, the workload and the
//! lever state as values and returns a string. `crate::web` supplies the three
//! and hands the result to the page. That is what lets the tests below assert on
//! the real export rather than on a mock of it.

use crate::diagnostics::{sample_ratio, FrameHistory, FrameSample, Stat, Verdict, Workload, WINDOW};
use crate::label::{LINES, STATION_OF_SLOT};
use crate::levers::{Levers, BODY_COUNT, BODY_SURFACE};
use crate::stations::all_surfaces;

use axiom_surface::{Surface, SurfaceChannel};

/// The channel names the export prints, indexed by `SurfaceChannel`.
const CHANNEL_NAMES: [&str; 7] = [
    "base_color",
    "roughness",
    "metallic",
    "normal",
    "emission",
    "opacity",
    "displacement",
];

/// **What one body's material costs**, read off its flattened surface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StationCost {
    /// The body's number on the stand, `1..=12`.
    pub body: usize,
    /// The station it belongs to.
    pub station: u8,
    /// Its caption's first line — the name a viewer reads on screen.
    pub name: &'static str,
    /// Its surface's structural digest, or `None` for the baked body.
    pub digest: Option<u64>,
    /// Per channel: the name, the flattened node count, and whether the channel
    /// is bound to a plain constant.
    ///
    /// **A channel bound to a constant that still reports dozens of nodes is
    /// the finding.** `Mix(a, b, mask)` is not folded when `a == b`, so a
    /// layered material's `Metallic`, `Normal`, `Emission`, `Opacity` and
    /// `Displacement` each carry a full composition tree that evaluates to a
    /// constant on every pixel.
    pub channels: Vec<(&'static str, usize, bool)>,
    /// Nodes the fragment stage evaluates per pixel — every channel but
    /// `Displacement`.
    pub fragment_nodes: usize,
    /// Nodes the vertex stage evaluates per vertex — the `Displacement` channel.
    pub vertex_nodes: usize,
}

impl StationCost {
    /// Every node this body's material evaluates, both stages.
    pub fn total_nodes(&self) -> usize {
        self.fragment_nodes + self.vertex_nodes
    }
}

/// One body's cost, from the surface it wears.
fn cost_of(body: usize, surface: Option<&Surface>) -> StationCost {
    let flattened = surface.and_then(|s| s.flatten().ok());
    let channels: Vec<(&'static str, usize, bool)> = flattened
        .as_ref()
        .map(|flat| {
            SurfaceChannel::ALL
                .iter()
                .map(|channel| {
                    let binding = flat.binding(*channel);
                    (
                        CHANNEL_NAMES[*channel as usize],
                        binding.as_graph().node_count(),
                        binding.is_constant(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let displacement = CHANNEL_NAMES[SurfaceChannel::Displacement as usize];
    StationCost {
        body: body + 1,
        station: STATION_OF_SLOT[body],
        name: LINES[body][0],
        digest: surface.map(|s| s.digest().raw()),
        fragment_nodes: channels
            .iter()
            .filter(|(name, _, _)| *name != displacement)
            .map(|(_, nodes, _)| nodes)
            .sum(),
        vertex_nodes: channels
            .iter()
            .filter(|(name, _, _)| *name == displacement)
            .map(|(_, nodes, _)| nodes)
            .sum(),
        channels,
    }
}

/// **What every body on the stand costs per pixel**, in slot order.
///
/// Computed by flattening each authored surface — the same call the preparation
/// barrier makes — so these are the graphs the WGSL emitter really saw.
pub fn station_costs() -> Vec<StationCost> {
    let surfaces = all_surfaces();
    (0..BODY_COUNT)
        .map(|body| cost_of(body, BODY_SURFACE[body].and_then(|index| surfaces.get(index))))
        .collect()
}

/// A JSON string literal, with the two characters that can break one escaped.
fn quoted(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

/// A distribution, as a JSON object.
fn stat_json(stat: &Stat) -> String {
    format!(
        "{{\"p05\":{:.3},\"p50\":{:.3},\"p95\":{:.3},\"max\":{:.3}}}",
        stat.p05, stat.p50, stat.p95, stat.max
    )
}

/// One station's cost, as a JSON object.
fn cost_json(cost: &StationCost) -> String {
    let channels: Vec<String> = cost
        .channels
        .iter()
        .map(|(name, nodes, constant)| {
            format!(
                "{{\"channel\":{},\"nodes\":{nodes},\"constant\":{constant}}}",
                quoted(name)
            )
        })
        .collect();
    format!(
        "{{\"body\":{},\"station\":{},\"name\":{},\"digest\":{},\"fragment_nodes\":{},\
         \"vertex_nodes\":{},\"total_nodes\":{},\"channels\":[{}]}}",
        cost.body,
        cost.station,
        quoted(cost.name),
        cost.digest
            .map(|digest| quoted(&format!("{digest:016X}")))
            .unwrap_or_else(|| "null".to_string()),
        cost.fragment_nodes,
        cost.vertex_nodes,
        cost.total_nodes(),
        channels.join(","),
    )
}

/// **The whole export.** Everything the panel knows, in one object.
///
/// `captured_ms` is the page clock at the moment of the tap — a label on the
/// reading, never an input to anything. Nothing in this function can reach
/// `EvalContext::time`; the deterministic path does not read a wall clock and
/// this does not change that.
pub fn diagnostics_json(
    history: &FrameHistory,
    workload: &Workload,
    levers: &Levers,
    captured_ms: f64,
) -> String {
    let gap = history.stat(|s| s.gap_ms);
    let cpu = history.stat(FrameSample::cpu_ms);
    let main = history.stat(FrameSample::main_ms);
    let residual = history.stat(FrameSample::residual_ms);
    let costs: Vec<String> = station_costs().iter().map(cost_json).collect();
    format!(
        "{{\"app\":\"shader-crucible\",\"captured_ms\":{captured_ms:.1},\
         \"frames_in_window\":{frames},\"window\":{WINDOW},\
         \"levers\":{levers_json},\
         \"frame\":{{\"fps_p50\":{fps:.2},\"gap_ms\":{gap_json},\"main_ms\":{main_json},\
         \"cpu_ms\":{cpu_json},\"residual_ms\":{residual_json},\"verdict\":{verdict}}},\
         \"spans_p50_ms\":{{\"app_render\":{render:.3},\"packet_of\":{packet:.3},\
         \"present\":{present:.3},\"panel\":{panel:.3}}},\
         \"workload\":{{\"backend\":{backend},\"draws\":{draws},\"batches\":{batches},\
         \"programs_used\":{programs},\"triangles\":{triangles},\"lights\":{lights},\
         \"backbuffer\":[{bw},{bh}],\"render_target_tier\":[{rw},{rh}],\
         \"render_scale_asked\":{rscale:.4},\
         \"css\":[{cw:.1},{chh:.1}],\"dpr\":{dpr:.3},\"sample_ratio\":{sample:.4},\
         \"prepared_programs\":{prepared_programs},\"prepared_surfaces\":{prepared_surfaces},\
         \"capability_profile\":{profile},\"degraded\":{degraded}}},\
         \"gpu\":{{\"available\":{gpu_available},\"reason\":{gpu_reason},\
         \"resolved_frame\":{gpu_frame},\"total_ms\":{gpu_total},\"passes\":{{{gpu_passes}}}}},\
         \"stations\":[{stations}]}}",
        frames = history.len(),
        levers_json = levers.state_json(),
        fps = 1000.0 / gap.p50.max(f64::MIN_POSITIVE),
        gap_json = stat_json(&gap),
        main_json = stat_json(&main),
        cpu_json = stat_json(&cpu),
        residual_json = stat_json(&residual),
        verdict = quoted(Verdict::of(history).headline()),
        render = history.stat(|s| s.render_ms).p50,
        packet = history.stat(|s| s.packet_ms).p50,
        present = history.stat(|s| s.present_ms).p50,
        panel = history.stat(|s| s.panel_ms).p50,
        backend = quoted(&workload.backend),
        draws = workload.draws,
        batches = workload.batches,
        programs = workload.programs_used,
        triangles = workload.triangles,
        lights = workload.lights,
        bw = workload.backbuffer.0,
        bh = workload.backbuffer.1,
        rw = workload.render_target.0,
        rh = workload.render_target.1,
        rscale = workload.render_scale,
        cw = workload.css.0,
        chh = workload.css.1,
        dpr = workload.dpr,
        sample = sample_ratio(workload),
        prepared_programs = workload.prepared_programs,
        prepared_surfaces = workload.prepared_surfaces,
        profile = quoted(&workload.profile),
        degraded = quoted(&workload.degraded),
        // The GPU reading travels as `available` plus a reason, never as a
        // number that might be a zero standing in for a missing measurement.
        // `passes` is an object keyed by pass name, and a pass the frame did not
        // run is simply absent from it.
        gpu_available = workload.gpu_available,
        gpu_reason = quoted(&workload.gpu_reason),
        gpu_frame = workload.gpu_frame,
        gpu_total = workload
            .gpu_available
            .then(|| format!("{:.4}", workload.gpu_total_ms))
            .unwrap_or_else(|| "null".to_string()),
        gpu_passes = workload
            .gpu_passes
            .iter()
            .map(|(name, ms)| format!("{}:{ms:.4}", quoted(name)))
            .collect::<Vec<String>>()
            .join(","),
        stations = costs.join(","),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::crucible_core;

    fn a_window() -> FrameHistory {
        let mut history = FrameHistory::new();
        (0..60).for_each(|_| {
            history.push(FrameSample {
                gap_ms: 33.3,
                render_ms: 1.0,
                packet_ms: 0.5,
                present_ms: 3.0,
                panel_ms: 0.05,
            })
        });
        history
    }

    fn a_workload() -> Workload {
        Workload {
            draws: 25,
            batches: 25,
            programs_used: 11,
            triangles: 11_182,
            lights: 2,
            backbuffer: (1122, 561),
            render_target: (1122, 561),
            render_scale: 1.0,
            css: (374.0, 187.0),
            dpr: 3.0,
            prepared_programs: 11,
            prepared_surfaces: 11,
            profile: "gpu/all".to_string(),
            degraded: "none".to_string(),
            backend: "GpuFallback".to_string(),
            gpu_available: false,
            gpu_reason: "the adapter offers no timestamp query".to_string(),
            gpu_passes: Vec::new(),
            gpu_total_ms: 0.0,
            gpu_frame: 0,
        }
    }

    /// **The map from a body to the surface it wears is the frame's own.**
    ///
    /// Checked against the `surface_program` the scene really puts on each body
    /// draw, so a station reordered in `stand::populate` fails here instead of
    /// attributing station 4's node count to station 5's body.
    #[test]
    fn the_body_to_surface_map_is_the_frames_own() {
        let (mut app, _) = crucible_core();
        let outcome = app.render(0);
        let digests: Vec<u64> = all_surfaces().iter().map(|s| s.digest().raw()).collect();
        // Draw 0 is the ground; draws 1..=12 are the bodies in slot order.
        (0..BODY_COUNT).for_each(|body| {
            let program = outcome.draws()[body + 1].surface_program();
            let expected = BODY_SURFACE[body].map(|index| digests[index]).unwrap_or(0);
            assert_eq!(
                program, expected,
                "body {} wears the wrong surface",
                body + 1
            );
        });
    }

    /// **The finding this export exists to carry, and the fix it drove.**
    ///
    /// Body 1 — the layered metal+paint that halved the frame rate when it
    /// filled the screen — used to flatten to **262 fragment + 39 vertex**
    /// nodes, and four of its seven channels were full composition trees
    /// computing a value that never varied: every surface in the tree bound them
    /// to the *same* plain constant, but the layer masks are fields, so
    /// `Mix(c, c, mask)` had a non-constant input and constant folding could not
    /// reach it.
    ///
    /// `axiom_field`'s exact algebraic identity `Mix(x, x, t) -> x` reaches
    /// exactly that node. The four channels are constant **bindings** again and
    /// the pins below are the measurement. `metallic` is deliberately not among
    /// them: its four surfaces bind `0.0 / 1.0 / 0.0 / 1.0`, so it genuinely
    /// varies with the masks and its graph is real work.
    #[test]
    fn the_layered_body_costs_only_the_channels_that_genuinely_vary() {
        let costs = station_costs();
        let layered = &costs[0];
        assert_eq!(layered.body, 1);
        assert_eq!(layered.station, 1);
        assert_eq!(
            layered.channels,
            vec![
                ("base_color", 58, false),
                ("roughness", 49, false),
                ("metallic", 39, false),
                ("normal", 1, true),
                ("emission", 1, true),
                ("opacity", 1, true),
                ("displacement", 1, true),
            ],
            "station 1's flattened per-channel cost moved"
        );
        assert_eq!(layered.fragment_nodes, 149, "was 262 before the identity");
        assert_eq!(layered.vertex_nodes, 1, "was 39 before the identity");
        // ...and body 1 is still genuinely the most expensive body on the stand,
        // which is why solo-ing it is the experiment.
        let heaviest = costs
            .iter()
            .max_by_key(|cost| cost.fragment_nodes)
            .expect("twelve bodies");
        assert_eq!(heaviest.body, 1, "the heaviest body moved");
    }

    /// The baked body carries no graph at all, and says so rather than reporting
    /// a zero that reads as "free".
    #[test]
    fn the_baked_body_has_no_per_pixel_graph() {
        let baked = &station_costs()[2];
        assert_eq!(baked.station, 3);
        assert_eq!(baked.digest, None);
        assert!(baked.channels.is_empty());
        assert_eq!(baked.total_nodes(), 0);
    }

    /// Every body on the stand is costed, and every station with a body appears.
    #[test]
    fn every_body_is_costed() {
        let costs = station_costs();
        assert_eq!(costs.len(), BODY_COUNT);
        costs.iter().enumerate().for_each(|(slot, cost)| {
            assert_eq!(cost.body, slot + 1);
            assert_eq!(cost.name, LINES[slot][0]);
        });
        let stations: std::collections::BTreeSet<u8> =
            costs.iter().map(|cost| cost.station).collect();
        assert_eq!(stations, (1_u8..=8).collect());
    }

    /// **The export is complete**: every category the panel shows is in it, and
    /// the lever state comes along so a number can be attributed to a
    /// configuration rather than floating free.
    #[test]
    fn the_export_carries_everything_the_panel_knows() {
        let json = diagnostics_json(&a_window(), &a_workload(), &Levers::SHIPPING, 1234.5);
        [
            "\"levers\":",
            "\"frame\":",
            "\"gap_ms\":",
            "\"residual_ms\":",
            "\"verdict\":",
            "\"spans_p50_ms\":",
            "\"workload\":",
            "\"backend\":\"GpuFallback\"",
            "\"capability_profile\":\"gpu/all\"",
            "\"degraded\":\"none\"",
            "\"backbuffer\":[1122,561]",
            "\"dpr\":3.000",
            "\"stations\":[",
            "\"fragment_nodes\":",
            "\"constant\":",
            "\"gpu\":{\"available\":false",
        ]
        .iter()
        .for_each(|needle| assert!(json.contains(needle), "missing {needle} in {json}"));
        // Balanced braces and brackets — the cheapest proof available natively
        // that what reaches the clipboard is one parseable object.
        let count = |c: char| json.chars().filter(|x| *x == c).count();
        assert_eq!(count('{'), count('}'));
        assert_eq!(count('['), count(']'));
        assert!(json.starts_with('{') && json.ends_with('}'));
    }

    /// **The export never invents a GPU time.** An unmeasurable GPU travels as
    /// `available: false` plus the engine's own reason and a `null` total — never
    /// as a zero, which downstream arithmetic would happily average.
    #[test]
    fn the_export_refuses_to_invent_a_gpu_time() {
        let json = diagnostics_json(&a_window(), &a_workload(), &Levers::SHIPPING, 0.0);
        assert!(json.contains("\"available\":false"));
        assert!(json.contains("\"total_ms\":null"));
        assert!(json.contains("no timestamp query"));
        assert!(json.contains("\"passes\":{}"));
    }

    /// A measured GPU travels pass by pass, with the frame the reading belongs
    /// to — which is never the frame on screen.
    #[test]
    fn a_measured_gpu_travels_pass_by_pass() {
        let measured = Workload {
            gpu_available: true,
            gpu_passes: vec![("shadow".to_string(), 19.25), ("main".to_string(), 8.4)],
            gpu_total_ms: 27.65,
            gpu_frame: 412,
            ..a_workload()
        };
        let json = diagnostics_json(&a_window(), &measured, &Levers::SHIPPING, 0.0);
        assert!(json.contains("\"available\":true"));
        assert!(json.contains("\"resolved_frame\":412"));
        assert!(json.contains("\"shadow\":19.2500"));
        assert!(json.contains("\"total_ms\":27.6500"));
    }

    /// The levers travel with the reading, so two exports can be diffed and the
    /// difference attributed.
    #[test]
    fn the_export_names_which_levers_were_pulled() {
        let pulled = Levers {
            captions: false,
            solo: Some(0),
            ..Levers::SHIPPING
        };
        let json = diagnostics_json(&a_window(), &a_workload(), &pulled, 0.0);
        assert!(json.contains("\"captions\":false"));
        assert!(json.contains("\"solo\":1"));
        assert!(json.contains("\"shipping\":false"));
    }

    /// A string with a quote in it does not break the object. Nothing the app
    /// authors contains one today; an engine that starts reporting a degradation
    /// with a quote in its `Debug` should not silently produce broken JSON.
    #[test]
    fn a_quoted_value_is_escaped() {
        assert_eq!(quoted("a\"b\\c"), "\"a\\\"b\\\\c\"");
    }
}
