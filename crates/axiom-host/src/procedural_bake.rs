//! A procedural texture bake, as a backend-neutral request and answer.
//!
//! A **bake** is a fragment program evaluated once per texel of a square tile
//! into render targets — the way a browser engine makes textures without
//! shipping any. It is the write-side counterpart of [`MaterialTexture`]: that
//! type carries the *pixels* an app already has, this one carries the *program*
//! that would produce them, so the pixels can be made on the device they are
//! going to be sampled on and never cross the CPU boundary at all.
//!
//! ## Why this is a host type and not a backend type
//!
//! For exactly the reason [`MaterialTexture`] is: it is the one place `axiom`,
//! `axiom-windowing`, `axiom-gpu-backend` and any future backend can all name a
//! type. A bake request has to travel from an app, through the engine facade,
//! to whichever backend holds a device — and the Module Law forbids a module
//! from publishing a second type beside its facade, so a request shaped in
//! `axiom-gpu-backend` would be unnameable by everyone who needs to send one.
//! The alternative — twelve positional arguments and a bare three-tuple back —
//! is the shape [`MaterialTexture`]'s own doc rejects for the same maps.
//!
//! ## The three outputs, and why they are one program
//!
//! The contract is the browser one: a single `owSurface(uv)` writes five values
//! — albedo, height, roughness, metalness, occlusion — and the bake runs it
//! once per output, selecting which of the five reach the colour attachment
//! with [`BakeOutput`]:
//!
//! ```text
//! albedo.rgb = base colour        albedo.a = height (or an alpha-test mask)
//! orm.r      = occlusion / cavity  orm.g   = roughness   orm.b = metalness
//! normal.rgb = tangent-space, OpenGL +Y — a Sobel over the height field
//! ```
//!
//! Three outputs rather than three programs because the noise stack behind them
//! is the expensive part and it is identical for all three; the source this
//! contract is drawn from (`Claude-of-Duty` `src/materials/generator.js`) bakes
//! a full 1K set in one framebuffer bind and four full-screen draws.
//!
//! ## Storage width is part of the algorithm
//!
//! [`ProceduralBakeRequest::linear_albedo`] is not a preference. A map that is
//! *colour* is written to an sRGB-encoding target and bound as sRGB, so the two
//! encodes cancel; a map that is *data* (a normal, a packed variation field) is
//! written and bound linear. Getting that pair out of step is the failure mode
//! where "a baked tile reads darker than the same graph rendered live", and the
//! reason the flag lives on the request rather than being inferred is that only
//! the author knows which kind a map is — the same argument
//! [`TextureSampling`] makes about filtering.

/// Which of the fragment program's five outputs a bake pass writes.
///
/// The discriminants are the wire values a backend compares against, and are
/// **order-dependent**: they are the integers the fragment program branches on,
/// so reordering this enum silently reassigns every output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BakeOutput {
    /// `vec4(h, h, h, 1)` — the scratch height field the normal pass
    /// differentiates. Never a bound texture; it exists only to feed the Sobel.
    #[default]
    Height = 0,
    /// `vec4(albedo, h)` — colour with the height in alpha.
    Albedo = 1,
    /// `vec4(occlusion, roughness, metalness, 1)`.
    Orm = 2,
}

impl BakeOutput {
    /// The integer a fragment program selects this output with.
    pub const fn code(self) -> i32 {
        self as i32
    }

    /// Every output, in wire order. The order is the contract; a test that
    /// walks this cannot drift from the discriminants above.
    pub const ALL: [BakeOutput; 3] = [BakeOutput::Height, BakeOutput::Albedo, BakeOutput::Orm];
}

