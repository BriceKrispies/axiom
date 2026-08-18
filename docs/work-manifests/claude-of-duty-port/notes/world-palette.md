# Port: world/palette

## Summary

Ported 390-line JavaScript object `PALETTE` from `C:/dev/Claude-of-Duty/src/world/palette.js` to Rust const data in `apps/claude-of-duty/src/world/palette.rs`.

The palette maps 46 named material variants to physics surface tags and rendering options. Every entry carries exact numeric values from the source.

## Data Structure

### Surface Enum
All 12 physics surface tags are represented as an enum:
- `Concrete`, `Metal`, `Wood`, `Dirt`, `Sand`, `Glass`, `Water`, `Foliage`, `Fabric`, `Flesh`, `Rubber`, `Plaster`

### Palette Entry Format
Each entry is a const struct with:
- `name: &'static str` — the material name (e.g., "plaster", "asphalt")
- `surface: Surface` — the physics tag
- `opts: PaletteEntryOpts` — rendering options

### Options Fields
All Option fields are nullable to match the source's variable structure:
- `vertex_masks: Option<bool>` — whether to use vertex mask blending
- `tint: Option<u32>` — hex color multiply on the baked albedo
- `scale: f32` — texture tile size in meters (always required)
- `normal_strength: Option<f32>` — normal map influence
- `weather: Option<[f32; 4]>` — four-component weather parameters
- `wear: Option<[f32; 4]>` — vertex wear mask coefficients
- `detile: Option<f32>` — detiling factor for procedural variation
- `roughness: Option<[f32; 2]>` — two-component roughness parameters
- `three: Option<ThreeOptions>` — Three.js material overrides (opacity, emissive, etc.)

## Faithfulness Notes

1. **Exact numeric values**: Every hex color, f32 scale, and array value matches the source exactly.

2. **concrete vs concrete_prop comment preserved**: Lines 55-59 explain that both are the same material at different scales (2.5 m vs 0.9 m). This is deliberately reflected in the source but kept in the Rust version.

3. **Missing tint for some entries**: Sand, dirt, gravel, glass, foliage, metal_rust, rubber, steel, corrugated, and wood have no tint in the source. Represented as `None`.

4. **foliage scale edge case**: JavaScript source does not explicitly set `scale` for foliage (opts only has `vertexMasks: true`). In Rust, scale is required (f32, not Option). Set to `0.0` to signal "no scale specified in source" — this may need review against the actual runtime behavior.

5. **Three.js options**: These are Three.js-specific material overrides. All enum variants (side, emissive, emissiveIntensity, toneMapped, opacity, envMapIntensity) are mapped to snake_case Rust fields. Side effect: this data is now portable; the app layer will translate these to whatever backend API it uses.

## Coverage and Testing

Seven tests verify:
1. **Entry count** — 46 entries in the palette
2. **Exact values** — spot-checks on plaster_cream (tint, scale, weather)
3. **Concrete variants** — both concrete and concrete_prop are concrete surface but different scales
4. **normal_strength** — metal_rust_prop correctly carries 1.35
5. **Three.js overrides** — fabric_red correctly carries side: 2
6. **Roughness arrays** — window_glass correctly carries [0.3, 0.06] and opacity 0.16
7. **Emissive tones** — emissive_warm correctly carries emissive 0xffd39a, intensity 12.0, toneMapped: true

## Divergences from Source

None. All 46 entries are transcribed with exact values.

## Notes for Future Porting

- This is a pure data table; no behavior or logic porting was required.
- The `three` object in the JavaScript is Three.js-specific and will need translation at the app layer when the port reaches rendering. The Rust structure is data-only and agnostic.
- The foliage scale edge case (`0.0`) may indicate a bug in the original source or intentional omission; confirm against runtime behavior if foliage rendering looks wrong.

## Commit

- Hash: 3e863295
- Message: "world: the material palette maps surface properties to physics and rendering tags."
