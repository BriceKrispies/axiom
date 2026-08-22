//! The runtime material shader's parameter block — **authored data**.
//!
//! Ported from Claude-of-Duty `src/materials/shader.js`'s `DEFAULT_PARAMS`
//! (source lines 697-780) and the `extendMaterial` uniform wiring below it.
//!
//! The source hands its shader ~30 loose uniforms. Axiom's surface calling
//! convention already has a home for exactly this: `SurfaceParams` is
//! `array<vec4<f32>, 32>`, written once at the preparation barrier. So the port
//! is a **packing**, not a new binding — 128 floats available, 19 slots used.
//!
//! ## Why this lives in a layer and not in the GPU backend
//!
//! It started in `axiom-gpu-backend`, next to the WGSL that reads it, and that
//! was the wrong home: **an app authors these values**, and authored data does
//! not belong in a module. It is the same split [`crate::Surface`] already
//! makes — the surface is a layer type, the WGSL generated from it is the
//! module's business — and putting the parameters anywhere else would force
//! either an app to depend on a backend or the host contract to name a module's
//! type.
//!
//! The WGSL that reads this block stays in `axiom-gpu-backend`. Only the
//! authored values and their packing live here.
//!
//! ## The slot map is a contract
//!
//! Every layer in this module reads `params.slots[N]` by index, so the map below
//! *is* the interface between twelve independently-written files. It is pinned
//! by [`tests::the_slot_map_is_pinned_index_for_index`]: a slot that moves
//! silently re-reads someone else's parameter, which is the same failure mode as
//! "an enum used as a table index is order-dependent" — a trap this port has
//! already been bitten by once, when consolidating two enums silently reindexed
//! every per-surface audio recipe.
//!
//! | slot | x | y | z | w |
//! |---|---|---|---|---|
//! | 0 | `uv_mode` | `local_space` | `scale` | `parallax` |
//! | 1 | `offset.x` | `offset.y` | `parallax_fade.near` | `parallax_fade.far` |
//! | 2 | `parallax_layers` | `detail_world` | `macro_relief` | `detile` |
//! | 3 | `detail[0..4]` — tiles-per-base-tile, normal, albedo, fade metres |
//! | 4 | `macro[0..4]` — world scale, albedo, roughness, hue |
//! | 5 | `macro_big[0..4]` — contrast, big amplitude, big world scale, unused |
//! | 6 | `patch[0..4]` — coverage, cell metres, albedo delta, roughness delta |
//! | 7 | `cloth[0..4]` — transmission, underside multiplier, fold, unused |
//! | 8 | `weather[0..4]` — dust, rain streaks, splash height, cavity grime |
//! | 9 | `wear[0..4]` — wear, grime, extra AO, unused |
//! | 10 | `wear_material[0..4]` — roughness, metalness, unused, tint amount |
//! | 11 | `roughness.scale` | `roughness.offset` | `roughness.minimum` | `ao_strength` |
//! | 12 | `normal_strength` | `ground_y` | `alpha_mask` | `vertex_masks` |
//! | 13 | `no_grad` | — | — | — |
//! | 14 | `tint` (linear rgb) | — |
//! | 15 | `wear_color` (linear rgb) | — |
//! | 16 | `dust_color` (linear rgb) | — |
//! | 17 | `grime_color` (linear rgb) | — |
//! | 18 | `rust_color` (linear rgb) | — |
//!
//! Slots 19-31 are unused and written as zero, so a shader that reads one gets a
//! defined value rather than whatever the previous material left behind.
//!
//! ## Colours are decoded with **three's** curve, not the GLSL one
//!
//! Every hex here reaches the source's shader through `new THREE.Color(hex)`,
//! which is three's `SRGBToLinear`. That is algebraically equal to the GLSL
//! `owSRGB` form the surface *generators* use, and numerically different —
//! because three writes the transform pre-multiplied and float arithmetic is
//! not associative.
//!
//! Measured over all 256 byte values, and the answer is sharper than "they
//! differ":
//!
//! | how the curve is computed | values differing | worst gap |
//! |---|---|---|
//! | f64 throughout | 254 / 256 | 1.08e-11 |
//! | f64, then narrowed to the f32 uniform | **0 / 256** | **0** |
//! | natively in f32 | 175 / 256 | 1.79e-7 |
//!
//! So at the resolution this parameter block actually transports, **the choice
//! of curve is unobservable — provided the curve is evaluated in f64 first**,
//! which is what the source does and what [`srgb_to_linear`] therefore does. The
//! third row is the trap: an f32-native transcription *introduces* a
//! disagreement the source does not have, on 175 of 256 inputs. This module's
//! first draft did exactly that, and the number above is what caught it.
//!
//! The distinction still earns its documentation, because the difference is very
//! much observable one layer up: the *bake* path computes in f64 and keeps f64,
//! which is row one.
//!
//! This is not hypothetical. The app side of this port shipped a function
//! documented as `new THREE.Color(hex)` whose body called the GLSL decode, with
//! a unit test asserting it matched the GLSL decode — so the test pinned the bug
//! rather than catching it. It was found only when a third slice, porting a file
//! where every colour goes through `THREE.Color`, captured all 256 decodes and
//! reported the mismatch in a neighbour. See [`srgb_to_linear`].

