//! Ported from Claude-of-Duty `src/weapons/geometry.js:423-444` (`mergeAll`,
//! `triCount`), plus the two Three.js utilities `mergeAll` leans on —
//! `mergeGeometries` and `mergeVertices` — ported from
//! `three/examples/jsm/utils/BufferGeometryUtils.js` (Three.js authors, MIT
//! licensed).
//!
//! `mergeAll`'s exact sequence (`geometry.js:423-438`) decides vertex order,
//! and vertex order decides whether a golden hash matches, so it is preserved
//! precisely: filter, short-circuit a single input, convert every remaining
//! input to non-indexed, normalize attributes, concatenate
//! (`mergeGeometries`), weld (`mergeVertices`), normalize once more.
//!
//! This is app code (`apps/`), outside the Branchless Law — plain `if`/`for`
//! throughout, matching the source's own control flow.

use std::collections::HashMap;

use super::geo::Geo;

/// `mergeAll(list)` (`geometry.js:423-438`).
///
/// The source's `list.filter(Boolean)` drops nullish entries; a Rust
/// `Vec<Geo>` cannot hold one, so the only source of an empty `clean` list
/// here is an empty `list` argument.
pub fn merge_all(list: Vec<Geo>) -> Option<Geo> {
    if list.is_empty() {
        return None;
    }
    if list.len() == 1 {
        // `if (clean.length === 1) return clean[0];` — returned as-is, no
        // non-indexing/normalize/weld pass.
        return Some(list.into_iter().next().unwrap());
    }
    Some(merge_many(list))
}

fn merge_many(clean: Vec<Geo>) -> Geo {
    let mut non_indexed: Vec<Geo> = Vec::with_capacity(clean.len());
    for g in clean {
        let mut g = if g.index.is_empty() { g } else { to_non_indexed(&g) };
        g.normalize_attributes();
        non_indexed.push(g);
    }
    let merged = merge_geometries(&non_indexed);
    let mut welded = merge_vertices(&merged);
    welded.normalize_attributes();
    welded
}

/// `BufferGeometry.toNonIndexed()`: expand indexed attributes into a flat
/// triangle soup, one entry per index. No counterpart in `geometry.js`
/// itself (`mergeAll` calls the Three method directly); this is that method,
/// specialized to `Geo`'s fixed position/normal/uv attribute set.
fn to_non_indexed(g: &Geo) -> Geo {
    let mut pos = Vec::with_capacity(g.index.len() * 3);
    let mut normal = Vec::with_capacity(g.index.len() * 3);
    let mut uv = Vec::with_capacity(g.index.len() * 2);
    for &i in &g.index {
        let i = i as usize;
        pos.extend_from_slice(&g.pos[i * 3..i * 3 + 3]);
        if !g.normal.is_empty() {
            normal.extend_from_slice(&g.normal[i * 3..i * 3 + 3]);
        }
        if !g.uv.is_empty() {
            uv.extend_from_slice(&g.uv[i * 2..i * 2 + 2]);
        }
    }
    Geo {
        pos,
        normal,
        uv,
        index: Vec::new(),
    }
}

/// `BufferGeometryUtils.mergeGeometries(geometries, useGroups = false)`
/// (`BufferGeometryUtils.js:133-322`), specialized to the path `mergeAll`
/// actually drives: every input already non-indexed, `useGroups = false`
/// (so no index merge, no group bookkeeping), and the fixed
/// position/normal/uv attribute set. `mergeAttributes`
/// (`BufferGeometryUtils.js:331-418`) for a plain (non-interleaved)
/// `Float32Array` attribute is exactly array concatenation in argument
/// order, which is what the loop below does directly per attribute.
fn merge_geometries(list: &[Geo]) -> Geo {
    let mut pos = Vec::new();
    let mut normal = Vec::new();
    let mut uv = Vec::new();
    for g in list {
        pos.extend_from_slice(&g.pos);
        normal.extend_from_slice(&g.normal);
        uv.extend_from_slice(&g.uv);
    }
    Geo {
        pos,
        normal,
        uv,
        index: Vec::new(),
    }
}

/// `BufferGeometryUtils.mergeVertices(geometry, tolerance)`
/// (`BufferGeometryUtils.js:644-800`), specialized to `tolerance = 1e-6` (the
/// fixed weld tolerance `mergeAll` always uses, `geometry.js:435`) and to
/// `Geo`'s fixed `position`/`normal`/`uv` attribute order (mirroring
/// `KEEP_ATTRS`, `geometry.js:30`, which is also the attribute insertion
/// order every real geometry in this kit carries).
///
/// The source hashes each vertex's attribute components by truncating
/// `value * hashMultiplier + hashAdditive` toward zero (JS `~~x`, i.e.
/// `ToInt32`, here `.trunc() as i64` — the two agree for any value in
/// realistic geometry range; `~~` additionally wraps mod 2^32 far outside
/// that range, which this port does not model) and welds vertices whose full
/// hash — position AND normal AND uv — collides; a hard edge (differing
/// normals) is never welded even at coincident positions. This always
/// receives non-indexed input from `merge_many` (mirroring `mergeAll`'s call
/// site), so every vertex is visited once, in order, exactly as the source's
/// `indices ? ... : i` loop does when `indices` is null.
fn merge_vertices(g: &Geo) -> Geo {
    const TOLERANCE: f64 = 1e-6;
    let half_tolerance = TOLERANCE * 0.5;
    let hash_multiplier = 10f64.powf((1.0 / TOLERANCE).log10());
    let hash_additive = half_tolerance * hash_multiplier;

    let mut hash_to_index: HashMap<[i64; 8], u32> = HashMap::new();
    let mut new_pos = Vec::new();
    let mut new_normal = Vec::new();
    let mut new_uv = Vec::new();
    let mut new_index = Vec::with_capacity(g.vert_count());
    let mut next_index: u32 = 0;

    for i in 0..g.vert_count() {
        let components = [
            g.pos[i * 3],
            g.pos[i * 3 + 1],
            g.pos[i * 3 + 2],
            g.normal[i * 3],
            g.normal[i * 3 + 1],
            g.normal[i * 3 + 2],
            g.uv[i * 2],
            g.uv[i * 2 + 1],
        ];
        let mut hash = [0i64; 8];
        for (slot, v) in hash.iter_mut().zip(components) {
            *slot = (f64::from(v) * hash_multiplier + hash_additive).trunc() as i64;
        }

        if let Some(&idx) = hash_to_index.get(&hash) {
            new_index.push(idx);
        } else {
            new_pos.extend_from_slice(&g.pos[i * 3..i * 3 + 3]);
            new_normal.extend_from_slice(&g.normal[i * 3..i * 3 + 3]);
            new_uv.extend_from_slice(&g.uv[i * 2..i * 2 + 2]);
            hash_to_index.insert(hash, next_index);
            new_index.push(next_index);
            next_index += 1;
        }
    }

    Geo {
        pos: new_pos,
        normal: new_normal,
        uv: new_uv,
        index: new_index,
    }
}