/// One texture set to bake: the program, its uniforms, and which maps to keep.
///
/// `key` names the bake for diagnostics and for a backend's program cache; it
/// is not interpreted. `surface_wgsl` is a fragment-program fragment defining
/// `owSurface`, and the backend supplies the uniform block and entry point
/// around it — which is why the request carries source text and not a compiled
/// anything: a compiled program is a device resource and this type must be
/// nameable on a machine with no device.
#[derive(Debug, Clone, PartialEq)]
pub struct ProceduralBakeRequest {
    key: String,
    surface_wgsl: String,
    size: u32,
    seed: f32,
    tint_a: [f32; 3],
    tint_b: [f32; 3],
    param: [f32; 4],
    world_size: f32,
    relief: f32,
    linear_albedo: bool,
    want_orm: bool,
    want_normal: bool,
}

impl ProceduralBakeRequest {
    /// A bake of `surface_wgsl` over a `size` x `size` tile, with every optional
    /// input at the value a backend would use if it were absent: seed `0`, white
    /// tints, zero params, a two-metre tile, two-centimetre relief, an sRGB
    /// albedo, and all three maps wanted.
    ///
    /// Those defaults are the *source* contract's defaults, not invented ones,
    /// and the builders below are the deltas an author actually authors — which
    /// keeps a nineteen-surface bake list from restating eleven fields that
    /// nobody varies.
    pub fn new(key: String, surface_wgsl: String, size: u32) -> Self {
        ProceduralBakeRequest {
            key,
            surface_wgsl,
            size,
            seed: 0.0,
            tint_a: [1.0, 1.0, 1.0],
            tint_b: [1.0, 1.0, 1.0],
            param: [0.0; 4],
            world_size: 2.0,
            relief: 0.02,
            linear_albedo: false,
            want_orm: true,
            want_normal: true,
        }
    }

    /// This bake with an explicit noise seed.
    #[must_use]
    pub fn with_seed(mut self, seed: f32) -> Self {
        self.seed = seed;
        self
    }

    /// This bake's two authored tints, **linear**. A generator that takes no
    /// tint leaves them white, which is what a multiply by them costs nothing.
    #[must_use]
    pub fn with_tints(mut self, tint_a: [f32; 3], tint_b: [f32; 3]) -> Self {
        self.tint_a = tint_a;
        self.tint_b = tint_b;
        self
    }

    /// This bake's four free parameters — the per-generator knobs.
    #[must_use]
    pub fn with_param(mut self, param: [f32; 4]) -> Self {
        self.param = param;
        self
    }

    /// The tile's physical size in metres and its peak-to-trough relief, also in
    /// metres. Their **ratio** is the slope the normal pass converts a per-texel
    /// height difference into, which is what makes the normal map physically
    /// consistent with the mapping scale the surface is later drawn at.
    #[must_use]
    pub fn with_scale(mut self, world_size: f32, relief: f32) -> Self {
        self.world_size = world_size;
        self.relief = relief;
        self
    }

    /// Skip the sRGB encode on the albedo target: this map is data, not colour.
    /// See the module doc.
    #[must_use]
    pub fn with_linear_albedo(mut self, linear_albedo: bool) -> Self {
        self.linear_albedo = linear_albedo;
        self
    }

    /// Which of the two optional maps to produce. Dropping one drops its pass;
    /// dropping the normal also drops the scratch height pass that only exists
    /// to feed it.
    #[must_use]
    pub fn with_maps(mut self, want_orm: bool, want_normal: bool) -> Self {
        self.want_orm = want_orm;
        self.want_normal = want_normal;
        self
    }

    /// The bake's name — a diagnostic label and a program-cache key.
    pub fn key(&self) -> &str {
        &self.key
    }

    /// The fragment-program fragment defining `owSurface`.
    pub fn surface_wgsl(&self) -> &str {
        &self.surface_wgsl
    }

    /// The tile's edge length in texels.
    pub const fn size(&self) -> u32 {
        self.size
    }

    /// The noise seed.
    pub const fn seed(&self) -> f32 {
        self.seed
    }

    /// The first authored tint, linear.
    pub const fn tint_a(&self) -> [f32; 3] {
        self.tint_a
    }

    /// The second authored tint, linear.
    pub const fn tint_b(&self) -> [f32; 3] {
        self.tint_b
    }

