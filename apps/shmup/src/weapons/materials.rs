//! Ported from Claude-of-Duty `src/weapons/materials.js:1-1215` — the whole
//! file.
//!
//! The weapon material set: a **re-parameterisation of the shared procedural
//! PBR library** ([`crate::materials::LIBRARY`]) for hand-held scale. Nothing
//! here generates a texel; every entry names one of the library's 19 surfaces
//! and overrides its parameters, because a weapon seen at 0.4 m needs
//! different numbers from the same surface on a building. Three things matter
//! at that distance and are what these overrides are for (`materials.js:5-24`):
//!
//!  1. **Texel density.** The library bakes for architecture (a 2.5 m tile).
//!     A weapon needs a 0.02-0.12 m tile plus a detail layer at ~1-5 mm, and
//!     `detail[3]` (the fade distance) pulled in to 4-6 m so the micro layer
//!     is at full strength in the hand.
//!  2. **Object-space projection.** `local_space` + `triplanar` nails the
//!     texture to the mesh, so nothing swims while the viewmodel animates and
//!     no UV unwrap is needed for procedurally merged geometry.
//!  3. **Edge wear.** Every weapon geometry gets curvature vertex masks baked
//!     ([`crate::materials::masks`]); these materials turn that mask into bare
//!     bright metal on the chamfers of high-contact parts.
//!
//! World-space weathering (dust, rain streaks, ground splash) is switched off
//! everywhere — it is driven by world Y, meaningless for something parented to
//! the camera. Cavity grime (`weather[3]`) is height-driven and stays on.
//!
//! # How an entry becomes a material
//!
//! `WEAPON_MATERIALS[key]` is `[libraryName, opts]`. `MaterialSystem.get`
//! (`src/materials/index.js:179-225`) then performs three independent merges,
//! all of them plain JS object spreads (last writer wins):
//!
//! ```text
//!   bake  = { ...LIBRARY[lib].bake,  ...opts.bake  }     index.js:129
//!   three = { ...LIBRARY[lib].three, ...opts.three }     index.js:193
//!   p     = { ...DEFAULT_PARAMS, ...LIBRARY[lib].mat, ...opts }
//!                                            (minus three/bake)  index.js:188
//! ```
//!
//! This module owns the first two — [`resolved_bake`] and [`resolved_three`] —
//! because both halves are ported data ([`crate::materials::LIBRARY`] plus the
//! table below). It deliberately does **not** own the third: `DEFAULT_PARAMS`
//! belongs to `src/materials/shader.js`, which is a separate slice (the
//! runtime material shader, destined for hand-written WGSL). What this module
//! declares are exactly the `opts`, which win that spread outright, so nothing
//! is lost by leaving the defaults where they live.
//!
//! # Traps checked in this slice
//!
//! * **An enum used as a table index is order-dependent.** This file is a
//!   table of per-surface recipes keyed by name, which is the exact shape that
//!   silently reindexed the per-surface audio recipes earlier in this port.
//!   The library each key names is stored as the library's own `&'static str`
//!   and resolved through [`crate::materials::LIBRARY`] at lookup time, so a
//!   reordering of either table cannot silently repoint an entry. The two
//!   orderings were also compared explicitly: `src/materials/library.js`'s 19
//!   keys and `crate::materials::LIBRARY`'s 19 entries **agree**, name for name
//!   and position for position (see the notes file).
//! * **`Float32Array`.** The source file contains none. It does contain one
//!   `Uint8Array` — the [`rim_ramp`] texture, whose eight-bit quantisation
//!   *is* part of the algorithm and is reproduced exactly.
//! * **`Math.hypot` is not `sqrt(x*x + y*y)`.** [`rim_ramp`] uses
//!   `f64::hypot`, the direct analogue, not the expanded form.
//! * **Float arithmetic is not associative.** See [`srgb_to_linear`]: three's
//!   sRGB decode is algebraically equal to the library's own GLSL `owSRGB`
//!   ([`crate::materials::noise::ow_srgb`]) and numerically *different* in 254
//!   of 256 byte values. Every hex colour in this file goes through three's
//!   grouping, because that is what the original does.
//! * **Dead computation in the source is still part of the source.** Two here:
//!   [`glass`]'s `tint` argument, and the `ior: 1.52` in the same call. Both
//!   are ported with the behaviour they actually produce and pinned by name in
//!   `tests/weapons_materials_port.rs`.
//!
//! # Precision
//!
//! JS numbers are `f64` and none of this data passes through a `Float32Array`
//! on the way to the merge, so the table is `f64` — the source's own
//! precision. [`resolved_bake`] narrows at the one seam where the ported
//! pipeline is already `f32` ([`crate::materials::BakeParams`], which
//! [`crate::materials::bake::BakeDef`] consumes as `f32`).
//!
//! This is app code (`apps/`), outside the Branchless Law and the Coverage
//! Law — plain `if`/`for` throughout.

use crate::materials::{BakeParams, LibraryEntry, ALIASES, LIBRARY};

/// How much of the sky hemisphere a shouldered weapon actually sees
/// (`materials.js:60`).
///
/// Applied to every weapon/hand material's `envMapIntensity` (see
/// [`MaterialRequest::Library`]) **and** to `viewScene.environmentIntensity`
/// in `weapons/index.js`, which is the one that actually bites: three ignores
/// `material.envMapIntensity` for a material lit by `scene.environment` alone.
pub const ENV_OCCLUSION: f64 = 0.24;

// ---------------------------------------------------------------------------
// The shared base
// ---------------------------------------------------------------------------

/// `const BASE` (`materials.js:28-49`) — the object every entry spreads
/// first.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BaseParams {
    /// `uvMode`.
    pub uv_mode: &'static str,
    /// `localSpace` — project in the object's local space, not world space.
    pub local_space: bool,
    /// `vertexMasks` — read the curvature wear/grime/AO vertex colours.
    pub vertex_masks: bool,
    /// `weather` = `[dust, rainStreaks, groundSplash, cavityGrime]`.
    ///
    /// The first three are driven by world Y, meaningless for something
    /// parented to the camera, so they are zero on every weapon surface.
    /// Cavity grime is driven by the surface's own object-space height
    /// channel (`shader.js`: `cav = 1 - owHeightS`), so it cannot swim: it
    /// darkens the valleys of the moulding stipple / anodising grain and adds
    /// AO to them.
    pub weather: [f64; 4],
    /// `macro` = `[worldScale, albedoStrength, roughnessStrength, hueStrength]`.
    ///
    /// Low amplitude, because the macro layer is the one thing sampled in
    /// world space and would otherwise crawl across the gun as the player
    /// moves.
    pub macro_: [f64; 4],
    /// `aoStrength`.
    pub ao_strength: f64,
}

/// `BASE` (`materials.js:28-49`).
pub const BASE: BaseParams = BaseParams {
    uv_mode: "triplanar",
    local_space: true,
    vertex_masks: true,
    weather: [0.0, 0.0, 0.0, 0.62],
    macro_: [0.55, 0.05, 0.07, 0.06],
    ao_strength: 1.0,
};

// ---------------------------------------------------------------------------
// One entry
// ---------------------------------------------------------------------------

/// `opts.bake` — the partial bake override an entry may carry.
///
/// Every field is optional because the source spreads a partial literal over
/// the library's own bake (`{ ...def.bake, ...opts.bake }`), so an absent key
/// leaves the library's value standing. `worldSize` and `param` are never
/// overridden by any weapon entry and so have no field here.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BakeOverride {
    pub size: Option<u32>,
    pub seed: Option<u32>,
    pub relief: Option<f64>,
    pub tint_a: Option<u32>,
    pub tint_b: Option<u32>,
}

/// `opts.three` — raw THREE material properties an entry sets.
///
/// Deliberately **not** [`crate::materials::ThreeOptions`], for two reasons
/// that are both about faithfulness rather than taste: that struct has no
/// `metalness` field (which `steel_soot` sets, and which is the whole point of
/// treating soot as a dielectric powder), and it is `f32` where this table is
/// `f64`. It carries a dozen fields no weapon entry ever touches, too.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ThreeOverride {
    /// `physical: true` selects `MeshPhysicalMaterial` over
    /// `MeshStandardMaterial`; `specularIntensity` does not exist without it.
    pub physical: Option<bool>,
    pub metalness: Option<f64>,
    pub specular_intensity: Option<f64>,
    pub anisotropy: Option<f64>,
    pub sheen: Option<f64>,
    pub sheen_roughness: Option<f64>,
    /// sRGB hex; three decodes it to linear at construction.
    pub sheen_color: Option<u32>,
}

