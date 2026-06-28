//! `HostParamValue`: one opaque session-parameter value (text or number).

use crate::score::Score;

/// One value in a [`crate::HostSessionParams`] map: either opaque text or an
/// opaque [`Score`] number.
///
/// The host boundary never interprets a param — the *game* does (a uid, a prize
/// threshold, a mode). The engine only carries it as data and never branches on
/// it (SPEC-12 §6). Projected to JS as the `string | number` half of
/// `Record<string, string | number>`; the number arm rides the same [`Score`]
/// boundary as an outcome score, so no naked `f64` ever reaches the surface.
#[derive(Debug, Clone, PartialEq)]
pub enum HostParamValue {
    /// An opaque string value.
    Text(String),
    /// An opaque numeric value (the `Score` f64 boundary).
    Number(Score),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_values_round_trip_and_compare() {
        let value = HostParamValue::Text(String::from("alpha"));
        assert_eq!(value, HostParamValue::Text(String::from("alpha")));
        assert_ne!(value, HostParamValue::Text(String::from("beta")));
    }

    #[test]
    fn number_values_round_trip_and_compare() {
        let value = HostParamValue::Number(Score::new(2.0));
        assert_eq!(value, HostParamValue::Number(Score::new(2.0)));
        assert_ne!(value, HostParamValue::Number(Score::new(3.0)));
    }

    #[test]
    fn text_and_number_are_distinct_variants() {
        assert_ne!(
            HostParamValue::Text(String::from("1")),
            HostParamValue::Number(Score::new(1.0))
        );
    }
}
