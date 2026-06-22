//! The profiler's data model and report serialization.
//!
//! This module is deliberately **pure**: it knows nothing about the Axiom
//! engine, wall-clock time, or the filesystem. It accumulates already-measured
//! nanosecond durations into named phases (each of which may carry child
//! subphases), computes the report aggregates (averages, percentages, frame
//! statistics), and serializes the result to JSON, Markdown, and CSV.
//!
//! Keeping it engine-free is what lets the percentage and roll-up math be
//! unit-tested in isolation. The serializers are hand-written so the tool pulls
//! in no external crates; the report is a flat structure of integers, strings,
//! and string arrays, for which a dependency would be overkill.

/// Phase `kind` tags. They classify how honest a phase's numbers are.
pub mod kind {
    /// Times genuine engine code through public facades (e.g. `axiom-render`).
    pub const REAL_ENGINE: &str = "real_engine";
    /// Times a faithful harness reconstruction of an engine algorithm whose
    /// real implementation is a single opaque call that cannot be instrumented
    /// from outside without modifying engine code (see `transform_update`).
    pub const REAL_ENGINE_MODEL: &str = "real_engine_model";
    /// Times work for a capability Axiom does not own as an engine system yet.
    pub const PLACEHOLDER: &str = "placeholder";
    /// Times harness/tooling work (setup, report serialization).
    pub const HARNESS: &str = "harness";
}

/// A single child measurement nested under a [`Phase`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subphase {
    pub name: String,
    pub total_ns: u128,
    pub sample_count: u64,
}

impl Subphase {
    pub fn new(name: impl Into<String>) -> Self {
        Subphase {
            name: name.into(),
            total_ns: 0,
            sample_count: 0,
        }
    }

    pub fn record(&mut self, ns: u128) {
        self.total_ns += ns;
        self.sample_count += 1;
    }

    /// Mean duration per recorded sample, or `0` when nothing was recorded.
    pub fn average_ns(&self) -> u128 {
        match self.sample_count {
            0 => 0,
            n => self.total_ns / u128::from(n),
        }
    }
}

/// A named measurement phase, optionally decomposed into ordered subphases.
///
/// A *leaf* phase (no subphases) accumulates its time directly via
/// [`Phase::record`]. A *composite* phase records into its subphases via
/// [`Phase::record_subphase`] and then has its own `total_ns` defined as the
/// sum of its subphases by [`Phase::finalize_from_subphases`]; this gives an
/// exact roll-up so subphase percentages add to 100% of the parent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Phase {
    pub name: String,
    pub kind: String,
    pub total_ns: u128,
    pub sample_count: u64,
    pub subphases: Vec<Subphase>,
}

impl Phase {
    pub fn new(name: impl Into<String>, kind: impl Into<String>) -> Self {
        Phase {
            name: name.into(),
            kind: kind.into(),
            total_ns: 0,
            sample_count: 0,
            subphases: Vec::new(),
        }
    }

    /// Record one duration directly against a leaf phase.
    pub fn record(&mut self, ns: u128) {
        self.total_ns += ns;
        self.sample_count += 1;
    }

    /// Record one duration against the named subphase, creating it (in first-
    /// seen order) if absent.
    pub fn record_subphase(&mut self, name: &str, ns: u128) {
        match self.subphases.iter_mut().find(|s| s.name == name) {
            Some(existing) => existing.record(ns),
            None => {
                let mut sub = Subphase::new(name);
                sub.record(ns);
                self.subphases.push(sub);
            }
        }
    }

    /// Define this composite phase's `total_ns` as the exact sum of its
    /// subphases, and stamp its per-iteration `sample_count`.
    pub fn finalize_from_subphases(&mut self, sample_count: u64) {
        self.total_ns = self.subphases.iter().map(|s| s.total_ns).sum();
        self.sample_count = sample_count;
    }

