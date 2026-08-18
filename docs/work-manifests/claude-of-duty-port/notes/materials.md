# Materials library port

**Commit:** e271b214  
**Tests:** 7 passed  
**Architecture check:** pass

## What was ported

The 19-entry materials library from `C:\dev\Claude-of-Duty\src\materials\library.js:16-401`, comprising:

### Masonry surfaces
- concrete
- concrete_floor
- brick
- plaster
- tile

### Ground surfaces
- asphalt
- sand
- dirt
- gravel

### Metal surfaces
- metal_rust
- metal_painted
- metal_brushed
- corrugated

### Organic surfaces
- wood
- fabric
- burlap
- foliage
- rubber
- glass

### Alias table (12 entries)
Maps user-friendly names to canonical library keys for physics surface lookup.

## Data structures

Three Rust structs model the JavaScript entry shape:

- **BakeParams**: texture generation parameters (size, world_size, relief, seed, param, tint_a, tint_b)
- **MatParams**: material rendering settings (scale, parallax, detile, detail, macro, weather, roughness, etc.)
- **ThreeOptions**: Three.js material options (physical, opacity, ior, sheen, anisotropy, etc.)

Each LibraryEntry holds a name, generator identifier, Surface enum tag, and these three parameter structs.

## Faithfulness notes

**glsl field → generator identifier**: The source's `glsl` field names GLSL shader generators (CONCRETE, BRICK, etc.). These are external to this module and handled separately. Each entry now has a `generator` string identifying which generator it uses (e.g., "concrete", "brick"). The actual shader source is not ported here.

**Exact numeric values**: All bake sizes, world sizes, relief depths, seeds, hex colours, and material parameters are transcribed exactly as they appear in the source. Two-decimal truncation on some floats is faithful to the original.

**Parameter vectors**: detail, macro, macro_big, patch, weather, and cloth arrays are all [f32; 4]. Wear material and roughness are [f32; 4] and [f32; 3] respectively, matching their source semantics.

**Optional fields**: Many parameters are Option<> to match the source's sparse definition. Absent fields read as None.

## Tests

7 tests verify:

1. **library_has_nineteen_surfaces**: Exact count.
2. **all_surfaces_have_canonical_names**: Names in expected order (concrete, concrete_floor, brick, … glass).
3. **aliases_resolve_to_real_entries**: Every alias maps to a real library entry.
4. **all_aliases_are_unique**: No duplicate alias keys.
5. **concrete_exact_bake_params**: Spot-check bake values (size=1024, world_size=2.5, relief=0.09, seed=11, param=[1,0,0,0]).
6. **metal_painted_tints**: Verify tint_a=0x4a5340, tint_b=0x2a2f26.
7. **glass_exact_values**: Comprehensive check on the most parameter-dense entry (bake size=512, world_size=2.0, relief=0.0008, seed=3; three.opacity=0.22, ior=1.52, env_map_intensity=1.6, depth_write=false).

## No divergences

The port is exact. All source constants, parameter order, and field semantics are preserved. Apps that need the materials library can now access these const data structures deterministically, with the understanding that the actual texture generation (the GLSL) is handled separately.
