//! The simulation knobs: peer count, network model, per-peer behavior, output.

use std::path::PathBuf;

use axiom::prelude::Vec3;

/// Which client each peer runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// A real engine `App` per peer (full fidelity; practical to ~16-32 peers).
    Engine,
    /// A cheap deterministic fold per peer (scales to hundreds; isolates netcode).
    Mock,
}

/// How a peer produces its per-tick input delta. All variants are deterministic
/// (seeded from the run's master seed), so the whole run replays.
#[derive(Debug, Clone, PartialEq)]
pub enum InputScript {
    /// Never moves (zero delta every tick).
    Idle,
    /// A fixed, looping list of deltas.
    Scripted(Vec<Vec3>),
    /// Seeded pseudo-random walk (a "button masher").
    RandomWalk,
    /// A smooth oscillation with the given period in ticks.
    Oscillate { period: u64 },
}

/// A way a peer can misbehave — each exercises a different defense (or the
/// desync referee).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheatKind {
    /// Sign inputs claiming *another* peer (with the cheater's own key) — the
    /// victim's roster key rejects them (`bad_signature`).
    ForgeOthers,
    /// Spray validly-signed inputs for far-future ticks — dropped as
    /// `out_of_window`; the victim's buffer stays bounded.
    Flood,
    /// Send inputs for already-confirmed past ticks — dropped as `out_of_window`.
    OutOfWindow,
    /// Send structurally-garbage bytes — rejected by `ingest` as a decode error.
    Malformed,
    /// Simulate a divergent world — honest peers' `reconcile` flags the desync.
    CorruptSim,
}

/// Whether a peer plays fair or misbehaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Behavior {
    /// Plays by the rules.
    Honest,
    /// Misbehaves in the given way.
    Cheater(CheatKind),
}

/// A window during which a peer's links are cut (it sends/receives nothing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Partition {
    /// The peer index (0-based) that is isolated.
    pub peer: usize,
    /// First driver tick of the outage (inclusive).
    pub from_tick: u64,
    /// End of the outage (exclusive).
    pub to_tick: u64,
}

/// Per-link network conditions, applied independently to each recipient and
/// seeded so the run is reproducible.
#[derive(Debug, Clone)]
pub struct NetworkConfig {
    /// Probability (per mille) a delivery is dropped.
    pub drop_per_mille: u32,
    /// Minimum delivery delay, in driver ticks.
    pub latency_min: u64,
    /// Maximum delivery delay, in driver ticks (jitter ⇒ reordering).
    pub latency_max: u64,
    /// Probability (per mille) a delivery is duplicated.
    pub duplicate_per_mille: u32,
    /// Whether peers resend their unconfirmed lead each tick (reliable link). If
    /// false, a dropped input stalls the lockstep gate (pure unreliable).
    pub retransmit: bool,
    /// Link-outage windows.
    pub partitions: Vec<Partition>,
}

impl Default for NetworkConfig {
    /// A perfect link: no loss, no delay, no duplication, reliable, no partitions.
    fn default() -> Self {
        NetworkConfig {
            drop_per_mille: 0,
            latency_min: 0,
            latency_max: 0,
            duplicate_per_mille: 0,
            retransmit: true,
            partitions: Vec::new(),
        }
    }
}

/// One peer's behavior knobs.
#[derive(Debug, Clone)]
pub struct PeerConfig {
    /// How this peer chooses its inputs.
    pub input: InputScript,
    /// Whether it plays fair.
    pub behavior: Behavior,
    /// Steps once every `tick_rate` driver ticks (1 = every tick; >1 = a slow
    /// tab that submits less often).
    pub tick_rate: u64,
    /// The driver tick at which this peer joins (starts submitting).
    pub join_tick: u64,
    /// The driver tick at which this peer leaves (stops submitting).
    pub leave_tick: u64,
}

impl Default for PeerConfig {
    /// An honest, always-on, random-walking peer.
    fn default() -> Self {
        PeerConfig {
            input: InputScript::RandomWalk,
            behavior: Behavior::Honest,
            tick_rate: 1,
            join_tick: 0,
            leave_tick: u64::MAX,
        }
    }
}

/// A full simulation specification. Build with [`SimConfig::new`] and adjust the
/// public fields; every randomized choice derives from [`Self::seed`], so two
/// runs with the same config produce an identical [`crate::SimReport`].
#[derive(Debug, Clone)]
pub struct SimConfig {
    /// Number of peers.
    pub peers: usize,
    /// Driver ticks to run.
    pub ticks: u64,
    /// Master seed — every RNG (network, inputs) derives from this.
    pub seed: u64,
    /// Which client backend every peer runs.
    pub backend: Backend,
    /// How far ahead of confirmation a peer may submit before it waits.
    pub max_ahead: u64,
    /// Network conditions.
    pub network: NetworkConfig,
    /// Per-peer behavior (length is normalized to `peers`: extra entries are
    /// dropped, missing ones default to an honest random-walker).
    pub per_peer: Vec<PeerConfig>,
    /// Whether to print a live per-tick line as the run proceeds.
    pub stream: bool,
    /// Where to write the per-(tick, peer) CSV, if anywhere.
    pub csv_path: Option<PathBuf>,
}

impl SimConfig {
    /// A default `peers`-by-`ticks` run: clean network, engine backend, all peers
    /// honest random-walkers, no streaming, no CSV.
    pub fn new(peers: usize, ticks: u64) -> Self {
        SimConfig {
            peers,
            ticks,
            seed: 0,
            backend: Backend::Engine,
            max_ahead: 8,
            network: NetworkConfig::default(),
            per_peer: vec![PeerConfig::default(); peers],
            stream: false,
            csv_path: None,
        }
    }

    /// The config for peer `i`, defaulting if `per_peer` is short.
    pub(crate) fn peer(&self, i: usize) -> PeerConfig {
        self.per_peer.get(i).cloned().unwrap_or_default()
    }
}
