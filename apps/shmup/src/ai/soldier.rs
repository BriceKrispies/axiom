//! Ported from Claude-of-Duty `src/ai/soldier.js:1-837`.
//!
//! AI — soldier assembly. Turns the part library into a finished, skinned,
//! material-grouped character. One geometry per visual variant, shared by
//! every instance of that variant; only the skeleton is per-instance.
//!
//! # What this module owns, and what it does not
//!
//! `soldier.js` builds **no geometry of its own**. Every triangle comes out of
//! [`crate::ai::parts`] (the part builders), [`crate::ai::geo`] (the lofting
//! toolkit and the `CharacterBuilder` that welds parts into one skinned
//! buffer) and [`crate::ai::weapon`] (the carried rifle). What lives here is
//! the **recipe**: which part builder is called, with which arguments, in
//! which order, bound to which bones, painted with which grime/dirt/dust/wear
//! budget — plus the occlusion proxy cage that drives the baked vertex AO, the
//! variant table, and the material-slot resolution.
//!
//! # Divergences from the source, and why
//!
//! * **`resolveMaterials` returns data, not materials.** In the source it
//!   returns live `THREE.MeshStandardMaterial` objects built by
//!   `SoldierMaterials.get()`. Here it returns [`MaterialRequest`] values —
//!   the exact set name, cache key, tint, roughness, metalness, normal scale,
//!   ao and detail-tile record the source passes to `get()`. Turning a request
//!   into a GPU material is the render tier's job, and keeping the resolution
//!   as pure data is what lets it be golden-tested at all. The
//!   `AiSystem.prewarmMaterials()` contract the source's split exists to serve
//!   (resolve every material a variant will ever ask for *without building a
//!   single triangle*, because geometry construction draws from the shared RNG
//!   stream) is preserved exactly: [`resolve_materials`] touches no RNG.
//!
//! * **The occlusion proxy cage is a function, not 28 inline statements.**
//!   `buildSoldier` calls `B.occlude(...)` 28 times in a row before it adds any
//!   part. [`occlusion_proxies`] returns that same list, in that same order,
//!   and `build_soldier` feeds it to the builder unchanged. It is a pure
//!   function of the rig, so lifting it out makes it directly checkable
//!   against the golden without a `CharacterBuilder`.
//!
//! * **RNG draws are bound before the struct literal.** JavaScript evaluates
//!   object properties in source order, so `{ ..., y: 1.236 + rng.range(...),
//!   ..., rz: rng.range(...) }` draws `y` first and `rz` second. Rust struct
//!   literal field evaluation order is also source order, but the fields are
//!   written in a different order than the source's, so both draws are bound
//!   to locals first, in the source's order. **Draw order is the contract.**
//!
//! # Source quirks preserved
//!
//! * `GRIP_L` is imported by `soldier.js` and never used. There is no way to
//!   spell an unused import in Rust without a warning, so it is not imported —
//!   noted here instead of silently dropped.
//! * `P.faceWrap`, `P.helmet` and `P.plateCarrier` are each handed the variant
//!   record `V`, and `parts.js` reads nothing out of it — a dead argument in
//!   all three. See the call sites.
//! * `nPouch` is 3 or 2 and never 1, but the source still writes
//!   `nPouch === 1 ? 0.5 : i / (nPouch - 1)`. Ported with the dead arm intact.
//! * `BORE_DIR` is already unit length when `rig.js` exports it, and
//!   `buildSoldier` normalises it again. Re-normalising a unit vector is not a
//!   no-op in floating point, so it is done again here — see [`normalize3`].

use crate::ai::geo::{CharacterBuilder, Mesh, Noise, PartOptions};
use crate::ai::parts::{self as p, HeadOpts, JacketOpts, LimbOpts, PouchOpts};
use crate::ai::rig::{BORE_DIR, GRIP_R, RIG};
use crate::ai::textures::CLOTH_TILE;
use crate::ai::weapon::{build_weapon, Weapon, WeaponStyle};
use crate::rng::Rng;

/// Metres of surface per texture tile. `cloth` is deliberately large: it is
/// the tile that has to carry the 0.2-0.4 m camo macro blotches, and the
/// 1.5 mm weave it can no longer resolve is supplied by the shader detail
/// layer instead.
///
/// A slice of pairs rather than a map: the order is the source's, and
/// `CharacterBuilder` only ever looks a name up.
pub const MATERIALS: [(&str, f64); 9] = [
    ("cloth", CLOTH_TILE),
    ("plate", 0.42),
    ("gear", 0.26),
    // Boots and gloves share the cordura bake with the pouches but NOT its
    // roughness: leather-and-rubber footwear is markedly smoother than
    // webbing, and having the whole kit sit at one gloss is half of why the
    // figure reads as one extruded blob. Own material name -> own geometry
    // group -> own roughness.
    ("boot", 0.26),
    ("skin", 0.20),
    ("polymer", 0.15),
    ("steel", 0.18),
    ("rubber", 0.11),
    ("glass", 1.0),
];

/// Tile size for one material name. Panics on an unknown name, matching
/// `MATERIALS[matName].tile` throwing on `undefined`.
fn material_tile(name: &str) -> f64 {
    MATERIALS
        .iter()
        .find(|(n, _)| *n == name)
        .unwrap_or_else(|| panic!("[ai] unknown material set \"{name}\""))
        .1
}

/// Roughness multiplier per material set, applied on top of the baked
/// roughness map so the *relative* variation the bake carries is preserved.
///
/// ```text
///   cloth 0.85   matte ripstop, map averages 0.905
///   plate 0.55   laminate over foam, map averages 0.62
///   boot  0.70   waxed leather / rubber, cordura map averages 0.79
/// ```
///
/// These three are the values the silhouette needs: at 25 m the only thing
/// that separates a plate carrier from the jacket under it is the width of its
/// specular lobe.
mod rough {
    pub const CLOTH: f64 = 0.85 / 0.905;
    pub const PLATE: f64 = 0.55 / 0.62;
    pub const BOOT: f64 = 0.7 / 0.79;
}

/// Detail tile size in metres — must match `bake_detail` in `textures`.
pub const DETAIL_TILE: f64 = 0.05;