/// One `WEAPON_MATERIALS` row: `key: [libraryName, { ...BASE, ...overrides }]`.
///
/// The eight non-optional fields below (`scale`, `tint`, `roughness`,
/// `normal_strength`, `detail`, `wear`, `wear_color`, `wear_material`) are set
/// by all fifteen entries; the four optional ones are set by some.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeaponMaterial {
    /// The `WEAPON_MATERIALS` key.
    pub key: &'static str,
    /// `def[0]` — the [`crate::materials::LIBRARY`] surface this
    /// re-parameterises. Stored as the library's own canonical name and
    /// resolved at lookup ([`library_entry`]) rather than as an index, so
    /// neither table's ordering can silently repoint an entry.
    pub library: &'static str,
    /// The `...BASE` spread, with the per-entry overrides Rust's struct-update
    /// syntax makes visible at the site (only `weather` is ever overridden).
    pub base: BaseParams,
    pub bake: Option<BakeOverride>,
    /// Metres per texture tile.
    pub scale: f64,
    /// `tint` — a `THREE.Color(r, g, b)`, i.e. **linear** floats passed
    /// straight through (three's rgb constructor does no colour-space
    /// conversion; only the hex constructor decodes). It multiplies the
    /// surface's own baked albedo, so it is not the linear albedo. For a
    /// `metalness: 1` entry (`steel`, `steel_bright`, `steel_black`, `brass`,
    /// `copper`) three folds the albedo into F0, so this is the **F0**, not an
    /// albedo — which is why those five move their tint and roughness where
    /// every dielectric moves its `specularIntensity`. Values above 1 are
    /// legitimate here: `brass` is `(2.3, 1.58, 0.74)`.
    pub tint: [f64; 3],
    /// `[scale, offset, minimum]` against the surface's own ORM green channel.
    pub roughness: [f64; 3],
    pub normal_strength: f64,
    /// `[tilesPerBaseTile, normalAmp, albedoAmp, fadeMetres]`.
    pub detail: [f64; 4],
    /// Vertex-mask amplitudes `[wear, grime, extraAO, unused]`.
    pub wear: [f64; 4],
    /// sRGB hex the wear mask mixes toward.
    pub wear_color: u32,
    /// `[roughness, metalness, unused, tintAmount]` where the wear mask is 1.
    pub wear_material: [f64; 4],
    /// sRGB hex. Absent on `brass` and `copper`, which inherit the library's.
    pub grime_color: Option<u32>,
    /// `[transmission, undersideMultiplier, foldAmount, unused]`. Set only by
    /// the three `fabric`-backed entries, to cancel the 0.20 sun-transmission
    /// term the library authored for market awnings: a glove is not an awning
    /// and a sleeve is opaque.
    pub cloth: Option<[f64; 4]>,
    pub three: ThreeOverride,
}

