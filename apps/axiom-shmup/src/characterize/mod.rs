//! The characterization harness: a frozen fingerprint of what this app does
//! *before* any of it is converted to a table.
//!
//! Not a port of anything. The source has no equivalent because the source was
//! never refactored under a fan-out.
//!
//! # Why this exists
//!
//! The port had an external oracle: run the original JavaScript under Node and
//! compare. **Datafication has no external oracle** — the "before" behaviour *is*
//! the current Rust. And the agents doing the conversion cannot build, so they
//! cannot produce an oracle of their own either.
//!
//! So the oracle is enumerated and frozen up front, and the scope of the work is
//! defined *by* the oracle: a recipe with no probe case does not get converted.
//!
//! # What a case fingerprints
//!
//! **Everything observable, not just the thing being converted.** A conversion
//! that emits the right number of particles into the wrong pool passes any test
//! that only inspects `add.raw()`. So a case digests every pool, every decal
//! stream, the light pool, and the RNG state *after* — because the shared random
//! stream is this game's identity, and a burst that takes one extra draw shifts
//! every later effect in the frame invisibly.
//!
//! # The rule the whole build-free scheme rests on
//!
//! > A conversion agent may only convert a recipe this ledger already covers.
//!
//! The ledger is written once, by the orchestrator, and agents may not
//! regenerate it. An agent that regenerates the oracle it is checked against has
//! proved nothing.
//!
//! # Reading a failure
//!
//! A row is `case channel count digest`. The `count` is not redundant with the
//! digest: a digest tells you *something* moved, a count tells you *how many
//! emissions* moved, and that is the difference between a five-minute triage and
//! an hour of it. A differing count is almost always a draw that was taken
//! unconditionally where the hand-written code took it lazily.

pub mod probes;

use std::fmt::Write as _;

use axiom_kernel::StableHash;

/// One canonical little-endian encoding, so a digest means one thing.
///
/// Floats go in as their IEEE **bit patterns**, never as decimal text. That is
/// what makes `-0.0` distinguishable from `0.0` and `NaN` reproducible — the
/// trap the port learned the expensive way when `JSON.stringify(NaN)` came back
/// as `null`. Every slice is length-prefixed, so a short buffer and a
/// zero-padded one are different.
#[derive(Default)]
pub struct Fingerprint {
    words: Vec<u64>,
}

impl Fingerprint {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn f32s(mut self, values: &[f32]) -> Self {
        self.words.push(values.len() as u64);
        self.words
            .extend(values.iter().map(|v| u64::from(v.to_bits())));
        self
    }

    pub fn f64s(mut self, values: &[f64]) -> Self {
        self.words.push(values.len() as u64);
        self.words.extend(values.iter().map(|v| v.to_bits()));
        self
    }

    pub fn u32s(mut self, values: &[u32]) -> Self {
        self.words.push(values.len() as u64);
        self.words.extend(values.iter().map(|v| u64::from(*v)));
        self
    }

    pub fn u64s(mut self, values: &[u64]) -> Self {
        self.words.push(values.len() as u64);
        self.words.extend_from_slice(values);
        self
    }

    pub fn digest(&self) -> u64 {
        StableHash::of_words(&self.words).raw()
    }
}

/// One observable channel of one case.
pub struct Channel {
    pub name: &'static str,
    pub count: u64,
    pub digest: u64,
}

impl Channel {
    pub fn new(name: &'static str, count: u64, fp: Fingerprint) -> Self {
        Self {
            name,
            count,
            digest: fp.digest(),
        }
    }
}

/// A case: a deterministic invocation of a recipe with pinned arguments and a
/// pinned seed, plus every channel it is observable through.
pub struct Capture {
    pub case: &'static str,
    pub channels: Vec<Channel>,
    /// Bounded raw dump, for triage. See [`Capture::witness`].
    pub witness: Option<Vec<f32>>,
}

impl Capture {
    pub fn new(case: &'static str, channels: Vec<Channel>) -> Self {
        Self {
            case,
            channels,
            witness: None,
        }
    }

    /// Attach a bounded raw dump.
    ///
    /// A digest that fails tells you *something* moved but not *which slot*. So
    /// a case may carry the first few emissions verbatim, which turns "the
    /// digest differs" into "slot 17 of particle 1 differs" — one literal
    /// transcribed wrong, versus a whole stride meaning the wrong emit pool,
    /// versus every slot from index k onward meaning a draw-order shift inside
    /// the burst.
    ///
    /// Bounded on purpose: the world's 585,630 triangles are digested, never
    /// dumped.
    pub fn witness(mut self, raw: &[f32], emissions: usize) -> Self {
        self.witness = Some(raw[..(emissions * WITNESS_STRIDE).min(raw.len())].to_vec());
        self
    }