/// ALBEDO BUDGET (linear, after the vertex tint multiplies the map)
///
/// MEASURED, not asserted — `node src/ai/selftest.mjs` prints this table from
/// the geometry and the real bakes, and `SoldierMaterials` prints the cloth
/// map's mean and range at boot. Current values:
///
/// ```text
///   uniform cloth      0.092-0.094   map mean 0.104, every texel in 0.040-0.152
///   helmet cover       0.064         deliberately off the uniform value
///   mag/admin pouches  0.058-0.076
///   knee + elbow pads  0.057-0.063
///   carrier            0.047         laminate, and smoother than the cloth
///   webbing / sling    0.051-0.054
///   boots              0.032
///   gloves             0.032-0.048
///   skin               0.152-0.190
/// ```
///
/// Real desert multicam is 0.18-0.32 and that is what this used to target, but
/// the environment it stands in currently behaves like 0.05-0.09 albedo on
/// screen (see the measurement table in `textures`), so a physically-honest
/// uniform rendered brighter than sunlit plaster and read as a white
/// mannequin. The whole kit is therefore scaled by one documented constant,
/// `KIT_CAL`, which keeps the *hierarchy* — cloth brightest, pouches under it,
/// carrier under that, boots and gloves darkest — because that internal value
/// structure is what breaks the "one extruded blob" read at 25 m. Raise
/// `CLOTH_BUDGET.mean` and `KIT_CAL` together if the world's albedo is ever
/// brought up to physical values.
mod gear {
    pub const WEBBING: [f64; 3] = [0.70, 0.70, 0.70];
    pub const SLING: [f64; 3] = [0.70, 0.70, 0.70];
    pub const POUCH: [f64; 3] = [0.84, 0.84, 0.84];
    pub const POUCH_ALT: [f64; 3] = [0.76, 0.76, 0.76];
    pub const DUMP: [f64; 3] = [0.72, 0.72, 0.72];
    pub const BELT: [f64; 3] = [0.62, 0.61, 0.57];
    pub const PAD: [f64; 3] = [0.55, 0.55, 0.55];
    pub const STRAP: [f64; 3] = [0.56, 0.56, 0.56];
    pub const WRAP: [f64; 3] = [0.56, 0.54, 0.50];
    pub const GLOVE: [f64; 3] = [0.38, 0.372, 0.363];
    pub const BOOT: [f64; 3] = [0.22, 0.209, 0.198];
    pub const LACE: [f64; 3] = [0.21, 0.204, 0.198];
    /// A hard ballistic mask is moulded polymer, not webbing: near-black with
    /// a clean sheen, which is what makes the lower face read as a mask at
    /// 35 m instead of another patch of tan cloth.
    pub const MASK: [f64; 3] = [0.62, 0.63, 0.66];
}

/// One visual variant. Each is a different silhouette, not a recolour: helmet
/// vs wrapped head, full plate vs chest rig, carbine vs long rifle.
///
/// The three tints are hue shifts at roughly unit luminance — value is set per
/// part by the albedo budget above, so a variant can change colour family
/// without dragging every piece of its kit out of the budget.
///
/// The source's variant records are sparse object literals: a key that is
/// absent reads back `undefined`, which is falsy. Here every key is present
/// and an absent source key becomes `false` / `None`.
#[derive(Clone, Debug, PartialEq)]
pub struct Variant {
    pub camo: &'static str,
    pub cloth_tint: [f64; 3],
    pub gear_tint: [f64; 3],
    pub plate_tint: [f64; 3],
    pub skin_tint: [f64; 3],
    pub helmet: bool,
    pub helmet_cover: bool,
    pub helmet_tint: Option<[f64; 3]>,
    pub head_wrap: bool,
    pub goggles: bool,
    pub goggles_down: bool,
    pub shades: bool,
    pub face_wrap: bool,
    pub mask_hard: bool,
    pub beard: bool,
    pub knee_pads: bool,
    pub full_carrier: bool,
    pub weapon: &'static str,
    pub bulk: f64,
    pub scale: f64,
}

/// The `vanguard` record, used as the base for the other two so that a key the
/// source omits is visibly a default rather than a transcription.
const VANGUARD: Variant = Variant {
    camo: "arid",
    cloth_tint: [1.03, 1.0, 0.94],
    gear_tint: [1.08, 0.98, 0.80], // coyote brown
    plate_tint: [1.02, 0.96, 0.84],
    skin_tint: [1.0, 0.94, 0.88],
    helmet: true,
    helmet_cover: true,
    helmet_tint: Some([0.72, 0.72, 0.68]),
    head_wrap: false,
    goggles: true,
    goggles_down: true,
    shades: false,
    face_wrap: true,
    mask_hard: false,
    beard: false,
    knee_pads: true,
    full_carrier: true,
    weapon: "carbine",
    bulk: 1.0,
    scale: 1.0,
};

const IRREGULAR: Variant = Variant {
    camo: "woodland",
    cloth_tint: [0.98, 1.02, 0.94],
    gear_tint: [0.92, 0.96, 0.74], // olive drab
    plate_tint: [0.90, 0.94, 0.80],
    skin_tint: [0.86, 0.80, 0.74],
    helmet: false,
    helmet_cover: false,
    helmet_tint: None,
    head_wrap: true,
    goggles: false,
    goggles_down: false,
    // dark wrap-around shooting glasses: the bare head needs a hard horizontal
    // dark band at the eye line or it is a featureless egg at 35 m
    shades: true,
    face_wrap: true,
    mask_hard: false,
    beard: true,
    knee_pads: false,
    full_carrier: false,
    weapon: "ak",
    bulk: 0.94,
    scale: 0.985,
};

const BREACHER: Variant = Variant {
    camo: "urban",
    cloth_tint: [0.98, 0.99, 1.02],
    gear_tint: [0.84, 0.86, 0.90], // wolf grey
    plate_tint: [0.86, 0.88, 0.92],
    skin_tint: [1.06, 0.98, 0.92],
    helmet: true,
    helmet_cover: false, // bare painted shell instead of a cloth cover
    helmet_tint: Some([0.82, 0.83, 0.86]),
    head_wrap: false,
    // goggles parked on the shell (not over the eyes like vanguard) plus a
    // hard ballistic half-mask: same helmet family, completely different head
    // read
    goggles: true,
    goggles_down: false,
    shades: false,
    face_wrap: true,
    mask_hard: true,
    beard: true,
    knee_pads: true,
    full_carrier: true,
    weapon: "carbine",
    bulk: 1.06,
    scale: 1.025,
};

/// Visual variants, in the source's declaration order.
pub const VARIANTS: [(&str, Variant); 3] =
    [("vanguard", VANGUARD), ("irregular", IRREGULAR), ("breacher", BREACHER)];

/// `VARIANTS[name] ?? VARIANTS.vanguard` — an unknown name falls back to
/// `vanguard` rather than failing.
pub fn variant(name: &str) -> &'static Variant {
    match name {
        "irregular" => &IRREGULAR,
        "breacher" => &BREACHER,
        _ => &VANGUARD,
    }
}

/// Every material slot a soldier's geometry is grouped by, IN THE ORDER
/// `CharacterBuilder::build()` emits them — which is the order the parts are
/// added in [`build_soldier`], deduplicated. All three variants use all nine.
///
/// THE ORDER IS LOAD-BEARING, and this is not a style preference. In the
/// source `THREE.Material` hands out globally incrementing ids and three sorts
/// the opaque render list by `material.id` (`painterSortStable`), including
/// the nine groups *within* one soldier. Create them in a different order and
/// the goggle lens draws before its frame instead of after it; with a depth
/// prepass in front, whichever coplanar surface is drawn last wins the
/// equal-depth test. MEASURED: prewarming these in a hand-written order moved
/// 2 pixels of the `combat` shot by 1/255 and failed the pixel gate.
/// [`build_soldier`] asserts the order below still matches.
pub const MATERIAL_SLOTS: [&str; 9] =
    ["cloth", "gear", "boot", "rubber", "plate", "polymer", "skin", "glass", "steel"];

/// A bone's bind-pose world position — `soldier.js`'s `bp()`.
fn bp(name: &str) -> [f64; 3] {
    RIG.bind_pos_of(name)
}

