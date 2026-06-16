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

/// The live state backing a peer's misbehavior.
#[derive(Debug)]
pub(crate) enum CheatState {
    /// Plays fair.
    None,
    /// An impersonator session: each frame claims another peer but is signed by
    /// an off-roster key → the victim rejects it as `bad_signature`. Boxed: a
    /// session is far larger than the other variants.
    Forge(Box<NetcodeApi>),
    /// Pre-generated, validly-signed frames whose ticks sit far beyond the
    /// admission window → dropped as `out_of_window`, victim buffer stays bounded.
    Flood { frames: Vec<Vec<u8>>, cursor: usize },
    /// Replays the cheater's own tick-0 frame forever; once the victim has
    /// confirmed past it, it is a past tick → `out_of_window`.
    OutOfWindow { first: Option<Vec<u8>> },
    /// Emits structural garbage → the victim's `ingest` fails to decode it.
    Malformed,
    /// Simulates a divergent world → honest peers' `reconcile` flags the desync.
    CorruptSim,
}

impl CheatState {
    /// Build the cheat state for a peer (or `None` if it is honest).
    pub(crate) fn build(
        kind: Option<CheatKind>,
        cheater_id: u64,
        cheater_key: &SigningKey,
        roster: &[(u64, VerifyingKey)],
        peers: usize,
        ticks: u64,
        seed: u64,
    ) -> Self {
        match kind {
            None => CheatState::None,
            Some(CheatKind::ForgeOthers) => {
                // Claim a *different* peer, signed by a key that is not in the roster.
                let target = (cheater_id % peers as u64) + 1;
                let off_key = SigningKey::from_seed(forge_seed(seed, cheater_id));
                let aux = NetcodeApi::new(
                    target,
                    off_key.clone(),
                    &[(target, off_key.verifying_key())],
                );
                CheatState::Forge(Box::new(aux))
            }
            Some(CheatKind::Flood) => CheatState::Flood {
                frames: flood_frames(cheater_id, cheater_key, roster, ticks),
                cursor: 0,
            },
            Some(CheatKind::OutOfWindow) => CheatState::OutOfWindow { first: None },
            Some(CheatKind::Malformed) => CheatState::Malformed,
            Some(CheatKind::CorruptSim) => CheatState::CorruptSim,
        }
    }

    /// Whether this peer simulates a divergent world.
    pub(crate) fn corrupts_sim(&self) -> bool {
        matches!(self, CheatState::CorruptSim)
    }

    /// Note this peer's own just-submitted frame (so `OutOfWindow` can replay it).
    pub(crate) fn note_submit(&mut self, frame: &[u8]) {
        if let CheatState::OutOfWindow { first } = self {
            if first.is_none() {
                *first = Some(frame.to_vec());
            }
        }
    }

    /// The extra bad frames to broadcast this tick.
    pub(crate) fn frames(&mut self, rng: &mut DeterministicRng) -> Vec<Vec<u8>> {
        match self {
            CheatState::None | CheatState::CorruptSim => Vec::new(),
            CheatState::Forge(aux) => vec![aux.submit_local(MOVE_KIND, &ZERO_DELTA)],
            CheatState::Flood { frames, cursor } => {
                if frames.is_empty() {
                    return Vec::new();
                }
                let f = frames[*cursor % frames.len()].clone();
                *cursor += 1;
                vec![f]
            }
            CheatState::OutOfWindow { first } => first.clone().into_iter().collect(),
            CheatState::Malformed => {
                let mut bytes = vec![0u8; 12];
                for b in &mut bytes {
                    *b = rng.next_bounded(256) as u8;
                }
                vec![bytes]
            }
        }
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
    for _ in 0..base {
        aux.submit_local(MOVE_KIND, &ZERO_DELTA); // advance the local tick cursor
    }
    (0..32)
        .map(|_| aux.submit_local(MOVE_KIND, &ZERO_DELTA))
        .collect()
}
