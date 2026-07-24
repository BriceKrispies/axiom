//! Container sniffing + WOFF → sfnt decompression.
//!
//! `fontdue` reads raw sfnt (TTF/OTF). WOFF wraps sfnt tables in zlib, so we
//! reconstruct the sfnt before handing it on. WOFF2 (brotli + table transforms)
//! is not reconstructed here — the tool reports it clearly and asks for a
//! pre-decompressed TTF/OTF/WOFF.

use std::io::Read;

use flate2::read::ZlibDecoder;

/// The container a font byte stream is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Container {
    /// Raw sfnt (TrueType/OpenType) — usable directly.
    Sfnt,
    /// WOFF 1.0 — zlib-per-table, reconstructable.
    Woff,
    /// WOFF2 — brotli + transforms, not supported here.
    Woff2,
    /// Unrecognised.
    Unknown,
}

/// Sniff the container from the leading magic bytes.
pub fn sniff(bytes: &[u8]) -> Container {
    match bytes.get(0..4) {
        Some(b"wOFF") => Container::Woff,
        Some(b"wOF2") => Container::Woff2,
        Some(&[0x00, 0x01, 0x00, 0x00]) | Some(b"OTTO") | Some(b"true") | Some(b"ttcf") => {
            Container::Sfnt
        }
        _ => Container::Unknown,
    }
}

/// Return raw sfnt bytes for `input`, decompressing WOFF when needed. Errors on
/// WOFF2 or an unrecognised container.
pub fn to_sfnt(bytes: &[u8]) -> Result<Vec<u8>, String> {
    match sniff(bytes) {
        Container::Sfnt => Ok(bytes.to_vec()),
        Container::Woff => decode_woff(bytes),
        Container::Woff2 => Err(
            "WOFF2 input: brotli + table transforms are not reconstructed by this tool. \
             Convert to TTF/OTF/WOFF first (e.g. with a webfont tool)."
                .to_owned(),
        ),
        Container::Unknown => Err("unrecognised font container (not sfnt/WOFF/WOFF2)".to_owned()),
    }
}

fn be_u16(bytes: &[u8], at: usize) -> Result<u16, String> {
    bytes
        .get(at..at + 2)
        .map(|b| u16::from_be_bytes([b[0], b[1]]))
        .ok_or_else(|| "truncated WOFF".to_owned())
}

fn be_u32(bytes: &[u8], at: usize) -> Result<u32, String> {
    bytes
        .get(at..at + 4)
        .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| "truncated WOFF".to_owned())
}

/// Reconstruct an sfnt from a WOFF 1.0 container.
fn decode_woff(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let flavor = be_u32(bytes, 4)?;
    let num_tables = be_u16(bytes, 12)? as usize;
    // Decode each table entry (20 bytes each) from the directory at offset 44.
    let mut tables: Vec<(u32, Vec<u8>, u32)> = Vec::with_capacity(num_tables);
    for i in 0..num_tables {
        let base = 44 + i * 20;
        let tag = be_u32(bytes, base)?;
        let offset = be_u32(bytes, base + 4)? as usize;
        let comp_len = be_u32(bytes, base + 8)? as usize;
        let orig_len = be_u32(bytes, base + 12)? as usize;
        let checksum = be_u32(bytes, base + 16)?;
        let raw = bytes
            .get(offset..offset + comp_len)
            .ok_or_else(|| "WOFF table extends past the buffer".to_owned())?;
        let data = if comp_len < orig_len {
            let mut out = Vec::with_capacity(orig_len);
            ZlibDecoder::new(raw)
                .read_to_end(&mut out)
                .map_err(|e| format!("WOFF table inflate failed: {e}"))?;
            out
        } else {
            raw.to_vec()
        };
        tables.push((tag, data, checksum));
    }
    Ok(assemble_sfnt(flavor, &tables))
}

/// Build a valid sfnt byte stream from decoded tables.
fn assemble_sfnt(flavor: u32, tables: &[(u32, Vec<u8>, u32)]) -> Vec<u8> {
    let n = tables.len() as u16;
    let entry_selector = (15u16.saturating_sub(n.leading_zeros() as u16)).min(15);
    let search_range = (1u16 << entry_selector) * 16;
    let range_shift = n * 16 - search_range;
    let mut out = Vec::new();
    out.extend_from_slice(&flavor.to_be_bytes());
    out.extend_from_slice(&n.to_be_bytes());
    out.extend_from_slice(&search_range.to_be_bytes());
    out.extend_from_slice(&entry_selector.to_be_bytes());
    out.extend_from_slice(&range_shift.to_be_bytes());
    let mut offset = 12 + tables.len() * 16;
    let mut bodies: Vec<u8> = Vec::new();
    for (tag, data, checksum) in tables {
        out.extend_from_slice(&tag.to_be_bytes());
        out.extend_from_slice(&checksum.to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        bodies.extend_from_slice(data);
        while bodies.len() % 4 != 0 {
            bodies.push(0);
        }
        offset = 12 + tables.len() * 16 + bodies.len();
    }
    out.extend_from_slice(&bodies);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffs_containers() {
        assert_eq!(sniff(&[0x00, 0x01, 0x00, 0x00]), Container::Sfnt);
        assert_eq!(sniff(b"OTTO...."), Container::Sfnt);
        assert_eq!(sniff(b"wOFFxxxx"), Container::Woff);
        assert_eq!(sniff(b"wOF2xxxx"), Container::Woff2);
        assert_eq!(sniff(b"junk"), Container::Unknown);
    }

    #[test]
    fn sfnt_passes_through_and_woff2_errors() {
        let sfnt = vec![0x00, 0x01, 0x00, 0x00, 0, 0, 0, 0];
        assert_eq!(to_sfnt(&sfnt).unwrap(), sfnt);
        assert!(to_sfnt(b"wOF2....").unwrap_err().contains("WOFF2"));
        assert!(to_sfnt(b"junk").is_err());
    }
}