/// `WEAPON_MATERIALS` (`materials.js:66-864`), in source order — "roughly from
/// receiver outward so the log reads like a parts list".
///
/// Order is load-bearing twice over: [`material_keys`] is
/// `Object.keys(WEAPON_MATERIALS)` (`materials.js:1215`), and the golden pins
/// each entry's index.
pub const WEAPON_MATERIALS: &[WeaponMaterial] = &[
    // -- alu ---------------------------------------------------------------
    // Hard-anodised aluminium — upper/lower receiver, rails, handguard.
    //
    // **Not `metal_painted`**: that surface is authored for industrial
    // painted steel and layers rust bloom, rain streaks and bright
    // bare-metal scratches over everything, which on a 0.2 m receiver reads
    // as a weathered dumpster. Type-III hard-coat anodising is a matte black
    // *dielectric* oxide with a fine sub-millimetre grain, so `rubber` is the
    // honest base and the bare-aluminium wear comes from the vertex edge
    // mask.
    WeaponMaterial {
        key: "alu",
        library: "rubber",
        base: BASE,
        bake: Some(BakeOverride {
            size: Some(1024),
            seed: Some(601),
            relief: Some(0.005),
            tint_a: None,
            tint_b: None,
        }),
        scale: 0.095,
        // MATERIAL CLASS 1 of 3 — hard-anodised aluminium. Deliberately COOL,
        // because the other two classes are a WARM polymer (class 2) and a
        // metal with no albedo at all (class 3, only an F0); hue is the only
        // separation cue that survives a part being 40 px wide in hipfire.
        //
        // THE VIEWMODEL EXPOSURE RECALIBRATION (materials.js:98-138), measured
        // by reading the framebuffer back with every specular path off:
        //
        //              base albedo   diffuse-only   with spec (shipped)
        //   rail          0.0033        L=106            L=192
        //   receiver      0.0033        L= 26            L= 67
        //   handguard     0.0027        L= 32            L=101
        //   magazine      0.0027        L= 26            L= 62
        //
        // Sixty percent of what the eye saw on the receiver was Fresnel, and a
        // dielectric's specular lobe carries no material identity, so no
        // amount of texturing could ever have shown up. Two coupled moves,
        // neither of which works alone: specularIntensity 0.5 -> 0.11 (a real
        // Type-III oxide is ~0.02 reflectance) and albedo x3 (0.098 -> 0.285,
        // putting the anodising at ~0.0095 linear). The hue ratio is unchanged.
        tint: [0.285, 0.302, 0.349],
        // 0.66/0.09 with a hard 0.24 floor lands the anodising at 0.31-0.53 —
        // matte, with enough range left that the detail layer's roughness
        // modulation reads as a grain.
        roughness: [0.66, 0.09, 0.24],
        // Tuned when the surface was specular-dominated, where a normal
        // perturbation only shifts the lobe; with diffuse in charge these are
        // the texture. 22 tiles over a 95 mm base tile is a 4.3 mm cell.
        normal_strength: 1.5,
        detail: [22.0, 1.2, 0.72, 5.0],
        // The vertex edge mask bleeds across chamfered panels (they have no
        // interior vertices), so the amplitude stays low and viewmodel.js's
        // exponent keeps it on the outer millimetre.
        wear: [0.2, 0.6, 0.5, 0.0],
        // MEASURED — the fix for "bright cream blocky bits". On a SMALL part
        // (a bolt-catch boss, a takedown pin head) every vertex is convex, so
        // the whole part got painted with wearColor: at 0x585c63 that is 0.107
        // linear, eleven times the anodising, and wearMaterial was flipping it
        // to metalness 1 at roughness 0.30 — a polished mirror. 0x3c4046 ->
        // 0x34383d is 0.037 linear, ~3.9x the oxide; roughness 0.54 and
        // metalness 0.8 take the mirror out.
        wear_color: 0x34383d,
        wear_material: [0.54, 0.8, 0.0, 0.8],
        grime_color: Some(0x0b0a08),
        cloth: None,
        three: ThreeOverride {
            physical: Some(true),
            specular_intensity: Some(0.11),
            ..NO_THREE
        },
    },
    // -- alu_fine ----------------------------------------------------------
    // The same anodising with the grain pulled in to ~0.5 mm.
    //
    // In ADS the optic body is 110-145 mm from the eye — three times closer
    // than the receiver ever gets — so the 1.5 mm stipple that reads as a
    // fine machined finish on the receiver reads as cast concrete on the
    // sight. Anything the player presses their eye against gets this.
    WeaponMaterial {
        key: "alu_fine",
        library: "rubber",
        base: BASE,
        bake: Some(BakeOverride {
            size: Some(1024),
            seed: Some(733),
            relief: Some(0.0025),
            tint_a: None,
            tint_b: None,
        }),
        scale: 0.038,
        // Same alloy and the same anodising bath as `alu`, one step darker and
        // one step smoother because an optic body is bead-blasted before it is
        // coated. x1.45 rather than the receiver's x3: MEASURED IN ADS, at
        // x2.2 the optic body area-averaged L=97 against a sunlit world wall
        // at 169 — a black sight reading as mid-grey plastic. 0.135 lands it
        // at ~70 with its chamfers still reaching 180+.
        tint: [0.135, 0.144, 0.165],
        // The bezel around the objective is exactly where a smooth facet turns
        // into a cream grazing ring in ADS.
        roughness: [0.56, 0.07, 0.26],
        normal_strength: 1.15,
        detail: [30.0, 0.85, 0.6, 4.0],
        wear: [0.18, 0.5, 0.5, 0.0],
        // Same argument as `alu`: the turret caps, the clamp rings and the
        // mount are all small convex parts whose every vertex reads as an edge.
        wear_color: 0x40444a,
        wear_material: [0.5, 0.8, 0.0, 0.75],
        grime_color: Some(0x0b0a08),
        cloth: None,
        // In ADS the eye looks straight down the tube, so every ray just
        // outside the exit pupil grazes the tube's own flank, and
        // MeshStandardMaterial hard-codes specularF90 = 1.0 — a matte black
        // oxide reflecting the sky like polished chrome, a 2.5 mm bright warm
        // band right around the sight picture. 0.45 -> 0.28 -> 0.16 -> 0.08:
        // re-measured radially, the band was still 225-262 px at ~200 sRGB at
        // 0.16. Halving again is the amplitude half of the fix; the other half
        // is geometric (the rear of the sight is a rubber bezel now, see
        // `parts.js` buildOptic `cup`).
        three: ThreeOverride {
            physical: Some(true),
            specular_intensity: Some(0.08),
            ..NO_THREE
        },
    },
    // -- steel -------------------------------------------------------------
    // Parkerised / phosphated steel: barrel, gas block, pins, small parts.
    //
    // Manganese phosphate is a genuine metal conversion coating — metalness
    // 1, F0 pulled well below neutral steel and roughness pushed up near 0.8,
    // which is what gives a barrel its dead, non-reflective grey-brown look.
    WeaponMaterial {
        key: "steel",
        library: "metal_brushed",
        base: BASE,
        bake: Some(BakeOverride {
            size: Some(512),
            seed: Some(617),
            relief: Some(0.006),
            tint_a: None,
            tint_b: None,
        }),
        scale: 0.12,
        // MATERIAL CLASS 3 of 3 — metalness 1, so this is F0, not an albedo.
        // 0.42 -> 0.30 -> 0.17. MEASURED IN ADS: the folded rear sight sits
        // 74 mm from the eye and was rendering the bottom 180 px of the frame
        // as a pale cream slab at L=210-224. `specularIntensity` cannot touch
        // a metal and roughness makes it WORSE past ~0.5 (a wider lobe on a
        // metal collects more of the env hemisphere), so F0 is the only lever.
        tint: [0.17, 0.162, 0.152],
        // The metal_brushed ORM runs ~0.30-0.60. The old [1.5, 0.34] mapped
        // that to 0.79-1.0 — saturated matte, and with metalness 1 a perfectly
        // matte metal has neither a specular lobe nor a diffuse term: a black
        // hole that picks up only the flat env average. [0.66, 0.24] with a
        // 0.42 floor lands parkerised steel at 0.35-0.56. MEASURED, and this
        // is as far as roughness goes: [0.60, 0.30] made the remaining bright
        // bead BRIGHTER, 0.509 -> 0.580 linear.
        roughness: [0.66, 0.24, 0.42],
        normal_strength: 1.2,
        detail: [13.0, 0.95, 0.42, 5.0],
        // A barrel and gas block DO polish on the high spots — more wear than
        // the receiver, still nowhere near a whole-surface effect.
        wear: [0.16, 0.55, 0.5, 0.0],
        wear_color: 0x62666b,
        wear_material: [0.26, 1.0, 0.0, 0.7],
        grime_color: Some(0x0c0a07),
        cloth: None,
        // Note: no `physical` here — it arrives from the library's own
        // `metal_brushed.three`, along with `anisotropyRotation: 0`.
        three: ThreeOverride {
            anisotropy: Some(0.1),
            ..NO_THREE
        },
    },
    // -- steel_soot --------------------------------------------------------
    // SOOTED steel — the muzzle device and the gas block.
    //
    // Everything within ~40 mm of a muzzle crown, and everything the gas
    // system vents through, is coated in carbon within a magazine of firing.
    // It is the single most recognisable "this weapon has been used" cue and
    // it lives exactly where the eye goes in the hipfire frame.
    WeaponMaterial {
        key: "steel_soot",
        library: "metal_brushed",
        // 0.75 cavity grime rather than BASE's 0.62: it fills the ports and
        // the flutes, which is where soot actually collects.
        base: BaseParams {
            weather: [0.0, 0.0, 0.0, 0.75],
            ..BASE
        },
        bake: Some(BakeOverride {
            size: Some(512),
            seed: Some(617),
            relief: Some(0.006),
            tint_a: None,
            tint_b: None,
        }),
        scale: 0.1,
        // CARBON IS NOT A METAL, and treating it as one is why every attempt
        // to darken the muzzle brake failed. MEASURED: as a metal at F0 0.085
        // x brushed base, roughness floored at 0.80, the brake's upper flank
        // still rendered L=230-237 — display white. With metalness 1 there is
        // no diffuse term at all, so the only thing on screen is a GGX lobe,
        // and a cylinder guarantees some band of it sits in the key's mirror
        // direction whatever the roughness. Dropping F0 and raising roughness
        // moved it by 7 code values across two attempts. Soot is a dielectric
        // powder sitting ON the phosphate: metalness 0.12 (below) makes the
        // surface diffuse-dominant, so it finally has an albedo to be dark
        // with, and the albedo comes down to match — 0.085 -> 0.022, ~0.013
        // linear, level with the anodised receiver.
        tint: [0.022, 0.02, 0.018],
        // Floored at 0.80, higher than anything else on the weapon. MEASURED:
        // at 0.62 the brake's top facet still rendered a 25 x 12 px cream
        // highlight at L=190. Carbon is the one surface on the gun where a
        // near-total diffusion of the lobe is also the physically right answer.
        roughness: [0.42, 0.5, 0.8],
        normal_strength: 1.3,
        detail: [15.0, 1.0, 0.5, 5.0],
        // A sooted brake has no bright high spots left on it: the
        // polish-through wear layer is cut to a third of `steel`'s.
        wear: [0.06, 0.7, 0.55, 0.0],
        wear_color: 0x3a3c3e,
        wear_material: [0.55, 1.0, 0.0, 0.6],
        grime_color: Some(0x070604),
        cloth: None,
        three: ThreeOverride {
            physical: Some(true),
            metalness: Some(0.12),
            specular_intensity: Some(0.1),
            anisotropy: Some(0.06),
            ..NO_THREE
        },
    },
    // -- steel_bright ------------------------------------------------------
    // Bare, oiled steel: bolt carrier, charging handle, trigger, sight
    // blades. These ARE polished metal, so they keep the brushed surface —
    // but with the anisotropy pulled right down, because a bolt carrier is
    // turned and machined, not sanded in one direction.
    //
    // No `bake` override: this is the only metal entry that bakes the
    // library's own `metal_brushed` texture set (512 px, seed 83, relief
    // 0.004) unmodified — as do `brass` and `copper`.
    WeaponMaterial {
        key: "steel_bright",
        library: "metal_brushed",
        base: BASE,
        bake: None,
        scale: 0.05,
        // Nitrided / oiled steel: a metal, so the "albedo" is its F0.
        // MEASURED IN ADS: the charging-handle latch rendered as a 60 px
        // MIRROR bead at L=235, the brightest object in the frame.
        // specularIntensity cannot touch it, so: F0 0.40 -> 0.27 and the
        // roughness floor 0.34 -> 0.48 (then 0.58).
        tint: [0.155, 0.155, 0.164],
        // The shiniest thing on the gun. MEASURED, twice: at [0.55, 0.055]
        // (min 0.22) and again at [0.5, 0.2] (min 0.32) the latch and the
        // takedown pin heads still rendered as mirror-chrome beads — a smooth
        // convex metal facing the viewmodel key needs a LOT of roughness
        // before its highlight stops being a specular point.
        roughness: [0.5, 0.44, 0.58],
        normal_strength: 1.0,
        detail: [12.0, 0.8, 0.3, 5.0],
        wear: [0.16, 0.45, 0.4, 0.0],
        wear_color: 0x5c6066,
        wear_material: [0.18, 1.0, 0.0, 0.6],
        grime_color: Some(0x0a0806),
        cloth: None,
        three: ThreeOverride {
            anisotropy: Some(0.12),
            ..NO_THREE
        },
    },
    // -- steel_black -------------------------------------------------------
    // Black nitrided steel — pistol slides, bolt bodies, small levers.
    //
    // A salt-bath nitride finish is a metal, but a very dark and fairly rough
    // one. Rendering a slide as plain brushed steel gives a broad flat
    // surface facing straight up at the sky and it blows out to cream — the
    // pistol ends up looking like it was carved from ivory.
    WeaponMaterial {
        key: "steel_black",
        library: "metal_brushed",
        base: BASE,
        bake: Some(BakeOverride {
            size: Some(512),
            seed: Some(829),
            relief: Some(0.004),
            tint_a: None,
            tint_b: None,
        }),
        scale: 0.07,
        // Metal, so this is F0. 0.24 -> 0.19 with the roughness floor up: a
        // nitrided slide is dark but it absolutely has a highlight running
        // down its top edge, and that highlight is the whole read.
        tint: [0.155, 0.158, 0.165],
        roughness: [0.56, 0.14, 0.36],
        normal_strength: 0.95,
        detail: [18.0, 0.7, 0.3, 5.0],
        wear: [0.24, 0.5, 0.5, 0.0],
        wear_color: 0x6a6f75,
        wear_material: [0.22, 1.0, 0.0, 0.75],
        grime_color: Some(0x0a0806),
        cloth: None,
        three: ThreeOverride {
            anisotropy: Some(0.14),
            ..NO_THREE
        },
    },
    // -- polymer -----------------------------------------------------------
    // Glass-filled polymer: magazine, stock, grip shell, handguard panels.
    WeaponMaterial {
        key: "polymer",
        library: "rubber",
        base: BASE,
        bake: Some(BakeOverride {
            size: Some(1024),
            seed: Some(149),
            relief: Some(0.009),
            tint_a: None,
            tint_b: None,
        }),
        scale: 0.055,
        // MATERIAL CLASS 2 of 3 — moulded glass-filled nylon furniture.
        // x2.7 with `alu`, keeping the 15%-darker/warmer offset that is the
        // whole polymer-vs-alloy separation cue: ~0.0075/0.0070/0.0064 linear
        // against the anodising's 0.0095/0.0101/0.0117. That pair of offsets
        // (a fifth of a stop of value, opposite hue bias) plus 0.13 more
        // roughness is what makes a polymer handguard read as a different
        // substance from the alloy receiver it is bolted to at 1080p.
        tint: [0.224, 0.211, 0.192],
        // 0.61-0.75 — semi-matte, a full 0.25 rougher than the anodising, so
        // the two catch the sky at visibly different rates as the gun sways.
        roughness: [0.63, 0.15, 0.3],
        // Glass-filled nylon has the most aggressive micro-texture on the gun
        // — a moulded stipple straight off the tool — and it is the
        // second-biggest area in frame after the receiver.
        normal_strength: 1.5,
        detail: [26.0, 1.15, 0.55, 6.0],
        wear: [0.26, 0.6, 0.5, 0.0],
        wear_color: 0x3e4145,
        wear_material: [0.46, 0.0, 0.0, 0.5],
        grime_color: Some(0x0b0a08),
        cloth: None,
        // Glass-filled nylon is a low-gloss dielectric: 0.02-0.025
        // reflectance, not glass's 0.04.
        three: ThreeOverride {
            physical: Some(true),
            specular_intensity: Some(0.13),
            ..NO_THREE
        },
    },
    // -- polymer_tan -------------------------------------------------------
    // Coyote / FDE polymer for furniture variation.
    WeaponMaterial {
        key: "polymer_tan",
        library: "rubber",
        base: BASE,
        // Seed only: size and relief stay at the library `rubber`'s 512 /
        // 0.013.
        bake: Some(BakeOverride {
            size: None,
            seed: Some(131),
            relief: None,
            tint_a: None,
            tint_b: None,
        }),
        scale: 0.08,
        // Flat dark earth: bright enough to read as a colour break against the
        // black furniture, dark enough to be paint. Only 1.6x rather than the
        // 2.7x the black polymer got — FDE is already the light material on
        // the gun and must not become the brightest thing in the frame.
        tint: [0.62, 0.498, 0.358],
        roughness: [0.63, 0.16, 0.3],
        normal_strength: 1.2,
        detail: [24.0, 1.0, 0.5, 5.0],
        wear: [0.24, 0.7, 0.5, 0.0],
        wear_color: 0x5c5340,
        wear_material: [0.44, 0.0, 0.0, 0.5],
        grime_color: Some(0x0f0c08),
        cloth: None,
        three: ThreeOverride {
            physical: Some(true),
            specular_intensity: Some(0.14),
            ..NO_THREE
        },
    },
    // -- rubber ------------------------------------------------------------
    // Soft rubber: grip overmould, butt pad, eyecup.
    WeaponMaterial {
        key: "rubber",
        library: "rubber",
        // 0.55 cavity grime rather than BASE's 0.62.
        base: BaseParams {
            weather: [0.0, 0.0, 0.0, 0.55],
            ..BASE
        },
        bake: Some(BakeOverride {
            size: None,
            seed: Some(211),
            relief: None,
            tint_a: None,
            tint_b: None,
        }),
        scale: 0.055,
        // The darkest thing on the weapon, ~0.0049 linear after the
        // recalibration. Very slightly warm rather than dead neutral —
        // moulded EPDM is never blue.
        tint: [0.147, 0.137, 0.127],
        roughness: [0.86, 0.04, 0.55],
        normal_strength: 1.35,
        // 1.2 mm pebble at this tile, at full amplitude. This material carries
        // the optic's eyepiece and objective bezels — the two annuli that face
        // the eye squarely in ADS — so its micro-relief is what keeps them
        // from reading as flat punched holes.
        detail: [14.0, 1.0, 0.55, 5.0],
        wear: [0.22, 0.8, 0.6, 0.0],
        wear_color: 0x24262a,
        wear_material: [0.72, 0.0, 0.0, 0.35],
        grime_color: Some(0x0a0908),
        cloth: None,
        // Rubber is a dielectric with ~0.02 specular reflectance, half glass's
        // 0.04, and three's specularF90 = 1.0 is what lights an edge-on
        // moulded surface like chrome. This material is the optic's rear
        // bezel, the outer circle of the whole ADS frame, so the grazing clamp
        // is not optional here — it is the reason the cream ring is gone.
        three: ThreeOverride {
            physical: Some(true),
            specular_intensity: Some(0.12),
            ..NO_THREE
        },
    },
    // -- brass -------------------------------------------------------------
    // Cartridge brass — chambered round, shells on the belt/carrier.
    //
    // No `grimeColor` override: this entry and `copper` are the only two that
    // inherit the library `metal_brushed`'s (which itself sets none, so the
    // shader default 0x2a2620 stands).
    WeaponMaterial {
        key: "brass",
        library: "metal_brushed",
        base: BASE,
        bake: None,
        scale: 0.05,
        // Metal, so this is F0. Cartridge brass really is a bright metal, but
        // a chambered round in a shadowed port was rendering as a lamp; pulled
        // back a third and roughened, which is what a fired-and-reloaded case
        // actually looks like.
        tint: [2.3, 1.58, 0.74],
        roughness: [0.55, 0.16, 0.36],
        normal_strength: 0.75,
        detail: [10.0, 0.55, 0.28, 4.0],
        wear: [0.8, 0.3, 0.3, 0.0],
        wear_color: 0xe8c98a,
        wear_material: [0.12, 1.0, 0.0, 0.8],
        grime_color: None,
        cloth: None,
        three: ThreeOverride {
            anisotropy: Some(0.05),
            ..NO_THREE
        },
    },
    // -- copper ------------------------------------------------------------
    // Copper jacket of a visible projectile tip.
    WeaponMaterial {
        key: "copper",
        library: "metal_brushed",
        base: BASE,
        bake: None,
        scale: 0.04,
        tint: [2.25, 1.4, 1.09],
        roughness: [0.6, 0.18, 0.34],
        normal_strength: 0.75,
        detail: [10.0, 0.55, 0.28, 4.0],
        wear: [0.5, 0.3, 0.3, 0.0],
        wear_color: 0xd9a271,
        wear_material: [0.2, 1.0, 0.0, 0.8],
        grime_color: None,
        cloth: None,
        three: ThreeOverride {
            anisotropy: Some(0.05),
            ..NO_THREE
        },
    },
    // -- glove -------------------------------------------------------------
    // Glove shell: warm dark nomex / goat-leather palm with a visible weave.
    //
    // THE HAND MUST NOT BE THE SAME COLOUR AS THE GUN. Measured on the r3
    // frames, the glove fingers sampled rgb(101,95,91) — 0.127 linear with
    // B-R = -10 — against a receiver at 0.121 linear, B-R = -7. Same value,
    // near enough the same hue, and the whole hand read as another anodised
    // part of the weapon. What separates a hand from a rifle is HUE, not
    // value: the gun is a cool blue-black dielectric, nomex and leather are
    // warm browns, so the shell tint went from a cool c(0.30, 0.293, 0.32) to
    // a 1.5:1 red-over-blue ratio, and the baked weave tints followed it out
    // of grey into brown.
    WeaponMaterial {
        key: "glove",
        library: "fabric",
        base: BASE,
        // Warm the baked weave as well as the tint: a cool-grey base modulated
        // by a warm tint still reads grey wherever the weave is light. This
        // overrides the library `fabric`'s own tintA 0x5a5445 / tintB 0x3a3830.
        bake: Some(BakeOverride {
            size: Some(512),
            seed: Some(401),
            relief: None,
            tint_a: Some(0x453a30),
            tint_b: Some(0x2a2320),
        }),
        scale: 0.032,
        // MEASURED WITH A LIVE UNIFORM SWEEP. The glove and the sleeve were
        // the only two surfaces on the rig that had never been through the
        // viewmodel's exposure calibration; the rig delivers roughly 20x the
        // irradiance per unit albedo that the world does, and every gun
        // material was quietly crushed to compensate while the arms were not:
        //   albedo x1     sleeve rgb(206,188,161)
        //   albedo x0.25  sleeve rgb(191,174,145)
        //   albedo x0.06  sleeve rgb(185,168,139)
        //   albedo x0     sleeve rgb(182,165,136)   <- still cream!
        // i.e. 4+ stops over, flat on the AgX shoulder, where nothing it is
        // made of can be seen. 0.30 -> 0.115 fixed the exposure but put the
        // glove 1.3 stops under the sleeve and the hand disappeared into the
        // gun; 0.19 lands the shell ~0.35 stop under the sleeve, which is the
        // interval the calibration asked for. The ratio is untouched.
        tint: [0.19, 0.155, 0.127],
        // 0.9+ is non-negotiable: a glove has no gloss lobe at all. The floor
        // stops the fabric ORM dipping into anything that could catch a
        // highlight.
        roughness: [0.92, 0.06, 0.78],
        normal_strength: 1.35,
        // ~1.5 mm weave at this scale, at full albedo/roughness amplitude.
        detail: [12.0, 0.85, 0.62, 6.0],
        // The wear and grime layers are NOT scaled by `tint`, so they have to
        // be tuned with it. Until this pass the glove geometry carried no
        // `color` attribute at all, so vColor was (0,0,0) and every one of
        // these numbers was dead (see `Arm.bakeSurfaceMasks`).
        wear: [0.34, 0.85, 0.75, 0.0],
        // Polished leather, not bare metal — the shine on a used glove is
        // where the dye has rubbed off, and that is a darker, warmer BROWN.
        wear_color: 0x2a2118,
        wear_material: [0.72, 0.0, 0.0, 0.4],
        grime_color: Some(0x080604),
        // A glove is not an awning: `fabric` ships a 0.20 sun-transmission
        // term (for canvas canopies) that the library merge was handing to the
        // hand.
        cloth: Some([0.0, 1.0, 0.0, 0.0]),
        // SHEEN IS A SPECULAR TERM AND IT IS NOT SCALED BY ALBEDO. At the
        // library's 0.55 with a cream sheenColor it was carrying the glove on
        // its own: with the albedo driven to zero the hand still rendered
        // rgb(182,165,136). 0.07 with a dark brown sheenColor is a faint bloom
        // on the high spots, which is all it should be. Leather's specular
        // reflectance is ~0.02, not glass's 0.04 — at 0.04 the back of a
        // gloved hand is a flat Fresnel sheet, which is the "robot armour"
        // read in one number.
        three: ThreeOverride {
            physical: Some(true),
            sheen: Some(0.07),
            sheen_roughness: Some(0.96),
            sheen_color: Some(0x201812),
            specular_intensity: Some(0.16),
            ..NO_THREE
        },
    },
    // -- glove_pad ---------------------------------------------------------
    // Reinforced palm / knuckle pads — rubberised, scuffed.
    WeaponMaterial {
        key: "glove_pad",
        library: "rubber",
        base: BASE,
        bake: Some(BakeOverride {
            size: None,
            seed: Some(307),
            relief: None,
            tint_a: None,
            tint_b: None,
        }),
        scale: 0.024,
        // Warm, and a stop under the shell so the pads read as a separate
        // material rather than as bolted-on plate. 0.20 -> 0.072 -> 0.118
        // lands the TPR half a stop under the glove.
        tint: [0.118, 0.095, 0.08],
        roughness: [1.0, 0.0, 0.78],
        // 1.3 -> 0.7. At 1.3 the rubber surface's own relief was deep enough
        // that every knuckle cap picked up a hard specular break across its
        // middle, and four caps each cut in half is eight facets on the back
        // of one hand: that is the "stack of slabs" read as much as the cap
        // geometry is. A moulded TPR pad is soft and slightly pebbled, not
        // machined.
        normal_strength: 0.7,
        detail: [9.0, 0.95, 0.6, 5.0],
        wear: [0.4, 0.75, 0.65, 0.0],
        wear_color: 0x241c14,
        wear_material: [0.78, 0.0, 0.0, 0.35],
        grime_color: Some(0x070504),
        cloth: None,
        // Moulded TPR is a low-reflectance elastomer, not glass. The flat 0.04
        // dielectric lobe on four knuckle caps facing the key is the "robot
        // armour" read.
        three: ThreeOverride {
            physical: Some(true),
            specular_intensity: Some(0.15),
            ..NO_THREE
        },
    },
    // -- glove_seam --------------------------------------------------------
    // Stitched seam down the outboard side of each finger.
    //
    // At 40 px across the whole hand the four fingers merge into one paddle;
    // the only thing that survives is a light line where the panels are sewn.
    // It is the same leather as the shell at a higher albedo (a seam is a
    // doubled, proud, dye-worn edge), and it is a separate material rather
    // than a vertex colour so it also picks up its own normal and roughness.
    WeaponMaterial {
        key: "glove_seam",
        library: "fabric",
        base: BASE,
        // The same weave bake as `glove` — same seed, same tints, same size —
        // so the two share one baked texture set.
        bake: Some(BakeOverride {
            size: Some(512),
            seed: Some(401),
            relief: None,
            tint_a: Some(0x453a30),
            tint_b: Some(0x2a2320),
        }),
        scale: 0.02,
        // 1.85x the recalibrated shell: at 1-3 px wide a seam needs more
        // separation than 1.4x to survive the AA filter. Same warm ratio as
        // the shell.
        tint: [0.35, 0.286, 0.234],
        roughness: [0.9, 0.06, 0.74],
        normal_strength: 1.0,
        detail: [24.0, 0.6, 0.45, 5.0],
        wear: [0.5, 0.8, 0.7, 0.0],
        wear_color: 0x3a2d20,
        wear_material: [0.7, 0.0, 0.0, 0.4],
        grime_color: Some(0x0a0806),
        cloth: Some([0.0, 1.0, 0.0, 0.0]),
        three: ThreeOverride {
            physical: Some(true),
            sheen: Some(0.08),
            sheen_roughness: Some(0.94),
            sheen_color: Some(0x2a2018),
            specular_intensity: Some(0.16),
            ..NO_THREE
        },
    },
    // -- sleeve ------------------------------------------------------------
    // Combat-shirt sleeve: coyote ripstop, dusty.
    //
    // THE SINGLE WORST SURFACE IN THE BUILD before the exposure
    // recalibration — see the long note on `glove`. The support forearm
    // crosses the lower third of every hipfire frame, and at L=191 it was the
    // brightest opaque thing on screen, 4 stops over, flat on the tone
    // curve's shoulder, and therefore completely without texture whatever its
    // maps said.
    WeaponMaterial {
        key: "sleeve",
        library: "fabric",
        base: BASE,
        bake: Some(BakeOverride {
            size: Some(512),
            seed: Some(503),
            relief: None,
            tint_a: Some(0x6e6047),
            tint_b: Some(0x4c4231),
        }),
        // 0.09 -> 0.05: a 50 mm ripstop tile. At 0.09 the base weave was
        // 2.2 px at the distance the support forearm actually sits (0.38-0.5 m)
        // and read as one flat value; the detail layer carries the thread,
        // this carries the panel-to-panel variation.
        scale: 0.05,
        // 0.42 -> 0.16, i.e. ~0.020/0.015/0.008 linear off the baked coyote
        // weave. A real sun-bleached coyote ripstop is 0.16-0.20 linear, so
        // this is the same 10x crush the whole gun carries and lands the
        // sleeve 2/3 of a stop ABOVE the glove, which is right: the shirt is a
        // lighter garment than the gloves and it should read as the warmest
        // object on the rig.
        tint: [0.16, 0.152, 0.138],
        roughness: [0.95, 0.05, 0.8],
        normal_strength: 1.45,
        // ~6 mm ripstop grid at this tile, at full amplitude on both albedo
        // and roughness. Roughness detail matters more than albedo detail
        // here: breaking the lobe up is what makes a surface look woven.
        detail: [9.0, 0.95, 0.7, 6.0],
        wear: [0.5, 0.9, 0.75, 0.0],
        // Dust on the fold crowns, not bare white canvas.
        wear_color: 0x4a4034,
        wear_material: [0.9, 0.0, 0.0, 0.45],
        grime_color: Some(0x0c0a06),
        // `fabric` is authored for market awnings and ships a 0.20
        // sun-transmission term plus an underside darkening. A sleeve is
        // opaque.
        cloth: Some([0.0, 1.0, 0.0, 0.0]),
        // Sheen 0.45 -> 0.09 and the sheenColor from cream to dark khaki. At
        // 0.45 this term alone rendered the sleeve at rgb(182,165,136) with
        // its albedo set to literally zero — it WAS the tan tube.
        // specularIntensity 0.14 is ripstop's ~0.016 reflectance rather than
        // glass's 0.04.
        three: ThreeOverride {
            physical: Some(true),
            sheen: Some(0.09),
            sheen_roughness: Some(0.96),
            sheen_color: Some(0x38301f),
            specular_intensity: Some(0.14),
            ..NO_THREE
        },
    },
];

