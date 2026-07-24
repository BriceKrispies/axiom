//! Native heap profile for End Zone, to pin the memory leak to an exact
//! allocation site instead of guessing. Drives the real app frame loop headless
//! under `dhat`'s heap profiler; on exit it prints the retained ("at t-end")
//! bytes and writes `dhat-heap.json` (a per-call-stack breakdown). A per-frame
//! leak shows as a large retained figure that scales with `--frames`.
//!
//!   cargo run -p axiom-end-zone --example leakcheck                  # advance + present
//!   cargo run -p axiom-end-zone --example leakcheck -- --no-present  # sim only
//!   cargo run -p axiom-end-zone --example leakcheck -- --frames 12000
//!
//! ## Recurrence gate (run in CI)
//! `--max-mb-per-frame <N>` makes the run FAIL (non-zero exit) if the steady-state
//! per-frame heap churn exceeds `N` MB. The render pipeline reuses retained buffers
//! so a well-behaved frame allocates almost nothing; any new per-frame allocation
//! (a `Vec::new`/`with_capacity`/`clone` back in the hot path) pushes churn over the
//! budget and trips this gate — which is how the recurring "gets laggy after an
//! hour" wasm-memory-fragmentation regression is kept from silently coming back.
//!   cargo run -p axiom-end-zone --example leakcheck -- --frames 2000 --max-mb-per-frame 3.0

use axiom_end_zone::app::{EndZoneApp, TouchInput};
use axiom_end_zone::config::EndZoneConfig;

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

fn arg_value(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let no_present = args.iter().any(|a| a == "--no-present");
    let no_advance = args.iter().any(|a| a == "--no-advance");
    let frames: u64 = arg_value(&args, "--frames")
        .and_then(|s| s.parse().ok())
        .unwrap_or(6000);
    let max_mb_per_frame: Option<f64> =
        arg_value(&args, "--max-mb-per-frame").and_then(|s| s.parse().ok());

    let profiler = dhat::Profiler::builder().testing().build();

    let mut app = EndZoneApp::new(EndZoneConfig::default());
    for _ in 0..frames {
        if !no_advance {
            app.advance(&[], TouchInput::default());
        }
        if !no_present {
            let _ = app.present();
        }
    }

    let stats = dhat::HeapStats::get();
    drop(profiler);
    let mb_per_frame = stats.total_bytes as f64 / frames.max(1) as f64 / 1.0e6;
    println!(
        "drove {frames} frames (advance={} present={}): churn {mb_per_frame:.2} MB/frame, \
         t-end {} bytes in {} blocks",
        !no_advance, !no_present, stats.curr_bytes, stats.curr_blocks
    );
    if let Some(budget) = max_mb_per_frame {
        assert!(
            mb_per_frame <= budget,
            "RENDER-CHURN REGRESSION: {mb_per_frame:.2} MB/frame exceeds the {budget:.2} MB budget \
             — the render pipeline is allocating per frame again (a Vec::new/with_capacity/clone \
             back in the hot path). Reuse a retained buffer instead. See \
             docs (endzone-render-churn-not-leak) for the reuse pattern."
        );
    }
}