    pub fn average_ns(&self) -> u128 {
        match self.sample_count {
            0 => 0,
            n => self.total_ns / u128::from(n),
        }
    }
}

/// The per-measured-iteration durations (a "frame" in `full` mode, or one
/// focused-phase iteration in a focus mode).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FrameTimings {
    pub durations_ns: Vec<u128>,
}

impl FrameTimings {
    pub fn new() -> Self {
        FrameTimings::default()
    }

    pub fn record(&mut self, ns: u128) {
        self.durations_ns.push(ns);
    }

    pub fn count(&self) -> u64 {
        self.durations_ns.len() as u64
    }

    pub fn total_ns(&self) -> u128 {
        self.durations_ns.iter().sum()
    }

    pub fn average_ns(&self) -> u128 {
        match self.durations_ns.len() {
            0 => 0,
            n => self.total_ns() / (n as u128),
        }
    }

    pub fn worst_ns(&self) -> u128 {
        self.durations_ns.iter().copied().max().unwrap_or(0)
    }

    pub fn best_ns(&self) -> u128 {
        self.durations_ns.iter().copied().min().unwrap_or(0)
    }

    pub fn over(&self, threshold_ns: u128) -> u64 {
        self.durations_ns
            .iter()
            .filter(|&&d| d > threshold_ns)
            .count() as u64
    }
}

/// 60 FPS frame budget, in nanoseconds (1s / 60).
pub const BUDGET_60_FPS_NS: u128 = 16_666_667;
/// 30 FPS frame budget, in nanoseconds (1s / 30).
pub const BUDGET_30_FPS_NS: u128 = 33_333_333;

/// Counters quantifying the per-iteration churn the harness drives — repeated
/// work (allocations, clones, pushes) that the stress loop performs every
/// measured iteration. They make the cost the subphases time concrete.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChurnCounters {
    pub transform_scratch_maps_allocated: u64,
    pub transform_parent_lookups: u64,
    pub world_transforms_written: u64,
    pub mesh_vec_clones: u64,
    pub render_inputs_created: u64,
    pub render_objects_pushed: u64,
    pub render_command_lists_built: u64,
}

impl ChurnCounters {
    /// `(label, value)` pairs in a stable order, for serialization.
    pub fn entries(&self) -> Vec<(&'static str, u64)> {
        vec![
            (
                "transform_scratch_maps_allocated",
                self.transform_scratch_maps_allocated,
            ),
            ("transform_parent_lookups", self.transform_parent_lookups),
            ("world_transforms_written", self.world_transforms_written),
            ("mesh_vec_clones", self.mesh_vec_clones),
            ("render_inputs_created", self.render_inputs_created),
            ("render_objects_pushed", self.render_objects_pushed),
            (
                "render_command_lists_built",
                self.render_command_lists_built,
            ),
        ]
    }
}

/// The fully-assembled profile report — serialized to JSON, Markdown, and CSV
/// and summarized to the terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileReport {
    pub focus_phase: String,
    pub object_count: u64,
    pub measured_frame_count: u64,
    pub warmup_frame_count: u64,
    pub build_profile: String,
    pub total_wall_time_ns: u128,
    pub frames: FrameTimings,
    pub phases: Vec<Phase>,
    pub placeholder_phases: Vec<String>,
    pub churn: ChurnCounters,
    pub git_commit_hash: Option<String>,
    pub notes: Vec<String>,
}

impl ProfileReport {
    /// The percentage base for top-level phases: the sum of every phase's
    /// total. This is the "measured phase time".
    pub fn measured_phase_time_ns(&self) -> u128 {
        self.phases.iter().map(|p| p.total_ns).sum()
    }

    /// A top-level phase's share of the measured phase time, as a percentage.
    pub fn phase_percent(&self, phase: &Phase) -> f64 {
        percent_of(phase.total_ns, self.measured_phase_time_ns())
    }

