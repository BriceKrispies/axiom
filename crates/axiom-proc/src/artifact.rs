//! [`Artifact`] — the neutral output of evaluating a recipe.
//!
//! An artifact is **opaque, domain-free data**: the sequence of `u64` node values
//! the recipe produced, stamped with the artifact byte-format [`SchemaVersion`]
//! and the recipe's generator version. What the words *mean* is a domain module's
//! job; here they are neutral bytes. It serializes to canonical little-endian
//! bytes and carries a stable [`StableHash`] digest over them — an index for
//! golden storage, never the determinism proof (bytes prove).

use axiom_kernel::{BinaryWriter, SchemaVersion, StableHash};

/// The neutral result of a recipe evaluation.
#[derive(Debug, PartialEq, Eq)]
pub struct Artifact {
    generator_version: u32,
    words: Vec<u64>,
}

impl Artifact {
    /// The artifact byte-format version. Bump it when the canonical byte layout
    /// changes, so stored goldens are invalidated deliberately, not silently.
    const SCHEMA: SchemaVersion = SchemaVersion::new(1, 0);

    /// Construct an artifact directly from its neutral words and generator
    /// version. Evaluation uses this to package its output; `proc-validate` uses
    /// it to produce a repaired artifact. The artifact is just data — its byte
    /// form is identical however it was made — so construction does not claim the
    /// words were *evaluated*; determinism remains a property of `ProcApi`.
    pub fn from_words(generator_version: u32, words: Vec<u64>) -> Self {
        Artifact {
            generator_version,
            words,
        }
    }

    /// The recipe generator version that produced this artifact.
    pub fn generator_version(&self) -> u32 {
        self.generator_version
    }

    /// The neutral output words.
    pub fn words(&self) -> &[u64] {
        &self.words
    }

    /// The canonical bytes: schema version, generator version, length, then the
    /// little-endian words.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        Artifact::SCHEMA.write_to(&mut writer);
        writer.write_u32(self.generator_version);
        writer.write_u64(self.words.len() as u64);
        self.words.iter().for_each(|&word| writer.write_u64(word));
        writer.into_bytes()
    }

    /// The stable digest over [`Self::to_bytes`] — an index for golden storage,
    /// never the determinism proof.
    pub fn digest(&self) -> StableHash {
        StableHash::of_bytes(&self.to_bytes())
    }
}
