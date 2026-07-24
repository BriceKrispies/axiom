//! Unicode range parsing: `U+0020-007E,U+00A0-00FF` → a sorted codepoint list.

/// Parse a comma-separated range spec into a sorted, de-duplicated list of
/// codepoints. Each item is `U+XXXX` (single) or `U+XXXX-YYYY` / `U+XXXX-U+YYYY`
/// (inclusive range). Hex is case-insensitive. Returns an error string for any
/// malformed item.
pub fn parse_ranges(spec: &str) -> Result<Vec<u32>, String> {
    let mut points: Vec<u32> = Vec::new();
    for item in spec.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let (lo, hi) = parse_item(item)?;
        if lo > hi {
            return Err(format!("range start past end: {item}"));
        }
        if hi - lo > 0x20_0000 {
            return Err(format!("range too large: {item}"));
        }
        points.extend(lo..=hi);
    }
    points.sort_unstable();
    points.dedup();
    Ok(points)
}

/// Parse one range item into an inclusive `(lo, hi)` pair.
fn parse_item(item: &str) -> Result<(u32, u32), String> {
    match item.split_once('-') {
        Some((lo, hi)) => Ok((parse_codepoint(lo)?, parse_codepoint(hi)?)),
        None => {
            let cp = parse_codepoint(item)?;
            Ok((cp, cp))
        }
    }
}

/// Parse a single `U+XXXX` (or bare hex) codepoint.
fn parse_codepoint(token: &str) -> Result<u32, String> {
    let hex = token
        .trim()
        .trim_start_matches("U+")
        .trim_start_matches("u+")
        .trim_start_matches("0x");
    u32::from_str_radix(hex, 16).map_err(|_| format!("bad codepoint: {token}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_and_ranges() {
        let cps = parse_ranges("U+0041").unwrap();
        assert_eq!(cps, vec![0x41]);
        let ascii = parse_ranges("U+0020-007E").unwrap();
        assert_eq!(ascii.first(), Some(&0x20));
        assert_eq!(ascii.last(), Some(&0x7E));
        assert_eq!(ascii.len(), 0x5F);
    }

    #[test]
    fn merges_multiple_and_dedups() {
        let cps = parse_ranges("U+0041-0043, U+0042-0044").unwrap();
        assert_eq!(cps, vec![0x41, 0x42, 0x43, 0x44]);
    }

    #[test]
    fn accepts_u_plus_on_both_ends() {
        assert_eq!(
            parse_ranges("U+0041-U+0043").unwrap(),
            vec![0x41, 0x42, 0x43]
        );
    }

    #[test]
    fn rejects_malformed_and_inverted() {
        assert!(parse_ranges("U+ZZZZ").is_err());
        assert!(parse_ranges("U+0050-0040").is_err());
        assert!(parse_ranges("U+0000-FFFFFFF").is_err());
    }

    #[test]
    fn ignores_blank_items() {
        assert_eq!(parse_ranges("U+0041,,").unwrap(), vec![0x41]);
    }
}
