//! A whole figure as portable data: a parent-before-child ordered part list.

use axiom_kernel::{BinaryReader, BinaryWriter, KernelResult};

use crate::figure_error::{FigureError, FigureResult};
use crate::part::FigurePart;

/// A complete articulated figure: a flat, parent-before-child ordered list of
/// [`FigurePart`]s. This is the portable unit an app authors once and both a
/// game and the animation lab load — the "1-1 structural" contract lives here.
/// The one invariant is a **parent-before-child ordering** (every part's parent
/// index is strictly less than its own index), which mirrors the animation
/// skeleton's rule and lets an app build the skeleton in a single forward pass.
#[derive(Debug, Clone, PartialEq)]
pub struct FigureDefinition {
    parts: Vec<FigurePart>,
}

impl FigureDefinition {
    /// Wrap a part list. Does not validate — call [`Self::validate`].
    pub fn new(parts: Vec<FigurePart>) -> Self {
        Self { parts }
    }

    /// The parts, in order.
    pub fn parts(&self) -> &[FigurePart] {
        &self.parts
    }

    /// The number of parts.
    pub fn part_count(&self) -> usize {
        self.parts.len()
    }

    /// Validate the hierarchy: every part's parent must be strictly earlier in
    /// the list. Returns the first violation as [`FigureError::BadParent`].
    pub fn validate(&self) -> FigureResult<()> {
        self.parts.iter().enumerate().try_for_each(|(index, part)| {
            part.parent.map_or(Ok(()), |parent| {
                ((parent as usize) < index)
                    .then_some(())
                    .ok_or(FigureError::BadParent)
            })
        })
    }

    /// Append the figure's bytes: a `u64` part count then each part in order.
    pub(crate) fn write_to(&self, writer: &mut BinaryWriter) {
        writer.write_u64(self.parts.len() as u64);
        self.parts.iter().for_each(|part| part.write_to(writer));
    }

    /// Read a figure written by [`FigureDefinition::write_to`].
    pub(crate) fn read_from(reader: &mut BinaryReader<'_>) -> KernelResult<FigureDefinition> {
        reader.read_u64().and_then(|count| {
            (0..count)
                .try_fold(Vec::new(), |mut parts, _| {
                    FigurePart::read_from(reader).map(|part| {
                        parts.push(part);
                        parts
                    })
                })
                .map(|parts| FigureDefinition { parts })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_math::{Transform, Vec3};

    fn part(parent: Option<u32>) -> FigurePart {
        parent.map_or_else(
            || FigurePart::root(Transform::IDENTITY, Vec3::new(1.0, 1.0, 1.0), Vec3::ZERO, 0),
            |p| FigurePart::child(p, Transform::IDENTITY, Vec3::new(1.0, 1.0, 1.0), Vec3::ZERO, 0),
        )
    }

    #[test]
    fn valid_hierarchy_validates_and_reports_counts() {
        let def = FigureDefinition::new(vec![part(None), part(Some(0)), part(Some(1))]);
        assert_eq!(def.validate(), Ok(()));
        assert_eq!(def.part_count(), 3);
        assert_eq!(def.parts().len(), 3);
    }

    #[test]
    fn forward_or_self_parent_is_bad_parent() {
        assert_eq!(
            FigureDefinition::new(vec![part(None), part(Some(2)), part(Some(0))]).validate(),
            Err(FigureError::BadParent)
        );
        assert_eq!(
            FigureDefinition::new(vec![part(None), part(Some(1))]).validate(),
            Err(FigureError::BadParent)
        );
    }

    #[test]
    fn figure_round_trips_through_bytes() {
        let def = FigureDefinition::new(vec![
            FigurePart::root(Transform::from_translation(Vec3::new(0.0, 1.0, 0.0)), Vec3::new(0.5, 0.6, 0.4), Vec3::ZERO, 1),
            FigurePart::child(0, Transform::from_translation(Vec3::new(0.0, -0.4, 0.0)), Vec3::new(0.2, 0.5, 0.2), Vec3::new(0.0, -0.2, 0.0), 2),
        ]);
        let mut w = BinaryWriter::new();
        def.write_to(&mut w);
        let bytes = w.into_bytes();
        assert_eq!(FigureDefinition::read_from(&mut BinaryReader::new(&bytes)).unwrap(), def);
    }

    #[test]
    fn truncated_figure_bytes_fail() {
        let mut w = BinaryWriter::new();
        w.write_u64(2); // claims 2 parts, provides none
        let bytes = w.into_bytes();
        assert!(FigureDefinition::read_from(&mut BinaryReader::new(&bytes)).is_err());
    }
}
