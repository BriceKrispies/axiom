//! A typed key/value pair attached to a log record.

/// The value side of a [`LogField`]. Private: callers construct fields through
/// the typed constructors and read them through the typed accessors, so the
/// representation can evolve without widening the public surface.
#[derive(Debug, Clone, PartialEq, Eq)]
enum FieldValue {
    I64(i64),
    U64(u64),
    Bool(bool),
    Str(&'static str),
}

/// A single structured field on a log record: a static key plus a typed value.
///
/// Fields are pure data with no formatting or allocation of dynamic strings
/// (string values are `&'static`), keeping records cheap and deterministic.
/// Equality is structural, so identical fields always compare equal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogField {
    key: &'static str,
    value: FieldValue,
}

impl LogField {
    /// A signed integer field.
    pub const fn i64(key: &'static str, value: i64) -> Self {
        LogField {
            key,
            value: FieldValue::I64(value),
        }
    }

    /// An unsigned integer field.
    pub const fn u64(key: &'static str, value: u64) -> Self {
        LogField {
            key,
            value: FieldValue::U64(value),
        }
    }

    /// A boolean field.
    pub const fn bool(key: &'static str, value: bool) -> Self {
        LogField {
            key,
            value: FieldValue::Bool(value),
        }
    }

    /// A static-string field.
    pub const fn str(key: &'static str, value: &'static str) -> Self {
        LogField {
            key,
            value: FieldValue::Str(value),
        }
    }

    /// The field key.
    pub const fn key(&self) -> &'static str {
        self.key
    }

    /// The signed-integer value, if this field holds one.
    pub const fn as_i64(&self) -> Option<i64> {
        match self.value {
            FieldValue::I64(v) => Some(v),
            _ => None,
        }
    }

    /// The unsigned-integer value, if this field holds one.
    pub const fn as_u64(&self) -> Option<u64> {
        match self.value {
            FieldValue::U64(v) => Some(v),
            _ => None,
        }
    }

    /// The boolean value, if this field holds one.
    pub const fn as_bool(&self) -> Option<bool> {
        match self.value {
            FieldValue::Bool(v) => Some(v),
            _ => None,
        }
    }

    /// The static-string value, if this field holds one.
    pub const fn as_str(&self) -> Option<&'static str> {
        match self.value {
            FieldValue::Str(v) => Some(v),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_accessors_match_constructor() {
        let f = LogField::i64("delta", -3);
        assert_eq!(f.key(), "delta");
        assert_eq!(f.as_i64(), Some(-3));
        assert_eq!(f.as_u64(), None);
        assert_eq!(f.as_bool(), None);
        assert_eq!(f.as_str(), None);
    }

    #[test]
    fn each_constructor_holds_its_own_kind() {
        assert_eq!(LogField::u64("n", 5).as_u64(), Some(5));
        assert_eq!(LogField::bool("ok", true).as_bool(), Some(true));
        assert_eq!(LogField::str("name", "axiom").as_str(), Some("axiom"));
    }

    #[test]
    fn equality_is_structural() {
        assert_eq!(LogField::i64("a", 1), LogField::i64("a", 1));
        assert_ne!(LogField::i64("a", 1), LogField::i64("a", 2));
        assert_ne!(LogField::i64("a", 1), LogField::u64("a", 1));
    }
}