/// Three's `Vector3.normalize()`, bit-for-bit: `divideScalar(length() || 1)`,
/// and `divideScalar(s)` is `multiplyScalar(1 / s)`.
///
/// **Not** `v / len` component-wise: one reciprocal computed and then
/// multiplied three times rounds differently from three divisions, and
/// `buildSoldier` feeds these straight into the glove and knuckle-guard
/// frames. `length()` is `sqrt(x*x + y*y + z*z)` summed left to right — do not
/// tidy it, and do not reach for `hypot`, which scales by the largest
/// magnitude first and rounds differently again.
fn normalize3(v: [f64; 3]) -> [f64; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    // `length() || 1`: JavaScript's `||` returns the right operand for a zero
    // (or NaN) length. `f64::signum` would be wrong here for the same family
    // of reason `sign` is not `signum` — spell the zero case out.
    let s = if len == 0.0 { 1.0 } else { 1.0 / len };
    [v[0] * s, v[1] * s, v[2] * s]
}

/// What the assembly handed `CharacterBuilder::add`, recorded as it goes.
///
/// The source keeps the equivalent: `CharacterBuilder.build()` returns a
/// `parts` array explicitly so "the albedo audit in `selftest.mjs` can report
/// the effective value of every single piece of kit". That list is
/// material-sorted and carries only vertex ranges, though — it cannot say
/// which builder was called with what. This one is in **add order** and
/// carries a bounding-box/centroid fingerprint of each mesh, which is what
/// makes the recipe checkable against the original without dumping ~16.7k
/// vertices per variant.
#[derive(Clone, Debug, PartialEq)]
pub struct AddRecord {
    pub name: String,
    pub material: String,
    pub bone: Option<String>,
    pub bones: Option<Vec<String>>,
    pub bias: Option<Vec<f64>>,
    pub colour: Option<[f64; 3]>,
    pub grime: Option<f64>,
    pub dirt: Option<f64>,
    pub dust: Option<f64>,
    pub wear: Option<f64>,
    pub vertices: usize,
    pub triangles: usize,
    pub min: [f64; 3],
    pub max: [f64; 3],
    pub centroid: [f64; 3],
}

/// `CharacterBuilder` plus the add-order record. Every call site below reads
/// exactly like the source's `B.add(...)` / `B.occlude(...)`.
struct Assembly<'a> {
    b: CharacterBuilder<'a>,
    adds: Vec<AddRecord>,
}

impl Assembly<'_> {
    fn occlude(&mut self, a: [f64; 3], b: [f64; 3], r: f64, k: f64) {
        self.b.occlude(a, b, r, k);
    }

    fn add(&mut self, mesh: Mesh, o: PartOptions) {
        let n = mesh.p.len() / 3;
        let mut min = [f64::INFINITY; 3];
        let mut max = [f64::NEG_INFINITY; 3];
        let (mut cx, mut cy, mut cz) = (0.0, 0.0, 0.0);
        for i in 0..n {
            for a in 0..3 {
                let v = mesh.p[i * 3 + a];
                if v < min[a] {
                    min[a] = v;
                }
                if v > max[a] {
                    max[a] = v;
                }
            }
            // Summed in vertex order and divided once — float addition is not
            // associative, and the capture script sums the same way.
            cx += mesh.p[i * 3];
            cy += mesh.p[i * 3 + 1];
            cz += mesh.p[i * 3 + 2];
        }
        self.adds.push(AddRecord {
            name: o.name.clone(),
            material: o.material.clone(),
            bone: o.bone.clone(),
            bones: o.bones.clone(),
            bias: o.bias.clone(),
            colour: o.colour,
            grime: o.grime,
            dirt: o.dirt,
            dust: o.dust,
            wear: o.wear,
            vertices: n,
            triangles: mesh.i.len() / 3,
            min,
            max,
            centroid: [cx / n as f64, cy / n as f64, cz / n as f64],
        });
        self.b.add(mesh, o);
    }
}

/// One occlusion proxy capsule: the AO cage `CharacterBuilder` bakes into the
/// vertex colours.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Occluder {
    pub a: [f64; 3],
    pub b: [f64; 3],
    pub r: f64,
    pub k: f64,
}

/// The 28 occlusion proxies `buildSoldier` registers before adding any part,
/// in the source's order (`soldier.js:190-223`). A pure function of the rig's
/// bind pose, so it is checkable on its own.
pub fn occlusion_proxies() -> Vec<Occluder> {
    let (sh_r, el_r, wr_r) = (bp("UpperArmR"), bp("ForearmR"), bp("HandR"));
    let (sh_l, el_l, wr_l) = (bp("UpperArmL"), bp("ForearmL"), bp("HandL"));
    let (hip_r, kn_r, an_r) = (bp("UpLegR"), bp("LegR"), bp("FootR"));
    let (hip_l, kn_l, an_l) = (bp("UpLegL"), bp("LegL"), bp("FootL"));

    let occ = |a: [f64; 3], b: [f64; 3], r: f64, k: f64| Occluder { a, b, r, k };

    let mut out = vec![
        occ([0.0, 0.95, -0.01], [0.0, 1.42, 0.0], 0.155, 1.0), // torso core
        occ([0.0, 1.17, 0.130], [0.0, 1.40, 0.138], 0.11, 1.0), // front plate
        occ([0.0, 1.20, -0.105], [0.0, 1.40, -0.112], 0.105, 0.8), // back plate
        occ([-0.17, 1.16, 0.02], [-0.17, 1.30, 0.02], 0.055, 0.7), // right side
        occ([0.17, 1.16, 0.02], [0.17, 1.30, 0.02], 0.055, 0.7),
        occ([0.0, 1.60, 0.0], [0.0, 1.80, -0.01], 0.122, 1.0), // helmet interior
        occ([-0.09, 1.655, 0.05], [0.09, 1.655, 0.05], 0.055, 1.0), // brim shadow
        occ(sh_r, el_r, 0.058, 0.7),
        occ(sh_l, el_l, 0.058, 0.7),
        occ(hip_r, kn_r, 0.085, 0.8),
        occ(hip_l, kn_l, 0.085, 0.8),
        occ([0.0, 0.90, -0.01], [0.0, 1.05, -0.01], 0.15, 0.8), // belt line
        // strap crossings — shoulder yokes and the diagonal sling run. These
        // are the places a real uniform is darkest: sweat, webbing dye and
        // ground-in dust all collect where nylon rubs cloth.
        occ([-0.085, 1.42, 0.06], [-0.085, 1.42, -0.06], 0.048, 1.0),
        occ([0.085, 1.42, 0.06], [0.085, 1.42, -0.06], 0.048, 1.0),
        occ([-0.13, 1.40, 0.055], [0.10, 1.10, 0.115], 0.032, 0.9), // sling diagonal
        occ([-0.02, 1.02, 0.10], [-0.02, 1.02, -0.10], 0.10, 0.6),  // carrier hem
        // cuffs, elbows and knees: the three places a uniform is always ground in
        occ(wr_r, [wr_r[0], wr_r[1] + 0.05, wr_r[2]], 0.044, 0.9),
        occ(wr_l, [wr_l[0], wr_l[1] + 0.05, wr_l[2]], 0.044, 0.9),
        occ(el_r, [el_r[0], el_r[1] - 0.02, el_r[2] - 0.01], 0.052, 0.8),
        occ(el_l, [el_l[0], el_l[1] - 0.02, el_l[2] - 0.01], 0.052, 0.8),
        occ(kn_r, [kn_r[0], kn_r[1] - 0.03, kn_r[2] + 0.01], 0.072, 0.8),
        occ(kn_l, [kn_l[0], kn_l[1] - 0.03, kn_l[2] + 0.01], 0.072, 0.8),
        occ([an_r[0], an_r[1] + 0.09, an_r[2]], [an_r[0], an_r[1] + 0.15, an_r[2]], 0.062, 0.8),
        occ([an_l[0], an_l[1] + 0.09, an_l[2]], [an_l[0], an_l[1] + 0.15, an_l[2]], 0.062, 0.8),
        occ(
            *GRIP_R,
            [
                GRIP_R[0] + BORE_DIR[0] * 0.4,
                GRIP_R[1] + BORE_DIR[1] * 0.4 + 0.09,
                GRIP_R[2] + BORE_DIR[2] * 0.4,
            ],
            0.045,
            0.6,
        ),
    ];
    for i in 0..3 {
        let x = (f64::from(i) - 1.0) * 0.078;
        out.push(occ([x, 1.20, 0.160], [x, 1.28, 0.164], 0.040, 0.8)); // mag pouches
    }
    out
}

