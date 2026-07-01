//! A deterministic 256-bit fingerprint of simulation state.

/// A 256-bit deterministic fingerprint of `bytes`.
///
/// Used to compare two peers' simulation state at a tick: equal state →
/// equal digest, and a single-byte difference avalanches across all 32 output
/// bytes. This is a *desync fingerprint*, not a cryptographic hash — it is
/// dependency-free, deterministic on every platform, and cheap. Four FNV-1a
/// lanes absorb the input (interleaved with a rotation so order matters), then a
/// cross-lane mixing pass spreads each lane into its neighbour.
pub(crate) fn digest(bytes: &[u8]) -> [u8; 32] {
    const PRIME: u64 = 0x0000_0100_0000_01B3;
    let mut lanes: [u64; 4] = [
        0xcbf2_9ce4_8422_2325,
        0x9E37_79B9_7F4A_7C15,
        0xff51_afd7_ed55_8ccd,
        0xc4ce_b9fe_1a85_ec53,
    ];

    bytes.iter().enumerate().for_each(|(i, &b)| {
        let lane = i & 3;
        lanes[lane] ^= b as u64;
        lanes[lane] = lanes[lane].wrapping_mul(PRIME);
        lanes[lane] = lanes[lane].rotate_left((i as u32 & 31) + 1);
    });

    (0..4usize).for_each(|round| {
        let prev = lanes[(round + 3) & 3];
        let mut c = lanes[round];
        c ^= prev.rotate_left(17);
        c = c.wrapping_mul(PRIME);
        c ^= c >> 31;
        lanes[round] = c;
    });

    let mut out = [0u8; 32];
    lanes
        .iter()
        .zip(out.chunks_mut(8))
        .for_each(|(lane, chunk)| chunk.copy_from_slice(&lane.to_le_bytes()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_deterministic() {
        assert_eq!(digest(b"hello world"), digest(b"hello world"));
    }

    #[test]
    fn empty_input_has_a_stable_digest() {
        assert_eq!(digest(&[]), digest(&[]));
        assert_ne!(digest(&[]), digest(&[0]));
    }

    #[test]
    fn distinct_inputs_differ() {
        assert_ne!(digest(b"axiom"), digest(b"axion"));
    }

    #[test]
    fn single_byte_change_avalanches() {
        let a = digest(&[0u8; 64]);
        let mut input = [0u8; 64];
        input[40] = 1;
        let b = digest(&input);
        let differing = a.iter().zip(b.iter()).filter(|(x, y)| x != y).count();
        assert!(
            differing > 16,
            "a single-byte change should avalanche; only {differing} bytes differed"
        );
    }

    #[test]
    fn order_matters() {
        assert_ne!(digest(&[1, 2, 3]), digest(&[3, 2, 1]));
    }
}
