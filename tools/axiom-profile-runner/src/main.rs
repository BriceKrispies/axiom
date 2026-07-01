//! `axiom-profile-runner` — native CPU profiling for Axiom's stress scenario,
//! now with per-subphase breakdown and focus modes.
//!
//! One command runs a deterministic native stress workload (full frame, or a
//! single focused phase) and emits a JSON + Markdown (+ optional CSV) report
//! plus a terminal summary showing where CPU time went across named phases and
//! subphases. See `README.md` for what this does and does not measure.
//!
//! This is repo **tooling**, and is the only place — together with
//! `scenario.rs` — allowed to read the wall clock.

mod report;
mod scenario;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use report::{kind, ChurnCounters, Phase, ProfileReport};
use scenario::{FocusPhase, ScenarioConfig};

const DEFAULT_OBJECTS: u64 = 25_000;
const DEFAULT_FRAMES: u64 = 600;
const DEFAULT_WARMUP: u64 = 0;
const DEFAULT_OUT: &str = "target/axiom-profile/latest";

const JSON_FILE: &str = "profile-report.json";
const MD_FILE: &str = "profile-report.md";
const CSV_FILE: &str = "profile-report.csv";

/// Parsed command-line configuration.
#[derive(Debug, PartialEq, Eq)]
struct Config {
    objects: u64,
    frames: u64,
    warmup: u64,
    focus: FocusPhase,
    csv: bool,
    out: PathBuf,
}

fn main() {
    let config = match parse_args(std::env::args().skip(1)) {
        Ok(config) => config,
        Err(message) => {
            eprintln!("error: {message}\n");
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    };

    let run_start = Instant::now();
    let outcome = scenario::run(ScenarioConfig {
        object_count: config.objects,
        measured_frames: config.frames,
        warmup_frames: config.warmup,
        focus: config.focus,
    });

    let mut report = ProfileReport {
        focus_phase: config.focus.as_str().to_string(),
        object_count: outcome.object_count,
        measured_frame_count: outcome.frames.count(),
        warmup_frame_count: config.warmup,
        build_profile: build_profile().to_string(),
        total_wall_time_ns: 0,
        frames: outcome.frames,
        phases: outcome.phases,
        placeholder_phases: outcome.placeholder_phases,
        churn: outcome.churn,
        git_commit_hash: git_commit_hash(),
        notes: outcome.notes,
    };

    // Measure serializing the report as the `report_write` phase, then re-
    // serialize the final report (now including that phase) for writing.
    let write_start = Instant::now();
    let _ = report.to_json();
    let _ = report.to_markdown();
    if config.csv {
        let _ = report.to_csv();
    }
    let report_write_ns = write_start.elapsed().as_nanos();
    let mut report_write = Phase::new("report_write", kind::HARNESS);
    report_write.record(report_write_ns);
    report.phases.push(report_write);

    report.total_wall_time_ns = run_start.elapsed().as_nanos();

    let json = report.to_json();
    let md = report.to_markdown();
    let json_path = config.out.join(JSON_FILE);
    let md_path = config.out.join(MD_FILE);
    let csv_path = config.out.join(CSV_FILE);
    if let Err(message) = write_reports(
        &config, &report, &json_path, &json, &md_path, &md, &csv_path,
    ) {
        eprintln!("error: {message}");
        std::process::exit(1);
    }

    print_summary(&report, &config, &json_path, &md_path, &csv_path);
}

const USAGE: &str = "\
usage: axiom-profile-runner [--objects N] [--frames N] [--warmup-frames N]
                            [--focus-phase full|transform_update|render_command_build]
                            [--csv] [--out DIR]

  --objects N        number of stress objects (default 25000)
  --frames N         measured frames / iterations (default 600)
  --warmup-frames N  unmeasured warmup iterations before measuring (default 0)
  --focus-phase P    full (default), transform_update, or render_command_build
  --csv              also write profile-report.csv
  --out DIR          output directory (default target/axiom-profile/latest)";

fn parse_args(args: impl Iterator<Item = String>) -> Result<Config, String> {
    let mut objects = DEFAULT_OBJECTS;
    let mut frames = DEFAULT_FRAMES;
    let mut warmup = DEFAULT_WARMUP;
    let mut focus = FocusPhase::Full;
    let mut csv = false;
    let mut out = PathBuf::from(DEFAULT_OUT);

    let mut args = args;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--objects" => objects = parse_u64("--objects", args.next())?,
            "--frames" => frames = parse_u64("--frames", args.next())?,
            "--warmup-frames" => warmup = parse_u64("--warmup-frames", args.next())?,
            "--focus-phase" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--focus-phase requires a value".to_string())?;
                focus = FocusPhase::parse(&value)?;
            }
            "--csv" => csv = true,
            "--out" => {
                out = args
                    .next()
                    .map(PathBuf::from)
                    .ok_or_else(|| "--out requires a directory path".to_string())?;
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            other => return Err(format!("unrecognized argument: {other}")),
        }
    }

    Ok(Config {
        objects,
        frames,
        warmup,
        focus,
        csv,
        out,
    })
}

fn parse_u64(flag: &str, value: Option<String>) -> Result<u64, String> {
    let raw = value.ok_or_else(|| format!("{flag} requires a number"))?;
    raw.parse::<u64>()
        .map_err(|_| format!("{flag} expects a non-negative integer, got `{raw}`"))
}