/// The detail-tile half of the two-scale material system.
#[derive(Clone, Debug, PartialEq)]
pub struct DetailSpec {
    pub set: &'static str,
    pub scale: f64,
    pub normal: f64,
    pub rough: f64,
}

/// One `SoldierMaterials.get(set, opts)` request, as pure data. `None` fields
/// are the ones the source leaves off the options object entirely (so
/// `SoldierMaterials.get` applies its own default *and* the cache key differs
/// from an explicitly-equal value) — the distinction is load-bearing for the
/// material cache, so it is preserved rather than folded into a default.
#[derive(Clone, Debug, PartialEq)]
pub struct MaterialSpec {
    pub set: String,
    pub key: String,
    pub tint: Option<[f64; 3]>,
    pub rough: Option<f64>,
    pub metal: Option<f64>,
    pub normal_scale: Option<f64>,
    pub ao: Option<f64>,
    pub detail: Option<DetailSpec>,
}

impl MaterialSpec {
    fn new(set: &str, key: &str) -> Self {
        MaterialSpec {
            set: set.to_string(),
            key: key.to_string(),
            tint: None,
            rough: None,
            metal: None,
            normal_scale: None,
            ao: None,
            detail: None,
        }
    }
}

/// A resolved material slot: either a set request, or the shared goggle-lens
/// glass (`materials.glass()`, which takes no options at all).
#[derive(Clone, Debug, PartialEq)]
pub enum MaterialRequest {
    Set(MaterialSpec),
    Glass,
}

/// Resolve a variant's material slot names to material requests.
///
/// Split out of [`build_soldier`] on purpose: `AiSystem.prewarmMaterials()`
/// needs every material a variant will ever ask for so their shader programs
/// can be compiled while a loading screen is up, and it must be able to get
/// them WITHOUT building a single triangle (geometry construction draws from
/// the shared RNG stream, so doing it early would move every downstream random
/// draw and change the picture). This function is a pure function of its name
/// and slots, so calling it early is free of side effects.
///
/// `detail` is the second half of the two-scale system: the base tile carries
/// the macro camo and the garment seams, this tile carries the weave and the
/// webbing ribbing. `scale` converts the base tile's UVs (metres / tile) into
/// the detail tile's, so the physical size of a thread is identical on a
/// sleeve, a pouch and a boot without any per-part tuning.
pub fn resolve_materials(name: &str, slots: &[String]) -> Vec<MaterialRequest> {
    let v = variant(name);
    let detail = |set: &'static str, mat_name: &str, normal: f64, rough: f64| DetailSpec {
        set,
        scale: material_tile(mat_name) / DETAIL_TILE,
        normal,
        rough,
    };
    slots
        .iter()
        .map(|n| match n.as_str() {
            "cloth" => MaterialRequest::Set(MaterialSpec {
                tint: Some(v.cloth_tint),
                rough: Some(rough::CLOTH),
                metal: Some(1.0),
                // 1.15, not 1.0: the base tile now carries a 1-2 cm crease
                // field and the folds have to actually catch the key light at
                // 25 m.
                normal_scale: Some(1.15),
                detail: Some(detail("cloth", "cloth", 0.45, 0.16)),
                ..MaterialSpec::new(&format!("camo_{}", v.camo), name)
            }),
            "plate" => MaterialRequest::Set(MaterialSpec {
                tint: Some(v.plate_tint),
                rough: Some(rough::PLATE),
                normal_scale: Some(1.0),
                detail: Some(detail("nylon", "plate", 0.45, 0.10)),
                ..MaterialSpec::new("plate", name)
            }),
            "gear" => MaterialRequest::Set(MaterialSpec {
                tint: Some(v.gear_tint),
                normal_scale: Some(1.1),
                detail: Some(detail("nylon", "gear", 0.5, 0.14)),
                ..MaterialSpec::new("nylon", name)
            }),
            "boot" => MaterialRequest::Set(MaterialSpec {
                tint: Some(v.gear_tint),
                rough: Some(rough::BOOT),
                normal_scale: Some(1.1),
                detail: Some(detail("nylon", "boot", 0.5, 0.10)),
                ..MaterialSpec::new("nylon", &format!("{name}_boot"))
            }),
            "skin" => MaterialRequest::Set(MaterialSpec {
                tint: Some(v.skin_tint),
                normal_scale: Some(0.8),
                ao: Some(0.6),
                ..MaterialSpec::new("skin", name)
            }),
            "polymer" => MaterialRequest::Set(MaterialSpec {
                normal_scale: Some(1.0),
                ..MaterialSpec::new("polymer", name)
            }),
            "steel" => MaterialRequest::Set(MaterialSpec {
                normal_scale: Some(1.0),
                ..MaterialSpec::new("steel", name)
            }),
            "rubber" => MaterialRequest::Set(MaterialSpec {
                normal_scale: Some(1.2),
                ..MaterialSpec::new("rubber", name)
            }),
            "glass" => MaterialRequest::Glass,
            // No variant's geometry produces a slot outside the nine above;
            // the source keeps this arm anyway and so does the port.
            _ => MaterialRequest::Set(MaterialSpec::new("polymer", name)),
        })
        .collect()
}

/// Vertex/triangle totals of one built soldier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SoldierStats {
    pub vertices: usize,
    pub triangles: usize,
}

/// One built variant: the shared geometry, the material requests for its
/// slots (in slot order), the per-part vertex ranges, the carried weapon, the
/// counts, and the variant record that was actually used.
pub struct SoldierBuild {
    pub geometry: crate::ai::geo::CharacterGeometry,
    pub material_names: Vec<String>,
    pub materials: Vec<MaterialRequest>,
    pub parts: Vec<crate::ai::geo::PartRange>,
    /// Every `add` the assembly made, in add order — see [`AddRecord`].
    pub adds: Vec<AddRecord>,
    pub weapon: Weapon,
    pub stats: SoldierStats,
    pub variant: &'static Variant,
    /// What the source writes to `console.warn`. Empty unless the material
    /// slot order drifted away from [`MATERIAL_SLOTS`].
    pub warnings: Vec<String>,
}

