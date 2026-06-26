//! [`FrameCapture`] — the deterministic per-frame artifact container.
//!
//! A capture holds the **opaque canonical bytes** the app produced for one
//! frame, indexed by kernel [`FrameIndex`]/[`Tick`], plus deterministic
//! diagnostic hashes. The byte arrays are treated as undifferentiated payloads:
//! this module never interprets them. Only owned bytes + primitive metadata are
//! stored — never references, `Rc`/`Arc`, trait objects, GPU/browser handles, or
//! callbacks. Pixel buffers / screenshots / texture dumps are out of scope.
//!
//! Byte equality is the source of truth; the hashes (FNV-1a) are diagnostics.

use axiom_kernel::{FrameIndex, StableHash, Tick};

/// One frame's deterministic artifact set. Equality compares the frame identity,
/// every byte array, and every hash (so it is exactly byte-for-byte identity).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameCapture {
    frame_index: FrameIndex,
    tick: Tick,
    input_bytes: Vec<u8>,
    runtime_bytes: Vec<u8>,
    state_bytes: Vec<u8>,
    render_bytes: Vec<u8>,
    input_hash: u64,
    runtime_hash: u64,
    state_hash: u64,
    render_hash: u64,
    final_hash: u64,
}

impl FrameCapture {
    /// Build a capture from a frame's opaque artifact bytes, computing the
    /// per-artifact hashes and the combined `final_hash` deterministically.
    pub(crate) fn new(
        frame_index: FrameIndex,
        tick: Tick,
        input_bytes: Vec<u8>,
        runtime_bytes: Vec<u8>,
        state_bytes: Vec<u8>,
        render_bytes: Vec<u8>,
    ) -> Self {
        let input_hash = StableHash::of_bytes(&input_bytes).raw();
        let runtime_hash = StableHash::of_bytes(&runtime_bytes).raw();
        let state_hash = StableHash::of_bytes(&state_bytes).raw();
        let render_hash = StableHash::of_bytes(&render_bytes).raw();
        let final_hash = StableHash::of_words(&[
            frame_index.raw(),
            tick.raw(),
            input_hash,
            runtime_hash,
            state_hash,
            render_hash,
        ])
        .raw();
        FrameCapture {
            frame_index,
            tick,
            input_bytes,
            runtime_bytes,
            state_bytes,
            render_bytes,
            input_hash,
            runtime_hash,
            state_hash,
            render_hash,
            final_hash,
        }
    }

    /// The frame index this capture belongs to.
    pub fn frame_index(&self) -> FrameIndex {
        self.frame_index
    }

    /// The simulation tick this capture belongs to.
    pub fn tick(&self) -> Tick {
        self.tick
    }

    /// The opaque input artifact bytes.
    pub fn input_bytes(&self) -> &[u8] {
        &self.input_bytes
    }

    /// The opaque runtime-step artifact bytes.
    pub fn runtime_bytes(&self) -> &[u8] {
        &self.runtime_bytes
    }

    /// The opaque state/snapshot artifact bytes.
    pub fn state_bytes(&self) -> &[u8] {
        &self.state_bytes
    }

    /// The opaque render-command artifact bytes.
    pub fn render_bytes(&self) -> &[u8] {
        &self.render_bytes
    }

    /// Diagnostic hash of the input bytes.
    pub fn input_hash(&self) -> u64 {
        self.input_hash
    }

    /// Diagnostic hash of the runtime bytes.
    pub fn runtime_hash(&self) -> u64 {
        self.runtime_hash
    }

    /// Diagnostic hash of the state bytes.
    pub fn state_hash(&self) -> u64 {
        self.state_hash
    }

    /// Diagnostic hash of the render bytes.
    pub fn render_hash(&self) -> u64 {
        self.render_hash
    }

    /// Diagnostic hash combining the frame identity + all per-artifact hashes.
    pub fn final_hash(&self) -> u64 {
        self.final_hash
    }