    /// A subphase's share of its parent phase's time, as a percentage.
    pub fn subphase_percent(parent: &Phase, sub: &Subphase) -> f64 {
        percent_of(sub.total_ns, parent.total_ns)
    }

    pub fn to_json(&self) -> String {
        let phases_json: Vec<String> = self.phases.iter().map(|p| self.phase_to_json(p)).collect();
        let churn_json = json_object(
            self.churn
                .entries()
                .into_iter()
                .map(|(k, v)| (k, v.to_string())),
        );
        let git = match &self.git_commit_hash {
            Some(hash) => format!("\"{}\"", json_escape(hash)),
            None => "null".to_string(),
        };

        format!(
            "{{\n  \
             \"focus_phase\": \"{focus}\",\n  \
             \"object_count\": {object_count},\n  \
             \"measured_frame_count\": {measured},\n  \
             \"warmup_frame_count\": {warmup},\n  \
             \"build_profile\": \"{build}\",\n  \
             \"total_wall_time_ns\": {total_wall},\n  \
             \"average_measured_iteration_time_ns\": {avg_iter},\n  \
             \"worst_measured_iteration_time_ns\": {worst_iter},\n  \
             \"best_measured_iteration_time_ns\": {best_iter},\n  \
             \"frames_over_16_666_667ns\": {over_60},\n  \
             \"frames_over_33_333_333ns\": {over_30},\n  \
             \"placeholder_phases\": {placeholders},\n  \
             \"churn\": {churn},\n  \
             \"phases\": [{phases}\n  ],\n  \
             \"git_commit_hash\": {git},\n  \
             \"notes\": {notes}\n\
             }}\n",
            focus = json_escape(&self.focus_phase),
            object_count = self.object_count,
            measured = self.measured_frame_count,
            warmup = self.warmup_frame_count,
            build = json_escape(&self.build_profile),
            total_wall = self.total_wall_time_ns,
            avg_iter = self.frames.average_ns(),
            worst_iter = self.frames.worst_ns(),
            best_iter = self.frames.best_ns(),
            over_60 = self.frames.over(BUDGET_60_FPS_NS),
            over_30 = self.frames.over(BUDGET_30_FPS_NS),
            placeholders = json_string_array(&self.placeholder_phases),
            churn = churn_json,
            phases = phases_json.join(","),
            git = git,
            notes = json_string_array(&self.notes),
        )
    }

    fn phase_to_json(&self, phase: &Phase) -> String {
        let subphases: Vec<String> = phase
            .subphases
            .iter()
            .map(|s| {
                format!(
                    "\n      {{ \"name\": \"{name}\", \"total_ns\": {total}, \
                     \"average_ns\": {avg}, \"sample_count\": {count}, \
                     \"percent_of_measured_phase_time\": {pct:.6} }}",
                    name = json_escape(&s.name),
                    total = s.total_ns,
                    avg = s.average_ns(),
                    count = s.sample_count,
                    pct = ProfileReport::subphase_percent(phase, s),
                )
            })
            .collect();
        let subphases_field = match subphases.is_empty() {
            true => "[]".to_string(),
            false => format!("[{}\n    ]", subphases.join(",")),
        };
        format!(
            "\n    {{ \"name\": \"{name}\", \"kind\": \"{kind}\", \"total_ns\": {total}, \
             \"average_ns\": {avg}, \"sample_count\": {count}, \
             \"percent_of_measured_phase_time\": {pct:.6}, \"subphases\": {subs} }}",
            name = json_escape(&phase.name),
            kind = json_escape(&phase.kind),
            total = phase.total_ns,
            avg = phase.average_ns(),
            count = phase.sample_count,
            pct = self.phase_percent(phase),
            subs = subphases_field,
        )
    }

    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str("# Axiom CPU Profile Report\n\n");
        out.push_str(&format!(
            "**Focus phase: `{}`** — {}\n\n",
            self.focus_phase,
            match self.focus_phase.as_str() {
                "full" => "full per-frame loop.",
                _ =>
                    "FOCUSED phase run: only this phase's workload was measured, not a full frame.",
            }
        ));

