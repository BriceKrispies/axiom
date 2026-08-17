//! **Station 10 — introspection.** The graph is machine-readable data, not
//! opaque source, and this is where the app proves it by reading its own
//! materials back.
//!
//! Three questions an agent (or a human staring at a wrong pixel) actually asks,
//! answered without a shader compiler anywhere near them:
//!
//! * **`explain()`** — what does this material *do*? One deterministic line per
//!   node, in id order (`n7: Mul(Scalar) <- n5, n6`). Output-only: nothing
//!   downstream parses it, and nothing may start.
//! * **`digest()`** — what *is* this material? The structural label a program
//!   cache keys on, which parameter values are deliberately outside of.
//! * **`diff()`** — what *changed*? Both sides are canonicalised first, so the
//!   answer is about what the field computes rather than about authoring noise
//!   — a dead branch, a duplicated subexpression, `a + b` written `b + a`.
//!
//! The `diff` this station shows is the sharpest one available: **station 4 at
//! two different tunings**. The two graphs compute visibly different pixels, and
//! the diff is **empty** — because nothing *structural* changed. That is the same
//! fact station 4 asserts about the digest, arrived at from the other end, and
//! together they are the whole argument for why a retune cannot recompile a
//! program.
//!
//! Beside it, a diff that is *not* empty: marble against wood. Two graphs of the
//! same shape and the same knob count that genuinely differ, so the reader can
//! see that an empty diff means something.

use axiom_field::{FieldDiff, FieldGraph};
use axiom_surface::{Surface, SurfaceChannel};

use crate::stations::{patterns, retune};

/// The `explain()` text of one station's base-colour graph — the material read
/// back as text, one line per node.
pub fn explain_base_color(surface: &Surface) -> Option<String> {
    surface
        .binding(SurfaceChannel::BaseColor)
        .as_field()
        .and_then(|graph| graph.explain().ok())
        .map(|explanation| explanation.text())
}

/// The base-colour graph of a surface, if it has one.
fn base_color(surface: &Surface) -> Option<FieldGraph> {
    surface
        .binding(SurfaceChannel::BaseColor)
        .as_field()
        .cloned()
}

/// **The empty diff.** Station 4 at two tunings: different pixels, identical
/// structure, so `diff` reports nothing added, nothing removed, nothing changed.
pub fn retune_diff() -> Option<FieldDiff> {
    let shipped = base_color(&retune::retune_surface())?;
    let retuned = base_color(&retune::retune_surface_tuned(retune::RetuneTuning {
        frequency: 19.0,
        sharpness: 5.5,
        warp: 1.9,
    }))?;
    shipped.diff(&retuned).ok()
}

/// **A diff that is not empty**, so the empty one above means something: marble
/// against wood.
pub fn pattern_diff() -> Option<FieldDiff> {
    let marble = patterns::marble();
    let wood = patterns::wood();
    marble.diff(&wood).ok()
}

/// The whole station as the lines a panel prints or a log dumps.
pub fn introspection_lines() -> Vec<String> {
    let mut lines = vec![
        "station 10 - introspection".to_string(),
        format!(
            "  station 4 digest ......... {}",
            retune::displayed_digest()
        ),
    ];
    lines.push(match retune_diff() {
        Some(diff) => format!(
            "  retune diff .............. +{} -{} ~{}  (a retune is not a structural change)",
            diff.added().len(),
            diff.removed().len(),
            diff.changed().len()
        ),
        None => "  retune diff .............. unavailable".to_string(),
    });
    lines.push(match pattern_diff() {
        Some(diff) => format!(
            "  marble vs wood diff ...... +{} -{} ~{}",
            diff.added().len(),
            diff.removed().len(),
            diff.changed().len()
        ),
        None => "  marble vs wood diff ...... unavailable".to_string(),
    });
    lines.extend(
        explain_base_color(&patterns::marble_surface())
            .unwrap_or_default()
            .lines()
            .take(6)
            .map(|line| format!("  marble  {line}")),
    );
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The load-bearing introspection result.** Two tunings of station 4
    /// compute different pixels and their canonicalised graphs are structurally
    /// identical, so the diff is empty.
    #[test]
    fn a_retune_is_an_empty_diff() {
        let diff = retune_diff().expect("both tunings canonicalise");
        assert!(
            diff.is_empty(),
            "a retune reported a structural change: +{} -{} ~{}",
            diff.added().len(),
            diff.removed().len(),
            diff.changed().len()
        );
    }

    /// ...and a genuinely different material is a non-empty diff, so the empty
    /// one above is a measurement and not a broken comparison.
    #[test]
    fn two_different_patterns_are_a_non_empty_diff() {
        let diff = pattern_diff().expect("both patterns canonicalise");
        assert!(!diff.is_empty());
    }

    /// `explain` is deterministic text, one line per node, and the app never
    /// parses it.
    #[test]
    fn explain_is_one_deterministic_line_per_node() {
        let surface = patterns::marble_surface();
        let text = explain_base_color(&surface).expect("marble's colour is a field");
        let graph = base_color(&surface).expect("a field");
        assert_eq!(text.lines().count(), graph.node_count());
        assert_eq!(Some(text), explain_base_color(&surface));
    }

    /// A surface whose base colour is a plain constant has nothing to explain,
    /// and says so rather than panicking.
    #[test]
    fn a_constant_channel_has_no_explanation() {
        let plain = axiom_surface::SurfaceBuilder::new()
            .build()
            .expect("a default surface is legal");
        assert_eq!(explain_base_color(&plain), None);
    }

    #[test]
    fn the_panel_lines_carry_the_digest_and_both_diffs() {
        let lines = introspection_lines();
        assert!(lines.iter().any(|l| l.contains(&retune::displayed_digest())));
        assert!(lines.iter().any(|l| l.contains("retune diff")));
        assert!(lines.iter().any(|l| l.contains("marble vs wood")));
        assert!(lines.iter().any(|l| l.starts_with("  marble  n")));
    }
}