    /// The ledger lines this case contributes.
    pub fn to_lines(&self) -> String {
        let mut s = String::new();
        for c in &self.channels {
            let _ = writeln!(s, "{} {} {} {:016x}", self.case, c.name, c.count, c.digest);
        }
        s
    }

    /// Assert this capture matches the frozen ledger.
    ///
    /// This is the assertion a conversion agent writes. It never asserts a value
    /// the agent reasoned out — only that the recipe still does what it did.
    pub fn assert_matches(&self, ledger: &Ledger) {
        assert!(
            !self.channels.is_empty(),
            "case `{}` observes nothing; a case with no channel proves nothing",
            self.case
        );
        for c in &self.channels {
            let want = ledger.row(self.case, c.name).unwrap_or_else(|| {
                panic!(
                    "no ledger row for `{} {}` — this recipe is not covered, so it \
                     must not be converted yet (see docs/work-manifests/\
                     shmup-datafication/01-agent-brief.md)",
                    self.case, c.name
                )
            });
            assert_eq!(
                c.count, want.0,
                "`{} {}`: emission COUNT moved ({} -> {}). A differing count is \
                 definitive: the driver emits a different number of things. Look \
                 first for `unwrap_or(rng.range(..))` where the source had \
                 `unwrap_or_else(|| rng.range(..))`, and for a band whose row is \
                 missing.",
                self.case, c.name, want.0, c.count
            );
            assert_eq!(
                c.digest, want.1,
                "`{} {}`: {} emission(s), same count, different bytes. Open \
                 tests/golden/witness/{}.hex and diff slot by slot: one slot is a \
                 literal transcribed wrong, a whole stride is the wrong emit \
                 pool, every slot from index k onward is a draw-order shift \
                 inside the burst.",
                self.case, c.name, c.count, self.case
            );
        }
    }
}

/// A particle record's stride in `f32`s — `crate::fx::particles::STRIDE`.
///
/// Restated here rather than imported so the witness dump's shape is stated
/// where it is read. If the two ever disagree, the test below fails.
const WITNESS_STRIDE: usize = 32;

/// The frozen ledger for one area, parsed from a committed golden.
///
/// Loaded through `include_str!`, so there is no runtime file IO, no
/// working-directory dependence, and a missing golden is a **compile error**
/// rather than a silently-skipped test.
pub struct Ledger {
    rows: Vec<(String, String, u64, u64)>,
}

impl Ledger {
    /// Parse a ledger from its text. Blank lines and `#` comments are ignored.
    pub fn parse(text: &str) -> Self {
        let rows = text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(|l| {
                let mut it = l.split_whitespace();
                let case = it.next().expect("ledger row: case").to_string();
                let channel = it.next().expect("ledger row: channel").to_string();
                let count = it
                    .next()
                    .and_then(|v| v.parse().ok())
                    .expect("ledger row: count");
                let digest = it
                    .next()
                    .and_then(|v| u64::from_str_radix(v, 16).ok())
                    .expect("ledger row: digest");
                (case, channel, count, digest)
            })
            .collect();
        Self { rows }
    }

