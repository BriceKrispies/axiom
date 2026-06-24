//! [`ValidationReport`] — the deterministic verdict of validating an artifact.

use axiom_kernel::{BinaryWriter, StableHash};

/// The outcome of validating an artifact against a constraint list: a verdict per
/// constraint (`(kind_code, satisfied, score)`), whether all passed, and the total
/// score. Deterministic and serializable, so reports golden-compare.
#[derive(Debug, PartialEq, Eq)]
pub struct ValidationReport {
    verdicts: Vec<(u32, bool, u64)>,
    all_satisfied: bool,
    total_score: u64,
}

impl ValidationReport {
    pub(crate) fn new(
        verdicts: Vec<(u32, bool, u64)>,
        all_satisfied: bool,
        total_score: u64,
    ) -> Self {
        ValidationReport {
            verdicts,
            all_satisfied,
            total_score,
        }
    }

    /// The per-constraint verdicts: `(kind_code, satisfied, score)`, in order.
    pub fn verdicts(&self) -> &[(u32, bool, u64)] {
        &self.verdicts
    }

    /// Whether every constraint was satisfied.
    pub fn all_satisfied(&self) -> bool {
        self.all_satisfied
    }

    /// The summed per-constraint score — a stable, ordered quality measure.
    pub fn total_score(&self) -> u64 {
        self.total_score
    }

    /// The canonical bytes: count, each `(kind_code, satisfied, score)`, then the
    /// `all_satisfied` flag and total score.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        writer.write_u64(self.verdicts.len() as u64);
        self.verdicts.iter().for_each(|&(kind, satisfied, score)| {
            writer.write_u32(kind);
            writer.write_bool(satisfied);
            writer.write_u64(score);
        });
        writer.write_bool(self.all_satisfied);
        writer.write_u64(self.total_score);
        writer.into_bytes()
    }

    /// The stable digest over [`Self::to_bytes`].
    pub fn digest(&self) -> StableHash {
        StableHash::of_bytes(&self.to_bytes())
    }
}