/// An all-`None` [`ThreeOverride`], so each entry's `three` block lists
/// exactly the properties the source's own `three: { … }` literal lists and
/// nothing else. `Default::default()` is not usable in a `const` initialiser.
const NO_THREE: ThreeOverride = ThreeOverride {
    physical: None,
    metalness: None,
    specular_intensity: None,
    anisotropy: None,
    sheen: None,
    sheen_roughness: None,
    sheen_color: None,
};

/// `MATERIAL_KEYS = Object.keys(WEAPON_MATERIALS)` (`materials.js:1215`), in
/// declaration order.
pub fn material_keys() -> Vec<&'static str> {
    WEAPON_MATERIALS.iter().map(|m| m.key).collect()
}

/// `WEAPON_MATERIALS[key]`.
pub fn weapon_material(key: &str) -> Option<&'static WeaponMaterial> {
    WEAPON_MATERIALS.iter().find(|m| m.key == key)
}

// ---------------------------------------------------------------------------
// The library merge
// ---------------------------------------------------------------------------

/// `MaterialSystem._resolve` (`src/materials/index.js:106-115`): a name that
/// is a library key resolves to itself, otherwise through the alias table,
/// otherwise falls back to `concrete` with a warning.
///
/// No weapon entry needs the alias hop or the fallback — every `library` field
/// is already a canonical key — but the resolution the source performs is
/// reproduced rather than assumed, because the alias table contains
/// `steel -> metal_brushed` and `rubber` is a real key, which is exactly the
/// pair a shortcut here would get wrong.
pub fn library_entry(name: &str) -> &'static LibraryEntry {
    LIBRARY
        .iter()
        .find(|e| e.name == name)
        .or_else(|| {
            ALIASES
                .iter()
                .find(|(alias, _)| *alias == name)
                .and_then(|(_, target)| LIBRARY.iter().find(|e| e.name == *target))
        })
        .unwrap_or_else(|| {
            LIBRARY
                .iter()
                .find(|e| e.name == "concrete")
                .expect("LIBRARY always contains concrete")
        })
}

