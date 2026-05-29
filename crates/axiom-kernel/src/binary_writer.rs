//! A deterministic little-endian byte writer.

/// An append-only buffer that serializes primitives in little-endian order.
///
/// Every write appends a fixed, byte-exact little-endian encoding, so the same
/// sequence of writes always yields the same bytes on every platform. Byte
/// slices are written length-prefixed (a little-endian `u32` count followed by
/// the bytes) so a [`crate::binary_reader::BinaryReader`] can recover their
/// bounds. `bool` encodes as a single `0` or `1` byte.
#[derive(Debug, Clone, Default)]
pub struct BinaryWriter {
    bytes: Vec<u8>,
}

impl BinaryWriter {
    /// Create an empty writer.
    pub fn new() -> Self {
        BinaryWriter { bytes: Vec::new() }
    }

    /// Append a `u8`.
    pub fn write_u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    /// Append a little-endian `u16`.
    pub fn write_u16(&mut self, value: u16) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    /// Append a little-endian `u32`.
    pub fn write_u32(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    /// Append a little-endian `u64`.
    pub fn write_u64(&mut self, value: u64) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    /// Append a little-endian `i32`.
    pub fn write_i32(&mut self, value: i32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    /// Append a little-endian `f32` (IEEE-754 bit pattern).
    pub fn write_f32(&mut self, value: f32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    /// Append a `bool` as a single `0`/`1` byte.
    pub fn write_bool(&mut self, value: bool) {
        self.bytes.push(u8::from(value));
    }

    /// Append a length-prefixed byte slice: a little-endian `u32` length then
    /// the bytes themselves.
    pub fn write_byte_slice(&mut self, value: &[u8]) {
        self.write_u32(value.len() as u32);
        self.bytes.extend_from_slice(value);
    }

    /// The bytes written so far.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The number of bytes written so far.
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Whether nothing has been written.
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Consume the writer, yielding the written bytes.
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_writer_is_empty() {
        let w = BinaryWriter::new();
        assert!(w.is_empty());
        assert_eq!(w.len(), 0);
    }

    #[test]
    fn default_writer_is_empty() {
        assert!(BinaryWriter::default().is_empty());
    }

    #[test]
    fn primitives_use_little_endian_layout() {
        let mut w = BinaryWriter::new();
        w.write_u16(0x0201);
        w.write_u32(0x0403_0201);
        assert_eq!(w.as_bytes(), &[0x01, 0x02, 0x01, 0x02, 0x03, 0x04]);
    }

    #[test]
    fn bool_is_one_byte() {
        let mut w = BinaryWriter::new();
        w.write_bool(true);
        w.write_bool(false);
        assert_eq!(w.into_bytes(), vec![1, 0]);
    }

    #[test]
    fn byte_slice_is_length_prefixed() {
        let mut w = BinaryWriter::new();
        w.write_byte_slice(&[0xAA, 0xBB, 0xCC]);
        // 3 as u32 little-endian, then the three bytes.
        assert_eq!(w.into_bytes(), vec![3, 0, 0, 0, 0xAA, 0xBB, 0xCC]);
    }

    #[test]
    fn writes_are_deterministic() {
        let mut a = BinaryWriter::new();
        let mut b = BinaryWriter::new();
        for w in [&mut a, &mut b] {
            w.write_u64(0x1122_3344_5566_7788);
            w.write_f32(1.5);
            w.write_i32(-7);
        }
        assert_eq!(a.into_bytes(), b.into_bytes());
    }
}