/// Three's `SRGBToLinear` (`three/src/math/ColorManagement.js`), per channel.
///
/// Written branchlessly because this is the spine: the knee is a blend of both
/// arms rather than an `if`. Evaluating the unused arm is safe — `c` arrives in
/// `[0, 1]`, so `c * 0.9478672986 + 0.0521327014` is always positive and `powf`
/// never sees a negative base.
///
/// The knee comparison is `<`, matching three. The GLSL `owSRGB` uses `>`; the
/// two disagree only at exactly `c == 0.04045`, which no `n / 255` produces.
pub fn srgb_to_linear(c: f64) -> f32 {
    // **f64 in, narrowed once on the way out** — what the source does. JavaScript
    // numbers are f64, `new THREE.Color(hex)` decodes in f64, and only the
    // finished value reaches an f32 uniform.
    //
    // The parameter is `f64` rather than `f32` on purpose, and it is the second
    // precision slip this function had: taking an `f32` would force the caller's
    // `n / 255` division into f32 too, and `(n as f32) / 255.0` widened is not
    // `f64::from(n) / 255.0`. Making the *input* f64 puts the whole chain in the
    // source's precision and leaves exactly one narrowing, here.
    let below = f64::from(u8::from(c < 0.04045));
    let low = c * 0.0773993808;
    let high = (c * 0.9478672986 + 0.0521327014).powf(2.4);
    (below * low + (1.0 - below) * high) as f32
}

/// `new THREE.Color(hex)` — unpack an sRGB hex triplet and decode each channel.
pub fn hex_to_linear(hex: u32) -> [f32; 3] {
    // `/ 255.0` in f64, matching `THREE.Color.setHex`. In f32 this division
    // alone moves the result on most byte values.
    [
        srgb_to_linear(f64::from((hex >> 16) & 0xff) / 255.0),
        srgb_to_linear(f64::from((hex >> 8) & 0xff) / 255.0),
        srgb_to_linear(f64::from(hex & 0xff) / 255.0),
    ]
}

/// How the shader builds its texture coordinate. `DEFAULT_PARAMS.uvMode`.
///
/// The discriminants are the wire values the WGSL compares against, so their
/// **order is part of the contract** and is pinned by a test. This is the shape
/// that silently reindexed every per-surface audio recipe elsewhere in this
/// port when two enums with different orders were merged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UvMode {
    /// Project on the world's dominant axis. The source's default.
    #[default]
    Planar,
    /// Three-axis projection, blended by the normal.
    Triplanar,
    /// The mesh's own interpolated parameterisation.
    Mesh,
}

impl UvMode {
    /// The value the WGSL compares against.
    pub fn wire(self) -> f32 {
        [0.0, 1.0, 2.0][self as usize]
    }
}