/// `{ ...LIBRARY[lib].bake, ...opts.bake }` (`src/materials/index.js:129`).
///
/// This is the bake merge **before** `MaterialSystem._size()`'s quality
/// quantisation (`index.js:130`), which is that system's own concern and at
/// quality 1 is the identity on 512 and 1024 anyway.
///
/// Narrows the overrides' `f64` to [`BakeParams`]'s `f32`, which is the width
/// [`crate::materials::bake::BakeDef`] consumes and therefore the width these
/// numbers reach the generator at. Every override is a short decimal literal,
/// so the narrowing is the same rounding the GPU upload performs.
pub fn resolved_bake(m: &WeaponMaterial) -> BakeParams {
    let lib = library_entry(m.library).bake;
    let o = m.bake.unwrap_or_default();
    BakeParams {
        size: o.size.unwrap_or(lib.size),
        world_size: lib.world_size,
        relief: o.relief.map_or(lib.relief, |r| r as f32),
        seed: o.seed.unwrap_or(lib.seed),
        param: lib.param,
        tint_a: o.tint_a.or(lib.tint_a),
        tint_b: o.tint_b.or(lib.tint_b),
    }
}

/// `{ ...LIBRARY[lib].three, ...opts.three }` (`src/materials/index.js:193`),
/// narrowed to the eight properties this merge can produce for a weapon entry.
///
/// The library halves that matter: `metal_brushed` contributes
/// `physical: true` and `anisotropyRotation: 0` to all five metal entries even
/// though none of them says so, and `fabric` contributes `physical: true` to
/// the three cloth entries (whose own `sheen*` values overwrite the library's
/// 0.55 / 0.85 / 0x8a8272 completely).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MergedThree {
    pub physical: Option<bool>,
    pub metalness: Option<f64>,
    pub specular_intensity: Option<f64>,
    pub anisotropy: Option<f64>,
    pub anisotropy_rotation: Option<f64>,
    pub sheen: Option<f64>,
    pub sheen_roughness: Option<f64>,
    pub sheen_color: Option<u32>,
}