    /// The memory footprint of this capture: the struct itself plus every owned
    /// byte array. This is what the timeline budgets against.
    pub(crate) fn byte_len(&self) -> usize {
        core::mem::size_of::<FrameCapture>()
            + self.input_bytes.len()
            + self.runtime_bytes.len()
            + self.state_bytes.len()
            + self.render_bytes.len()
    }

    /// Whether every artifact payload is empty (all four byte arrays empty).
    pub fn is_empty_payload(&self) -> bool {
        self.input_bytes.is_empty()
            & self.runtime_bytes.is_empty()
            & self.state_bytes.is_empty()
            & self.render_bytes.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cap(input: &[u8], runtime: &[u8], state: &[u8], render: &[u8]) -> FrameCapture {
        FrameCapture::new(
            FrameIndex::new(4),
            Tick::new(40),
            input.to_vec(),
            runtime.to_vec(),
            state.to_vec(),
            render.to_vec(),
        )
    }

    #[test]
    fn constructor_stores_frame_index_and_tick() {
        let c = cap(b"i", b"r", b"s", b"d");
        assert_eq!(c.frame_index(), FrameIndex::new(4));
        assert_eq!(c.tick(), Tick::new(40));
        assert_eq!(c.input_bytes(), b"i");
        assert_eq!(c.runtime_bytes(), b"r");
        assert_eq!(c.state_bytes(), b"s");
        assert_eq!(c.render_bytes(), b"d");
        assert!(format!("{c:?}").contains("FrameCapture"));
    }

    #[test]
    fn byte_len_includes_every_byte_array() {
        let small = cap(b"", b"", b"", b"");
        let big = cap(b"aaaa", b"bb", b"c", b"dddddd");
        assert_eq!(big.byte_len() - small.byte_len(), 4 + 2 + 1 + 6);
        assert!(small.byte_len() >= core::mem::size_of::<FrameCapture>());
    }

    #[test]
    fn empty_payload_is_detected() {
        assert!(cap(b"", b"", b"", b"").is_empty_payload());
        assert!(!cap(b"", b"", b"x", b"").is_empty_payload());
    }

    #[test]
    fn identical_payloads_produce_identical_hashes() {
        let a = cap(b"i", b"r", b"s", b"d");
        let b = cap(b"i", b"r", b"s", b"d");
        assert_eq!(a.input_hash(), b.input_hash());
        assert_eq!(a.runtime_hash(), b.runtime_hash());
        assert_eq!(a.state_hash(), b.state_hash());
        assert_eq!(a.render_hash(), b.render_hash());
        assert_eq!(a.final_hash(), b.final_hash());
        assert_eq!(a, b);
    }

    #[test]
    fn changed_bytes_change_only_the_relevant_hash() {
        let base = cap(b"i", b"r", b"s", b"d");
        let in2 = cap(b"I", b"r", b"s", b"d");
        assert_ne!(base.input_hash(), in2.input_hash());
        assert_eq!(base.runtime_hash(), in2.runtime_hash());
        assert_ne!(base.final_hash(), in2.final_hash());

        let rt2 = cap(b"i", b"R", b"s", b"d");
        assert_ne!(base.runtime_hash(), rt2.runtime_hash());
        assert_eq!(base.input_hash(), rt2.input_hash());

        let st2 = cap(b"i", b"r", b"S", b"d");
        assert_ne!(base.state_hash(), st2.state_hash());

        let rd2 = cap(b"i", b"r", b"s", b"D");
        assert_ne!(base.render_hash(), rd2.render_hash());
    }

    #[test]
    fn final_hash_is_deterministic_and_identity_sensitive() {
        let a = cap(b"i", b"r", b"s", b"d");
        // Same payload, different frame identity → different final hash.
        let b = FrameCapture::new(
            FrameIndex::new(5),
            Tick::new(40),
            b"i".to_vec(),
            b"r".to_vec(),
            b"s".to_vec(),
            b"d".to_vec(),
        );
        assert_ne!(a.final_hash(), b.final_hash());
        assert_ne!(a, b);
    }
}
