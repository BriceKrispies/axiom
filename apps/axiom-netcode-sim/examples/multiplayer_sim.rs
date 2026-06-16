//! Watch N lockstep clients react. Run with:
//!
//! ```sh
//! cargo run -p axiom-netcode-sim --example multiplayer_sim
//! ```
//!
//! It runs four scenarios — a healthy group, a lossy/jittery link, a slow tab,
//! and a room with a forging + flooding + corrupt cheater — printing a live
//! per-tick stream and a per-client summary, and writing a CSV for the last one.

use axiom_netcode_sim::{run_simulation, Backend, Behavior, CheatKind, Partition, SimConfig};

fn main() {
    healthy_group();
    lossy_link();
    a_slow_tab_and_a_partition();
    a_room_full_of_cheaters();
}

/// Eight real engine clients on a perfect link — everyone tracks in lockstep.
fn healthy_group() {
    banner("8 real-engine peers, clean link");
    let mut c = SimConfig::new(8, 90);
    c.backend = Backend::Engine;
    c.seed = 0x1234;
    c.stream = false; // flip to true to watch every tick
    run_simulation(&c).print_summary();
}

/// A lossy, jittery, duplicating link — retransmission still gets everyone there,
/// just with visible confirm latency.
fn lossy_link() {
    banner("6 peers, 30% loss + jitter + duplication");
    let mut c = SimConfig::new(6, 120);
    c.backend = Backend::Mock;
    c.seed = 99;
    c.network.drop_per_mille = 300;
    c.network.latency_min = 0;
    c.network.latency_max = 4;
    c.network.duplicate_per_mille = 80;
    run_simulation(&c).print_summary();
}

/// One slow tab gates the group; one peer drops off the network for a while and
/// then recovers.
fn a_slow_tab_and_a_partition() {
    banner("5 peers: a slow tab + a temporary partition");
    let mut c = SimConfig::new(5, 120);
    c.backend = Backend::Mock;
    c.seed = 7;
    c.per_peer[0].tick_rate = 3; // a third as often
    c.network.partitions = vec![Partition {
        peer: 3,
        from_tick: 20,
        to_tick: 50,
    }];
    run_simulation(&c).print_summary();
}

/// A room with three different cheaters — every attack is shrugged off; only the
/// corrupt-sim peer desyncs (itself), and `reconcile` catches it.
fn a_room_full_of_cheaters() {
    banner("6 peers: forge + flood + corrupt-sim cheaters (CSV written)");
    let mut c = SimConfig::new(6, 80);
    c.backend = Backend::Mock;
    c.seed = 0xBADBAD;
    c.per_peer[1].behavior = Behavior::Cheater(CheatKind::ForgeOthers);
    c.per_peer[2].behavior = Behavior::Cheater(CheatKind::Flood);
    c.per_peer[3].behavior = Behavior::Cheater(CheatKind::CorruptSim);
    let csv = std::env::temp_dir().join("netcode_sim.csv");
    c.csv_path = Some(csv.clone());
    run_simulation(&c).print_summary();
    println!("wrote per-tick CSV to {}", csv.display());
}

fn banner(title: &str) {
    println!("\n────────────────────────────────────────────────────────");
    println!("  {title}");
    println!("────────────────────────────────────────────────────────");
}