impl MergedThree {
    /// `const usePhysical = threeProps.physical === true`
    /// (`src/materials/index.js:194`) — selects `MeshPhysicalMaterial` over
    /// `MeshStandardMaterial`. Strict `=== true`, so an absent key is false.
    pub fn use_physical(&self) -> bool {
        self.physical == Some(true)
    }
}

/// See [`MergedThree`].
pub fn resolved_three(m: &WeaponMaterial) -> MergedThree {
    let lib = library_entry(m.library).three;
    let o = m.three;
    MergedThree {
        physical: o.physical.or_else(|| lib.and_then(|l| l.physical)),
        metalness: o.metalness,
        specular_intensity: o
            .specular_intensity
            .or_else(|| lib.and_then(|l| l.specular_intensity).map(f64::from)),
        anisotropy: o
            .anisotropy
            .or_else(|| lib.and_then(|l| l.anisotropy).map(f64::from)),
        // No weapon entry sets this; it arrives from `metal_brushed` alone.
        anisotropy_rotation: lib.and_then(|l| l.anisotropy_rotation).map(f64::from),
        sheen: o.sheen.or_else(|| lib.and_then(|l| l.sheen).map(f64::from)),
        sheen_roughness: o
            .sheen_roughness
            .or_else(|| lib.and_then(|l| l.sheen_roughness).map(f64::from)),
        sheen_color: o.sheen_color.or_else(|| lib.and_then(|l| l.sheen_color)),
    }
}

// ---------------------------------------------------------------------------
// Colour
// ---------------------------------------------------------------------------

/// Three's `SRGBToLinear` (`three/src/math/ColorManagement.js`), applied per
/// channel by `new THREE.Color(hex)` — the decode every hex colour in this
/// file goes through before it reaches a uniform.
///
/// **This is deliberately not [`crate::materials::noise::ow_srgb`].** That
/// function transcribes the library's *GLSL* `owSRGB`
/// (`(c + 0.055) / 1.055`, raised to 2.4, and `c / 12.92` below the knee).
/// Three writes the same transform pre-multiplied:
/// `c * 0.9478672986 + 0.0521327014` and `c * 0.0773993808`. Algebraically
/// identical, numerically not — **float arithmetic is not associative**, and
/// 254 of the 256 byte values differ, by up to 1.08e-11. `tests/
/// weapons_materials/golden.json` captures three's decode of all 256, so a
/// port that reached for the GLSL version would fail rather than drift.
///
/// The branch is `<`, matching three (`owSRGB`'s is `>`); the two disagree
/// only at exactly `c == 0.04045`, which no `n / 255` ever hits.
pub use crate::materials::three_color::srgb_to_linear;

/// `new THREE.Color(hex)` — unpack an sRGB hex triplet and decode each channel
/// through [`srgb_to_linear`].
pub fn hex_to_linear(hex: u32) -> [f64; 3] {
    [
        srgb_to_linear(f64::from((hex >> 16) & 0xff) / 255.0),
        srgb_to_linear(f64::from((hex >> 8) & 0xff) / 255.0),
        srgb_to_linear(f64::from(hex & 0xff) / 255.0),
    ]
}

/// `THREE.Color.multiplyScalar` — used by [`lens_ring`] and [`reticle`], both
/// of which push a colour above 1 on purpose (they are additive and unlit).
fn multiply_scalar(c: [f64; 3], s: f64) -> [f64; 3] {
    [c[0] * s, c[1] * s, c[2] * s]
}

// ---------------------------------------------------------------------------
// The materials `WeaponMaterials` owns outright
// ---------------------------------------------------------------------------

/// `THREE.FrontSide` / `BackSide` / `DoubleSide` — 0 / 1 / 2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Front = 0,
    Back = 1,
    Double = 2,
}

/// `THREE.NormalBlending` / `AdditiveBlending` — 1 / 2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blending {
    Normal = 1,
    Additive = 2,
}

/// Which THREE material class the description constructs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaterialKind {
    Basic,
    Standard,
    Physical,
}

/// One of the seven materials `WeaponMaterials` constructs itself, as data.
///
/// A field is `Some` exactly where the source's constructor literal sets it;
/// everything else is left to three's own defaults and is not restated here.
/// `color`/`specular_color`/`sheen_color` are stored **already decoded to
/// linear**, because that is what the constructor produces and what the
/// renderer uploads.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CustomMaterial {
    /// `m.name`.
    pub name: &'static str,
    pub kind: MaterialKind,
    /// The cache key the source stores this instance under.
    pub cache_key: CacheKey,
    /// Linear RGB.
    pub color: [f64; 3],
    pub transparent: Option<bool>,
    pub opacity: Option<f64>,
    pub blending: Option<Blending>,
    pub depth_write: Option<bool>,
    pub depth_test: Option<bool>,
    pub side: Option<Side>,
    pub tone_mapped: Option<bool>,
    pub fog: Option<bool>,
    pub roughness: Option<f64>,
    pub metalness: Option<f64>,
    pub specular_intensity: Option<f64>,
    /// Linear RGB.
    pub specular_color: Option<[f64; 3]>,
    pub env_map_intensity: Option<f64>,
    /// The value the material ends up carrying. See [`glass`] for why this is
    /// not always the number the source's `ior:` line says.
    pub ior: Option<f64>,
    pub reflectivity: Option<f64>,
    pub iridescence: Option<f64>,
    pub iridescence_ior: Option<f64>,
    pub iridescence_thickness_range: Option<[f64; 2]>,
    pub sheen: Option<f64>,
    /// Linear RGB.
    pub sheen_color: Option<[f64; 3]>,
    pub sheen_roughness: Option<f64>,
    pub premultiplied_alpha: Option<bool>,
    /// `alphaMap: this._rimRamp()`.
    pub alpha_map_is_rim_ramp: bool,
}

/// A `this.cache` key. The source builds these with template literals, so a
/// number is formatted the way JS formats it — notably `${0x3b6e8c}` is the
/// **decimal** `3894924`, not `3b6e8c`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CacheKey {
    /// A bare literal: `'cavity'`, `'optic_tube'`.
    Literal(&'static str),
    /// `` `glass:${tint}` ``.
    Glass(u32),
    /// `` `lensRing:${intensity}` ``.
    LensRing(f64),
    /// `` `vignette:${strength}` ``.
    Vignette(f64),
    /// `` `reticleOutline:${opacity}` ``.
    ReticleOutline(f64),
    /// `` `reticle:${color}:${intensity}` ``.
    Reticle(u32, f64),
}

impl CacheKey {
    /// Render the key exactly as the JS template literal does.
    pub fn to_key(self) -> String {
        match self {
            CacheKey::Literal(s) => s.to_string(),
            CacheKey::Glass(tint) => format!("glass:{tint}"),
            CacheKey::LensRing(i) => format!("lensRing:{}", js_number(i)),
            CacheKey::Vignette(s) => format!("vignette:{}", js_number(s)),
            CacheKey::ReticleOutline(o) => format!("reticleOutline:{}", js_number(o)),
            CacheKey::Reticle(c, i) => format!("reticle:{c}:{}", js_number(i)),
        }
    }
}

/// JS `String(n)` for the finite non-negative values these keys carry: an
/// integral value prints without a decimal point (`1`, not `1.0`), everything
/// else prints its shortest round-tripping form, which is what Rust's default
/// `f64` Display already produces (`0.14`, `6.5`).
fn js_number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e21 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

