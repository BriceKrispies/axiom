//! The per-phase root-motion command: how the actor's root translates over a
//! phase. Authored as target *names*; resolved to positions by the compiler.

use axiom_math::Vec3;

/// The kind of root motion. The discriminant orders the resolution table
/// (`Hold`/`Settle` ignore the endpoints; `MoveToward` lerps `from`→`to`), so it
/// must not be reshuffled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootMotionKind {
    /// The root stays where the previous phase left it.
    Hold,
    /// The root settles in place (identical motion to `Hold`; a distinct name so
    /// a recover phase reads as intentional rather than a plain hold).
    Settle,
    /// The root lerps from the `from` target to the `to` target across the phase.
    MoveToward,
}

/// An authored root-motion command referencing targets by name.
#[derive(Debug, Clone, PartialEq)]
pub struct RootMotion {
    kind: RootMotionKind,
    from_name: Option<String>,
    to_name: Option<String>,
}

impl RootMotion {
    /// Hold in place.
    pub(crate) fn hold() -> Self {
        RootMotion {
            kind: RootMotionKind::Hold,
            from_name: None,
            to_name: None,
        }
    }

    /// Settle in place.
    pub(crate) fn settle() -> Self {
        RootMotion {
            kind: RootMotionKind::Settle,
            from_name: None,
            to_name: None,
        }
    }

    /// Move the root from target `from` to target `to`.
    pub(crate) fn move_toward(from: &str, to: &str) -> Self {
        RootMotion {
            kind: RootMotionKind::MoveToward,
            from_name: Some(from.to_string()),
            to_name: Some(to.to_string()),
        }
    }

    /// The command kind.
    pub(crate) fn kind(&self) -> RootMotionKind {
        self.kind
    }

    /// The source (`from`) target name (only a `MoveToward` carries one).
    pub(crate) fn source_name(&self) -> Option<&str> {
        self.from_name.as_deref()
    }

    /// The destination (`to`) target name (only a `MoveToward` carries one).
    pub(crate) fn dest_name(&self) -> Option<&str> {
        self.to_name.as_deref()
    }
}

/// A resolved root-motion command carrying endpoint positions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedRootMotion {
    kind: RootMotionKind,
    from: Vec3,
    to: Vec3,
}

impl ResolvedRootMotion {
    /// Construct from a kind and resolved endpoints.
    pub(crate) fn new(kind: RootMotionKind, from: Vec3, to: Vec3) -> Self {
        ResolvedRootMotion { kind, from, to }
    }

    /// The root position at the *start* of the phase given the running root
    /// `carry` from prior phases: a `MoveToward` starts at `from`; `Hold`/`Settle`
    /// keep the carried position. Selected by table index — no branch.
    pub(crate) fn start(&self, carry: Vec3) -> Vec3 {
        [carry, carry, self.from][self.kind as usize]
    }

    /// The root position at the *end* of the phase given the running root `carry`:
    /// a `MoveToward` ends at `to`; `Hold`/`Settle` keep the carried position.
    pub(crate) fn end(&self, carry: Vec3) -> Vec3 {
        [carry, carry, self.to][self.kind as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authored_variants_carry_their_names() {
        assert_eq!(RootMotion::hold().kind(), RootMotionKind::Hold);
        assert_eq!(RootMotion::hold().source_name(), None);
        assert_eq!(RootMotion::settle().kind(), RootMotionKind::Settle);
        let m = RootMotion::move_toward("a", "b");
        assert_eq!(m.kind(), RootMotionKind::MoveToward);
        assert_eq!(m.source_name(), Some("a"));
        assert_eq!(m.dest_name(), Some("b"));
    }

    #[test]
    fn resolved_endpoints_select_by_kind() {
        let carry = Vec3::new(1.0, 2.0, 3.0);
        let from = Vec3::new(-1.0, 0.0, 0.0);
        let to = Vec3::new(5.0, 0.0, 0.0);
        let hold = ResolvedRootMotion::new(RootMotionKind::Hold, from, to);
        assert_eq!(hold.start(carry), carry);
        assert_eq!(hold.end(carry), carry);
        let settle = ResolvedRootMotion::new(RootMotionKind::Settle, from, to);
        assert_eq!(settle.start(carry), carry);
        assert_eq!(settle.end(carry), carry);
        let move_toward = ResolvedRootMotion::new(RootMotionKind::MoveToward, from, to);
        assert_eq!(move_toward.start(carry), from);
        assert_eq!(move_toward.end(carry), to);
    }
}
