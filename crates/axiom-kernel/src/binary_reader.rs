//! A bounds-checked little-endian byte reader.

use crate::error::KernelError;
use crate::error_code::KernelErrorCode;
use crate::error_scope::KernelErrorScope;
use crate::result::KernelResult;

/// A cursor that decodes little-endian primitives written by a
/// [`crate::binary_writer::BinaryWriter`].
///
/// Every read is bounds-checked: if fewer bytes remain than the value requires,
/// the read fails with [`KernelErrorCode::OutOfBounds`] (or
/// [`KernelErrorCode::TruncatedData`] for a length-prefixed slice whose body is
/// cut short) and the cursor is left unmoved. Reads never panic and never read
/// uninitialized memory.
#[derive(Debug, Clone)]
pub struct BinaryReader<'a> {
    data: &'a [u8],
    position: usize,
}

impl<'a> BinaryReader<'a> {
    /// Wrap a byte slice for reading from the start.
    pub fn new(data: &'a [u8]) -> Self {
        BinaryReader { data, position: 0 }
    }

    /// Bytes not yet consumed.
    pub fn remaining(&self) -> usize {
        self.data.len() - self.position
    }

    /// The current read position.
    pub fn position(&self) -> usize {
        self.position
    }

    fn take(&mut self, count: usize) -> KernelResult<&'a [u8]> {
        if self.remaining() < count {
            return Err(KernelError::new(
                KernelErrorScope::Binary,
                KernelErrorCode::OutOfBounds,
                "binary reader ran past the end of the buffer",
            ));
        }
        let start = self.position;
        self.position += count;
        Ok(&self.data[start..self.position])
    }

    /// Read a `u8`.
    pub fn read_u8(&mut self) -> KernelResult<u8> {
        Ok(self.take(1)?[0])
    }

    /// Read a little-endian `u16`.
    pub fn read_u16(&mut self) -> KernelResult<u16> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    /// Read a little-endian `u32`.
    pub fn read_u32(&mut self) -> KernelResult<u32> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// Read a little-endian `u64`.
    pub fn read_u64(&mut self) -> KernelResult<u64> {
        let b = self.take(8)?;
        Ok(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    /// Read a little-endian `i32`.
    pub fn read_i32(&mut self) -> KernelResult<i32> {
        let b = self.take(4)?;
        Ok(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// Read a little-endian `f32`.
    pub fn read_f32(&mut self) -> KernelResult<f32> {
        let b = self.take(4)?;
        Ok(f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// Read a `bool` encoded as a single `0`/`1` byte. Any non-zero byte reads
    /// as `true`.
    pub fn read_bool(&mut self) -> KernelResult<bool> {
        Ok(self.read_u8()? != 0)
    }

    /// Read a length-prefixed byte slice written by
    /// [`crate::binary_writer::BinaryWriter::write_byte_slice`].
    ///
    /// Fails with [`KernelErrorCode::TruncatedData`] if the declared length
    /// exceeds the remaining bytes.
    pub fn read_byte_slice(&mut self) -> KernelResult<&'a [u8]> {
        let len = self.read_u32()? as usize;
        if self.remaining() < len {
            return Err(KernelError::new(
                KernelErrorScope::Binary,
                KernelErrorCode::TruncatedData,
                "length-prefixed byte slice extends past the buffer",
            ));
        }
        self.take(len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::binary_writer::BinaryWriter;

    #[test]
    fn primitive_round_trip_in_order() {
        let mut w = BinaryWriter::new();
        w.write_u8(0xAB);
        w.write_u16(0x1234);
        w.write_u32(0xDEAD_BEEF);
        w.write_u64(0x0102_0304_0506_0708);
        w.write_i32(-42);
        w.write_f32(2.5);
        w.write_bool(true);
        let bytes = w.into_bytes();

        let mut r = BinaryReader::new(&bytes);
        assert_eq!(r.read_u8().unwrap(), 0xAB);
        assert_eq!(r.read_u16().unwrap(), 0x1234);
        assert_eq!(r.read_u32().unwrap(), 0xDEAD_BEEF);
        assert_eq!(r.read_u64().unwrap(), 0x0102_0304_0506_0708);
        assert_eq!(r.read_i32().unwrap(), -42);
        assert_eq!(r.read_f32().unwrap(), 2.5);
        assert!(r.read_bool().unwrap());
        assert_eq!(r.remaining(), 0);
    }

    #[test]
    fn byte_slice_round_trip() {
        let mut w = BinaryWriter::new();
        w.write_byte_slice(&[9, 8, 7, 6]);
        let bytes = w.into_bytes();

        let mut r = BinaryReader::new(&bytes);
        assert_eq!(r.read_byte_slice().unwrap(), &[9, 8, 7, 6]);
    }

    #[test]
    fn out_of_bounds_read_fails_and_does_not_advance() {
        let bytes = [1u8, 2, 3]; // only 3 bytes
        let mut r = BinaryReader::new(&bytes);
        let err = r.read_u32().unwrap_err(); // needs 4
        assert_eq!(err.scope(), KernelErrorScope::Binary);
        assert_eq!(err.code(), KernelErrorCode::OutOfBounds);
        assert_eq!(r.position(), 0, "failed read must not advance the cursor");
    }

    #[test]
    fn truncated_length_prefixed_slice_fails() {
        // Declares length 10 but provides only 2 body bytes.
        let mut w = BinaryWriter::new();
        w.write_u32(10);
        w.write_u8(1);
        w.write_u8(2);
        let bytes = w.into_bytes();

        let mut r = BinaryReader::new(&bytes);
        let err = r.read_byte_slice().unwrap_err();
        assert_eq!(err.code(), KernelErrorCode::TruncatedData);
    }

    #[test]
    fn empty_buffer_reads_fail() {
        let mut r = BinaryReader::new(&[]);
        assert_eq!(
            r.read_u8().unwrap_err().code(),
            KernelErrorCode::OutOfBounds
        );
    }
}

#[cfg(test)]
mod cov {
    use super::*;
    use crate::binary_writer::BinaryWriter;

    #[test]
    fn round_trips_every_read() {
        let mut w = BinaryWriter::new();
        w.write_u16(0x0102);
        w.write_u32(0x0304_0506);
        w.write_u64(0x0708_090A_0B0C_0D0E);
        w.write_i32(-5);
        w.write_f32(1.5);
        w.write_bool(true);
        w.write_byte_slice(&[9, 8, 7]);
        let bytes = w.into_bytes();
        let mut r = BinaryReader::new(&bytes);
        assert_eq!(r.read_u16().unwrap(), 0x0102);
        assert_eq!(r.read_u32().unwrap(), 0x0304_0506);
        assert_eq!(r.read_u64().unwrap(), 0x0708_090A_0B0C_0D0E);
        assert_eq!(r.read_i32().unwrap(), -5);
        assert_eq!(r.read_f32().unwrap(), 1.5);
        assert!(r.read_bool().unwrap());
        assert_eq!(r.read_byte_slice().unwrap(), &[9, 8, 7]);
        assert_eq!(r.remaining(), 0);
        assert_eq!(r.position(), bytes.len());
    }

    #[test]
    fn read_past_end_is_out_of_bounds() {
        let mut r = BinaryReader::new(&[0u8]);
        assert!(r.read_u32().is_err());
    }

    #[test]
    fn truncated_byte_slice_is_err() {
        let mut w = BinaryWriter::new();
        w.write_u32(100); // declared length far beyond the buffer
        let bytes = w.into_bytes();
        let mut r = BinaryReader::new(&bytes);
        assert!(r.read_byte_slice().is_err());
    }
}

#[cfg(test)]
mod cov2 {
    use super::*;

    #[test]
    fn every_read_propagates_out_of_bounds() {
        assert!(BinaryReader::new(&[0u8; 0]).read_u8().is_err());
        assert!(BinaryReader::new(&[0u8]).read_u16().is_err());
        assert!(BinaryReader::new(&[0u8, 1, 2]).read_u32().is_err());
        assert!(BinaryReader::new(&[0u8; 7]).read_u64().is_err());
        assert!(BinaryReader::new(&[0u8, 1, 2]).read_i32().is_err());
        assert!(BinaryReader::new(&[0u8, 1, 2]).read_f32().is_err());
        assert!(BinaryReader::new(&[0u8; 0]).read_bool().is_err());
        assert!(BinaryReader::new(&[0u8; 0]).read_byte_slice().is_err());
    }
}