/// An all-`None` [`CustomMaterial`] skeleton the seven constructors fill in,
/// so each one lists exactly the properties its source literal lists.
const CUSTOM_NONE: CustomMaterial = CustomMaterial {
    name: "",
    kind: MaterialKind::Basic,
    cache_key: CacheKey::Literal(""),
    color: [0.0, 0.0, 0.0],
    transparent: None,
    opacity: None,
    blending: None,
    depth_write: None,
    depth_test: None,
    side: None,
    tone_mapped: None,
    fog: None,
    roughness: None,
    metalness: None,
    specular_intensity: None,
    specular_color: None,
    env_map_intensity: None,
    ior: None,
    reflectivity: None,
    iridescence: None,
    iridescence_ior: None,
    iridescence_thickness_range: None,
    sheen: None,
    sheen_color: None,
    sheen_roughness: None,
    premultiplied_alpha: None,
    alpha_map_is_rim_ramp: false,
};

/// `cavity()` (`materials.js:1178-1202`) — matte black interior: bores, lens
/// housings, ejection port cavity, every engraved rollmark stroke.
///
/// Truly black and truly matte. Anything with a specular lobe left in it
/// catches the sky from inside the optic tube and paints a bright crescent
/// across the bottom of the sight picture — and `MeshStandardMaterial` has no
/// way to say "no specular lobe", because it hard-codes F0 = 0.04 and
/// specularF90 = 1.0. `MeshPhysicalMaterial` with `specularIntensity` 0.04 is
/// the same black with the Fresnel taken out.
pub fn cavity() -> CustomMaterial {
    CustomMaterial {
        name: "ow-cavity",
        kind: MaterialKind::Physical,
        cache_key: CacheKey::Literal("cavity"),
        color: hex_to_linear(0x0a0c0e),
        roughness: Some(1.0),
        metalness: Some(0.0),
        specular_intensity: Some(0.04),
        env_map_intensity: Some(0.18),
        side: Some(Side::Double),
        ..CUSTOM_NONE
    }
}

/// `opticTube()` (`materials.js:942-963`) — the inside of the optic tube: a
/// LIGHT TRAP, not a cavity.
///
/// [`cavity`] is 0x0a0c0e — effectively zero — which has nothing for the fill
/// or the bounce off the objective to land on; measured in ads.png the tube
/// interior sampled rgb(27,36,53), carrying nothing but a flat blue env term,
/// so the objective read as "a flat grey gradient disc". A real
/// anodised/flocked tube bore is 0.018-0.025 linear: black, but a black you
/// can see a gradient across. Roughness 0.9 and a hard specular clamp keep it
/// from throwing the cream ring around the front lip.
///
/// The source's comment says "0x272a2c is 0.0205 linear — the middle of the
/// band"; the colour it actually sets is **0x1d2023** (0.0123-0.0168 linear).
/// The comment is stale, not the code — transcribed as written.
pub fn optic_tube() -> CustomMaterial {
    CustomMaterial {
        name: "ow-optic-tube",
        kind: MaterialKind::Physical,
        cache_key: CacheKey::Literal("optic_tube"),
        color: hex_to_linear(0x1d2023),
        roughness: Some(0.9),
        metalness: Some(0.0),
        specular_intensity: Some(0.12),
        env_map_intensity: Some(0.3),
        side: Some(Side::Double),
        ..CUSTOM_NONE
    }
}

/// `lensRing(intensity = 0.14)` (`materials.js:974-992`) — the bright
/// inner-edge reflection ring just inside the objective rim.
///
/// Looking into a real coated objective the one unmistakable cue is a thin,
/// very bright arc where the inside of the bezel is reflected in the glass. It
/// is a specular feature of the lens, so it does not belong on the bezel
/// geometry (which is what produced the fat cream ring) — it is its own 0.4 mm
/// ring, unlit and additive, sitting on the glass.
pub fn lens_ring(intensity: f64) -> CustomMaterial {
    CustomMaterial {
        name: "ow-lens-ring",
        kind: MaterialKind::Basic,
        cache_key: CacheKey::LensRing(intensity),
        color: multiply_scalar(hex_to_linear(0x9fc4d8), intensity),
        transparent: Some(true),
        opacity: Some(0.5),
        blending: Some(Blending::Additive),
        depth_write: Some(false),
        side: Some(Side::Double),
        tone_mapped: Some(true),
        fog: Some(false),
        ..CUSTOM_NONE
    }
}

/// `LENS_RING_INTENSITY` — `lensRing`'s default argument.
pub const LENS_RING_INTENSITY: f64 = 0.14;

/// `glass(tint = 0x3b6e8c)` (`materials.js:1007-1065`) — optic glass, an
/// AR-coated dielectric, not a smoked window.
///
/// A broadband AR stack leaves a residual reflection whose hue swings with
/// angle: green at normal incidence through violet to magenta by ~70 degrees.
/// That swing is the single cue that says "there is glass in the tube", and it
/// is Fresnel-driven, so it is built from two terms that peak at opposite ends
/// of the angle range — `specularColor` tints F0 (normal incidence, green) and
/// `sheen` is a grazing lobe (magenta) — with three's `iridescence` (a real
/// thin film, a 5-layer MgF2/TiO2 stack at 220-560 nm) filling the transition.
///
/// # Two source defects, both ported as they behave
///
/// 1. **`tint` is dead.** The parameter reaches the cache key
///    (`` `glass:${tint}` ``) and nothing else; the material's colour is the
///    literal 0x121c22 whatever is passed. Calling `glass(0xff0000)` therefore
///    allocates a second, byte-identical material under a second cache key.
///    Kept, because the judgement that a value is dead can be wrong and
///    preserving it costs nothing.
/// 2. **`ior: 1.52` is dead too, and this one changes a number.** Three
///    defines `reflectivity` as a property whose setter *writes* `ior`:
///    `ior = (1 + 0.4 * reflectivity) / (1 - 0.4 * reflectivity)`
///    (`MeshPhysicalMaterial.js:146-157`). `Material.setValues` applies the
///    constructor literal's keys in order, and `reflectivity: 0.55` comes
///    **after** `ior: 1.52`, so the shipped material's index of refraction is
///    `(1 + 0.22) / (1 - 0.22)` = 1.5641…, not 1.52. The authored 1.52 is
///    recorded in [`GLASS_AUTHORED_IOR`] and goes nowhere, exactly as in the
///    source.
pub fn glass(tint: u32) -> CustomMaterial {
    CustomMaterial {
        name: "ow-optic-glass",
        kind: MaterialKind::Physical,
        cache_key: CacheKey::Glass(tint),
        // Not `tint`. See the doc above.
        color: hex_to_linear(0x121c22),
        transparent: Some(true),
        // Opacity is the *absorption*, so it has to stay low: at 0.3 the sight
        // reads as a smoked lens and the world behind it goes muddy.
        opacity: Some(0.1),
        // 0.03: inside the 0.02-0.04 band. Below 0.02 the reflection collapses
        // to a single pixel-sized sun spot and the lens reads as a hole again.
        roughness: Some(0.03),
        metalness: Some(0.0),
        // The literal's `ior: 1.52`, overwritten by `reflectivity: 0.55` two
        // lines later. Grouping transcribed literally — float arithmetic is
        // not associative.
        ior: Some((1.0 + 0.4 * 0.55) / (1.0 - 0.4 * 0.55)),
        reflectivity: Some(0.55),
        specular_intensity: Some(1.0),
        // GREEN at normal incidence — the residual an AR stack cannot cancel.
        specular_color: Some(hex_to_linear(0x59c489)),
        iridescence: Some(1.0),
        iridescence_ior: Some(1.4),
        iridescence_thickness_range: Some([220.0, 560.0]),
        // MAGENTA at grazing. 0.85 / roughness 0.08 -> 0.42 / 0.30: MEASURED
        // in the ADS frame, a tight magenta rim lobe on a curved element,
        // sampled against an 8-bit framebuffer with the composite's grain on
        // top, resolved as a field of violet chroma speckle across the whole
        // optic — read as compression artefacts rather than as a coating.
        // Halving the amplitude and quadrupling the lobe width keeps the hue
        // swing and takes the noise out of it.
        sheen: Some(0.42),
        sheen_color: Some(hex_to_linear(0xa856b8)),
        sheen_roughness: Some(0.3),
        env_map_intensity: Some(2.4),
        side: Some(Side::Double),
        depth_write: Some(false),
        premultiplied_alpha: Some(true),
        ..CUSTOM_NONE
    }
}

/// `glass`'s default argument. Dead beyond the cache key — see [`glass`].
pub const GLASS_TINT: u32 = 0x3b6e8c;

/// The `ior: 1.52` the source's [`glass`] literal writes, immediately
/// clobbered by its `reflectivity: 0.55`. Recorded so the defect is visible
/// rather than silently dropped; nothing reads it.
pub const GLASS_AUTHORED_IOR: f64 = 1.52;

/// `lensVignette(strength = 0.34)` (`materials.js:1110-1128`) — an unlit dark
/// disc that sits just behind the ocular lens, transparent in the middle and
/// opaque-ish at the rim. `strength` is the peak darkening at the very edge of
/// the aperture.
pub fn lens_vignette(strength: f64) -> CustomMaterial {
    CustomMaterial {
        name: "ow-lens-vignette",
        kind: MaterialKind::Basic,
        cache_key: CacheKey::Vignette(strength),
        color: hex_to_linear(0x05070a),
        transparent: Some(true),
        opacity: Some(strength),
        alpha_map_is_rim_ramp: true,
        depth_write: Some(false),
        side: Some(Side::Double),
        tone_mapped: Some(true),
        fog: Some(false),
        ..CUSTOM_NONE
    }
}