        out.push_str("## Run\n\n");
        out.push_str(&format!("- object_count: {}\n", self.object_count));
        out.push_str(&format!(
            "- measured_frame_count: {}\n",
            self.measured_frame_count
        ));
        out.push_str(&format!(
            "- warmup_frame_count: {}\n",
            self.warmup_frame_count
        ));
        out.push_str(&format!("- build_profile: {}\n", self.build_profile));
        out.push_str(&format!(
            "- git_commit_hash: {}\n",
            self.git_commit_hash.as_deref().unwrap_or("null")
        ));
        out.push('\n');

        out.push_str("## Measured iteration timing\n\n");
        out.push_str(&format!(
            "- total_wall_time_ns: {}\n",
            self.total_wall_time_ns
        ));
        out.push_str(&format!(
            "- average_measured_iteration_time_ns: {}\n",
            self.frames.average_ns()
        ));
        out.push_str(&format!(
            "- worst_measured_iteration_time_ns: {}\n",
            self.frames.worst_ns()
        ));
        out.push_str(&format!(
            "- best_measured_iteration_time_ns: {}\n",
            self.frames.best_ns()
        ));
        out.push_str(&format!(
            "- frames_over_16_666_667ns (60 FPS): {}\n",
            self.frames.over(BUDGET_60_FPS_NS)
        ));
        out.push_str(&format!(
            "- frames_over_33_333_333ns (30 FPS): {}\n",
            self.frames.over(BUDGET_30_FPS_NS)
        ));
        out.push('\n');

        out.push_str("## Phases\n\n");
        out.push_str("| phase | kind | total_ns | average_ns | sample_count | % of measured |\n");
        out.push_str("|---|---|---:|---:|---:|---:|\n");
        for phase in &self.phases {
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} | {:.2}% |\n",
                phase.name,
                phase.kind,
                phase.total_ns,
                phase.average_ns(),
                phase.sample_count,
                self.phase_percent(phase)
            ));
            for sub in &phase.subphases {
                out.push_str(&format!(
                    "| &nbsp;&nbsp;↳ {} | | {} | {} | {} | {:.2}% of parent |\n",
                    sub.name,
                    sub.total_ns,
                    sub.average_ns(),
                    sub.sample_count,
                    ProfileReport::subphase_percent(phase, sub)
                ));
            }
        }
        out.push('\n');

        out.push_str("## Placeholder phases\n\n");
        match self.placeholder_phases.is_empty() {
            true => out.push_str("_(none in this run)_\n"),
            false => {
                for name in &self.placeholder_phases {
                    out.push_str(&format!("- `{name}`\n"));
                }
            }
        }
        out.push('\n');

        out.push_str("## Harness churn counters\n\n");
        for (label, value) in self.churn.entries() {
            out.push_str(&format!("- {label}: {value}\n"));
        }
        out.push('\n');

        out.push_str("## Notes\n\n");
        for note in &self.notes {
            out.push_str(&format!("- {note}\n"));
        }
        out
    }

    /// A flat CSV of every phase and subphase row, plus the churn counters.
    pub fn to_csv(&self) -> String {
        let mut out = String::new();
        out.push_str("section,name,kind,parent,total_ns,average_ns,sample_count,percent\n");
        for phase in &self.phases {
            out.push_str(&format!(
                "phase,{},{},,{},{},{},{:.6}\n",
                csv_field(&phase.name),
                csv_field(&phase.kind),
                phase.total_ns,
                phase.average_ns(),
                phase.sample_count,
                self.phase_percent(phase)
            ));
            for sub in &phase.subphases {
                out.push_str(&format!(
                    "subphase,{},,{},{},{},{},{:.6}\n",
                    csv_field(&sub.name),
                    csv_field(&phase.name),
                    sub.total_ns,
                    sub.average_ns(),
                    sub.sample_count,
                    ProfileReport::subphase_percent(phase, sub)
                ));
            }
        }
        for (label, value) in self.churn.entries() {
            out.push_str(&format!("churn,{label},,,{value},,,\n"));
        }
        out
    }
}