    /// The four free parameters.
    pub const fn param(&self) -> [f32; 4] {
        self.param
    }

    /// Metres the tile spans.
    pub const fn world_size(&self) -> f32 {
        self.world_size
    }

    /// Peak-to-trough relief, in metres.
    pub const fn relief(&self) -> f32 {
        self.relief
    }

    /// Whether the albedo target skips the sRGB encode.
    pub const fn linear_albedo(&self) -> bool {
        self.linear_albedo
    }

    /// Whether the ORM map is wanted.
    pub const fn want_orm(&self) -> bool {
        self.want_orm
    }

    /// Whether the tangent-space normal map is wanted.
    pub const fn want_normal(&self) -> bool {
        self.want_normal
    }

    /// How many render passes this request costs: one per wanted map, plus the
    /// scratch height pass the normal needs.
    ///
    /// The one derived fact on this type, and it is here rather than at a
    /// backend because it is a property of the *request* — a bake budget is
    /// something an app can compute before it has a device.
    pub const fn pass_count(&self) -> u32 {
        1 + (self.want_orm as u32) + 2 * (self.want_normal as u32)
    }
}

/// What a device produces for one [`ProceduralBakeRequest`]: RGBA8 texels,
/// row-major, `size * size * 4` bytes a map.
///
/// Row 0 is the `v ≈ 0` row, matching [`MapPixels`]' convention, so a map can
/// be moved into a [`MaterialTexture`] without a flip.
///
/// [`MapPixels`]: crate::MapPixels
/// [`MaterialTexture`]: crate::MaterialTexture
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProceduralBakeMaps {
    albedo: Vec<u8>,
    orm: Option<Vec<u8>>,
    normal: Option<Vec<u8>>,
    size: u32,
}

impl ProceduralBakeMaps {
    /// The maps a bake produced. `orm` and `normal` are `None` exactly when the
    /// request did not want them.
    pub const fn new(
        size: u32,
        albedo: Vec<u8>,
        orm: Option<Vec<u8>>,
        normal: Option<Vec<u8>>,
    ) -> Self {
        ProceduralBakeMaps {
            albedo,
            orm,
            normal,
            size,
        }
    }

    /// The tile's edge length in texels.
    pub const fn size(&self) -> u32 {
        self.size
    }

    /// Colour in RGB, height in alpha.
    pub fn albedo(&self) -> &[u8] {
        &self.albedo
    }

    /// `(occlusion, roughness, metalness, 1)`, linear.
    pub fn orm(&self) -> Option<&[u8]> {
        self.orm.as_deref()
    }

    /// Tangent-space normal, `* 0.5 + 0.5`, linear.
    pub fn normal(&self) -> Option<&[u8]> {
        self.normal.as_deref()
    }