/// `lensVignette`'s default argument.
pub const LENS_VIGNETTE_STRENGTH: f64 = 0.34;

/// `reticleOutline(opacity = 0.8)` (`materials.js:1135-1153`) — the reticle's
/// dark outline.
///
/// Additive blending cannot draw anything darker than the background, so the
/// 0.5 px keyline that keeps a 2 px dot legible against a blown-out sky has to
/// be a separate normally-blended ring.
pub fn reticle_outline(opacity: f64) -> CustomMaterial {
    CustomMaterial {
        name: "ow-reticle-outline",
        kind: MaterialKind::Basic,
        cache_key: CacheKey::ReticleOutline(opacity),
        color: hex_to_linear(0x14060a),
        transparent: Some(true),
        opacity: Some(opacity),
        depth_write: Some(false),
        depth_test: Some(true),
        side: Some(Side::Double),
        tone_mapped: Some(false),
        fog: Some(false),
        ..CUSTOM_NONE
    }
}

/// `reticleOutline`'s default argument.
pub const RETICLE_OUTLINE_OPACITY: f64 = 0.8;

/// `reticle(color = 0xff2a12, intensity = 6.5)` (`materials.js:1156-1175`) —
/// additive, unlit, depth-tested.
pub fn reticle(color: u32, intensity: f64) -> CustomMaterial {
    CustomMaterial {
        name: "ow-reticle",
        kind: MaterialKind::Basic,
        cache_key: CacheKey::Reticle(color, intensity),
        color: multiply_scalar(hex_to_linear(color), intensity),
        transparent: Some(true),
        opacity: Some(1.0),
        blending: Some(Blending::Additive),
        depth_write: Some(false),
        depth_test: Some(true),
        side: Some(Side::Double),
        tone_mapped: Some(true),
        fog: Some(false),
        ..CUSTOM_NONE
    }
}

/// `reticle`'s default arguments.
pub const RETICLE_COLOR: u32 = 0xff2a12;
/// See [`RETICLE_COLOR`].
pub const RETICLE_INTENSITY: f64 = 6.5;

// ---------------------------------------------------------------------------
// `_rimRamp`
// ---------------------------------------------------------------------------

/// `_rimRamp`'s `const N = 64` (`materials.js:1077`).
pub const RIM_RAMP_N: usize = 64;

/// `_rimRamp()` (`materials.js:1075-1103`) — a radial alpha ramp, 1 at the rim
/// and 0 in the middle, as a 64x64 RGBA `Uint8Array`.
///
/// Used by the tube vignette and the eye-relief ring: a real sight darkens
/// 6-8% toward the edge of the exit pupil because the field stop and the tube
/// wall eat the outer rays, and that soft darkening is a large part of why
/// looking through glass looks different from looking through a hole.
///
/// Two things here are the algorithm, not incidental:
///
/// * **`Math.hypot(u, v)`, not `sqrt(u*u + v*v)`.** Hypot scales by the larger
///   magnitude first and rounds differently. `f64::hypot` is the direct
///   analogue and is what this uses.
/// * **The `Uint8Array`.** Every alpha is `round(a * 255)` and read back as a
///   byte; the eight-bit quantisation is part of the texture, so the port
///   produces the bytes, not the floats.
///
/// Row-major, `y` outer and `x` inner, RGBA interleaved — the source's own
/// write order. The RGB channels are a constant 255; only alpha ramps.
/// Texture state (`RGBAFormat`, `LinearFilter` min and mag,
/// `ClampToEdgeWrapping` on both axes, `generateMipmaps = false`) lives in
/// [`RIM_RAMP_TEXTURE`].
pub fn rim_ramp() -> Vec<u8> {
    let n = RIM_RAMP_N;
    let nf = n as f64;
    let mut data = vec![0u8; n * n * 4];
    for y in 0..n {
        for x in 0..n {
            let u = (x as f64 + 0.5) / nf - 0.5;
            let v = (y as f64 + 0.5) / nf - 0.5;
            let r = f64::min(1.0, u.hypot(v) * 2.0);
            // flat centre, then a smooth ramp over the outer third of the
            // aperture
            let t = f64::max(0.0, (r - 0.8) / 0.2);
            let a = t * t * (3.0 - 2.0 * t);
            let i = (y * n + x) * 4;
            data[i] = 255;
            data[i + 1] = 255;
            data[i + 2] = 255;
            // JS `Math.round` is round-half-UP (toward +infinity), not Rust's
            // round-half-away-from-zero. `a` is in [0, 1] so the two agree
            // here, but the source's operator is the one transcribed.
            data[i + 3] = (a * 255.0 + 0.5).floor() as u8;
        }
    }
    data
}

/// The `THREE.DataTexture` state `_rimRamp` sets (`materials.js:1094-1099`),
/// as the raw THREE constants the source names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RimRampTexture {
    /// `THREE.RGBAFormat`.
    pub format: u32,
    /// `THREE.LinearFilter`.
    pub min_filter: u32,
    /// `THREE.LinearFilter`.
    pub mag_filter: u32,
    /// `THREE.ClampToEdgeWrapping`.
    pub wrap_s: u32,
    /// `THREE.ClampToEdgeWrapping`.
    pub wrap_t: u32,
    pub generate_mipmaps: bool,
    pub width: u32,
    pub height: u32,
}

/// See [`rim_ramp`].
pub const RIM_RAMP_TEXTURE: RimRampTexture = RimRampTexture {
    format: 1023,
    min_filter: 1006,
    mag_filter: 1006,
    wrap_s: 1001,
    wrap_t: 1001,
    generate_mipmaps: false,
    width: RIM_RAMP_N as u32,
    height: RIM_RAMP_N as u32,
};

// ---------------------------------------------------------------------------
// `WeaponMaterials.get`
// ---------------------------------------------------------------------------

/// `_fallback(key)` (`materials.js:913-926`) — used only when the materials
/// subsystem is unavailable (the standalone harness).
///
/// Two source quirks, both faithful: `steel_soot` is **not** in the metal
/// list, so a sooted brake falls back to the dielectric grey; and `copper`
/// **is**, but the colour ternary only special-cases `brass`, so copper falls
/// back to steel's 0x3a3d42 rather than to anything copper-coloured.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FallbackMaterial {
    pub color: u32,
    pub roughness: f64,
    pub metalness: f64,
}

/// See [`FallbackMaterial`].
pub fn fallback(key: &str) -> FallbackMaterial {
    let metal = key == "steel"
        || key == "steel_bright"
        || key == "steel_black"
        || key == "brass"
        || key == "copper";
    FallbackMaterial {
        color: if key == "brass" {
            0xb08d3a
        } else if metal {
            0x3a3d42
        } else {
            0x2a2b2e
        },
        roughness: if metal { 0.38 } else { 0.72 },
        metalness: if metal { 1.0 } else { 0.0 },
    }
}

/// What `WeaponMaterials.get(key)` resolves to (`materials.js:883-910`).
#[derive(Debug, Clone, PartialEq)]
pub enum MaterialRequest {
    /// One of the five keys handled before anything else, returning a
    /// material this subsystem owns and disposes.
    Custom(CustomMaterial),
    /// `this.lib.get(def[0], def[1])`, then two properties applied to the
    /// returned instance.
    Library {
        entry: &'static WeaponMaterial,
        /// `m.shadowSide = THREE.FrontSide` — the viewmodel is drawn with its
        /// own near plane; nothing about it should write into the world's
        /// shadow cascades.
        shadow_side: Side,
        /// `m.envMapIntensity = ENV_OCCLUSION`. Without it the gun samples the
        /// full bright sky IBL while the street around it is in shade, which
        /// is the single most obvious "sticker pasted on the frame" tell. The
        /// opts are unique to this subsystem, so the library instance being
        /// tuned here is ours alone.
        env_map_intensity: f64,
    },
    /// No table entry, or no materials subsystem at all.
    Fallback(FallbackMaterial),
}

/// `WeaponMaterials.get(key)` (`materials.js:883-910`).
///
/// The five special keys are tested **before** the `WEAPON_MATERIALS` lookup
/// and before the cache, so a table entry could never shadow one; that check
/// order is the contract, and it is why `get('glass')` returns the optic glass
/// rather than the library's `glass` surface.
///
/// `has_library` is `!!this.lib` — the `ctx.peek('materials')` the constructor
/// captured. When it is false every table key falls back too, not just the
/// unknown ones.
pub fn material_request(key: &str, has_library: bool) -> MaterialRequest {
    match key {
        "cavity" => return MaterialRequest::Custom(cavity()),
        "optic_tube" => return MaterialRequest::Custom(optic_tube()),
        "glass" => return MaterialRequest::Custom(glass(GLASS_TINT)),
        "lens_ring" => return MaterialRequest::Custom(lens_ring(LENS_RING_INTENSITY)),
        "lens_vig" => return MaterialRequest::Custom(lens_vignette(LENS_VIGNETTE_STRENGTH)),
        _ => {}
    }
    match weapon_material(key) {
        Some(entry) if has_library => MaterialRequest::Library {
            entry,
            shadow_side: Side::Front,
            env_map_intensity: ENV_OCCLUSION,
        },
        _ => MaterialRequest::Fallback(fallback(key)),
    }
}