/// `DEFAULT_PARAMS`, field for field.
///
/// Field names and units are the source's, so the port can be diffed against it
/// by eye. Where the source's comment records *why* a default is what it is,
/// that comment is carried over rather than summarised — two of them document
/// real bugs and their fixes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MaterialParams {
    pub uv_mode: UvMode,
    /// Project in the object's local space instead of world space.
    pub local_space: bool,
    /// Metres per texture tile.
    pub scale: f32,
    pub offset: [f32; 2],
    /// Parallax depth in metres; 0 disables.
    pub parallax: f32,
    pub parallax_fade: [f32; 2],
    pub parallax_layers: f32,
    /// Tiles-per-base-tile, normal strength, albedo strength, fade metres.
    pub detail: [f32; 4],
    /// Metres the shared detail tile spans in the world.
    ///
    /// The source's comment on this is worth keeping whole: `detail[0]` is
    /// expressed *per base tile*, which silently tied the micro layer's world
    /// scale to the macro layer's. A prop-scale variant (`scale` 0.55 m) with
    /// `detail[0] = 10` mapped the 0.25 m detail bake into 55 mm — every 1.6 mm
    /// grain became 0.35 mm, under one pixel at 0.5 m, so the whole micro layer
    /// filtered away and every prop read as flat colour up close. Measurably:
    /// "cranking detail[2] from 0.42 to 2.5 on the market stall changed the
    /// frame by nothing at all." So `detail[0]` is DERIVED from `scale` unless
    /// this is 0. 0.26 m matches the bake's authored `worldSize` of 0.25 m.
    pub detail_world: f32,
    /// World scale, albedo strength, roughness strength, hue strength.
    pub macro_: [f32; 4],
    /// `[contrast, bigAmplitude, bigWorldScale, unused]`. `1/bigWorldScale` is
    /// the period in metres and the coarsest band is a third of that, so 0.028
    /// gives ~12 m features.
    pub macro_big: [f32; 4],
    /// `[coverage 0..1, cell metres, albedo delta, roughness delta]`. 0
    /// coverage disables the layer.
    pub patch: [f32; 4],
    /// `[transmission 0..1, underside albedo multiplier, fold amount, unused]`.
    /// Transmission 0 and multiplier 1 disable the whole cloth layer.
    pub cloth: [f32; 4],
    /// Macro-gradient normal tilt on up-facing surfaces (ruts / drifts).
    pub macro_relief: f32,
    /// De-tiling second-sample blend amount; 0 disables the extra fetches.
    pub detile: f32,
    /// Dust, rain streaks, ground-splash height, cavity grime.
    pub weather: [f32; 4],
    pub ground_y: f32,
    /// Vertex-colour masks: wear, grime, extra AO, unused.
    pub wear: [f32; 4],
    /// `[roughness, METALNESS, unused, tint amount]` where the wear mask is 1.
    ///
    /// The metalness default is **0**, and that is a fix, not an oversight: it
    /// used to be 0.5, so every worn edge on concrete, plaster, brick, timber,
    /// hessian and the road turned half metal and picked up a specular tint it
    /// has no business having. Only the metal library entries, which set their
    /// own `wearMaterial`, should ever raise it.
    pub wear_material: [f32; 4],
    pub wear_color: u32,
    pub dust_color: u32,
    pub grime_color: u32,
    pub rust_color: u32,
    pub tint: u32,
    pub normal_strength: f32,
    /// `[scale, offset, minimum]`.
    pub roughness: [f32; 3],
    pub ao_strength: f32,
    pub alpha_mask: bool,
    pub vertex_masks: bool,
    pub no_grad: bool,
}