    /// How many bytes one map of this size occupies.
    pub const fn map_bytes(size: u32) -> usize {
        (size as usize) * (size as usize) * 4
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_output_codes_are_the_wire_values() {
        assert_eq!(BakeOutput::Height.code(), 0);
        assert_eq!(BakeOutput::Albedo.code(), 1);
        assert_eq!(BakeOutput::Orm.code(), 2);
        assert_eq!(BakeOutput::default(), BakeOutput::Height);
        assert_eq!(
            BakeOutput::ALL.map(BakeOutput::code),
            [0, 1, 2],
            "ALL must walk the outputs in wire order"
        );
    }

    #[test]
    fn a_new_request_carries_the_contracts_own_defaults() {
        let request = ProceduralBakeRequest::new("gravel".to_string(), "…".to_string(), 1024);
        assert_eq!(request.key(), "gravel");
        assert_eq!(request.surface_wgsl(), "…");
        assert_eq!(request.size(), 1024);
        assert_eq!(request.seed(), 0.0);
        assert_eq!(request.tint_a(), [1.0, 1.0, 1.0]);
        assert_eq!(request.tint_b(), [1.0, 1.0, 1.0]);
        assert_eq!(request.param(), [0.0; 4]);
        assert_eq!(request.world_size(), 2.0);
        assert_eq!(request.relief(), 0.02);
        assert!(!request.linear_albedo(), "a colour map is the default");
        assert!(request.want_orm());
        assert!(request.want_normal());
    }

    #[test]
    fn every_builder_moves_exactly_its_own_field() {
        let base = ProceduralBakeRequest::new("k".to_string(), "s".to_string(), 8);
        let tuned = base
            .clone()
            .with_seed(7.5)
            .with_tints([0.1, 0.2, 0.3], [0.4, 0.5, 0.6])
            .with_param([1.0, 2.0, 3.0, 4.0])
            .with_scale(0.25, 0.0034)
            .with_linear_albedo(true)
            .with_maps(false, true);
        assert_eq!(tuned.seed(), 7.5);
        assert_eq!(tuned.tint_a(), [0.1, 0.2, 0.3]);
        assert_eq!(tuned.tint_b(), [0.4, 0.5, 0.6]);
        assert_eq!(tuned.param(), [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(tuned.world_size(), 0.25);
        assert_eq!(tuned.relief(), 0.0034);
        assert!(tuned.linear_albedo());
        assert!(!tuned.want_orm());
        assert!(tuned.want_normal());
        // Nothing a builder was not asked about moved.
        assert_eq!(tuned.key(), base.key());
        assert_eq!(tuned.surface_wgsl(), base.surface_wgsl());
        assert_eq!(tuned.size(), base.size());
        assert_ne!(tuned, base);
    }

    #[test]
    fn a_pass_costs_one_draw_and_a_normal_costs_two() {
        let base = ProceduralBakeRequest::new("k".to_string(), "s".to_string(), 8);
        assert_eq!(
            base.pass_count(),
            4,
            "albedo + ORM + the scratch height + the Sobel"
        );
        assert_eq!(base.clone().with_maps(true, false).pass_count(), 2);
        assert_eq!(
            base.clone().with_maps(false, true).pass_count(),
            3,
            "dropping ORM keeps the height pass, which only the normal needs"
        );
        assert_eq!(base.with_maps(false, false).pass_count(), 1);
    }

    #[test]
    fn the_maps_report_what_the_request_asked_for() {
        let full = ProceduralBakeMaps::new(2, vec![1; 16], Some(vec![2; 16]), Some(vec![3; 16]));
        assert_eq!(full.size(), 2);
        assert_eq!(full.albedo(), &[1; 16]);
        assert_eq!(full.orm(), Some(&[2; 16][..]));
        assert_eq!(full.normal(), Some(&[3; 16][..]));

        let albedo_only = ProceduralBakeMaps::new(2, vec![1; 16], None, None);
        assert_eq!(albedo_only.orm(), None);
        assert_eq!(albedo_only.normal(), None);
        assert_ne!(albedo_only, full);
    }

    /// A bake failure is diagnosed from a log line, so every one of these types
    /// has to be printable — and a derived `Debug` is only *reached* by a test
    /// that formats it, because `assert_eq!` formats nothing when it passes.
    #[test]
    fn every_type_prints_what_it_is() {
        let request = ProceduralBakeRequest::new("gravel".to_string(), "src".to_string(), 512);
        let printed = format!("{request:?}");
        assert!(
            printed.contains("gravel") && printed.contains("512"),
            "a request must name itself and its size: {printed}"
        );
        let maps = ProceduralBakeMaps::new(1, vec![0; 4], None, None);
        assert!(format!("{maps:?}").contains("size: 1"));
        assert_eq!(format!("{:?}", BakeOutput::Orm), "Orm");
    }

    #[test]
    fn a_map_is_four_bytes_a_texel() {
        assert_eq!(ProceduralBakeMaps::map_bytes(1), 4);
        assert_eq!(ProceduralBakeMaps::map_bytes(1024), 1024 * 1024 * 4);
        assert_eq!(
            ProceduralBakeMaps::new(4, vec![0; 64], None, None).albedo().len(),
            ProceduralBakeMaps::map_bytes(4)
        );
    }
}
