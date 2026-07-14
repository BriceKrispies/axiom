//! The stable identifier shared by a button and the door(s) it controls.
//!
//! A button and a door are linked when they carry the same [`GroupId`] — for
//! example `"main"`. A door is open whenever any solid actor stands on *any*
//! button of the same group. The group id is the one durable link between the
//! two; the puzzle has no separate per-object button/door id, because identity
//! that matters to gameplay is exactly "which group are you wired to".

use std::fmt;

/// The default group new buttons and doors are painted with in the editor.
pub const DEFAULT_GROUP: &str = "main";

/// A button/door wiring group, e.g. `"main"`.
///
/// A group id is just a string, but it is wrapped so the type system records
/// *why* the string exists (it links a button to a door) and so an empty group
/// — which validation rejects — is never confused with a real one.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GroupId(String);

impl GroupId {
    /// Wrap a raw group string. The string is taken verbatim; emptiness is a
    /// *validation* concern (an empty group is a level error), not a construction
    /// error, so the editor can hold a half-typed group without panicking.
    pub fn new(raw: impl Into<String>) -> Self {
        GroupId(raw.into())
    }

    /// The default group (`"main"`).
    pub fn default_group() -> Self {
        GroupId(DEFAULT_GROUP.to_string())
    }

    /// The underlying group string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Is the group string empty? An empty group is invalid (a button/door must
    /// name a real group), surfaced by level validation.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Display for GroupId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for GroupId {
    fn from(s: &str) -> Self {
        GroupId::new(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_main() {
        assert_eq!(GroupId::default_group().as_str(), "main");
        assert!(!GroupId::default_group().is_empty());
    }

    #[test]
    fn empty_group_is_detected() {
        assert!(GroupId::new("").is_empty());
        assert_eq!(GroupId::from("main"), GroupId::default_group());
        assert_eq!(GroupId::new("a").to_string(), "a");
    }
}