impl Default for MaterialParams {
    /// `DEFAULT_PARAMS`, value for value.
    fn default() -> Self {
        MaterialParams {
            uv_mode: UvMode::Planar,
            local_space: false,
            scale: 2.0,
            offset: [0.0, 0.0],
            parallax: 0.0,
            parallax_fade: [6.0, 14.0],
            parallax_layers: 22.0,
            detail: [11.0, 0.55, 0.35, 16.0],
            detail_world: 0.26,
            macro_: [0.045, 0.35, 0.1, 0.35],
            macro_big: [1.0, 0.0, 0.03, 0.0],
            patch: [0.0, 2.6, 0.12, -0.08],
            cloth: [0.0, 1.0, 0.0, 0.0],
            macro_relief: 0.0,
            detile: 0.0,
            weather: [0.35, 0.3, 0.55, 0.4],
            ground_y: 0.0,
            wear: [0.5, 0.7, 0.5, 0.0],
            wear_material: [0.42, 0.0, 0.0, 0.5],
            wear_color: 0x008d_8b86 & 0x00ff_ffff,
            dust_color: 0x6b_6154,
            grime_color: 0x2a_2620,
            rust_color: 0x6d_3a1c,
            tint: 0xff_ffff,
            normal_strength: 1.0,
            roughness: [1.0, 0.0, 0.06],
            ao_strength: 1.0,
            alpha_mask: false,
            vertex_masks: false,
            no_grad: false,
        }
    }
}

/// The number of `vec4` slots `SurfaceParams` provides.
pub const SLOT_COUNT: usize = 32;

/// The number this packing actually writes; the rest are zeroed.
pub const SLOTS_USED: usize = 19;

impl MaterialParams {
    /// The source's `#ifdef OW_DETILE` condition:
    /// `p.detile > 0 && p.uvMode !== 'triplanar'` (`extendMaterial:851`).
    ///
    /// This is a **compile-time** decision in the source and it stays one here:
    /// when it is false the de-tiling block must not be emitted at all, because
    /// driving the height blend with `t = 0` is *not* bit-identical to omitting
    /// it — measured at 1 ULP on 17.2% of operands.
    ///
    /// It lives on the parameters rather than in the GPU backend because
    /// [`crate::SurfaceKind::code`] needs it: de-tiled and un-de-tiled are two
    /// different programs, so the gate is part of a surface's *identity*, and
    /// identity is a layer concern. The backend's `detile_enabled` defers here,
    /// so there is exactly one definition of the rule.
    pub fn detile_enabled(&self) -> bool {
        (self.detile > 0.0) & !matches!(self.uv_mode, UvMode::Triplanar)
    }

