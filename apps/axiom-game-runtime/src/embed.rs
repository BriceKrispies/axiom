//! The embed seam binding (SPEC-12): decode an inbound [`HostSessionConfig`] and
//! latch the single outbound [`HostOutcome`].
//!
//! This is the **pure, native-testable core** of the host channel. The browser
//! API that actually carries these — `window.location.search` in, parent
//! `postMessage` out — lives in [`crate::wasm`], the `wasm32` platform edge.
//! Here there is no browser symbol and no clock: a config is decoded from a
//! string the platform arm already read, and the outcome is latched in memory
//! for the platform arm to forward. As an app this code is outside the engine's
//! branchless / coverage gates, but it is written branchlessly anyway (iterator
//! combinators over the query pairs, `Option::or` to latch) so the workspace
//! branch-ratchet stays put; it ships the slice test that proves the seam
//! end-to-end.

use axiom::prelude::{
    HostApi, HostOutcome, HostParamValue, HostSessionConfig, HostSessionParams, Score,
};

/// Decode a URL query string (`seed=123&mode=ranked&threshold=9.5`) into a
/// validated [`HostSessionConfig`].
///
/// The `seed` key is the determinism input (SPEC-12 §6), parsed as a `u64`
/// (absent or unparsable ⇒ `0`). Every other key becomes an opaque param: a
/// value that parses as an `f64` is a [`HostParamValue::Number`], otherwise
/// [`HostParamValue::Text`]. Parameter order follows the query string, which the
/// stable-ordered [`HostSessionParams`] preserves.
pub(crate) fn decode_session_config(raw_query: &str) -> HostSessionConfig {
    let trimmed = raw_query.strip_prefix('?').unwrap_or(raw_query);
    // Every non-empty `key=value` pair, in query order.
    let pairs: Vec<(&str, &str)> = trimmed
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let mut kv = pair.splitn(2, '=');
            (kv.next().unwrap_or(""), kv.next().unwrap_or(""))
        })
        .collect();
    // `seed` is the last `seed=` value parsed as u64 (absent/unparsable ⇒ 0).
    let seed = pairs
        .iter()
        .filter(|(key, _)| *key == "seed")
        .map(|(_, value)| value.parse::<u64>().unwrap_or(0))
        .last()
        .unwrap_or(0);
    // Every other non-empty key becomes a typed opaque param, order preserved.
    let params = pairs
        .iter()
        .filter(|(key, _)| (*key != "seed") & !key.is_empty())
        .fold(HostSessionParams::new(), |params, (key, value)| {
            let param = value
                .parse::<f64>()
                .map(|number| HostParamValue::Number(Score::new(number)))
                .unwrap_or_else(|_| HostParamValue::Text((*value).to_string()));
            params.with((*key).to_string(), param)
        });
    HostApi::new().session_config(seed, params)
}

/// Latches the engine's single terminal [`HostOutcome`].
///
/// `reportOutcome` is emit-exactly-once (SPEC-12 §4.2): the first report is
/// accepted and every later one is a no-op, so a game cannot report two terminal
/// states. The platform arm forwards the latched outcome to the host channel.
#[derive(Debug, Default)]
pub(crate) struct OutcomeLatch {
    reported: Option<HostOutcome>,
}

impl OutcomeLatch {
    /// An empty latch — nothing reported yet.
    pub(crate) fn new() -> Self {
        OutcomeLatch::default()
    }

    /// Accept `outcome` iff none has been latched yet; returns whether it was
    /// accepted (the first call is `true`, every later call `false`).
    pub(crate) fn report(&mut self, outcome: HostOutcome) -> bool {
        let accepted = self.reported.is_none();
        // Keep an already-latched outcome; otherwise latch this one.
        self.reported = self.reported.take().or(Some(outcome));
        accepted
    }

    /// The latched outcome, if one has been reported.
    pub(crate) fn reported(&self) -> Option<&HostOutcome> {
        self.reported.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_reads_seed_and_typed_params_in_order() {
        let config = decode_session_config("?seed=5&mode=ranked&threshold=9.5");
        assert_eq!(config.seed(), 5);
        assert_eq!(
            config.params().get("mode"),
            Some(&HostParamValue::Text(String::from("ranked")))
        );
        assert_eq!(
            config.params().get("threshold"),
            Some(&HostParamValue::Number(Score::new(9.5)))
        );
    }

    #[test]
    fn decode_defaults_seed_to_zero_when_absent_or_unparsable() {
        assert_eq!(decode_session_config("").seed(), 0);
        assert_eq!(decode_session_config("seed=not-a-number").seed(), 0);
    }

    #[test]
    fn latch_accepts_first_outcome_and_rejects_later_ones() {
        let host = HostApi::new();
        let first = host.outcome(true, Score::new(10.0));
        let mut latch = OutcomeLatch::new();
        assert!(latch.report(first.clone()));
        assert!(!latch.report(host.outcome(false, Score::new(0.0))));
        assert_eq!(latch.reported(), Some(&first));
    }
}