/// Build one variant.
///
/// `rng` is the shared stream: it is forked once for the assembly's noise
/// field, drawn from twice per magazine pouch, and handed to
/// [`build_weapon`] — in that order. Nothing else touches it.
pub fn build_soldier(name: &str, rng: &mut Rng) -> SoldierBuild {
    let v = variant(name);
    let nz = Noise::new(&mut rng.fork());
    let mut b = Assembly { b: CharacterBuilder::new(&RIG, &nz, &MATERIALS), adds: Vec::new() };

    let (sh_r, el_r, wr_r) = (bp("UpperArmR"), bp("ForearmR"), bp("HandR"));
    let (sh_l, el_l, wr_l) = (bp("UpperArmL"), bp("ForearmL"), bp("HandL"));
    let (hip_r, kn_r, an_r) = (bp("UpLegR"), bp("LegR"), bp("FootR"));
    let (hip_l, kn_l, an_l) = (bp("UpLegL"), bp("LegL"), bp("FootL"));
    let head = bp("Head");

    /* ---------------- occlusion proxies (drives baked vertex AO) -------- */
    for o in occlusion_proxies() {
        b.occlude(o.a, o.b, o.r, o.k);
    }

    /* ---------------- uniform ------------------------------------------ */
    b.add(
        p::jacket_torso(&nz, &JacketOpts { bulk: v.bulk, ..JacketOpts::default() }),
        PartOptions {
            material: "cloth".to_string(),
            bones: bones(&[
                "Hips", "Spine", "Spine1", "Spine2", "Neck", "ClavicleR", "ClavicleL", "UpperArmR",
                "UpperArmL",
            ]),
            bias: Some(vec![1.0, 1.0, 1.0, 1.0, 0.8, 0.55, 0.55, 0.30, 0.30]),
            colour: Some([1.0, 1.0, 1.0]),
            grime: Some(0.85),
            dirt: Some(0.20),
            dust: Some(0.34),
            wear: Some(0.08),
            name: "jacket".to_string(),
            ..PartOptions::default()
        },
    );
    b.add(
        p::pelvis(&nz),
        PartOptions {
            material: "cloth".to_string(),
            bones: bones(&["Hips", "Spine", "UpLegR", "UpLegL"]),
            bias: Some(vec![1.0, 0.7, 0.5, 0.5]),
            colour: Some([0.97, 0.97, 0.97]),
            grime: Some(0.9),
            dirt: Some(0.35),
            dust: Some(0.2),
            name: "pelvis".to_string(),
            ..PartOptions::default()
        },
    );
    b.add(
        p::collar(&nz),
        PartOptions {
            material: "cloth".to_string(),
            bones: bones(&["Neck", "Spine2", "Head"]),
            bias: Some(vec![1.0, 0.8, 0.3]),
            colour: Some([0.92, 0.92, 0.91]),
            grime: Some(1.0),
            dust: Some(0.3),
            name: "collar".to_string(),
            ..PartOptions::default()
        },
    );

    // sleeves
    for (sh, el, wr, side, suffix) in
        [(sh_r, el_r, wr_r, -1.0_f64, "R"), (sh_l, el_l, wr_l, 1.0_f64, "L")]
    {
        // deltoid cap. The sleeve tube alone meets the torso in a socket,
        // which is half of the "tube arms" read; this gives the shoulder an
        // actual shape and a highlight to catch the key light on.
        b.add(
            p::shoulder_cap(&nz, sh, side),
            PartOptions {
                material: "cloth".to_string(),
                bones: bones(&[
                    format!("Clavicle{suffix}").as_str(),
                    format!("UpperArm{suffix}").as_str(),
                    "Spine2",
                ]),
                bias: Some(vec![0.8, 1.0, 0.4]),
                colour: Some([1.0, 1.0, 1.0]),
                grime: Some(0.7),
                dust: Some(0.5),
                wear: Some(0.06),
                name: format!("shoulder{suffix}"),
                ..PartOptions::default()
            },
        );
        b.add(
            p::limb_tube(
                &nz,
                [sh[0] + side * 0.012, sh[1] + 0.055, sh[2]],
                el,
                wr,
                &[0.050, 0.062, 0.056, 0.050, 0.046, 0.042, 0.038],
                &LimbOpts {
                    rings: 22,
                    seg: 16,
                    fold: 0.0016,
                    // 3 mm creases: at 35 m that is sub-pixel as displacement
                    // but the normals it generates are what put light and
                    // shade *inside* the sleeve outline.
                    crease: 0.0030,
                    bend: [0.0, 0.0, -1.0], // sleeve bunches inside the elbow
                    ..LimbOpts::default()
                },
            ),
            PartOptions {
                material: "cloth".to_string(),
                bones: bones(&[
                    format!("Clavicle{suffix}").as_str(),
                    format!("UpperArm{suffix}").as_str(),
                    format!("Forearm{suffix}").as_str(),
                    format!("Hand{suffix}").as_str(),
                    "Spine2",
                ]),
                bias: Some(vec![0.5, 1.0, 1.0, 0.7, 0.25]),
                colour: Some([1.0, 1.0, 1.0]),
                grime: Some(0.8),
                dirt: Some(0.15),
                dust: Some(0.3),
                wear: Some(0.12),
                name: format!("sleeve{suffix}"),
                ..PartOptions::default()
            },
        );
        // elbow reinforcement patch
        b.add(
            p::limb_tube(
                &nz,
                [el[0] * 0.98 + sh[0] * 0.02, el[1] + 0.05, el[2] - 0.005],
                el,
                [el[0] * 0.98 + wr[0] * 0.02, el[1] - 0.05, el[2] + 0.004],
                &[0.050, 0.054, 0.050],
                &LimbOpts { rings: 5, seg: 14, fold: 0.001, ..LimbOpts::default() },
            ),
            PartOptions {
                material: "gear".to_string(),
                bones: bones(&[format!("UpperArm{suffix}").as_str(), format!("Forearm{suffix}").as_str()]),
                bias: Some(vec![1.0, 1.0]),
                colour: Some(gear::PAD),
                grime: Some(1.0),
                dirt: Some(0.25),
                dust: Some(0.22),
                wear: Some(0.16),
                name: format!("elbowPad{suffix}"),
                ..PartOptions::default()
            },
        );
    }

    // trousers
    for (hip, kn, an, suffix) in [(hip_r, kn_r, an_r, "R"), (hip_l, kn_l, an_l, "L")] {
        b.add(
            p::limb_tube(
                &nz,
                hip,
                kn,
                [an[0], an[1] + 0.085, an[2] + 0.008],
                &[0.090, 0.085, 0.076, 0.068, 0.062, 0.060, 0.064],
                &LimbOpts {
                    rings: 24,
                    seg: 17,
                    fold: 0.0018,
                    // trousers crease harder than sleeves and stack on the
                    // boot cuff
                    crease: 0.0042,
                    bend: [0.0, 0.0, -1.0], // gathers behind the knee
                    ..LimbOpts::default()
                },
            ),
            PartOptions {
                material: "cloth".to_string(),
                bones: bones(&[
                    "Hips",
                    format!("UpLeg{suffix}").as_str(),
                    format!("Leg{suffix}").as_str(),
                    format!("Foot{suffix}").as_str(),
                ]),
                bias: Some(vec![0.6, 1.0, 1.0, 0.5]),
                colour: Some([0.98, 0.98, 0.97]),
                grime: Some(0.8),
                dirt: Some(0.72),
                dust: Some(0.22),
                wear: Some(0.10),
                name: format!("leg{suffix}"),
                ..PartOptions::default()
            },
        );
        // cargo pocket on the outer thigh
        let side: f64 = if suffix == "R" { -1.0 } else { 1.0 };
        b.add(
            p::pouch(
                &nz,
                &PouchOpts {
                    hx: 0.052,
                    hy: 0.070,
                    hz: 0.026,
                    x: hip[0] + side * 0.062,
                    y: hip[1] - 0.16,
                    z: 0.026,
                    rz: side * 0.06,
                    ry: side * 0.55,
                    bend: 0.11,
                    ..PouchOpts::default()
                },
            ),
            PartOptions {
                material: "cloth".to_string(),
                bones: bones(&[format!("UpLeg{suffix}").as_str(), "Hips"]),
                bias: Some(vec![1.0, 0.4]),
                colour: Some([0.95, 0.95, 0.94]),
                grime: Some(0.9),
                dirt: Some(0.5),
                dust: Some(0.4),
                wear: Some(0.16),
                name: format!("cargo{suffix}"),
                ..PartOptions::default()
            },
        );
        if v.knee_pads {
            b.add(
                p::knee_pad(&nz, kn, side),
                PartOptions {
                    material: "gear".to_string(),
                    bones: bones(&[format!("Leg{suffix}").as_str(), format!("UpLeg{suffix}").as_str()]),
                    bias: Some(vec![1.0, 0.5]),
                    colour: Some(gear::PAD),
                    grime: Some(1.0),
                    dirt: Some(0.9),
                    dust: Some(0.28),
                    wear: Some(0.20),
                    name: format!("knee{suffix}"),
                    ..PartOptions::default()
                },
            );
        }
        // boots
        b.add(
            p::boot(&nz, an, side),
            PartOptions {
                material: "boot".to_string(),
                bones: bones(&[
                    format!("Leg{suffix}").as_str(),
                    format!("Foot{suffix}").as_str(),
                    format!("Toe{suffix}").as_str(),
                ]),
                bias: Some(vec![0.55, 1.0, 0.6]),
                colour: Some(gear::BOOT),
                grime: Some(0.9),
                dirt: Some(0.85),
                dust: Some(0.5),
                wear: Some(0.3),
                name: format!("boot{suffix}"),
                ..PartOptions::default()
            },
        );
        b.add(
            p::boot_sole(an),
            PartOptions {
                material: "rubber".to_string(),
                bones: bones(&[format!("Foot{suffix}").as_str(), format!("Toe{suffix}").as_str()]),
                bias: Some(vec![1.0, 0.8]),
                grime: Some(0.9),
                dirt: Some(1.0),
                name: format!("sole{suffix}"),
                ..PartOptions::default()
            },
        );
        b.add(
            p::boot_laces(an),
            PartOptions {
                material: "boot".to_string(),
                bone: Some(format!("Foot{suffix}")),
                colour: Some(gear::LACE),
                grime: Some(0.9),
                dirt: Some(0.85),
                name: format!("laces{suffix}"),
                ..PartOptions::default()
            },
        );
    }

    /* ---------------- load-bearing gear -------------------------------- */
    // `P.plateCarrier(nz, V)`: `parts.js` never reads its second argument.
    // Dead in the source; not invented here.
    b.add(
        p::plate_carrier(&nz),
        PartOptions {
            material: "plate".to_string(),
            bones: bones(&["Spine", "Spine1", "Spine2", "ClavicleR", "ClavicleL"]),
            bias: Some(vec![0.7, 1.0, 1.0, 0.45, 0.45]),
            colour: Some([0.72, 0.72, 0.72]),
            grime: Some(0.85),
            dirt: Some(0.3),
            dust: Some(0.30),
            wear: Some(0.18),
            name: "carrier".to_string(),
            ..PartOptions::default()
        },
    );
    b.add(
        p::carrier_webbing(),
        PartOptions {
            material: "gear".to_string(),
            bones: bones(&["Spine1", "Spine2"]),
            bias: Some(vec![1.0, 1.0]),
            colour: Some(gear::WEBBING),
            grime: Some(1.0),
            dust: Some(0.3),
            wear: Some(0.26),
            name: "webbing".to_string(),
            ..PartOptions::default()
        },
    );

    // magazine pouches across the front, deliberately not evenly loaded
    let n_pouch: i32 = if v.full_carrier { 3 } else { 2 };
    for i in 0..n_pouch {
        // `nPouch === 1` never happens (it is 3 or 2), but the guard is in the
        // source and a dead arm is still part of the source.
        let t = if n_pouch == 1 { 0.5 } else { f64::from(i) / f64::from(n_pouch - 1) };
        let x = (t - 0.5) * (if n_pouch > 2 { 0.156 } else { 0.09 });
        // Both draws, in the source's property-evaluation order: `y` before
        // `rz`. See the module doc.
        let y = 1.236 + rng.range(-0.006, 0.006);
        let rz = rng.range(-0.05, 0.05);
        b.add(
            p::pouch(
                &nz,
                &PouchOpts {
                    hx: 0.033,
                    hy: 0.056,
                    hz: 0.034,
                    x,
                    y,
                    z: 0.148,
                    rx: -0.10,
                    rz,
                    lid_tilt: if i == 1 { -0.5 } else { 0.0 },
                    bend: 0.26,
                    ..PouchOpts::default()
                },
            ),
            PartOptions {
                material: "gear".to_string(),
                bones: bones(&["Spine1", "Spine2", "Spine"]),
                bias: Some(vec![1.0, 0.8, 0.4]),
                colour: Some(if i == 1 { gear::POUCH_ALT } else { gear::POUCH }),
                grime: Some(0.9),
                dirt: Some(0.25),
                dust: Some(0.5),
                wear: Some(0.30),
                name: format!("magPouch{i}"),
                ..PartOptions::default()
            },
        );
        // a magazine sticking out of the open pouch
        if i == 1 {
            let mag = p::pouch(
                &nz,
                &PouchOpts {
                    hx: 0.0145,
                    hy: 0.042,
                    hz: 0.023,
                    x,
                    y: 1.308,
                    z: 0.152,
                    rx: -0.12,
                    ..PouchOpts::default()
                },
            );
            b.add(
                mag,
                PartOptions {
                    material: "polymer".to_string(),
                    bones: bones(&["Spine1", "Spine2"]),
                    bias: Some(vec![1.0, 0.8]),
                    grime: Some(0.4),
                    wear: Some(0.2),
                    name: "spareMag".to_string(),
                    ..PartOptions::default()
                },
            );
        }
    }

    // radio on the left chest, admin pouch on the right, IFAK on the belt
    b.add(
        p::pouch(
            &nz,
            &PouchOpts {
                hx: 0.032,
                hy: 0.058,
                hz: 0.028,
                x: 0.112,
                y: 1.336,
                z: 0.118,
                ry: 0.35,
                rz: 0.10,
                bend: 0.24,
                ..PouchOpts::default()
            },
        ),
        PartOptions {
            material: "gear".to_string(),
            bones: bones(&["Spine2", "ClavicleL", "Spine1"]),
            bias: Some(vec![1.0, 0.5, 0.5]),
            colour: Some(gear::POUCH_ALT),
            grime: Some(0.9),
            dust: Some(0.5),
            wear: Some(0.22),
            name: "radio".to_string(),
            ..PartOptions::default()
        },
    );
    // antenna
    {
        let ant = p::pouch(
            &nz,
            &PouchOpts {
                hx: 0.005,
                hy: 0.075,
                hz: 0.005,
                x: 0.116,
                y: 1.424,
                z: 0.104,
                rx: -0.18,
                ..PouchOpts::default()
            },
        );
        b.add(
            ant,
            PartOptions {
                material: "polymer".to_string(),
                bones: bones(&["Spine2", "ClavicleL"]),
                bias: Some(vec![1.0, 0.6]),
                grime: Some(0.3),
                name: "antenna".to_string(),
                ..PartOptions::default()
            },
        );
    }
    b.add(
        p::belt(&nz),
        PartOptions {
            material: "gear".to_string(),
            bones: bones(&["Hips", "Spine"]),
            bias: Some(vec![1.0, 0.5]),
            colour: Some(gear::BELT),
            grime: Some(1.0),
            dirt: Some(0.4),
            dust: Some(0.25),
            wear: Some(0.26),
            name: "belt".to_string(),
            ..PartOptions::default()
        },
    );
    b.add(
        p::hip_pouch(&nz, -1.0),
        PartOptions {
            material: "gear".to_string(),
            bones: bones(&["Hips", "Spine"]),
            bias: Some(vec![1.0, 0.4]),
            colour: Some(gear::DUMP),
            grime: Some(0.95),
            dirt: Some(0.6),
            dust: Some(0.45),
            wear: Some(0.26),
            name: "dumpPouch".to_string(),
            ..PartOptions::default()
        },
    );

    /* ---------------- head --------------------------------------------- */
    let wrapped = v.face_wrap;
    b.add(
        p::head_mesh(&nz, head, &HeadOpts::default()),
        PartOptions {
            material: "skin".to_string(),
            bone: Some("Head".to_string()),
            colour: Some([1.0, 1.0, 1.0]),
            grime: Some(0.3),
            dirt: Some(0.06),
            name: "head".to_string(),
            ..PartOptions::default()
        },
    );
    b.add(
        p::nose(&nz, head),
        PartOptions {
            material: "skin".to_string(),
            bone: Some("Head".to_string()),
            grime: Some(0.25),
            name: "nose".to_string(),
            ..PartOptions::default()
        },
    );
    b.add(
        p::ear(&nz, head, -1.0),
        PartOptions {
            material: "skin".to_string(),
            bone: Some("Head".to_string()),
            grime: Some(0.45),
            name: "earR".to_string(),
            ..PartOptions::default()
        },
    );
    b.add(
        p::ear(&nz, head, 1.0),
        PartOptions {
            material: "skin".to_string(),
            bone: Some("Head".to_string()),
            grime: Some(0.45),
            name: "earL".to_string(),
            ..PartOptions::default()
        },
    );
    for side in [-1.0_f64, 1.0_f64] {
        b.add(
            p::eyeball(head, side),
            PartOptions {
                material: "polymer".to_string(),
                bone: Some("Head".to_string()),
                colour: Some([0.55, 0.5, 0.45]),
                grime: Some(0.2),
                // Both eyes are added under the same part name, as in the
                // source — `parts` is a debug/audit list, not a key.
                name: "eye".to_string(),
                ..PartOptions::default()
            },
        );
    }
    // neck
    b.add(
        p::limb_tube(
            &nz,
            [head[0], head[1] - 0.10, head[2] - 0.012],
            [head[0], head[1] - 0.05, head[2] - 0.008],
            [head[0], head[1], head[2]],
            &[0.058, 0.056, 0.054],
            &LimbOpts { rings: 5, seg: 14, fold: 0.001, ..LimbOpts::default() },
        ),
        PartOptions {
            material: "skin".to_string(),
            bones: bones(&["Neck", "Head", "Spine2"]),
            bias: Some(vec![1.0, 0.7, 0.4]),
            grime: Some(0.5),
            name: "neck".to_string(),
            ..PartOptions::default()
        },
    );

    if wrapped {
        // A hard ballistic mask is moulded polymer on the *same* geometry: the
        // seam and the bridge fold read as the mask's edge and nose vent
        // instead of a cloth hem, and it lands far darker than any cloth in
        // the kit, which is what gives that variant a legible face at range.
        //
        // `P.faceWrap(nz, head, V)`: the third argument is dead in `parts.js`.
        b.add(
            p::face_wrap(&nz, head),
            PartOptions {
                material: if v.mask_hard { "polymer" } else { "gear" }.to_string(),
                bones: bones(&["Head", "Neck"]),
                bias: Some(vec![1.0, 0.5]),
                colour: Some(if v.mask_hard {
                    gear::MASK
                } else if v.helmet {
                    gear::WRAP
                } else {
                    [0.78, 0.74, 0.66]
                }),
                grime: Some(if v.mask_hard { 0.5 } else { 0.85 }),
                dirt: Some(if v.mask_hard { 0.1 } else { 0.2 }),
                dust: Some(if v.mask_hard { 0.2 } else { 0.3 }),
                wear: Some(if v.mask_hard { 0.3 } else { 0.1 }),
                name: "faceWrap".to_string(),
                ..PartOptions::default()
            },
        );
    }

    if v.helmet {
        // A covered helmet is CLOTH, not plastic: the camo cover is the single
        // biggest reason a helmet reads as a helmet rather than a bowling
        // ball. Its tint deliberately lands off the uniform value so the head
        // separates from the torso at range. A bare shell goes on the laminate
        // set instead.
        //
        // `P.helmet(nz, head, V)`: the third argument is dead in `parts.js`.
        b.add(
            p::helmet(&nz, head),
            PartOptions {
                material: if v.helmet_cover { "cloth" } else { "plate" }.to_string(),
                bone: Some("Head".to_string()),
                colour: Some(v.helmet_tint.unwrap_or([1.0, 1.0, 1.0])),
                grime: Some(0.6),
                dirt: Some(0.2),
                dust: Some(0.55),
                wear: Some(0.4),
                name: "helmet".to_string(),
                ..PartOptions::default()
            },
        );
        b.add(
            p::helmet_hardware(&nz, head),
            PartOptions {
                material: "polymer".to_string(),
                bone: Some("Head".to_string()),
                grime: Some(0.5),
                wear: Some(0.3),
                name: "helmetHW".to_string(),
                ..PartOptions::default()
            },
        );
        b.add(
            p::chin_strap(head),
            PartOptions {
                material: "gear".to_string(),
                bones: bones(&["Head", "Neck"]),
                bias: Some(vec![1.0, 0.4]),
                colour: Some(gear::STRAP),
                grime: Some(0.95),
                dust: Some(0.25),
                wear: Some(0.18),
                name: "chinStrap".to_string(),
                ..PartOptions::default()
            },
        );
        if v.goggles {
            let g = p::goggles(head, v.goggles_down);
            b.add(
                g.frame,
                PartOptions {
                    material: "polymer".to_string(),
                    bone: Some("Head".to_string()),
                    grime: Some(0.4),
                    wear: Some(0.35),
                    name: "goggleFrame".to_string(),
                    ..PartOptions::default()
                },
            );
            b.add(
                g.strap,
                PartOptions {
                    material: "gear".to_string(),
                    bone: Some("Head".to_string()),
                    colour: Some(gear::STRAP),
                    grime: Some(0.85),
                    dust: Some(0.3),
                    name: "goggleStrap".to_string(),
                    ..PartOptions::default()
                },
            );
            b.add(
                p::goggle_lens(head, v.goggles_down),
                PartOptions {
                    material: "glass".to_string(),
                    bone: Some("Head".to_string()),
                    colour: Some([1.0, 1.0, 1.0]),
                    grime: Some(0.15),
                    name: "goggleLens".to_string(),
                    ..PartOptions::default()
                },
            );
        }
    } else if v.head_wrap {
        let wrap = p::head_scarf(&nz, head);
        b.add(
            wrap,
            PartOptions {
                // hue comes from the variant's cloth tint; the VALUE sits
                // below the uniform so the head is never the brightest thing
                // on the figure
                material: "cloth".to_string(),
                bone: Some("Head".to_string()),
                colour: Some([0.82, 0.80, 0.76]),
                grime: Some(0.8),
                dirt: Some(0.25),
                dust: Some(0.4),
                wear: Some(0.14),
                name: "shemagh".to_string(),
                ..PartOptions::default()
            },
        );
    }

    if v.shades {
        let s = p::sunglasses(head);
        b.add(
            s.frame,
            PartOptions {
                material: "polymer".to_string(),
                bone: Some("Head".to_string()),
                grime: Some(0.4),
                wear: Some(0.3),
                name: "shadeFrame".to_string(),
                ..PartOptions::default()
            },
        );
        b.add(
            s.lens,
            PartOptions {
                material: "glass".to_string(),
                bone: Some("Head".to_string()),
                colour: Some([1.0, 1.0, 1.0]),
                grime: Some(0.2),
                name: "shadeLens".to_string(),
                ..PartOptions::default()
            },
        );
    }

    /* ---------------- hands + weapon ----------------------------------- */
    // `BORE_DIR` is already unit length; the source normalises it again and so
    // does this. See [`normalize3`].
    let bore = normalize3(*BORE_DIR);
    let palm_r = normalize3([-0.55, 0.35, -0.75]);
    let palm_l = normalize3([0.75, 0.30, -0.60]);
    let grip_axis_r = normalize3([0.18, 0.92, -0.34]); // down the grip
    let grip_axis_l = bore;

    b.add(
        p::glove(&nz, wr_r, grip_axis_r, palm_r, -1.0),
        PartOptions {
            material: "boot".to_string(),
            bones: bones(&["HandR", "ForearmR"]),
            bias: Some(vec![1.0, 0.35]),
            colour: Some(gear::GLOVE),
            grime: Some(0.9),
            dirt: Some(0.3),
            dust: Some(0.25),
            wear: Some(0.28),
            name: "gloveR".to_string(),
            ..PartOptions::default()
        },
    );
    b.add(
        p::knuckle_guard(wr_r, grip_axis_r, palm_r),
        PartOptions {
            material: "polymer".to_string(),
            bone: Some("HandR".to_string()),
            grime: Some(0.5),
            wear: Some(0.35),
            name: "knuckleR".to_string(),
            ..PartOptions::default()
        },
    );
    b.add(
        p::glove(&nz, wr_l, grip_axis_l, palm_l, 1.0),
        PartOptions {
            material: "boot".to_string(),
            bones: bones(&["HandL", "ForearmL"]),
            bias: Some(vec![1.0, 0.35]),
            colour: Some(gear::GLOVE),
            grime: Some(0.9),
            dirt: Some(0.3),
            dust: Some(0.25),
            wear: Some(0.28),
            name: "gloveL".to_string(),
            ..PartOptions::default()
        },
    );
    b.add(
        p::knuckle_guard(wr_l, grip_axis_l, palm_l),
        PartOptions {
            material: "polymer".to_string(),
            bone: Some("HandL".to_string()),
            grime: Some(0.5),
            wear: Some(0.35),
            name: "knuckleL".to_string(),
            ..PartOptions::default()
        },
    );

    let w = build_weapon(&nz, WeaponStyle::from_name(v.weapon), Some(rng));
    b.add(
        clone_mesh(&w.steel),
        PartOptions {
            material: "steel".to_string(),
            bone: Some("HandR".to_string()),
            grime: Some(0.55),
            wear: Some(0.25),
            name: "wpnSteel".to_string(),
            ..PartOptions::default()
        },
    );
    b.add(
        clone_mesh(&w.polymer),
        PartOptions {
            material: "polymer".to_string(),
            bone: Some("HandR".to_string()),
            grime: Some(0.5),
            wear: Some(0.3),
            name: "wpnPoly".to_string(),
            ..PartOptions::default()
        },
    );
    b.add(
        clone_mesh(&w.rubber),
        PartOptions {
            material: "rubber".to_string(),
            bone: Some("HandR".to_string()),
            grime: Some(0.6),
            name: "wpnRubber".to_string(),
            ..PartOptions::default()
        },
    );
    if !w.glass.p.is_empty() {
        b.add(
            clone_mesh(&w.glass),
            PartOptions {
                material: "glass".to_string(),
                bone: Some("HandR".to_string()),
                grime: Some(0.1),
                name: "wpnGlass".to_string(),
                ..PartOptions::default()
            },
        );
    }

    // sling: body-bound so it stays on the chest as the arms move
    b.add(
        p::sling(w.foregrip, w.stock_top),
        PartOptions {
            material: "gear".to_string(),
            bones: bones(&["Spine2", "Spine1", "ClavicleR", "ClavicleL", "Hips"]),
            bias: Some(vec![1.0, 0.8, 0.5, 0.5, 0.3]),
            colour: Some(gear::SLING),
            grime: Some(1.0),
            dust: Some(0.2),
            wear: Some(0.22),
            name: "sling".to_string(),
            ..PartOptions::default()
        },
    );

    let Assembly { mut b, adds } = b;
    let built = b.build();
    // Guard the prewarm contract: see MATERIAL_SLOTS.
    let mut warnings = Vec::new();
    if !material_slot_order_matches(&built.material_names) {
        warnings.push(format!(
            "[ai] material slot order changed ({}); update MATERIAL_SLOTS or prewarmMaterials will reorder opaque draws",
            built.material_names.join(",")
        ));
    }
    let mats = resolve_materials(name, &built.material_names);

    // `SoldierBuild` mirrors the source's returned object, which surfaces
    // `materialNames`/`parts`/`stats` alongside the geometry even though the
    // geometry already carries them. Copy them out before `built` is moved
    // into the `geometry` field rather than reshaping the public struct.
    let material_names = built.material_names.clone();
    let parts = built.parts.clone();
    let stats = SoldierStats { vertices: built.vertices, triangles: built.triangles };

    SoldierBuild {
        geometry: built,
        material_names,
        materials: mats,
        parts,
        adds,
        weapon: w,
        stats,
        variant: v,
        warnings,
    }
}

/// `built.materialNames.join() === MATERIAL_SLOTS.filter(s =>
/// built.materialNames.includes(s)).join()` — the emitted material order must
/// be [`MATERIAL_SLOTS`] restricted to the slots this variant actually uses.
fn material_slot_order_matches(material_names: &[String]) -> bool {
    let expected: Vec<&str> = MATERIAL_SLOTS
        .iter()
        .copied()
        .filter(|s| material_names.iter().any(|n| n == s))
        .collect();
    material_names.len() == expected.len()
        && material_names.iter().zip(expected.iter()).all(|(a, b)| a == b)
}

/// `PartOptions::bones` takes owned names because the sleeve/trouser loops
/// build theirs with a side suffix (`Clavicle{R,L}`).
fn bones(names: &[&str]) -> Option<Vec<String>> {
    Some(names.iter().map(|s| (*s).to_string()).collect())
}

/// The weapon's meshes are added to the builder AND returned to the caller in
/// `SoldierBuild::weapon` (the source hands out the same JS object twice).
/// Rust needs one of the two to be a copy; the returned `Weapon` keeps the
/// originals so its anchor points and meshes still line up.
fn clone_mesh(m: &Mesh) -> Mesh {
    m.clone()
}
