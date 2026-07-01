//! `HostSessionParams`: an opaque, stably-ordered session-parameter map.

use crate::host_param_value::HostParamValue;

/// An opaque key→value map of session parameters with a **stable iteration
/// order** (SPEC-12 §6).
/// Order is a determinism requirement: the projected JS record and any logged
/// form must be reproducible, so the entries are held in an order-preserving
/// vector (insertion order), never a hash map with random iteration. The host
/// boundary only carries these values — the game interprets them.
/// Keys are not de-duplicated; [`Self::with`] appends in order and
/// [`Self::get`] returns the first match, keeping construction branchless.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HostSessionParams {
    entries: Vec<(String, HostParamValue)>,
}

impl HostSessionParams {
    /// An empty parameter map.
    pub fn new() -> Self {
        HostSessionParams::default()
    }

    /// Append `key → value`, preserving insertion order, and return the map.
    pub fn with(mut self, key: String, value: HostParamValue) -> Self {
        self.entries.push((key, value));
        self
    }

    /// The number of parameters.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the map has no parameters.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The value for `key`, or `None` if absent (first match wins).
    pub fn get(&self, key: &str) -> Option<&HostParamValue> {
        self.entries
            .iter()
            .find(|(stored, _)| stored.as_str() == key)
            .map(|(_, value)| value)
    }

    /// The parameters in stable insertion order.
    pub fn entries(&self) -> &[(String, HostParamValue)] {
        &self.entries
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::score::Score;

    fn sample() -> HostSessionParams {
        HostSessionParams::new()
            .with(String::from("mode"), HostParamValue::Text(String::from("ranked")))
            .with(String::from("threshold"), HostParamValue::Number(Score::new(9.0)))
    }

    #[test]
    fn empty_map_reports_empty() {
        let params = HostSessionParams::new();
        assert_eq!(params.len(), 0);
        assert!(params.is_empty());
        assert_eq!(params.entries(), &[]);
        assert_eq!(params.get("missing"), None);
    }

    #[test]
    fn with_preserves_insertion_order() {
        let params = sample();
        assert_eq!(params.len(), 2);
        assert!(!params.is_empty());
        assert_eq!(
            params.entries(),
            &[
                (String::from("mode"), HostParamValue::Text(String::from("ranked"))),
                (String::from("threshold"), HostParamValue::Number(Score::new(9.0))),
            ]
        );
    }

    #[test]
    fn get_returns_present_value_and_none_for_absent_key() {
        let params = sample();
        assert_eq!(
            params.get("threshold"),
            Some(&HostParamValue::Number(Score::new(9.0)))
        );
        assert_eq!(params.get("absent"), None);
    }

    #[test]
    fn equal_inputs_build_equal_maps_and_order_matters() {
        assert_eq!(sample(), sample());
        let reversed = HostSessionParams::new()
            .with(String::from("threshold"), HostParamValue::Number(Score::new(9.0)))
            .with(String::from("mode"), HostParamValue::Text(String::from("ranked")));
        assert_ne!(sample(), reversed);
    }
}
