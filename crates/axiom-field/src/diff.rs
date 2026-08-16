//! What changed between two graphs.
//!
//! ## Both sides are canonicalised first, and that is the whole design
//!
//! A diff taken on authored graphs is dominated by noise: a dead branch nobody
//! reads, the same subexpression written twice, a constant chain nobody folded,
//! `a + b` written as `b + a`. None of that is a change in what the field
//! *computes*, and an agent told about it learns nothing.
//!
//! [`crate::FieldGraph::canonicalize`] already removes exactly that noise — it
//! was built for the program-cache key, and this is its second consumer. After
//! it, node ids are dense `0..n`, the emission order is fixed, and two graphs
//! that compute the same thing are byte-identical. So the diff is the honest,
//! id-ordered comparison of two canonical forms and nothing more:
//!
//! * **changed** — an id both graphs have, whose node differs in operator,
//!   parameter words or inputs.
//! * **added** — an id only the later graph has.
//! * **removed** — an id only the earlier graph has.
//!
//! The declared output is not diffed separately, because it cannot differ on its
//! own: every node of a canonical graph is reachable from its output and every
//! input names a strictly-earlier node, so the output is always the last id.
//! Two canonical graphs with the same nodes have the same output.
//!
//! **Symmetric-stable:** the same pair always yields the same result, and
//! diffing a graph with itself is empty. **Canonicalising both sides is the
//! expensive part** — accept it: correctness beats speed for a query an agent
//! runs once per edit, and it is authoring-time work either way.

use axiom_recipe::NodeId;

use crate::field_error::FieldResult;
use crate::field_graph::FieldGraph;

/// What changed between two graphs, in canonical node ids.
///
/// `changed` ids name nodes of **both** graphs, `added` ids name nodes of the
/// later graph only, and `removed` ids name nodes of the earlier graph only.
/// Every list is ascending.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FieldDiff {
    added: Vec<NodeId>,
    removed: Vec<NodeId>,
    changed: Vec<NodeId>,
}

impl FieldDiff {
    /// Nodes the later graph has and the earlier one does not.
    pub fn added(&self) -> &[NodeId] {
        &self.added
    }

    /// Nodes the earlier graph has and the later one does not.
    pub fn removed(&self) -> &[NodeId] {
        &self.removed
    }

    /// Nodes both graphs have, whose operator, parameter words or inputs differ.
    pub fn changed(&self) -> &[NodeId] {
        &self.changed
    }

    /// Whether the two graphs are the same program.
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() & self.removed.is_empty() & self.changed.is_empty()
    }
}

impl FieldGraph {
    /// What changed between this graph and `after`, both canonicalised first.
    ///
    /// Fails exactly when either side fails to canonicalise — a graph that does
    /// not type has no canonical form, and there is nothing honest to diff it
    /// against.
    ///
    /// Authoring-time only: it canonicalises two graphs per call.
    pub fn diff(&self, after: &FieldGraph) -> FieldResult<FieldDiff> {
        self.canonicalize().and_then(|before| {
            after
                .canonicalize()
                .map(|after| compare(&before, &after))
        })
    }
}

/// Compare two already-canonical graphs, id by id.
fn compare(before: &FieldGraph, after: &FieldGraph) -> FieldDiff {
    FieldDiff {
        changed: before
            .recipe()
            .nodes()
            .iter()
            .zip(after.recipe().nodes().iter())
            .enumerate()
            .filter(|(_index, (old, new))| old != new)
            .map(|(index, _pair)| NodeId::from_raw(index as u32))
            .collect(),
        added: ids(before.node_count(), after.node_count()),
        removed: ids(after.node_count(), before.node_count()),
    }
}

