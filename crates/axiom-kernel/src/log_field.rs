//! A typed key/value pair attached to a log record.

/// The value side of a [`LogField`]. Private: callers construct fields through
/// the typed constructors and read them through the typed accessors, so the
/// representation can evolve without widening the public surface.
///
/// A tagged struct rather than an enum: `kind` selects the live field and the
/// rest hold defaults (`0` / `false` / `""`), so payload extraction is
/// branchless. Derived equality compares all fields, which is correct because
/// the inactive fields are fixed at their defaults for a given `kind`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FieldValue {
    kind: u8,
    i64v: i64,
    u64v: u64,
    boolv: bool,
    strv: &'static str,
}

impl FieldValue {
    const KIND_I64: u8 = 0;
    const KIND_U64: u8 = 1;
    const KIND_BOOL: u8 = 2;
    const KIND_STR: u8 = 3;

    const fn i64(value: i64) -> Self {
        FieldValue {
            kind: Self::KIND_I64,
            i64v: value,
            u64v: 0,
            boolv: false,
            strv: "",
        }
    }

    const fn u64(value: u64) -> Self {
        FieldValue {
            kind: Self::KIND_U64,
            i64v: 0,
            u64v: value,
            boolv: false,
            strv: "",
        }
    }

    const fn bool(value: bool) -> Self {
        FieldValue {
            kind: Self::KIND_BOOL,
            i64v: 0,
            u64v: 0,
            boolv: value,
            strv: "",
        }
    }

    const fn str(value: &'static str) -> Self {
        FieldValue {
            kind: Self::KIND_STR,
            i64v: 0,
            u64v: 0,
            boolv: false,
            strv: value,
        }
    }
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
            value: FieldValue::i64(value),
        }
    }

    /// An unsigned integer field.
    pub const fn u64(key: &'static str, value: u64) -> Self {
        LogField {
            key,
            value: FieldValue::u64(value),
        }
    }

    /// A boolean field.
    pub const fn bool(key: &'static str, value: bool) -> Self {
        LogField {
            key,
            value: FieldValue::bool(value),
        }
    }

    /// A static-string field.
    pub const fn str(key: &'static str, value: &'static str) -> Self {
        LogField {
            key,
            value: FieldValue::str(value),
        }
    }

    /// The field key.
    pub const fn key(&self) -> &'static str {
        self.key
    }

    /// The signed-integer value, if this field holds one.
    pub fn as_i64(&self) -> Option<i64> {
        (self.value.kind == FieldValue::KIND_I64).then_some(self.value.i64v)
    }

    /// The unsigned-integer value, if this field holds one.
    pub fn as_u64(&self) -> Option<u64> {
        (self.value.kind == FieldValue::KIND_U64).then_some(self.value.u64v)
    }

    /// The boolean value, if this field holds one.
    pub fn as_bool(&self) -> Option<bool> {
        (self.value.kind == FieldValue::KIND_BOOL).then_some(self.value.boolv)
    }

    /// The static-string value, if this field holds one.
    pub fn as_str(&self) -> Option<&'static str> {
        (self.value.kind == FieldValue::KIND_STR).then_some(self.value.strv)
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

#[cfg(test)]
mod cov {
    use super::*;

    #[test]
    fn as_i64_returns_none_for_a_non_integer_field() {
        assert_eq!(LogField::bool("k", true).as_i64(), None);
        assert_eq!(LogField::i64("k", 5).as_i64(), Some(5));
    }
}