/// `numerator / denominator * 100`, or `0.0` when the denominator is zero.
fn percent_of(numerator: u128, denominator: u128) -> f64 {
    match denominator {
        0 => 0.0,
        d => (numerator as f64) / (d as f64) * 100.0,
    }
}

fn json_object<'a>(entries: impl Iterator<Item = (&'a str, String)>) -> String {
    let body: Vec<String> = entries
        .map(|(k, v)| format!("\"{}\": {}", json_escape(k), v))
        .collect();
    match body.is_empty() {
        true => "{}".to_string(),
        false => format!("{{ {} }}", body.join(", ")),
    }
}

fn json_string_array(items: &[String]) -> String {
    let body: Vec<String> = items
        .iter()
        .map(|s| format!("\"{}\"", json_escape(s)))
        .collect();
    match body.is_empty() {
        true => "[]".to_string(),
        false => format!("[{}]", body.join(", ")),
    }
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out
}

/// Quote a CSV field iff it contains a comma, quote, or newline.
fn csv_field(s: &str) -> String {
    match s.contains([',', '"', '\n']) {
        false => s.to_string(),
        true => format!("\"{}\"", s.replace('"', "\"\"")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn composite_phase(name: &str, subs: &[(&str, u128, u64)]) -> Phase {
        let mut phase = Phase::new(name, kind::REAL_ENGINE);
        for &(sub_name, total, count) in subs {
            for _ in 0..count {
                phase.record_subphase(sub_name, total / u128::from(count.max(1)));
            }
        }
        phase.finalize_from_subphases(count_iterations(subs));
        phase
    }

    /// The per-iteration sample count = the max sample_count across subphases
    /// (subphases recorded once per iteration here).
    fn count_iterations(subs: &[(&str, u128, u64)]) -> u64 {
        subs.iter().map(|&(_, _, c)| c).max().unwrap_or(0)
    }

    #[test]
    fn subphase_totals_roll_up_into_parent_total_exactly() {
        let mut phase = Phase::new("transform_update", kind::REAL_ENGINE_MODEL);
        phase.record_subphase("a", 100);
        phase.record_subphase("b", 250);
        phase.record_subphase("a", 50); // a -> 150
        phase.finalize_from_subphases(2);
        let sub_sum: u128 = phase.subphases.iter().map(|s| s.total_ns).sum();
        assert_eq!(phase.total_ns, sub_sum);
        assert_eq!(phase.total_ns, 400);
        assert_eq!(phase.sample_count, 2);
        assert_eq!(phase.average_ns(), 200);
    }

    #[test]
    fn subphase_percentages_are_correct_and_sum_to_one_hundred() {
        let phase = composite_phase(
            "render_command_build",
            &[
                ("create", 100, 1),
                ("clone", 300, 1),
                ("push", 500, 1),
                ("finalize", 100, 1),
            ],
        );
        assert_eq!(phase.total_ns, 1000);
        let pcts: Vec<f64> = phase
            .subphases
            .iter()
            .map(|s| ProfileReport::subphase_percent(&phase, s))
            .collect();
        assert_eq!(pcts[0], 10.0);
        assert_eq!(pcts[2], 50.0);
        let sum: f64 = pcts.iter().sum();
        assert!(
            (sum - 100.0).abs() < 1e-9,
            "subphase percents summed to {sum}"
        );
    }

    #[test]
    fn empty_subphase_and_phase_averages_are_zero_and_do_not_panic() {
        let phase = Phase::new("setup", kind::HARNESS);
        assert_eq!(phase.average_ns(), 0);
        let sub = Subphase::new("x");
        assert_eq!(sub.average_ns(), 0);
        // A zero-total parent yields 0% subphase shares, no divide-by-zero.
        assert_eq!(ProfileReport::subphase_percent(&phase, &sub), 0.0);
    }

    #[test]
    fn focus_phase_is_serialized_into_json() {
        let report = sample_report("transform_update");
        let json = report.to_json();
        assert!(json.contains("\"focus_phase\": \"transform_update\""));
    }

    #[test]
    fn json_includes_every_required_top_level_field() {
        let json = sample_report("full").to_json();
        for key in [
            "focus_phase",
            "object_count",
            "measured_frame_count",
            "warmup_frame_count",
            "build_profile",
            "average_measured_iteration_time_ns",
            "placeholder_phases",
            "churn",
            "phases",
            "git_commit_hash",
            "notes",
        ] {
            assert!(json.contains(&format!("\"{key}\"")), "missing key {key}");
        }
        // Phase objects carry kind + subphases.
        assert!(json.contains("\"kind\""));
        assert!(json.contains("\"subphases\""));
        assert!(json.contains("\"percent_of_measured_phase_time\""));
    }

    #[test]
    fn markdown_includes_indented_subphase_rows() {
        let md = sample_report("full").to_markdown();
        assert!(md.contains("↳ create"));
        assert!(md.contains("% of parent"));
        // Focus banner present.
        assert!(md.contains("Focus phase: `full`"));
    }

    #[test]
    fn markdown_lists_placeholder_phases_and_churn() {
        let md = sample_report("full").to_markdown();
        assert!(md.contains("## Placeholder phases"));
        assert!(md.contains("`bounds_update_placeholder`"));
        assert!(md.contains("## Harness churn counters"));
        assert!(md.contains("mesh_vec_clones: 4"));
    }

    #[test]
    fn csv_has_phase_and_subphase_and_churn_rows() {
        let csv = sample_report("full").to_csv();
        assert!(
            csv.starts_with("section,name,kind,parent,total_ns,average_ns,sample_count,percent\n")
        );
        assert!(csv.contains("phase,render_command_build,real_engine,,"));
        assert!(csv.contains("subphase,create,,render_command_build,"));
        assert!(csv.contains("churn,mesh_vec_clones,,,4,,,"));
    }

    #[test]
    fn frame_timings_derive_stats_and_over_budget_counts() {
        let mut frames = FrameTimings::new();
        frames.record(40_000_000);
        frames.record(20_000_000);
        frames.record(1_000_000);
        assert_eq!(frames.count(), 3);
        assert_eq!(frames.average_ns(), 20_333_333);
        assert_eq!(frames.worst_ns(), 40_000_000);
        assert_eq!(frames.best_ns(), 1_000_000);
        assert_eq!(frames.over(BUDGET_60_FPS_NS), 2);
        assert_eq!(frames.over(BUDGET_30_FPS_NS), 1);
    }

    fn sample_report(focus: &str) -> ProfileReport {
        let render = composite_phase(
            "render_command_build",
            &[
                ("create", 100, 1),
                ("clone", 300, 1),
                ("push", 500, 1),
                ("finalize", 100, 1),
            ],
        );
        let mut bounds = Phase::new("bounds_update_placeholder", kind::PLACEHOLDER);
        bounds.record(200);
        let mut frames = FrameTimings::new();
        frames.record(5_000_000);
        frames.record(6_000_000);
        ProfileReport {
            focus_phase: focus.to_string(),
            object_count: 25_000,
            measured_frame_count: 2,
            warmup_frame_count: 1,
            build_profile: "debug".to_string(),
            total_wall_time_ns: frames.total_ns(),
            frames,
            phases: vec![render, bounds],
            placeholder_phases: vec!["bounds_update_placeholder".to_string()],
            churn: ChurnCounters {
                mesh_vec_clones: 4,
                ..ChurnCounters::default()
            },
            git_commit_hash: None,
            notes: vec!["a note".to_string()],
        }
    }
}
