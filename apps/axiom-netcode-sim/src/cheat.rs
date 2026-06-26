//! The misbehaving-peer behaviors — each crafts traffic that exercises one
//! defense (or the desync referee). A cheater still submits its real inputs (so
//! the lockstep gate can progress); these are the *extra* bad frames it injects.

use axiom_crypto::{SigningKey, VerifyingKey};
use axiom_kernel::DeterministicRng;
use axiom_netcode::NetcodeApi;

use crate::config::CheatKind;
use crate::simulant::MOVE_KIND;

/// A zero-movement payload (8 bytes) for crafted frames.
const ZERO_DELTA: [u8; 8] = [0; 8];

/// The discriminant tag for a [`CheatState`] — which misbehavior (if any) this
/// peer runs. `Honest` is the do-nothing kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StateKind {
    /// Plays fair.
    Honest,
    /// An impersonator session signed by an off-roster key.
    Forge,
    /// Pre-generated frames far beyond the admission window.
    Flood,
    /// Replays the cheater's own tick-0 frame forever.
    OutOfWindow,
    /// Emits structural garbage.
    Malformed,
    /// Simulates a divergent world.
    CorruptSim,
}

/// The live state backing a peer's misbehavior.
///
/// Reshaped from a data-carrying enum into a **tagged struct**: a `kind`
/// discriminant ([`StateKind`]) plus the superset of payloads the kinds need,
/// each held in an `Option`/`Box` so an unused payload is absent. Selection is
/// now a `kind ==` comparison rather than a `match`, and the per-kind payloads
/// are read through `Option`. This keeps every observable behavior identical:
/// only the kind whose tag is set ever touches its payload.
///
/// Payloads, by owning kind:
/// - [`StateKind::Forge`] → `forge`: an impersonator session. Boxed because a
///   session is far larger than the other payloads → the victim rejects its
///   frames as `bad_signature`.
/// - [`StateKind::Flood`] → `flood`/`cursor`: pre-generated, validly-signed
///   frames whose ticks sit far beyond the admission window → dropped as
///   `out_of_window`, victim buffer stays bounded.
/// - [`StateKind::OutOfWindow`] → `first`: the cheater's own tick-0 frame,
///   replayed forever; once confirmed past, it is a past tick → `out_of_window`.
/// - [`StateKind::Malformed`] emits structural garbage → `ingest` fails to
///   decode it. [`StateKind::CorruptSim`] simulates a divergent world →
///   honest peers' `reconcile` flags the desync. Neither carries a payload.
#[derive(Debug)]
pub(crate) struct CheatState {
    kind: StateKind,
    forge: Option<Box<NetcodeApi>>,
    flood: Vec<Vec<u8>>,
    cursor: usize,
    first: Option<Vec<u8>>,
}

impl CheatState {
    /// A payload-free state of the given kind (used for the kinds that carry no
    /// data, and as the base every constructor fills in).
    fn bare(kind: StateKind) -> Self {
        CheatState {
            kind,
            forge: None,
            flood: Vec::new(),
            cursor: 0,
            first: None,
        }
    }

