//! The shared definition of a stable, strongly-typed kernel identifier.
//!
//! Every kernel ID is a distinct newtype over `u64` with identical, fully
//! deterministic behavior. Rather than copy that behavior — and its tests —
//! into six files, [`define_id`] emits one type, its API, and a complete unit
//! test module per invocation. Each ID module file therefore still exposes
//! exactly one public type, and each gets its own direct behavioral tests.

/// Define a strongly-typed `u64`-backed identifier with `0` reserved as the
/// null / invalid value.
///
/// Generated API: `NULL`, `from_raw`, `raw`, `is_valid`, and stable byte
/// serialization through the kernel binary writer/reader (`write_to` /
/// `read_from`). The type derives total ordering and hashing so it can key
/// deterministic collections.
macro_rules! define_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(u64);

        impl $name {
            /// The null / invalid identifier. Its raw value is always `0`.
            pub const NULL: $name = $name(0);

            /// Construct an identifier from its raw value.
            ///
            /// `0` constructs the null id; any other value is valid.
            pub const fn from_raw(raw: u64) -> Self {
                $name(raw)
            }

            /// The raw value backing this identifier.
            pub const fn raw(self) -> u64 {
                self.0
            }

            /// Whether this identifier is valid (non-null).
            pub const fn is_valid(self) -> bool {
                self.0 != 0
            }

            /// Serialize this identifier to the writer as a little-endian `u64`.
            pub fn write_to(self, writer: &mut crate::binary_writer::BinaryWriter) {
                writer.write_u64(self.0);
            }

            /// Read an identifier previously written with [`Self::write_to`].
            pub fn read_from(
                reader: &mut crate::binary_reader::BinaryReader<'_>,
            ) -> crate::result::KernelResult<Self> {
                Ok($name(reader.read_u64()?))
            }
        }

        impl Default for $name {
            fn default() -> Self {
                $name::NULL
            }
        }

        #[cfg(test)]
        mod tests {
            use super::$name;

            #[test]
            fn null_is_invalid_and_zero() {
                assert!(!$name::NULL.is_valid());
                assert_eq!($name::NULL.raw(), 0);
                assert_eq!($name::default(), $name::NULL);
            }

            #[test]
            fn from_raw_round_trips_and_is_valid() {
                let id = $name::from_raw(42);
                assert!(id.is_valid());
                assert_eq!(id.raw(), 42);
            }

            #[test]
            fn ordering_and_equality_are_numeric() {
                assert!($name::from_raw(1) < $name::from_raw(2));
                assert_eq!($name::from_raw(7), $name::from_raw(7));
                assert_ne!($name::from_raw(7), $name::from_raw(8));
            }

            #[test]
            fn binary_serialization_round_trips() {
                let id = $name::from_raw(0x0102_0304_0506_0708);
                let mut writer = crate::binary_writer::BinaryWriter::new();
                id.write_to(&mut writer);
                let bytes = writer.into_bytes();
                assert_eq!(bytes.len(), 8, "id must serialize to exactly 8 bytes");

                let mut reader = crate::binary_reader::BinaryReader::new(&bytes);
                let restored = $name::read_from(&mut reader).unwrap();
                assert_eq!(restored, id);
                assert_eq!(reader.remaining(), 0);
            }
        }
    };
}

pub(crate) use define_id;
