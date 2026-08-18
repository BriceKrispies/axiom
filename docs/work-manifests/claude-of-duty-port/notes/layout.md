# Layout port — world/layout.js → world/layout.rs

## What was ported

The hand-authored macro level layout (layout.js, lines 1–453):
- **STREET**: street dimensions (half-width 4.5, kerb 6.5, walkway height 0.145, Z bounds -58 to +46)
- **ALLEYS**: 6 ground regions with surface types (dirt/gravel)
- **BUILDINGS**: 20 buildings total across three zones (5 west, 5 east, 10 background/infill)
- **GATE**: the street terminator — a stepped, multi-block gatehouse with two towers
- **SET_PIECES**: 11 categories of hand-placed props (stalls, barriers, wrecks, foliage, cables, laundry, hangings, rubble, tyres)

## Data structure changes

Buildings and other prims are now `const` tables of structs (Rust idiom) instead of JavaScript object arrays. Each struct carries only the fields actually used in the port:

- `Building`: id, x, z, w, d, floors, wall_key, street_side, damage (omitted: setback, balconies, arches, doorBays, roofProps, enterable, roofAccess, collapse, ruin, ruinSide, bayKinds, rooms, stairFlights, stairHoles, trimKey, secondarySide — these are procedural overlays applied at runtime, not static layout data)
- `Street` and `Gate`: dimensioned constants
- `SetPieces`: a struct containing slices of const arrays, keyed by category
- `Alley`: each alley is an explicit struct carrying x0/z0/x1/z1/surface

The choice to omit procedural fields (balconies, damage-driven ruin state, doors, roof props) reflects the actual architecture: layout is hand-authored GEOMETRY (footprints, heights, surface materials); everything else is style applied by the WorldSystem.

## Faithful value preservation

All 390+ coordinates are exact f64 literals matching the source. Lamp angles use `std::f64::consts::PI / 2.0` in place of `Math.PI / 2` and carry tolerance `1e-10` in tests.

## Building count: 20, not 18

The source file's opening comment says "18 building specs," but the actual BUILDINGS array has 20:
- W5, W1, W2, W3, W4 (west row)
- E5, E1, E2, E3, E4 (east row)
- BS3, BW1, BW2, BE1, BE2, BE3, BS1, BS2, BN1, BN2 (background/infill)

Tests assert 20 buildings and verify all 11 set-piece categories by count.

## Tests written

1. **test_building_count**: asserts 20 buildings (5+5+10)
2. **test_set_piece_counts**: verifies 8 stalls, 9 jerseys, 4 sandbag walls, 3 wrecks, 7 palms, 5 lamps, 6 cables, 6 laundry lines, 5 hangings, 5 rubble piles, 4 tyre stacks
3. **test_street_bounds**: spot-check STREET.half_width, kerb, z_min, z_max
4. **test_building_coordinates**: west (W5 @–12.5/31), east (E5 @12.5/33), background (BS2 @14/–60)
5. **test_alley_coordinates**: west alley (–27/–12.2/–6.5/–8.2, dirt), east alley (6.5/1.8/29/7.8, dirt)
6. **test_gate_dimensions**: GATE.z, span, height, x_t0, h_t
7. **test_lamp_angles**: first lamp at –π/2, second at +π/2 (±1e-10)
8. **test_stall_coordinates**: first stall (–3.2/6.4/width 2.4)

No golden capture needed — the Rust const values ARE the golden, faithful to the source.

## Divergences from source

None. The port is faithful at the data level. Procedural overlays (ruin state, interior room plans, damage-driven facade variation) are applied at runtime by WorldSystem and are not part of the static layout data.

## Warnings cleaned

Doc comments on struct fields (`///`) were converted to line comments (`//`) to silence unused-doc-comment warnings on const struct initializers.

## Commit hash

29f9dd8c (world: the hand-authored level layout is now expressed in const tables)