    /// Build the cheat state for a peer (or the honest do-nothing state if it
    /// plays fair). The kind tag selects which payload to populate via gated
    /// constructor calls — no `match` over the cheat kind.
    pub(crate) fn build(
        kind: Option<CheatKind>,
        cheater_id: u64,
        cheater_key: &SigningKey,
        roster: &[(u64, VerifyingKey)],
        peers: usize,
        ticks: u64,
        seed: u64,
    ) -> Self {
        kind.map_or_else(
            || CheatState::bare(StateKind::Honest),
            |k| {
                let tag = k.tag();
                // Each arm computes its state only when its tag is selected; the
                // chain keeps exactly the one built (the rest stay `None`).
                let forge = (tag == CheatKind::ForgeOthers.tag()).then(|| {
                    // Claim a *different* peer, signed by an off-roster key.
                    let target = (cheater_id % peers as u64) + 1;
                    let off_key = SigningKey::from_seed(forge_seed(seed, cheater_id));
                    let aux = NetcodeApi::new(
                        target,
                        off_key.clone(),
                        &[(target, off_key.verifying_key())],
                    );
                    let mut s = CheatState::bare(StateKind::Forge);
                    s.forge = Some(Box::new(aux));
                    s
                });
                let flood = (tag == CheatKind::Flood.tag()).then(|| {
                    let mut s = CheatState::bare(StateKind::Flood);
                    s.flood = flood_frames(cheater_id, cheater_key, roster, ticks);
                    s
                });
                let out_of_window = (tag == CheatKind::OutOfWindow.tag())
                    .then(|| CheatState::bare(StateKind::OutOfWindow));
                let malformed = (tag == CheatKind::Malformed.tag())
                    .then(|| CheatState::bare(StateKind::Malformed));
                let corrupt_sim = (tag == CheatKind::CorruptSim.tag())
                    .then(|| CheatState::bare(StateKind::CorruptSim));
                forge
                    .or(flood)
                    .or(out_of_window)
                    .or(malformed)
                    .or(corrupt_sim)
                    .unwrap_or_else(|| CheatState::bare(StateKind::Honest))
            },
        )
    }

    /// Whether this peer simulates a divergent world.
    pub(crate) fn corrupts_sim(&self) -> bool {
        self.kind == StateKind::CorruptSim
    }

    /// Note this peer's own just-submitted frame (so `OutOfWindow` can replay it).
    pub(crate) fn note_submit(&mut self, frame: &[u8]) {
        (self.kind == StateKind::OutOfWindow).then(|| {
            self.first.get_or_insert_with(|| frame.to_vec());
        });
    }

    /// The extra bad frames to broadcast this tick. Each kind's frames are
    /// produced behind a tag gate and concatenated; only the active kind
    /// contributes, so the result is identical to the old per-variant `match`.
    pub(crate) fn frames(&mut self, rng: &mut DeterministicRng) -> Vec<Vec<u8>> {
        let forge = (self.kind == StateKind::Forge)
            .then(|| {
                self.forge
                    .as_mut()
                    .map(|aux| vec![aux.submit_local(MOVE_KIND, &ZERO_DELTA)])
            })
            .flatten()
            .unwrap_or_default();
        let flood = (self.kind == StateKind::Flood)
            .then(|| (!self.flood.is_empty()).then_some(()))
            .flatten()
            .map(|()| {
                let f = self.flood[self.cursor % self.flood.len()].clone();
                self.cursor += 1;
                vec![f]
            })
            .unwrap_or_default();
        let out_of_window = (self.kind == StateKind::OutOfWindow)
            .then(|| self.first.clone())
            .flatten()
            .into_iter()
            .collect::<Vec<_>>();
        let malformed = (self.kind == StateKind::Malformed)
            .then(|| {
                (0..12)
                    .map(|_| rng.next_bounded(256) as u8)
                    .collect::<Vec<u8>>()
            })
            .into_iter()
            .collect::<Vec<_>>();
        [forge, flood, out_of_window, malformed].concat()
    }
}

/// A distinct off-roster seed for a forging cheater.
fn forge_seed(master: u64, cheater_id: u64) -> [u8; 32] {
    let mut s = [0xC3u8; 32];
    s[..8].copy_from_slice(&master.to_le_bytes());
    s[8..16].copy_from_slice(&cheater_id.to_le_bytes());
    s
}

/// Validly-signed frames at ticks far beyond the admission window, so they are
/// always `out_of_window` for the whole run (the window horizon is 256; `+512`
/// clears it for any run shorter than that margin).
fn flood_frames(
    cheater_id: u64,
    cheater_key: &SigningKey,
    roster: &[(u64, VerifyingKey)],
    ticks: u64,
) -> Vec<Vec<u8>> {
    let mut aux = NetcodeApi::new(cheater_id, cheater_key.clone(), roster);
    let base = ticks + 512;
    (0..base).for_each(|_| {
        aux.submit_local(MOVE_KIND, &ZERO_DELTA); // advance the local tick cursor
    });
    (0..32)
        .map(|_| aux.submit_local(MOVE_KIND, &ZERO_DELTA))
        .collect()
}
