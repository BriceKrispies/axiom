//! [`SpaceApi`] — the deterministic content-addressing facade.
//!
//! It mints [`Address`]es (root → child → parent over a hierarchical `u64`
//! key-path) and integrates them with the kernel's canonical byte + digest
//! primitives: a deterministic [`axiom_kernel::StableHash`] over an address's
//! canonical serialization, and a byte round-trip through
//! [`axiom_kernel::BinaryWriter`]/[`axiom_kernel::BinaryReader`]. The same address
//! always yields the same bytes and the same digest on every platform — the
//! property every future generator relies on to key entropy and index artifacts.
//! Branchless throughout (combinator chains, no `?`).

use axiom_kernel::{BinaryReader, BinaryWriter, KernelResult, StableHash};

use crate::address::Address;

/// The content-addressing facade. Stateless: addresses are values and every
/// operation is a pure deterministic function of its inputs.
#[derive(Debug)]
pub struct SpaceApi;

impl SpaceApi {
    /// The root address — the empty path.
    pub fn root() -> Address {
        Address::from_segments(Vec::new())
    }

    /// The child of `parent` at key `segment` (appends one level).
    pub fn child(parent: &Address, segment: u64) -> Address {
        let mut segments = parent.segments().to_vec();
        segments.push(segment);
        Address::from_segments(segments)
    }

    /// The parent of `address`, or `None` for the root. Branchless
    /// (`split_last().map(..)`).
    pub fn parent(address: &Address) -> Option<Address> {
        address
            .segments()
            .split_last()
            .map(|(_last, rest)| Address::from_segments(rest.to_vec()))
    }

    /// The address's canonical bytes: a length-prefixed little-endian segment
    /// list, so addresses of different depth can never alias.
    pub fn to_bytes(address: &Address) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        writer.write_u64(address.depth() as u64);
        address
            .segments()
            .iter()
            .for_each(|&segment| writer.write_u64(segment));
        writer.into_bytes()
    }

    /// Read an address back from [`Self::to_bytes`]. A truncated buffer is a
    /// clean error, never a panic. Branchless combinator chain (no `?`).
    pub fn from_bytes(bytes: &[u8]) -> KernelResult<Address> {
        let mut reader = BinaryReader::new(bytes);
        let length = reader.read_u64();
        length
            .and_then(|length| {
                (0..length)
                    .map(|_| reader.read_u64())
                    .collect::<KernelResult<Vec<u64>>>()
            })
            .map(Address::from_segments)
    }

    /// The stable digest of `address` — the kernel [`StableHash`] over its
    /// canonical bytes. Equal addresses digest equally; the length prefix keeps
    /// distinct addresses distinct.
    pub fn digest(address: &Address) -> StableHash {
        StableHash::of_bytes(&SpaceApi::to_bytes(address))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn root_child_parent_navigate_the_hierarchy() {
        let root = SpaceApi::root();
        assert!(root.is_root());
        assert_eq!(SpaceApi::parent(&root), None);

        let a = SpaceApi::child(&root, 4);
        let b = SpaceApi::child(&a, 9);
        assert_eq!(b.segments(), &[4, 9]);
        assert_eq!(SpaceApi::parent(&b), Some(a.clone()));
        assert_eq!(SpaceApi::parent(&a), Some(root));
    }

    #[test]
    fn serialization_round_trips() {
        let address = SpaceApi::child(&SpaceApi::child(&SpaceApi::root(), 1), 2);
        let bytes = SpaceApi::to_bytes(&address);
        assert_eq!(SpaceApi::from_bytes(&bytes), Ok(address));
        let root = SpaceApi::root();
        assert_eq!(SpaceApi::from_bytes(&SpaceApi::to_bytes(&root)), Ok(root));
    }

    #[test]
    fn from_bytes_rejects_a_truncated_buffer() {
        let mut bytes = SpaceApi::to_bytes(&SpaceApi::child(&SpaceApi::root(), 7));
        bytes.truncate(bytes.len() - 1);
        assert!(SpaceApi::from_bytes(&bytes).is_err());
        assert!(SpaceApi::from_bytes(&[]).is_err());
    }

    #[test]
    fn digest_is_stable_and_distinguishes_addresses() {
        let a = SpaceApi::child(&SpaceApi::root(), 5);
        assert_eq!(SpaceApi::digest(&a), SpaceApi::digest(&a.clone()));
        assert_ne!(
            SpaceApi::digest(&a),
            SpaceApi::digest(&SpaceApi::child(&SpaceApi::root(), 6))
        );
        // Length prefix keeps a prefix-address distinct from a deeper one.
        let deeper = SpaceApi::child(&a, 0);
        assert_ne!(SpaceApi::digest(&a), SpaceApi::digest(&deeper));
    }

    #[test]
    fn digests_are_collision_free_over_a_swept_domain() {
        let mut addresses = vec![SpaceApi::root()];
        for x in 0..6u64 {
            let depth1 = SpaceApi::child(&SpaceApi::root(), x);
            addresses.push(depth1.clone());
            for y in 0..6u64 {
                addresses.push(SpaceApi::child(&depth1, y));
            }
        }
        let digests: HashSet<u64> = addresses
            .iter()
            .map(|a| SpaceApi::digest(a).raw())
            .collect();
        assert_eq!(digests.len(), addresses.len());
    }
}
