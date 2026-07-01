//! [`sample_until_valid`] — the generative counterpart to validation.
//!
//! Validation asks "is this artifact good?"; generation under constraints asks
//! "keep drawing until one is." That rejection-sampling loop — *draw a candidate,
//! test it, keep the first that passes, else fall back to the last drawn* — is a
//! generic, domain-free combinator: it knows nothing about what a candidate is or
//! why it passes. The distributions a caller draws from and the band a candidate
//! must land in are the caller's content; the bounded "sample until valid" control
//! flow is shared substrate, so it lives here beside the constraint machinery.
//!
//! It is **bounded** (at most `attempts` draws, `attempts` clamped so at least one
//! draw always happens), **short-circuiting** (it stops drawing the instant a
//! candidate passes, so a caller's RNG stream advances by exactly the draws it
//! would with a hand-rolled reject loop), and **branchless** (the loop is an
//! iterator `try_fold`; the keep-first / keep-last decision is combinator
//! selection, never `if`/`match`).

/// Draw up to `attempts` candidates with `sample`, returning the first for which
/// `valid` is true, or — if none pass — the last one drawn. Never panics:
/// `attempts` is clamped to at least one, so a candidate is always produced and
/// returned even at `attempts` `0` or `1`.
///
/// Draws happen in order (candidate 1, then 2, …) and stop the moment a candidate
/// passes, so wiring this over a deterministic random source advances that source
/// by exactly the draws a hand-written reject loop would.
///
/// The mechanism: draw the first candidate as the fold seed, then for each further
/// attempt either **break** with the accumulator when it already passes (`Err` is
/// the short-circuit residual carrying the winner) or **continue** by drawing the
/// next candidate as the new accumulator (`Ok`). On exhaustion the fold yields the
/// last-drawn accumulator (`Ok`); `unwrap_or_else` then collapses the `Result<T, T>`
/// — winner-or-last — to a single `T` without a branch.
pub fn sample_until_valid<T>(
    attempts: u32,
    mut sample: impl FnMut() -> T,
    valid: impl Fn(&T) -> bool,
) -> T {
    let first = sample();
    (1..attempts.max(1))
        .try_fold(first, |prev, _| {
            (!valid(&prev))
                .then(|| sample())
                .map(Ok)
                .unwrap_or_else(|| Err(prev))
        })
        .unwrap_or_else(|winner| winner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    /// A deterministic candidate source: yields `0, 1, 2, …` and counts draws, so
    /// a test can assert both the returned value and exactly how many draws ran
    /// (the short-circuit / RNG-advance guarantee). The draw counter is shared
    /// (`Rc`) so the reader observes the sampler's increments.
    fn counting_source() -> (impl Fn() -> u32, impl Fn() -> u32) {
        let next = Cell::new(0u32);
        let draws = Rc::new(Cell::new(0u32));
        let draws_read = {
            let draws = Rc::clone(&draws);
            move || draws.get()
        };
        let sample = move || {
            draws.set(draws.get() + 1);
            let v = next.get();
            next.set(v + 1);
            v
        };
        (sample, draws_read)
    }

    /// A predicate no candidate satisfies. Named (not an inline closure) so the
    /// "none pass" test executes its body while the clamped edge tests
    /// (`attempts` 0/1, where the empty range never invokes the predicate) can
    /// reuse the *same* function without leaving an unexecuted closure behind.
    fn rejects_all(_candidate: &u32) -> bool {
        false
    }

    #[test]
    fn returns_first_candidate_when_it_passes() {
        // Candidate 0 passes immediately -> exactly one draw, no further sampling.
        let (sample, draws) = counting_source();
        let got = sample_until_valid(8, sample, |&c| c == 0);
        assert_eq!(got, 0);
        assert_eq!(
            draws(),
            1,
            "must stop drawing at the first passing candidate"
        );
    }

    #[test]
    fn returns_kth_candidate_and_stops_there() {
        // Candidates 0,1,2 fail; 3 passes -> four draws, returns 3, no fifth draw.
        let (sample, draws) = counting_source();
        let got = sample_until_valid(16, sample, |&c| c == 3);
        assert_eq!(got, 3);
        assert_eq!(draws(), 4, "must stop the instant a candidate passes");
    }

    #[test]
    fn returns_last_drawn_when_none_pass() {
        // Nothing passes -> draws all `attempts` (0..=4) and returns the last (4).
        let (sample, draws) = counting_source();
        let got = sample_until_valid(5, sample, rejects_all);
        assert_eq!(got, 4, "falls back to the last drawn candidate");
        assert_eq!(draws(), 5, "exhausts exactly `attempts` draws");
    }

    #[test]
    fn attempts_zero_is_clamped_to_a_single_draw() {
        // `attempts = 0` still draws once and returns it, even though it fails.
        let (sample, draws) = counting_source();
        let got = sample_until_valid(0, sample, rejects_all);
        assert_eq!(got, 0);
        assert_eq!(draws(), 1, "attempts is clamped so >=1 draw always happens");
    }

    #[test]
    fn attempts_one_draws_once_and_returns_it() {
        // `attempts = 1`: the single draw is both the first and the last, so it is
        // returned whether or not it would pass.
        let (sample, draws) = counting_source();
        let got = sample_until_valid(1, sample, rejects_all);
        assert_eq!(got, 0);
        assert_eq!(draws(), 1);
    }

    #[test]
    fn is_deterministic_for_the_same_source_and_predicate() {
        let (sample_a, _) = counting_source();
        let (sample_b, _) = counting_source();
        let a = sample_until_valid(10, sample_a, |&c| c >= 2);
        let b = sample_until_valid(10, sample_b, |&c| c >= 2);
        assert_eq!(a, b);
        assert_eq!(a, 2, "candidate 2 is the first to satisfy c >= 2");
    }
}
