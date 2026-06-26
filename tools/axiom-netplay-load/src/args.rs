//! Command-line configuration for the load generator.
//!
//! Hand-parsed `--key value` flags (no `clap` dependency, mirroring the worker
//! binary's tiny parser). The first positional argument selects the scenario.

/// Which load scenario to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Single-node capacity: N players across R rooms on one node.
    Soak,
    /// Matchmaker throughput: hammer `POST /matchmake`, measure rate + spread.
    Matchmake,
    /// Scaleout distribution: matchmake against a director, assert rooms spread
    /// across nodes, then connect players to their assigned nodes.
    Scaleout,
    /// Crash-recovery under load: soak one node while periodically killing its
    /// out-of-process sim worker; assert authoritative ticks keep advancing.
    Resilience,
}

impl Mode {
    fn parse(s: &str) -> Result<Mode, String> {
        match s {
            "soak" => Ok(Mode::Soak),
            "matchmake" => Ok(Mode::Matchmake),
            "scaleout" => Ok(Mode::Scaleout),
            "resilience" => Ok(Mode::Resilience),
            other => Err(format!(
                "unknown mode {other:?} (expected soak|matchmake|scaleout|resilience)"
            )),
        }
    }
}

/// The fully-resolved run configuration.
#[derive(Debug, Clone)]
pub struct Config {
    pub mode: Mode,
    /// HTTP base of the node/director, e.g. `http://localhost:8090`.
    pub target: String,
    /// Explicit game-socket URL; when absent it is derived from `target`.
    pub ws: Option<String>,
    pub players: usize,
    pub rooms: usize,
    pub duration_secs: f64,
    pub intent_hz: f64,
    pub requests: usize,
    pub concurrency: usize,
    /// Per-connection ramp delay (ms) so a soak doesn't open every socket in the
    /// same instant — a gentler, more realistic arrival curve.
    pub ramp_ms: u64,
    pub kill_every_secs: f64,
    pub worker_image: String,
    pub min_nodes: usize,
    // Pass/fail thresholds (drive the process exit code).
    pub max_p99_ms: f64,
    pub min_accept_rate: f64,
    pub min_tick_advance: u64,
}

impl Config {
    /// Parse argv (excluding the program name).
    pub fn parse(args: &[String]) -> Result<Config, String> {
        let mode_str = args.first().ok_or_else(Config::usage)?;
        let mode = Mode::parse(mode_str)?;

        let mut cfg = Config {
            mode,
            target: "http://localhost:8090".to_string(),
            ws: None,
            players: 50,
            rooms: 1,
            duration_secs: 10.0,
            intent_hz: 60.0,
            requests: 200,
            concurrency: 32,
            ramp_ms: 2,
            kill_every_secs: 3.0,
            worker_image: "axiom-netplay-worker".to_string(),
            min_nodes: 2,
            max_p99_ms: 250.0,
            min_accept_rate: 0.90,
            min_tick_advance: 30,
        };

        let mut rest = &args[1..];
        while let [key, value, tail @ ..] = rest {
            apply(&mut cfg, key, value)?;
            rest = tail;
        }
        // A dangling flag with no value is a usage error.
        match rest {
            [stray] => Err(format!("flag {stray:?} is missing a value")),
            _ => Ok(cfg),
        }
    }

    /// The game-socket URL: the explicit `--ws`, else `target` with the scheme
    /// swapped to `ws://` and `/ws` appended.
    pub fn ws_url(&self) -> String {
        self.ws.clone().unwrap_or_else(|| {
            let base = self
                .target
                .strip_prefix("http://")
                .unwrap_or(&self.target)
                .trim_end_matches('/');
            format!("ws://{base}/ws")
        })
    }

    fn usage() -> String {
        "usage: axiom-netplay-load <soak|matchmake|scaleout|resilience> [--key value ...]"
            .to_string()
    }
}