/// The ids in `shared..end`, or nothing when there are none.
fn ids(shared: usize, end: usize) -> Vec<NodeId> {
    (shared..end).map(|id| NodeId::from_raw(id as u32)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_recipe::Scalar;

    use crate::field_builder::FieldBuilder;
    use crate::field_error::FieldErrorCode;
    use crate::field_op::FieldOp;
    use crate::field_value::FieldValue;
    use crate::ids::FieldId;

    fn scalar(value: f32) -> FieldValue {
        FieldValue::scalar(Scalar::new(value))
    }

    /// `length(point) + tint` — three live nodes and a literal.
    fn tinted(tint: f32) -> FieldGraph {
        let (build, point) = FieldBuilder::new(FieldId::of_name("field/diff"), 1).push(
            FieldOp::Point,
            Vec::new(),
            Vec::new(),
        );
        let (build, length) = build.push(FieldOp::Length, Vec::new(), vec![point]);
        let (build, amount) = build.push_const(scalar(tint));
        let (build, sum) = build.push(FieldOp::Add, Vec::new(), vec![length, amount]);
        build.build(sum)
    }

    #[test]
    fn a_graph_diffed_with_itself_is_empty() {
        let diff = tinted(1.0).diff(&tinted(1.0)).expect("it types");
        assert!(diff.is_empty());
        assert_eq!(diff.added(), &[]);
        assert_eq!(diff.removed(), &[]);
        assert_eq!(diff.changed(), &[]);
        assert_eq!(diff, FieldDiff::default());
    }

    #[test]
    fn two_authorings_of_one_program_diff_to_nothing() {
        // The same field written with a dead branch, a duplicated subexpression
        // and its operands the other way up.
        let (build, point) = FieldBuilder::new(FieldId::of_name("field/diff"), 1).push(
            FieldOp::Point,
            Vec::new(),
            Vec::new(),
        );
        let (build, _dead) = build.push(FieldOp::Uv, Vec::new(), Vec::new());
        let (build, length) = build.push(FieldOp::Length, Vec::new(), vec![point]);
        let (build, _again) = build.push(FieldOp::Length, Vec::new(), vec![point]);
        let (build, amount) = build.push_const(scalar(1.0));
        let (build, sum) = build.push(FieldOp::Add, Vec::new(), vec![amount, length]);
        let messy = build.build(sum);

        assert_ne!(messy.node_count(), tinted(1.0).node_count());
        assert!(messy
            .diff(&tinted(1.0))
            .expect("both type")
            .is_empty());
    }

    #[test]
    fn a_retuned_literal_names_exactly_the_node_that_changed() {
        let diff = tinted(1.0).diff(&tinted(2.0)).expect("both type");
        // Canonical order is Point, Length, Const, Add — the literal is node 2.
        assert_eq!(diff.changed(), &[NodeId::from_raw(2)]);
        assert_eq!(diff.added(), &[]);
        assert_eq!(diff.removed(), &[]);
        assert!(!diff.is_empty());
        // Symmetric-stable: the same pair always answers the same way.
        assert_eq!(tinted(1.0).diff(&tinted(2.0)), Ok(diff));
    }

    #[test]
    fn a_grown_graph_reports_added_ids_and_a_shrunk_one_reports_removed_ids() {
        let base = tinted(1.0);
        let grown = base
            .insert_before(base.output(), &{
                let (build, point) = FieldBuilder::new(FieldId::from_raw(1), 1).push(
                    FieldOp::Point,
                    Vec::new(),
                    Vec::new(),
                );
                let (build, magnitude) = build.push(FieldOp::Abs, Vec::new(), vec![point]);
                build.build(magnitude)
            })
            .expect("the output exists");
        assert_eq!(grown.validate(), Ok(()));

        let forward = base.diff(&grown).expect("both type");
        assert_eq!(forward.added(), &[NodeId::from_raw(4)]);
        assert_eq!(forward.removed(), &[]);

        let backward = grown.diff(&base).expect("both type");
        assert_eq!(backward.removed(), &[NodeId::from_raw(4)]);
        assert_eq!(backward.added(), &[]);
        assert_eq!(forward.changed(), backward.changed());
    }

    #[test]
    fn a_graph_that_does_not_type_cannot_be_diffed_from_either_side() {
        let (build, point) = FieldBuilder::new(FieldId::from_raw(2), 1).push(
            FieldOp::Point,
            Vec::new(),
            Vec::new(),
        );
        let (build, uv) = build.push(FieldOp::Uv, Vec::new(), Vec::new());
        let (build, bad) = build.push(FieldOp::Add, Vec::new(), vec![point, uv]);
        let broken = build.build(bad);
        assert_eq!(
            broken
                .diff(&tinted(1.0))
                .expect_err("Vec3 and Vec2 do not meet")
                .kind(),
            FieldErrorCode::TypeMismatch
        );
        assert_eq!(
            tinted(1.0)
                .diff(&broken)
                .expect_err("Vec3 and Vec2 do not meet")
                .kind(),
            FieldErrorCode::TypeMismatch
        );
    }
}
