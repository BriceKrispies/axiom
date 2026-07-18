//! Native heap profile for End Zone, to pin the memory leak to an exact
//! allocation site instead of guessing. Drives the real app frame loop headless
//! under `dhat`'s heap profiler; on exit it prints the retained ("at t-end")
//! bytes and writes `dhat-heap.json` (a per-call-stack breakdown). A per-frame
//! leak shows as a large retained figure that scales with `--frames`.
//!
//!   cargo run -p axiom-end-zone --example leakcheck                  # advance + present
//!   cargo run -p axiom-end-zone --example leakcheck -- --no-present  # sim only
//!   cargo run -p axiom-end-zone --example leakcheck -- --frames 12000

use axiom_end_zone::app::{EndZoneApp, TouchInput};
use axiom_end_zone::config::EndZoneConfig;

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let no_present = args.iter().any(|a| a == "--no-present");
    let no_advance = args.iter().any(|a| a == "--no-advance");
    let frames: u64 = args
        .iter()
        .position(|a| a == "--frames")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(6000);

    let _profiler = dhat::Profiler::new_heap();

    let mut app = EndZoneApp::new(EndZoneConfig::default());
    for _ in 0..frames {
        if !no_advance {
            app.advance(&[], TouchInput::default());
        }
        if !no_present {
            let _ = app.present();
        }
    }
    println!(
        "drove {frames} frames (advance={} present={}); see dhat's 'At t-end' + dhat-heap.json",
        !no_advance, !no_present
    );
}