    fn row(&self, case: &str, channel: &str) -> Option<(u64, u64)> {
        self.rows
            .iter()
            .find(|(c, ch, _, _)| c == case && ch == channel)
            .map(|(_, _, count, digest)| (*count, *digest))
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The `fx` area's frozen ledger.
    pub fn fx() -> Self {
        Self::parse(include_str!("../../tests/golden/fx.ledger"))
    }
}

/// Write every area's ledger and witness dumps to `tests/golden/`.
///
/// Run deliberately, never as part of an ordinary test pass:
///
/// ```text
/// SHMUP_RECAPTURE=1 cargo test -p axiom-shmup --lib characterize::recapture
/// ```
///
/// **Agents may not run this.** The ledger is the oracle a conversion is checked
/// against; an agent that regenerates its own oracle has proved nothing.
/// Datafication is byte-identical by definition, so a conversion that
/// legitimately changes the ledger is a conversion that is wrong.
#[test]
fn recapture() {
    if std::env::var("SHMUP_RECAPTURE").is_err() {
        return;
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    std::fs::create_dir_all(root.join("witness")).expect("golden dir");

    let areas: [(&str, fn() -> Vec<Capture>); 1] = [("fx", probes::fx::all)];
    for (area, all) in areas {
        let captures = all();
        let mut text = format!(
            "# {area} — frozen by characterize::recapture. Do not hand-edit.\n\
             # case channel count digest\n"
        );
        for cap in &captures {
            text.push_str(&cap.to_lines());
            if let Some(w) = &cap.witness {
                let hex: String = w
                    .chunks(8)
                    .map(|row| {
                        row.iter()
                            .map(|v| format!("{:08x}", v.to_bits()))
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                std::fs::write(root.join(format!("witness/{}.hex", cap.case)), hex + "\n")
                    .expect("witness");
            }
        }
        std::fs::write(root.join(format!("{area}.ledger")), text).expect("ledger");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The standing regression suite.** Every frozen `fx` case, replayed
    /// against the committed ledger.
    ///
    /// This is the test a conversion agent's own in-file test duplicates for one
    /// case. Having it here as well means a conversion that lands without its own
    /// test still cannot pass silently.
    #[test]
    fn every_frozen_fx_case_still_does_what_it_did() {
        let ledger = Ledger::fx();
        assert!(!ledger.is_empty(), "the fx ledger is empty — run recapture");
        for cap in probes::fx::all() {
            cap.assert_matches(&ledger);
        }
    }

    /// The ledger has no rows for cases that no longer exist, and no cases
    /// without rows. A stale row is how a deleted recipe stops being noticed.
    #[test]
    fn the_ledger_and_the_case_list_agree() {
        let ledger = Ledger::fx();
        let cases = probes::fx::all();
        let rows: usize = cases.iter().map(|c| c.channels.len()).sum();
        assert_eq!(
            ledger.len(),
            rows,
            "{} ledger row(s) for {} case(s) worth of channels — a stale row means \
             a recipe was deleted and nothing noticed",
            ledger.len(),
            rows
        );
    }

    #[test]
    fn the_witness_stride_matches_the_particle_record() {
        assert_eq!(WITNESS_STRIDE, crate::fx::particles::STRIDE);
    }

    #[test]
    fn a_fingerprint_distinguishes_negative_zero_from_zero() {
        let a = Fingerprint::new().f32s(&[0.0]).digest();
        let b = Fingerprint::new().f32s(&[-0.0]).digest();
        assert_ne!(a, b, "bit patterns, not values — -0.0 == 0.0 numerically");
    }

    #[test]
    fn a_fingerprint_distinguishes_a_short_buffer_from_a_zero_padded_one() {
        let a = Fingerprint::new().f32s(&[1.0]).digest();
        let b = Fingerprint::new().f32s(&[1.0, 0.0]).digest();
        assert_ne!(a, b, "slices are length-prefixed");
    }

    #[test]
    fn nan_fingerprints_reproducibly() {
        let a = Fingerprint::new().f64s(&[f64::NAN]).digest();
        let b = Fingerprint::new().f64s(&[f64::NAN]).digest();
        assert_eq!(a, b);
    }

    #[test]
    fn a_ledger_round_trips_through_its_text() {
        let cap = Capture::new(
            "demo",
            vec![Channel::new("add", 3, Fingerprint::new().f32s(&[1.0, 2.0]))],
        );
        let ledger = Ledger::parse(&cap.to_lines());
        cap.assert_matches(&ledger);
        assert_eq!(ledger.len(), 1);
    }

    #[test]
    fn comments_and_blank_lines_are_ignored() {
        let ledger = Ledger::parse("# a comment\n\n  demo add 3 00000000000000ff\n");
        assert_eq!(ledger.len(), 1);
        assert_eq!(ledger.row("demo", "add"), Some((3, 255)));
    }

    #[test]
    #[should_panic(expected = "emission COUNT moved")]
    fn a_changed_count_fails_loudly() {
        let ledger = Ledger::parse("demo add 3 00000000000000ff");
        Capture::new("demo", vec![Channel::new("add", 4, Fingerprint::new())])
            .assert_matches(&ledger);
    }

    #[test]
    #[should_panic(expected = "no ledger row")]
    fn an_uncovered_recipe_refuses_to_pass() {
        let ledger = Ledger::parse("other add 3 00000000000000ff");
        Capture::new("demo", vec![Channel::new("add", 3, Fingerprint::new())])
            .assert_matches(&ledger);
    }

    #[test]
    #[should_panic(expected = "observes nothing")]
    fn a_case_with_no_channel_is_not_a_proof() {
        Capture::new("demo", vec![]).assert_matches(&Ledger::parse(""));
    }

    #[test]
    fn a_witness_is_bounded_by_the_emissions_asked_for() {
        let raw = vec![1.0_f32; WITNESS_STRIDE * 10];
        let cap = Capture::new("demo", vec![]).witness(&raw, 2);
        assert_eq!(cap.witness.expect("witness").len(), WITNESS_STRIDE * 2);
    }
}
