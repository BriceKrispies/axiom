//! CLI argument parsing, `--ticks`/`--markers` list parsing, and the
//! `--plan`-file merge (explicit CLI args override plan-file values).
//!
//! Parsing is hand-rolled (mirroring `axiom-shot`'s `flag()`), no clap. The
//! output of [`merge`] is a [`MergedArgs`]: backend/viewport/columns resolved to
//! concrete values, `ticks`/`markers` parsed to lists, and the remaining
//! identity fields (`app`/`scenario`/`camera`/`out`) left optional for `main` to
//! resolve against the app registry.

use crate::capture_plan::{Backend, PlanFile, Viewport};
use crate::FilmstripError;

/// Raw CLI flags, before merging with a plan file. Value flags are `--name val`;
/// `--debug-overlays` is a valueless presence flag.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RawArgs {
    pub app: Option<String>,
    pub scenario: Option<String>,
    pub backend: Option<String>,
    pub viewport: Option<String>,
    pub columns: Option<u32>,
    pub ticks: Option<String>,
    pub markers: Option<String>,
    pub camera: Option<String>,
    pub debug_overlays: bool,
    pub out: Option<String>,
    pub plan: Option<String>,
}

/// The merged, format-validated run request. `app`/`scenario`/`camera`/`out` stay
/// optional — `main` resolves them against the app registry (and derives a
/// default `out`). `ticks`/`markers` are the two capture modes (exactly one must
/// be non-empty, enforced downstream).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergedArgs {
    pub app: Option<String>,
    pub scenario: Option<String>,
    pub backend: Backend,
    pub viewport: Viewport,
    pub columns: u32,
    pub camera: Option<String>,
    pub debug_overlays: bool,
    pub ticks: Option<Vec<u64>>,
    pub markers: Option<Vec<String>>,
    pub out: Option<String>,
}

/// Parse `argv` (already stripped of the program name) into [`RawArgs`].
pub fn parse_argv(args: &[String]) -> Result<RawArgs, FilmstripError> {
    let columns = value(args, "--columns")
        .map(|c| {
            c.parse::<u32>()
                .map_err(|_| FilmstripError::InvalidColumns(c))
        })
        .transpose()?;
    Ok(RawArgs {
        app: value(args, "--app"),
        scenario: value(args, "--scenario"),
        backend: value(args, "--backend"),
        viewport: value(args, "--viewport"),
        columns,
        ticks: value(args, "--ticks"),
        markers: value(args, "--markers"),
        camera: value(args, "--camera"),
        debug_overlays: args.iter().any(|a| a == "--debug-overlays"),
        out: value(args, "--out"),
        plan: value(args, "--plan"),
    })
}

/// Merge CLI [`RawArgs`] over an optional [`PlanFile`] (CLI wins for every
/// field), resolving formats. Defaults: backend `gpu`, viewport `1280x720`,
/// columns `4`, debug off.
pub fn merge(cli: &RawArgs, plan: Option<&PlanFile>) -> Result<MergedArgs, FilmstripError> {
    let backend = cli
        .backend
        .clone()
        .or_else(|| plan.and_then(|p| p.backend.clone()))
        .map(|b| Backend::parse(&b))
        .transpose()?
        .unwrap_or(Backend::Gpu);

    let viewport = resolve_viewport(cli, plan)?;

    let columns = cli
        .columns
        .or_else(|| plan.and_then(|p| p.columns))
        .unwrap_or(4);

    let ticks = cli
        .ticks
        .as_deref()
        .map(parse_ticks)
        .transpose()?
        .or_else(|| plan.and_then(|p| p.ticks.clone()));

    let markers = cli
        .markers
        .as_deref()
        .map(parse_markers)
        .or_else(|| plan.and_then(|p| p.markers.clone()));

    let debug_overlays = cli.debug_overlays || plan.and_then(|p| p.debug_overlays).unwrap_or(false);

    Ok(MergedArgs {
        app: cli.app.clone().or_else(|| plan.and_then(|p| p.app.clone())),
        scenario: cli
            .scenario
            .clone()
            .or_else(|| plan.and_then(|p| p.scenario.clone())),
        backend,
        viewport,
        columns,
        camera: cli
            .camera
            .clone()
            .or_else(|| plan.and_then(|p| p.camera.clone())),
        debug_overlays,
        ticks,
        markers,
        out: cli.out.clone().or_else(|| plan.and_then(|p| p.out.clone())),
    })
}

/// CLI `--viewport WxH` wins; else the plan's `viewport_width`/`viewport_height`
/// pair (both required together); else the `1280x720` default.
fn resolve_viewport(cli: &RawArgs, plan: Option<&PlanFile>) -> Result<Viewport, FilmstripError> {
    match cli.viewport.as_deref() {
        Some(s) => Viewport::parse(s),
        None => match plan.and_then(|p| p.viewport_width.zip(p.viewport_height)) {
            Some((w, h)) => Viewport::parse(&format!("{w}x{h}")),
            None => Ok(Viewport {
                width: 1280,
                height: 720,
            }),
        },
    }
}