    /// Pack into the `array<vec4<f32>, 32>` the surface calling convention
    /// already provides. See the module doc's slot map — it is the contract
    /// every layer reads against.
    pub fn pack(&self) -> [[f32; 4]; SLOT_COUNT] {
        let mut slots = [[0.0_f32; 4]; SLOT_COUNT];
        let tint = hex_to_linear(self.tint);
        let wear_color = hex_to_linear(self.wear_color);
        let dust = hex_to_linear(self.dust_color);
        let grime = hex_to_linear(self.grime_color);
        let rust = hex_to_linear(self.rust_color);
        slots[0] = [
            self.uv_mode.wire(),
            f32::from(u8::from(self.local_space)),
            self.scale,
            self.parallax,
        ];
        slots[1] = [
            self.offset[0],
            self.offset[1],
            self.parallax_fade[0],
            self.parallax_fade[1],
        ];
        slots[2] = [
            self.parallax_layers,
            self.detail_world,
            self.macro_relief,
            self.detile,
        ];
        slots[3] = self.detail;
        slots[4] = self.macro_;
        slots[5] = self.macro_big;
        slots[6] = self.patch;
        slots[7] = self.cloth;
        slots[8] = self.weather;
        slots[9] = self.wear;
        slots[10] = self.wear_material;
        slots[11] = [
            self.roughness[0],
            self.roughness[1],
            self.roughness[2],
            self.ao_strength,
        ];
        slots[12] = [
            self.normal_strength,
            self.ground_y,
            f32::from(u8::from(self.alpha_mask)),
            f32::from(u8::from(self.vertex_masks)),
        ];
        slots[13] = [f32::from(u8::from(self.no_grad)), 0.0, 0.0, 0.0];
        slots[14] = [tint[0], tint[1], tint[2], 0.0];
        slots[15] = [wear_color[0], wear_color[1], wear_color[2], 0.0];
        slots[16] = [dust[0], dust[1], dust[2], 0.0];
        slots[17] = [grime[0], grime[1], grime[2], 0.0];
        slots[18] = [rust[0], rust[1], rust[2], 0.0];
        slots
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Three's decode is not the GLSL one, and the difference is large enough to
    /// matter. This is the assertion that keeps the module doc's claim honest —
    /// the app side of this port shipped the wrong decode behind a comment
    /// saying it was the right one.
    /// The curve is three's, evaluated in f64 and narrowed once — which is what
    /// the source does. Both halves of that sentence are load-bearing, so both
    /// are asserted.
    ///
    /// At f32-uniform resolution the two curves are indistinguishable, so a test
    /// asserting "they differ" would be asserting something this module cannot
    /// observe. What it *can* observe, and what actually went wrong in the first
    /// draft, is the precision: computing the curve natively in f32 disagrees
    /// with the f64-then-narrow answer on 175 of 256 byte values.
    #[test]
    fn the_curve_is_evaluated_in_f64_and_narrowed_exactly_once() {
        // Reference: three's decode in f64, narrowed at the end.
        let three_f64_narrowed = |n: u32| -> f32 {
            let c = f64::from(n) / 255.0;
            let v = [c * 0.0773993808, (c * 0.9478672986 + 0.0521327014).powf(2.4)]
                [usize::from(c >= 0.04045)];
            v as f32
        };
        // The trap: the same curve evaluated natively in f32.
        let three_f32_native = |n: u32| -> f32 {
            let c = (n as f32) / 255.0;
            [c * 0.0773993808, (c * 0.9478672986 + 0.0521327014).powf(2.4)]
                [usize::from(c >= 0.04045)]
        };

        let wrong_precision = (0u32..=255)
            .filter(|&n| srgb_to_linear(f64::from(n) / 255.0).to_bits() != three_f64_narrowed(n).to_bits())
            .count();
        assert_eq!(
            wrong_precision, 0,
            "srgb_to_linear must equal three's f64 decode narrowed once",
        );

        let precision_matters = (0u32..=255)
            .filter(|&n| three_f64_narrowed(n).to_bits() != three_f32_native(n).to_bits())
            .count();
        assert!(
            precision_matters > 128,
            "only {precision_matters} of 256 values distinguish an f64-then-narrow              decode from an f32-native one (expected ~175). If that has collapsed,              the precision note in the module doc is no longer earning its place.",
        );
    }

    #[test]
    fn the_srgb_decode_maps_the_endpoints_exactly() {
        assert_eq!(srgb_to_linear(0.0), 0.0);
        assert!((srgb_to_linear(1.0) - 1.0).abs() < 1e-6);
        // Below the knee is the linear arm.
        assert!((srgb_to_linear(0.02) - (0.02 * 0.0773993808_f64) as f32).abs() < 1e-12);
    }

    #[test]
    fn hex_unpacks_the_channels_in_rgb_order() {
        let c = hex_to_linear(0xff_0000);
        assert!(c[0] > 0.99 && c[1] == 0.0 && c[2] == 0.0);
        let c = hex_to_linear(0x00_00ff);
        assert!(c[0] == 0.0 && c[1] == 0.0 && c[2] > 0.99);
    }

    /// The discriminants are wire values the WGSL compares against; reordering
    /// this enum silently re-reads a different projection.
    #[test]
    fn the_uv_mode_wire_values_are_pinned() {
        assert_eq!(UvMode::Planar.wire(), 0.0);
        assert_eq!(UvMode::Triplanar.wire(), 1.0);
        assert_eq!(UvMode::Mesh.wire(), 2.0);
        assert_eq!(UvMode::default(), UvMode::Planar);
    }

    /// The slot map is the interface between twelve independently-written
    /// layers. A slot that moves silently re-reads someone else's parameter.
    #[test]
    fn the_slot_map_is_pinned_index_for_index() {
        let p = MaterialParams::default();
        let s = p.pack();
        assert_eq!(s[0], [0.0, 0.0, 2.0, 0.0], "uv_mode, local_space, scale, parallax");
        assert_eq!(s[1], [0.0, 0.0, 6.0, 14.0], "offset, parallax_fade");
        assert_eq!(s[2], [22.0, 0.26, 0.0, 0.0], "layers, detail_world, relief, detile");
        assert_eq!(s[3], [11.0, 0.55, 0.35, 16.0], "detail");
        assert_eq!(s[4], [0.045, 0.35, 0.1, 0.35], "macro");
        assert_eq!(s[5], [1.0, 0.0, 0.03, 0.0], "macro_big");
        assert_eq!(s[6], [0.0, 2.6, 0.12, -0.08], "patch");
        assert_eq!(s[7], [0.0, 1.0, 0.0, 0.0], "cloth");
        assert_eq!(s[8], [0.35, 0.3, 0.55, 0.4], "weather");
        assert_eq!(s[9], [0.5, 0.7, 0.5, 0.0], "wear");
        assert_eq!(s[10], [0.42, 0.0, 0.0, 0.5], "wear_material");
        assert_eq!(s[11], [1.0, 0.0, 0.06, 1.0], "roughness, ao_strength");
        assert_eq!(s[12], [1.0, 0.0, 0.0, 0.0], "normal_strength, ground_y, flags");
        assert_eq!(s[13], [0.0, 0.0, 0.0, 0.0], "no_grad");
        // `tint` is white, so its linear decode is 1 on every channel.
        assert!((s[14][0] - 1.0).abs() < 1e-6 && (s[14][2] - 1.0).abs() < 1e-6);
    }

    /// The metalness default is a documented fix. If it drifts back to 0.5,
    /// every worn edge in the game turns half metal again.
    #[test]
    fn de_tiling_is_off_by_default_and_never_runs_under_triplanar() {
        assert!(!MaterialParams::default().detile_enabled());
        assert!(MaterialParams { detile: 0.01, ..MaterialParams::default() }.detile_enabled());
        // The source's gate is an AND: triplanar excludes de-tiling outright.
        assert!(!MaterialParams {
            detile: 1.0,
            uv_mode: UvMode::Triplanar,
            ..MaterialParams::default()
        }
        .detile_enabled());
        // Exactly zero is off — `> 0`, not `>= 0`.
        assert!(!MaterialParams { detile: 0.0, ..MaterialParams::default() }.detile_enabled());
    }

    #[test]
    fn the_wear_metalness_default_is_zero_not_half() {
        assert_eq!(MaterialParams::default().wear_material[1], 0.0);
    }

    #[test]
    fn every_unused_slot_is_written_as_zero() {
        let s = MaterialParams::default().pack();
        let nonzero = s[SLOTS_USED..]
            .iter()
            .filter(|slot| slot.iter().any(|v| *v != 0.0))
            .count();
        assert_eq!(nonzero, 0, "a shader reading an unused slot must see a defined value");
    }

    #[test]
    fn the_booleans_pack_as_one_and_zero() {
        let p = MaterialParams {
            local_space: true,
            alpha_mask: true,
            vertex_masks: true,
            no_grad: true,
            ..MaterialParams::default()
        };
        let s = p.pack();
        assert_eq!(s[0][1], 1.0);
        assert_eq!(s[12][2], 1.0);
        assert_eq!(s[12][3], 1.0);
        assert_eq!(s[13][0], 1.0);
    }

    #[test]
    fn a_non_default_uv_mode_reaches_the_wire() {
        let p = MaterialParams { uv_mode: UvMode::Triplanar, ..MaterialParams::default() };
        assert_eq!(p.pack()[0][0], 1.0);
        let p = MaterialParams { uv_mode: UvMode::Mesh, ..MaterialParams::default() };
        assert_eq!(p.pack()[0][0], 2.0);
    }

    #[test]
    fn the_five_colours_land_in_their_own_slots() {
        let p = MaterialParams {
            tint: 0xff_0000,
            wear_color: 0x00_ff00,
            dust_color: 0x00_00ff,
            grime_color: 0xff_0000,
            rust_color: 0x00_ff00,
            ..MaterialParams::default()
        };
        let s = p.pack();
        assert!(s[14][0] > 0.99, "tint red");
        assert!(s[15][1] > 0.99, "wear green");
        assert!(s[16][2] > 0.99, "dust blue");
        assert!(s[17][0] > 0.99, "grime red");
        assert!(s[18][1] > 0.99, "rust green");
    }
}
