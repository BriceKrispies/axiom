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

/// How a peer produces its per-tick input delta. All forms are deterministic
/// (seeded from the run's master seed), so the whole run replays.
///
/// Reshaped from a data-carrying enum into a **tagged struct**: a `kind`
/// discriminant (`ScriptKind`) plus the payload each kind needs (the scripted
/// delta list, the oscillation period). A `None`/`0` payload is the kind that
/// does not carry one. This makes selection a tag comparison instead of a
/// `match`, while `InputScript::Idle`/`InputScript::Scripted`/etc. keep working
/// as constructors for callers.
#[derive(Debug, Clone, PartialEq)]
pub struct InputScript {
    kind: ScriptKind,
    /// The looping delta list (only present for `ScriptKind::Scripted`).
    scripted: Option<Vec<Vec3>>,
    /// The oscillation period in ticks (only meaningful for
    /// `ScriptKind::Oscillate`).
    period: u64,
}

/// The discriminant tag for an [`InputScript`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScriptKind {
    /// Never moves (zero delta every tick).
    Idle,
    /// A fixed, looping list of deltas.
    Scripted,
    /// Seeded pseudo-random walk (a "button masher").
    RandomWalk,
    /// A smooth oscillation with a period in ticks.
    Oscillate,
}

impl InputScript {
    /// Never moves (zero delta every tick).
    #[allow(non_upper_case_globals)]
    pub const Idle: InputScript = InputScript {
        kind: ScriptKind::Idle,
        scripted: None,
        period: 0,
    };

    /// Seeded pseudo-random walk (a "button masher").
    #[allow(non_upper_case_globals)]
    pub const RandomWalk: InputScript = InputScript {
        kind: ScriptKind::RandomWalk,
        scripted: None,
        period: 0,
    };

    /// A fixed, looping list of deltas.
    #[allow(non_snake_case)]
    pub fn Scripted(deltas: Vec<Vec3>) -> InputScript {
        InputScript {
            kind: ScriptKind::Scripted,
            scripted: Some(deltas),
            period: 0,
        }
    }

    /// A smooth oscillation with the given period in ticks.
    #[allow(non_snake_case)]
    pub fn Oscillate(period: u64) -> InputScript {
        InputScript {
            kind: ScriptKind::Oscillate,
            scripted: None,
            period,
        }
    }

    /// This script's discriminant tag (lets callers select branchlessly).
    pub(crate) fn kind(&self) -> ScriptKind {
        self.kind
    }

    /// The scripted delta list, if this is a [`ScriptKind::Scripted`] script.
    pub(crate) fn scripted(&self) -> Option<&[Vec3]> {
        self.scripted.as_deref()
    }

    /// The oscillation period (meaningful only for [`ScriptKind::Oscillate`]).
    pub(crate) fn period(&self) -> u64 {
        self.period
    }
}

/// A way a peer can misbehave — each exercises a different defense (or the
/// desync referee).
///
/// Reshaped into a **tagged struct** wrapping a single discriminant byte, with
/// one associated `const` per kind. This keeps `CheatKind::ForgeOthers` (and
/// the rest) working as value constructors for callers while turning kind
/// selection into a `==` on the tag rather than a `match`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct CheatKind {
    tag: u8,
}

impl CheatKind {
    /// Sign inputs claiming *another* peer (with the cheater's own key) — the
    /// victim's roster key rejects them (`bad_signature`).
    #[allow(non_upper_case_globals)]
    pub const ForgeOthers: CheatKind = CheatKind { tag: 0 };
    /// Spray validly-signed inputs for far-future ticks — dropped as
    /// `out_of_window`; the victim's buffer stays bounded.
    #[allow(non_upper_case_globals)]
    pub const Flood: CheatKind = CheatKind { tag: 1 };
    /// Send inputs for already-confirmed past ticks — dropped as `out_of_window`.
    #[allow(non_upper_case_globals)]
    pub const OutOfWindow: CheatKind = CheatKind { tag: 2 };
    /// Send structurally-garbage bytes — rejected by `ingest` as a decode error.
    #[allow(non_upper_case_globals)]
    pub const Malformed: CheatKind = CheatKind { tag: 3 };
    /// Simulate a divergent world — honest peers' `reconcile` flags the desync.
    #[allow(non_upper_case_globals)]
    pub const CorruptSim: CheatKind = CheatKind { tag: 4 };

    /// This kind's discriminant tag (lets the cheat-state builder dispatch
    /// branchlessly on equality rather than matching variants).
    pub(crate) fn tag(self) -> u8 {
        self.tag
    }
}

impl std::fmt::Debug for CheatKind {
    /// Preserves the original enum's `Debug` spelling (`ForgeOthers`, `Flood`,
    /// …) so console summaries print exactly as before.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = [
            "ForgeOthers",
            "Flood",
            "OutOfWindow",
            "Malformed",
            "CorruptSim",
        ]
        .get(self.tag as usize)
        .copied()
        .unwrap_or("CheatKind");
        f.write_str(name)
    }
}

/// Whether a peer plays fair or misbehaves.
///
/// Reshaped into a **tagged struct**: an `Option<CheatKind>` payload that is
/// the whole tag — `None` is honest, `Some(kind)` is a cheater of that kind.
/// `Behavior::Honest` stays a value constructor (an associated `const`) and
/// `Behavior::Cheater(kind)` stays one (an associated fn), so callers are
/// unchanged; extracting the kind is now `Behavior::cheat_kind` (a field
/// read) rather than a `match`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Behavior {
    cheat: Option<CheatKind>,
}

impl Behavior {
    /// Plays by the rules.
    #[allow(non_upper_case_globals)]
    pub const Honest: Behavior = Behavior { cheat: None };

    /// Misbehaves in the given way.
    #[allow(non_snake_case)]
    pub fn Cheater(kind: CheatKind) -> Behavior {
        Behavior { cheat: Some(kind) }
    }

    /// The cheat kind this peer runs, or `None` if it plays fair.
    pub(crate) fn cheat_kind(self) -> Option<CheatKind> {
        self.cheat
    }
}

impl std::fmt::Debug for Behavior {
    /// Preserves the original enum's `Debug` spelling (`Honest`,
    /// `Cheater(ForgeOthers)`) so console summaries print exactly as before.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `Some(kind)` formats as the original `Cheater(<kind>)` tuple variant;
        // `None` as the original `Honest` unit variant. Render the text once
        // (no `if`/`match`) and write it through a single `f` borrow.
        let rendered = self
            .cheat
            .map_or_else(|| "Honest".to_string(), |kind| format!("Cheater({kind:?})"));
        f.write_str(&rendered)
    }
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
