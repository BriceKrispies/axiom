//! `axiom-netplay-load` — a headless load generator for the server-authoritative
//! netplay backend.
//!
//! It opens many concurrent WebSocket players that speak the **real** wire
//! protocol (encoded by `axiom-net-protocol`, sequenced by the engine's own
//! `axiom-client-core` state machine), so a load run can never drift from the
//! true client. Four scenarios stress different parts of the stack:
//!
//! * `soak`        — single-node capacity (N players across R rooms).
//! * `matchmake`   — matchmaker HTTP throughput and assignment spread.
//! * `scaleout`    — a director distributing rooms across nodes, then real play.
//! * `resilience`  — crash-recovery: kill the out-of-process worker under load
//!   and prove the authoritative loop keeps advancing.
//!
//! Each scenario prints a stats report and a PASS/FAIL verdict; the process exit
//! code is `0` on PASS, `1` on FAIL, `2` on a usage error — so it can gate CI.

mod args;
mod http;
mod player;
mod scenarios;
mod stats;

use args::Config;

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let cfg = match Config::parse(&argv) {
        Ok(cfg) => cfg,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to build the tokio runtime");

    let passed = runtime.block_on(scenarios::run(&cfg));
    std::process::exit(if passed { 0 } else { 1 });
}
