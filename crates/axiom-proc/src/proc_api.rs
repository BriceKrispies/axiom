//! [`ProcApi`] — the procedural-generation facade.
//!
//! It validates a [`Recipe`] as a DAG, keys a deterministic entropy stream by
//! `(seed, address, recipe.version)`, and evaluates the recipe into a neutral
//! [`Artifact`] + [`ProcTrace`]. An invalid recipe is rejected **as data**
//! (`None`), never a panic. Evaluation is resumable and **budget-independent**:
//! [`Self::begin`] hands back an [`Evaluation`] the caller steps; [`Self::evaluate`]
//! runs it to completion, and the two produce byte-identical output. Branchless.

use axiom_entropy::EntropyApi;
use axiom_space::Address;

use crate::artifact::Artifact;
use crate::evaluation::Evaluation;
use crate::node::nodes_form_dag;
use crate::recipe::Recipe;
use crate::trace::ProcTrace;

/// The procedural-generation facade. Stateless: an evaluation is a pure
/// deterministic function of `(recipe, seed, address)`.
#[derive(Debug)]
pub struct ProcApi;

impl ProcApi {
    /// Begin a resumable evaluation of `recipe` at `address` under `seed`. Returns
    /// `None` when the recipe is not a valid DAG (a forward / self / back
    /// reference) — rejected as data, never a panic. The entropy stream is keyed
    /// by the recipe's version, so a version bump re-keys the whole evaluation.
    pub fn begin(recipe: &Recipe, seed: u64, address: &Address) -> Option<Evaluation> {
        nodes_form_dag(recipe.nodes()).then(|| {
            let stream = EntropyApi::stream(seed, address, recipe.version());
            Evaluation::new(recipe.nodes().to_vec(), recipe.version(), stream)
        })
    }

    /// Evaluate `recipe` to completion, returning its neutral artifact + trace (or
    /// `None` for an invalid recipe). Equivalent to [`Self::begin`] then stepping
    /// to the end — the budget-independent whole-evaluation.
    pub fn evaluate(
        recipe: &Recipe,
        seed: u64,
        address: &Address,
    ) -> Option<(Artifact, ProcTrace)> {
        ProcApi::begin(recipe, seed, address).map(|mut evaluation| {
            evaluation.step(recipe.len());
            evaluation.into_output()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_space::SpaceApi;

    fn addr(segments: &[u64]) -> Address {
        segments
            .iter()
            .fold(SpaceApi::root(), |a, &s| SpaceApi::child(&a, s))
    }

    // A recipe exercising all four ops: c=const(5), d=draw, s=add(c,d), x=xor(s,c).
    fn sample() -> Recipe {
        let mut r = Recipe::new(3);
        let c = r.const_node(5);
        let d = r.draw();
        let s = r.add(c, d);
        let _x = r.xor(s, c);
        r
    }

    #[test]
    fn evaluation_is_deterministic_byte_for_byte() {
        let r = sample();
        let (a1, t1) = ProcApi::evaluate(&r, 7, &addr(&[1, 2])).unwrap();
        let (a2, t2) = ProcApi::evaluate(&r, 7, &addr(&[1, 2])).unwrap();
        assert_eq!(a1.to_bytes(), a2.to_bytes());
        assert_eq!(t1.to_bytes(), t2.to_bytes());
        assert_eq!(a1, a2);
        assert_eq!(t1, t2);
    }

    #[test]
    fn invalid_recipe_is_rejected_as_data() {
        let mut bad = Recipe::new(1);
        bad.add(5, 6);
        assert!(ProcApi::begin(&bad, 0, &addr(&[0])).is_none());
        assert!(ProcApi::evaluate(&bad, 0, &addr(&[0])).is_none());
        let mut selfref = Recipe::new(1);
        selfref.draw();
        selfref.add(1, 0);
        assert!(ProcApi::begin(&selfref, 0, &addr(&[0])).is_none());
    }

    #[test]
    fn budget_is_independent_incremental_equals_whole() {
        let r = sample();
        let whole = ProcApi::evaluate(&r, 7, &addr(&[1, 2])).unwrap();
        let mut eval = ProcApi::begin(&r, 7, &addr(&[1, 2])).unwrap();
        assert!(!eval.is_done());
        while !eval.is_done() {
            eval.step(1);
        }
        assert_eq!(whole, eval.into_output());
    }

    #[test]
    fn over_budget_step_and_empty_recipe_finish_cleanly() {
        let mut eval = ProcApi::begin(&sample(), 7, &addr(&[1, 2])).unwrap();
        eval.step(100);
        assert!(eval.is_done());
        let empty = Recipe::new(1);
        assert!(empty.is_empty());
        let mut e = ProcApi::begin(&empty, 0, &addr(&[0])).unwrap();
        assert!(e.is_done());
        e.step(3);
        let (artifact, trace) = e.into_output();
        assert!(artifact.words().is_empty());
        assert!(trace.is_empty());
    }

    #[test]
    fn version_bump_changes_then_restores_the_artifact() {
        let a = addr(&[1, 2]);
        let mut v3 = Recipe::new(3);
        v3.draw();
        let mut v4 = Recipe::new(4);
        v4.draw();
        let art_v3 = ProcApi::evaluate(&v3, 7, &a).unwrap().0;
        let art_v4 = ProcApi::evaluate(&v4, 7, &a).unwrap().0;
        assert_ne!(art_v3, art_v4);
        assert_eq!(art_v3.generator_version(), 3);
        let mut v3b = Recipe::new(3);
        v3b.draw();
        assert_eq!(art_v3, ProcApi::evaluate(&v3b, 7, &a).unwrap().0);
    }

    #[test]
    fn trace_matches_the_artifact() {
        let r = sample();
        let (artifact, trace) = ProcApi::evaluate(&r, 7, &addr(&[1, 2])).unwrap();
        assert_eq!(trace.len(), r.len());
        assert_eq!(artifact.words().len(), r.len());
        let trace_values: Vec<u64> = trace.steps().iter().map(|&(_, v)| v).collect();
        assert_eq!(trace_values, artifact.words());
        // Op codes recorded in order: Const=0, Draw=1, Add=2, Xor=3.
        let ops: Vec<u32> = trace.steps().iter().map(|&(op, _)| op).collect();
        assert_eq!(ops, vec![0, 1, 2, 3]);
    }

    #[test]
    fn golden_artifact_and_trace_digests_are_stable() {
        let (artifact, trace) = ProcApi::evaluate(&sample(), 7, &addr(&[1, 2])).unwrap();
        assert_eq!(artifact.digest().raw(), 4_507_544_175_111_723_444);
        assert_eq!(trace.digest().raw(), 11_030_477_466_451_214_382);
    }

    #[test]
    fn types_are_debug() {
        let r = sample();
        let eval = ProcApi::begin(&r, 7, &addr(&[1, 2])).unwrap();
        let (artifact, trace) = ProcApi::evaluate(&r, 7, &addr(&[1, 2])).unwrap();
        assert!(!format!("{r:?}").is_empty());
        assert!(!format!("{eval:?}").is_empty());
        assert!(!format!("{artifact:?}").is_empty());
        assert!(!format!("{trace:?}").is_empty());
        assert!(!format!("{:?}", ProcApi).is_empty());
    }
}