#[allow(clippy::too_many_arguments)]
fn write_reports(
    config: &Config,
    report: &ProfileReport,
    json_path: &Path,
    json: &str,
    md_path: &Path,
    md: &str,
    csv_path: &Path,
) -> Result<(), String> {
    std::fs::create_dir_all(&config.out).map_err(|e| {
        format!(
            "could not create output directory {}: {e}",
            config.out.display()
        )
    })?;
    std::fs::write(json_path, json)
        .map_err(|e| format!("could not write {}: {e}", json_path.display()))?;
    std::fs::write(md_path, md)
        .map_err(|e| format!("could not write {}: {e}", md_path.display()))?;
    if config.csv {
        std::fs::write(csv_path, report.to_csv())
            .map_err(|e| format!("could not write {}: {e}", csv_path.display()))?;
    }
    Ok(())
}

fn build_profile() -> &'static str {
    match cfg!(debug_assertions) {
        true => "debug",
        false => "release",
    }
}

fn git_commit_hash() -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;
    match output.status.success() {
        true => {
            let hash = String::from_utf8_lossy(&output.stdout).trim().to_string();
            (!hash.is_empty()).then_some(hash)
        }
        false => None,
    }
}

fn print_summary(
    report: &ProfileReport,
    config: &Config,
    json_path: &Path,
    md_path: &Path,
    csv_path: &Path,
) {
    println!("axiom-profile-runner — native CPU profile");
    println!("  focus_phase={}", report.focus_phase);
    let mode = match report.focus_phase.as_str() {
        "full" => "full per-frame loop",
        _ => "FOCUSED phase run (only this phase measured, not a full frame)",
    };
    println!("  mode: {mode}");
    println!(
        "  object_count={}  measured_frames={}  warmup_frames={}  profile={}",
        report.object_count,
        report.measured_frame_count,
        report.warmup_frame_count,
        report.build_profile
    );
    println!(
        "  average_measured_iteration={} ns  worst={} ns  best={} ns",
        report.frames.average_ns(),
        report.frames.worst_ns(),
        report.frames.best_ns()
    );
    println!(
        "  frames > 16.67ms (60 FPS): {}    frames > 33.33ms (30 FPS): {}",
        report.frames.over(report::BUDGET_60_FPS_NS),
        report.frames.over(report::BUDGET_30_FPS_NS)
    );

    println!("  parent phase breakdown (% of measured phase time):");
    for phase in &report.phases {
        println!(
            "    {:<38} {:>14} ns/iter  {:>6.2}%   [{}]",
            phase.name,
            phase.average_ns(),
            report.phase_percent(phase),
            phase.kind
        );
        for sub in &phase.subphases {
            println!(
                "      ↳ {:<34} {:>14} ns  {:>6.2}% of parent",
                sub.name,
                sub.average_ns(),
                ProfileReport::subphase_percent(phase, sub)
            );
        }
    }

    let placeholders = match report.placeholder_phases.is_empty() {
        true => "(none)".to_string(),
        false => report.placeholder_phases.join(", "),
    };
    println!("  placeholder phases: {placeholders}");

    println!("  harness churn counters:");
    print_churn(&report.churn);

    println!("  JSON report: {}", json_path.display());
    println!("  Markdown report: {}", md_path.display());
    if config.csv {
        println!("  CSV report: {}", csv_path.display());
    }
}

fn print_churn(churn: &ChurnCounters) {
    for (label, value) in churn.entries() {
        println!("    {label}: {value}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> impl Iterator<Item = String> {
        items
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
            .into_iter()
    }

    #[test]
    fn parse_args_uses_documented_defaults_when_empty() {
        let config = parse_args(args(&[])).expect("no args is valid");
        assert_eq!(config.objects, DEFAULT_OBJECTS);
        assert_eq!(config.frames, DEFAULT_FRAMES);
        assert_eq!(config.warmup, DEFAULT_WARMUP);
        assert_eq!(config.focus, FocusPhase::Full);
        assert!(!config.csv);
        assert_eq!(config.out, PathBuf::from(DEFAULT_OUT));
    }

    #[test]
    fn parse_args_reads_every_flag() {
        let config = parse_args(args(&[
            "--objects",
            "100",
            "--frames",
            "5",
            "--warmup-frames",
            "7",
            "--focus-phase",
            "render_command_build",
            "--csv",
            "--out",
            "some/dir",
        ]))
        .expect("flags are valid");
        assert_eq!(config.objects, 100);
        assert_eq!(config.frames, 5);
        assert_eq!(config.warmup, 7);
        assert_eq!(config.focus, FocusPhase::RenderCommandBuild);
        assert!(config.csv);
        assert_eq!(config.out, PathBuf::from("some/dir"));
    }

    #[test]
    fn parse_args_accepts_transform_focus() {
        let config = parse_args(args(&["--focus-phase", "transform_update"])).unwrap();
        assert_eq!(config.focus, FocusPhase::TransformUpdate);
    }

    #[test]
    fn parse_args_rejects_invalid_focus_phase_with_clear_error() {
        let err = parse_args(args(&["--focus-phase", "gpu_timing"])).unwrap_err();
        assert!(err.contains("gpu_timing"));
        assert!(
            err.contains("transform_update"),
            "error names the allowed values"
        );
    }

    #[test]
    fn parse_args_rejects_unknown_flags_and_missing_values() {
        assert!(parse_args(args(&["--bogus"])).is_err());
        assert!(parse_args(args(&["--objects"])).is_err());
        assert!(parse_args(args(&["--objects", "not-a-number"])).is_err());
        assert!(parse_args(args(&["--focus-phase"])).is_err());
        assert!(parse_args(args(&["--warmup-frames", "x"])).is_err());
    }

    #[test]
    fn build_profile_is_debug_under_test() {
        assert_eq!(build_profile(), "debug");
    }
}
