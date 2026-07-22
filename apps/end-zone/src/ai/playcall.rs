//! The pre-snap defensive selector: given the offense's intent and the game
//! situation, pick a *sensible* [`DefensiveCall`] — and vary the choice and its
//! alignment deterministically so the defense never lines up identically twice.
//!
//! This is the huddle-time counterpart to [`crate::ai::overseer`]: the overseer
//! adapts *after* the snap; this picks the call the defense breaks the huddle
//! with. It is a pure function of the situation plus a variation key derived
//! from run state (`seed`, snap index, down), so a run replays bit-for-bit while
//! the same offensive play still draws a different-but-reasonable answer snap to
//! snap. Nothing here is random — the key IS the only variation source.

use crate::data::playbook::defensive_calls;
use crate::data::{Coverage, DefenseFront, DefensiveCall, OffenseTag};

/// The number of calls in the defensive playbook.
const CALL_COUNT: usize = 5;

/// A deterministic 64-bit mix (splitmix64 finalizer) — the one variation source.
fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The variation key for one snap: folds the run seed, the snap index, and the
/// down into a single deterministic value the selector draws from.
pub fn variation_key(seed: u64, snap_index: u64, down: u8) -> u64 {
    mix(seed ^ mix(snap_index.wrapping_mul(0x100_0001) ^ u64::from(down)))
}

/// A signed unit jitter in `[-1, 1]` for stream `n` of the key.
fn signed_unit(key: u64, n: u64) -> f32 {
    let h = mix(key ^ n.wrapping_mul(0x9E37_79B9));
    (((h & 0xFFFF) as f32) / 32_767.5) - 1.0
}

/// How appropriate each defensive call is for the situation. Higher is more
/// likely to be chosen; a zero weight is never picked. This is the "make sense"
/// half — the key only ever chooses *among* the sensible answers.
pub fn call_weights(tag: OffenseTag, down: u8, distance: f32, heat: u8) -> [u32; CALL_COUNT] {
    // Index order matches `defensive_calls()`:
    // 0 COVER MAN, 1 COVER ZONE, 2 NICKEL ZONE, 3 EDGE BLITZ, 4 PREVENT.
    let mut w: [i32; CALL_COUNT] = match tag {
        OffenseTag::QuickPass => [3, 1, 2, 3, 0],
        OffenseTag::DeepPass => [2, 3, 2, 1, 1],
        OffenseTag::Flood => [1, 3, 3, 1, 1],
    };

    let long = distance >= 8.0;
    let short = distance <= 3.0;
    if long {
        w[1] += 2; // more zone help
        w[2] += 1;
        w[4] += 2; // prevent is on the table
        w[3] -= 1; // blitz is riskier when they only need a chunk
        w[0] -= 1;
    }
    if short {
        w[0] += 2; // tight man
        w[3] += 2; // sell out to stop it
        w[4] -= 3; // never prevent a short-yardage down
        w[1] -= 1;
    }
    // Fourth down sharpens toward the extremes.
    if down >= 4 {
        w[if long { 4 } else { 3 }] += 2;
    }

    // Heat is the aggression dial: hot defenses blitz and press; cool ones sit
    // back. Bounded so the situation still dominates the choice.
    let h = i32::from(heat);
    w[3] += h / 2;
    w[0] += h / 3;
    w[4] -= h / 2;

    let mut out = [0u32; CALL_COUNT];
    for i in 0..CALL_COUNT {
        out[i] = w[i].max(0) as u32;
    }
    // A degenerate all-zero situation always has a plain man answer.
    if out.iter().all(|&x| x == 0) {
        out[0] = 1;
    }
    out
}

/// Pick a call index from the weights using the variation key — a deterministic
/// weighted choice, so the same weights + key always land on the same call, but
/// a different key (a different snap) lands elsewhere among the sensible set.
fn weighted_pick(weights: &[u32; CALL_COUNT], key: u64) -> usize {
    let total: u32 = weights.iter().sum();
    let mut roll = (mix(key ^ 0xF00D) % u64::from(total)) as u32;
    for (i, &weight) in weights.iter().enumerate() {
        if roll < weight {
            return i;
        }
        roll -= weight;
    }
    0
}

/// Apply bounded alignment jitter to a call's formation so no two snaps show
/// the exact same picture. Front players (near the line) move less than deep
/// defenders, so the front stays sound while the coverage disguises itself.
fn jitter_alignment(call: &mut DefensiveCall, key: u64) {
    let front_scale = match call.front {
        DefenseFront::Base => 0.6,
        DefenseFront::Nickel => 0.7,
        DefenseFront::Dime => 0.8,
    };
    for (i, slot) in call.formation.slots.iter_mut().enumerate() {
        let n = i as u64;
        let on_line = slot.position.downfield <= 2.5;
        let lateral_bound = if on_line { 0.5 } else { 0.9 } * front_scale;
        let downfield_bound = if on_line { 0.2 } else { 0.7 } * front_scale;
        slot.position.lateral += signed_unit(key, n * 2) * lateral_bound;
        slot.position.downfield += signed_unit(key, n * 2 + 1) * downfield_bound;
    }
}

/// The chosen call plus which playbook index it came from (for inspection/HUD).
#[derive(Debug, Clone, PartialEq)]
pub struct DefenseSelection {
    pub index: usize,
    pub call: DefensiveCall,
}

/// Select the defensive call for a snap: a sensible answer to the offense's
/// `tag` and the down/distance/heat, chosen and aligned deterministically from
/// `key`. Same situation + same key → same call; a fresh key → a fresh look.
pub fn select_defense(
    tag: OffenseTag,
    down: u8,
    distance: f32,
    heat: u8,
    key: u64,
) -> DefenseSelection {
    let weights = call_weights(tag, down, distance, heat);
    let index = weighted_pick(&weights, key);
    let mut call = defensive_calls()[index].clone();
    jitter_alignment(&mut call, key);
    DefenseSelection { index, call }
}

/// Whether a call presses (man) or sits (zone) — a compact read for the HUD or
/// tests, without exposing the full assignment list.
pub fn is_pressing(call: &DefensiveCall) -> bool {
    matches!(call.coverage, Coverage::Man | Coverage::Blitz)
}