/// Parse a comma-separated tick list (`"0,10,20"`) into ordered `u64`s. Empty or
/// non-integer entries are an error.
pub fn parse_ticks(s: &str) -> Result<Vec<u64>, FilmstripError> {
    let parts: Vec<&str> = s
        .split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    let bad = || FilmstripError::InvalidTicks(s.to_string());
    (!parts.is_empty()).then_some(()).ok_or_else(bad)?;
    parts
        .iter()
        .map(|p| p.parse::<u64>().map_err(|_| bad()))
        .collect()
}

/// Parse a comma-separated marker-name list into trimmed, non-empty names.
pub fn parse_markers(s: &str) -> Vec<String> {
    s.split(',')
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .map(str::to_string)
        .collect()
}

/// The value following `name` in `args`, if present.
fn value(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parse_ticks_reads_a_list_and_rejects_garbage() {
        assert_eq!(parse_ticks("0,10,20,30").unwrap(), vec![0, 10, 20, 30]);
        assert_eq!(parse_ticks(" 5 , 7 ").unwrap(), vec![5, 7]);
        assert!(matches!(
            parse_ticks(""),
            Err(FilmstripError::InvalidTicks(_))
        ));
        assert!(matches!(
            parse_ticks("0,x,2"),
            Err(FilmstripError::InvalidTicks(_))
        ));
        assert!(matches!(
            parse_ticks("-3"),
            Err(FilmstripError::InvalidTicks(_))
        ));
    }

    #[test]
    fn parse_markers_trims_and_drops_empties() {
        assert_eq!(parse_markers("a,b,c"), vec!["a", "b", "c"]);
        assert_eq!(parse_markers(" a , , b "), vec!["a", "b"]);
        assert!(parse_markers("").is_empty());
    }

    #[test]
    fn parse_argv_reads_flags_and_the_debug_presence_flag() {
        let a = parse_argv(&argv(&[
            "--app",
            "soccer_penalty",
            "--backend",
            "canvas2d",
            "--ticks",
            "0,10",
            "--columns",
            "3",
            "--debug-overlays",
            "--out",
            "x.png",
        ]))
        .unwrap();
        assert_eq!(a.app.as_deref(), Some("soccer_penalty"));
        assert_eq!(a.backend.as_deref(), Some("canvas2d"));
        assert_eq!(a.ticks.as_deref(), Some("0,10"));
        assert_eq!(a.columns, Some(3));
        assert!(a.debug_overlays);
        assert_eq!(a.out.as_deref(), Some("x.png"));
        assert!(!parse_argv(&argv(&["--app", "x"])).unwrap().debug_overlays);
    }

    #[test]
    fn bad_columns_is_an_error() {
        assert!(matches!(
            parse_argv(&argv(&["--columns", "lots"])),
            Err(FilmstripError::InvalidColumns(_))
        ));
    }

    #[test]
    fn merge_applies_defaults_when_nothing_is_set() {
        let m = merge(&RawArgs::default(), None).unwrap();
        assert_eq!(m.backend, Backend::Gpu);
        assert_eq!(
            m.viewport,
            Viewport {
                width: 1280,
                height: 720
            }
        );
        assert_eq!(m.columns, 4);
        assert!(!m.debug_overlays);
        assert!(m.ticks.is_none() && m.markers.is_none());
    }

    #[test]
    fn cli_overrides_plan_values() {
        let plan = PlanFile {
            app: Some("soccer_penalty".into()),
            backend: Some("gpu".into()),
            columns: Some(2),
            ticks: Some(vec![1, 2, 3]),
            viewport_width: Some(800),
            viewport_height: Some(500),
            out: Some("plan.png".into()),
            ..PlanFile::default()
        };
        let cli = RawArgs {
            backend: Some("canvas2d".into()),
            columns: Some(5),
            ticks: Some("9,9".into()),
            viewport: Some("640x400".into()),
            ..RawArgs::default()
        };
        let m = merge(&cli, Some(&plan)).unwrap();
        // CLI wins where set...
        assert_eq!(m.backend, Backend::Canvas2d);
        assert_eq!(m.columns, 5);
        assert_eq!(m.ticks, Some(vec![9, 9]));
        assert_eq!(
            m.viewport,
            Viewport {
                width: 640,
                height: 400
            }
        );
        // ...plan fills where the CLI is silent.
        assert_eq!(m.app.as_deref(), Some("soccer_penalty"));
        assert_eq!(m.out.as_deref(), Some("plan.png"));
    }

    #[test]
    fn plan_viewport_pair_is_used_when_cli_is_silent() {
        let plan = PlanFile {
            viewport_width: Some(320),
            viewport_height: Some(200),
            ..PlanFile::default()
        };
        let m = merge(&RawArgs::default(), Some(&plan)).unwrap();
        assert_eq!(
            m.viewport,
            Viewport {
                width: 320,
                height: 200
            }
        );
    }
}