fn apply(cfg: &mut Config, key: &str, value: &str) -> Result<(), String> {
    let num = || {
        value
            .parse::<f64>()
            .map_err(|_| format!("{key}: not a number: {value:?}"))
    };
    let int = || {
        value
            .parse::<usize>()
            .map_err(|_| format!("{key}: not an integer: {value:?}"))
    };
    let big = || {
        value
            .parse::<u64>()
            .map_err(|_| format!("{key}: not an integer: {value:?}"))
    };
    match key {
        "--target" => cfg.target = value.to_string(),
        "--ws" => cfg.ws = Some(value.to_string()),
        "--players" => cfg.players = int()?,
        "--rooms" => cfg.rooms = int()?.max(1),
        "--duration" => cfg.duration_secs = num()?,
        "--intent-hz" => cfg.intent_hz = num()?.max(1.0),
        "--requests" => cfg.requests = int()?,
        "--concurrency" => cfg.concurrency = int()?.max(1),
        "--ramp-ms" => cfg.ramp_ms = big()?,
        "--kill-every" => cfg.kill_every_secs = num()?,
        "--worker-image" => cfg.worker_image = value.to_string(),
        "--min-nodes" => cfg.min_nodes = int()?,
        "--max-p99-ms" => cfg.max_p99_ms = num()?,
        "--min-accept-rate" => cfg.min_accept_rate = num()?,
        "--min-tick-advance" => cfg.min_tick_advance = big()?,
        other => return Err(format!("unknown flag {other:?}")),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_mode_and_defaults() {
        let cfg = Config::parse(&args(&["soak"])).unwrap();
        assert_eq!(cfg.mode, Mode::Soak);
        assert_eq!(cfg.players, 50);
        assert_eq!(cfg.target, "http://localhost:8090");
    }

    #[test]
    fn parses_each_mode() {
        assert_eq!(
            Config::parse(&args(&["matchmake"])).unwrap().mode,
            Mode::Matchmake
        );
        assert_eq!(
            Config::parse(&args(&["scaleout"])).unwrap().mode,
            Mode::Scaleout
        );
        assert_eq!(
            Config::parse(&args(&["resilience"])).unwrap().mode,
            Mode::Resilience
        );
    }

    #[test]
    fn overrides_flags() {
        let cfg = Config::parse(&args(&[
            "soak",
            "--players",
            "200",
            "--rooms",
            "8",
            "--duration",
            "5.5",
            "--target",
            "http://host:9000",
        ]))
        .unwrap();
        assert_eq!(cfg.players, 200);
        assert_eq!(cfg.rooms, 8);
        assert_eq!(cfg.duration_secs, 5.5);
        assert_eq!(cfg.target, "http://host:9000");
    }

    #[test]
    fn derives_ws_url_from_target() {
        let cfg = Config::parse(&args(&["soak", "--target", "http://localhost:8101/"])).unwrap();
        assert_eq!(cfg.ws_url(), "ws://localhost:8101/ws");
    }

    #[test]
    fn explicit_ws_url_wins() {
        let cfg = Config::parse(&args(&["soak", "--ws", "ws://n/ws"])).unwrap();
        assert_eq!(cfg.ws_url(), "ws://n/ws");
    }

    #[test]
    fn rooms_is_at_least_one() {
        let cfg = Config::parse(&args(&["soak", "--rooms", "0"])).unwrap();
        assert_eq!(cfg.rooms, 1);
    }

    #[test]
    fn unknown_mode_is_an_error() {
        assert!(Config::parse(&args(&["bogus"])).is_err());
    }

    #[test]
    fn missing_mode_is_an_error() {
        assert!(Config::parse(&[]).is_err());
    }

    #[test]
    fn unknown_flag_is_an_error() {
        assert!(Config::parse(&args(&["soak", "--nope", "1"])).is_err());
    }

    #[test]
    fn dangling_flag_is_an_error() {
        assert!(Config::parse(&args(&["soak", "--players"])).is_err());
    }

    #[test]
    fn non_numeric_value_is_an_error() {
        assert!(Config::parse(&args(&["soak", "--players", "lots"])).is_err());
        assert!(Config::parse(&args(&["soak", "--duration", "soon"])).is_err());
        assert!(Config::parse(&args(&["soak", "--ramp-ms", "x"])).is_err());
    }
}
