//! Ported from Claude-of-Duty `src/weapons/parts.js:170-381` — the barrel,
//! gas block and muzzle-device group.
//!
//! See `parts.js:19-31`: every dimension is a real, published firearm
//! measurement, not an eyeballed guess. Weapon-local space is `+X` right,
//! `+Y` up, `-Z` toward the muzzle, origin at the shooting hand's anchor —
//! the convention `geometry.js:28-30` documents and the `geometry` module
//! (`03-weapon-geometry-api.md`) carries forward.
//!
//! This is app code (`apps/`), outside the Branchless Law — plain `if`/`match`
//! throughout, matching the JS this replaces. Rust has no default arguments,
//! so every JS `?? value` default is documented on the option struct/function
//! and callers pass it explicitly (same convention as `parts::hardware` and
//! `geometry::primitives`).

use axiom_math::{Mat4, Quat, Vec3};

use crate::weapons::geometry::primitives::{box_geo, knurl_band, lathe_z, tube_z};
use crate::weapons::geometry::{merge_all, Assembly, Geo, Xform};
use crate::weapons::parts::hardware::{add_screw, MountAxis};

/// `BufferGeometry.translate(x, y, z)`, applied directly to a not-yet-added
/// piece (`parts.js`'s `g1.translate(...)`/`slot.translate(...)`/
/// `lug.translate(...)`/`k.translate(...)` calls in [`add_muzzle_device`], all
/// made before the piece reaches `mergeAll`/`Assembly.add`). Reuses
/// [`Geo::apply`] — the same normal-matrix-correct transform
/// `Assembly::add`/`geometry::primitives::xform` use — rather than a second,
/// parallel transform path.
fn translate(g: &mut Geo, x: f32, y: f32, z: f32) {
    g.apply(&Mat4::translation(Vec3::new(x, y, z)));
}

/// `BufferGeometry.rotateZ(angle)`, the other direct-geometry op
/// [`add_muzzle_device`] needs (the a2 birdcage's slot fan and the tri-lug's
/// three lugs).
fn rotate_z(g: &mut Geo, angle: f32) {
    let q = Quat::from_axis_angle(Vec3::UNIT_Z, angle).expect("Vec3::UNIT_Z is nonzero");
    g.apply(&Mat4::from_quaternion(q));
}

/* -------------------------------------------------------------------------- */
/*  barrel                                                                    */
/* -------------------------------------------------------------------------- */

/// `o` on `addBarrel(asm, matSteel, matCavity, o)` (`parts.js:178-222`).
/// `r_barrel` is the source's own option key (`o.rBarrel`) even though it
/// becomes the *bore* radius internally (`const rBore = o.rBarrel ?? 0.0072`)
/// — kept as named here for fidelity with the call sites that set it.
/// `z_breech`/`z_muzzle` have no JS default (`o.zBreech`/`o.zMuzzle` are read
/// bare); `Default` sets them to `0.0` only so the rest of the struct can use
/// struct-update syntax — every real caller sets both explicitly.
#[derive(Clone, Copy, Debug)]
pub struct BarrelOpts {
    pub y: f32,
    pub z_breech: f32,
    pub z_muzzle: f32,
    pub r_chamber: f32,
    pub r_barrel: f32,
    pub r_gas: f32,
    /// `o.gasAt`; `None` reproduces `?? zMuzzle + len * 0.34`.
    pub gas_at: Option<f32>,
    pub seg: u32,
    /// `o.knurl !== false` — anything other than a literal `false` keeps the
    /// knurled band, so the JS default is "on".
    pub knurl: bool,
}

impl Default for BarrelOpts {
    fn default() -> Self {
        BarrelOpts {
            y: 0.0,
            z_breech: 0.0,
            z_muzzle: 0.0,
            r_chamber: 0.0112,
            r_barrel: 0.0072,
            r_gas: 0.0092,
            gas_at: None,
            seg: 22,
            knurl: true,
        }
    }
}

/// `addBarrel`'s return: `{ gasAt, rBore }` (`parts.js:221`), so the caller
/// can lay out the gas block and muzzle device.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BarrelResult {
    pub gas_at: f32,
    pub r_bore: f32,
}

/// Stepped barrel with a chamber shoulder, a gas journal and a knurled
/// section. Returns the muzzle Z so the device can be bolted onto the crown
/// (`addBarrel`, `parts.js:178-222`).
pub fn add_barrel(asm: &mut Assembly, mat_steel: &str, mat_cavity: &str, o: BarrelOpts) -> BarrelResult {
    let BarrelOpts {
        y,
        z_breech,
        z_muzzle,
        r_chamber,
        r_barrel: r_bore,
        r_gas,
        gas_at,
        seg,
        knurl,
    } = o;
    let len = z_breech - z_muzzle;
    let gas_at = gas_at.unwrap_or(z_muzzle + len * 0.34);

    let profile = [
        [0.0, 0.0],
        [0.0, r_chamber + 0.0018],
        [0.004, r_chamber + 0.0022],
        [0.02, r_chamber + 0.0022],
        [0.022, r_chamber],
        [len * 0.24, r_chamber],
        [len * 0.26, r_bore + 0.0012],
        [z_breech - gas_at - 0.012, r_bore + 0.0012],
        [z_breech - gas_at - 0.01, r_gas],
        [z_breech - gas_at + 0.012, r_gas],
        [z_breech - gas_at + 0.014, r_bore],
        [len - 0.014, r_bore],
        [len - 0.012, r_bore + 0.0009],
        [len - 0.001, r_bore + 0.0009],
        [len, r_bore * 0.72],
    ];
    // Authored from the breech forward; flip so +axial runs toward -Z.
    let g = lathe_z(&profile, seg, 0.0, std::f32::consts::TAU);
    asm.add(
        g,
        mat_steel,
        Some(Xform {
            y,
            z: z_breech,
            ry: std::f32::consts::PI,
            ..Default::default()
        }),
    );

    // Bore: a real dark tube so the crown does not read as a painted dot.
    let bore = tube_z(r_bore * 0.7, r_bore * 0.42, len * 0.5, 14, 0.0002);
    asm.add(
        bore,
        mat_cavity,
        Some(Xform {
            y,
            z: z_muzzle + len * 0.25,
            ..Default::default()
        }),
    );

    // Knurled section behind the muzzle threads.
    if knurl {
        let k = knurl_band(r_bore + 0.0006, 0.012, 26, 0.00035, 3);
        asm.add(
            k,
            mat_steel,
            Some(Xform {
                y,
                z: z_muzzle + 0.026,
                ..Default::default()
            }),
        );
    }

    BarrelResult { gas_at, r_bore }
}

/* -------------------------------------------------------------------------- */
/*  gas block                                                                 */
/* -------------------------------------------------------------------------- */

/// `o` on `addGasBlock(asm, matSteel, o)` (`parts.js:228-244`). `r_barrel`
/// mirrors the source's own option key (`o.rBarrel`), used here as the
/// barrel's actual radius (for the gas-tube offset), not renamed as
/// [`BarrelOpts::r_barrel`] is. `z`/`tube_to` have no JS default; `Default`
/// zeroes them purely so struct-update syntax works — every real caller sets
/// both.
#[derive(Clone, Copy, Debug)]
pub struct GasBlockOpts {
    pub y: f32,
    pub z: f32,
    pub r_barrel: f32,
    pub w: f32,
    pub h: f32,
    pub len: f32,
    pub tube_to: f32,
}

impl Default for GasBlockOpts {
    fn default() -> Self {
        GasBlockOpts {
            y: 0.0,
            z: 0.0,
            r_barrel: 0.0072,
            w: 0.021,
            h: 0.019,
            len: 0.026,
            tube_to: 0.0,
        }
    }
}

/// Gas block + gas tube. Low-profile block with two set screws and the tube
/// running back over the barrel into the receiver (`addGasBlock`,
/// `parts.js:228-244`).
pub fn add_gas_block(asm: &mut Assembly, mat_steel: &str, o: GasBlockOpts) {
    let GasBlockOpts {
        y,
        z,
        r_barrel: r,
        w,
        h,
        len,
        tube_to,
    } = o;

    let body_g = box_geo(w, h, len, 0.0008, 2);
    asm.add(
        body_g,
        mat_steel,
        Some(Xform {
            y: y - 0.0015,
            z,
            ..Default::default()
        }),
    );

    add_screw(asm, mat_steel, 0.0, y - h / 2.0 + 0.0015, z - 0.007, 0.0022, MountAxis::Y, 0.006);
    add_screw(asm, mat_steel, 0.0, y - h / 2.0 + 0.0015, z + 0.007, 0.0022, MountAxis::Y, 0.006);

    // gas tube back to the receiver
    let tube_len = tube_to - z;
    let t = tube_z(0.0026, 0.0014, tube_len.abs(), 10, 0.0002);
    asm.add(
        t,
        mat_steel,
        Some(Xform {
            y: y + r + 0.0052,
            z: z + tube_len / 2.0,
            ..Default::default()
        }),
    );
}

/* -------------------------------------------------------------------------- */
/*  muzzle devices                                                            */
/* -------------------------------------------------------------------------- */

/// `kind` on `addMuzzleDevice` (`parts.js:250`) — a bare string in the
/// source, indexing both `MUZZLE_LEN` and an `if`/`else if`/`else` geometry
/// chain that treats *any* string besides `'brake'`/`'a2'`/`'comp'` as the
/// tri-lug/flash-hider case. Every real call site passes one of exactly these
/// four strings (`MUZZLE_LEN`'s own keys, `parts::hardware::MUZZLE_LEN`), so
/// this enum is exhaustive over real usage: unlike the source, there is no
/// reachable "unknown kind" case that would fall through to `MUZZLE_LEN[kind]
/// ?? 0.05`, and this port does not model that dead fallback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MuzzleKind {
    Brake,
    A2,
    Comp,
    Trilug,
}

/// `addMuzzleDevice`'s return: `{ len, crownZ }` (`parts.js:380`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MuzzleDeviceResult {
    pub len: f32,
    pub crown_z: f32,
}

/// Muzzle devices. All of them get a real bore, a crush washer, chamfered
/// ports and a crowned exit — the muzzle is the part the player stares at
/// while firing (`addMuzzleDevice`, `parts.js:250-381`). `y` default `0.0`
/// (`parts.js:250`).
pub fn add_muzzle_device(
    asm: &mut Assembly,
    mat_steel: &str,
    mat_cavity: &str,
    kind: MuzzleKind,
    z_barrel_end: f32,
    r_barrel: f32,
    y: f32,
) -> MuzzleDeviceResult {
    let len = match kind {
        MuzzleKind::Brake => crate::weapons::parts::hardware::MUZZLE_LEN.brake,
        MuzzleKind::A2 => crate::weapons::parts::hardware::MUZZLE_LEN.a2,
        MuzzleKind::Comp => crate::weapons::parts::hardware::MUZZLE_LEN.comp,
        MuzzleKind::Trilug => crate::weapons::parts::hardware::MUZZLE_LEN.trilug,
    };
    let r_out = r_barrel + 0.0038;
    // The device threads onto the barrel, so its rear face sits at the
    // barrel end and the crown ends up `len` further forward.
    let z_crown = z_barrel_end - len;

    let mut parts: Vec<Geo> = Vec::new();
    match kind {
        MuzzleKind::Brake => {
            parts.push(lathe_z(
                &[
                    [0.0, r_barrel + 0.0012],
                    [0.006, r_barrel + 0.0022],
                    [0.008, r_out],
                    [len - 0.01, r_out],
                    [len - 0.008, r_out * 0.96],
                    [len - 0.002, r_out * 0.96],
                    [len, r_out * 0.8],
                    [len, r_barrel * 0.66],
                    [len - 0.006, r_barrel * 0.62],
                ],
                20,
                0.0,
                std::f32::consts::TAU,
            ));
            // Three pairs of side ports, chamfered, plus a top pair for
            // muzzle rise. The source builds a fresh `port` box each
            // iteration only to `.clone()` and `.dispose()` it straight
            // away (`g1 = port.clone(); ...; port.dispose()`) — a JS
            // idiom with no Rust counterpart, since `box_geo` already
            // owns the geometry it returns; this port just translates it
            // directly.
            (0u32..3).for_each(|i| {
                let z = 0.016 + i as f32 * 0.013;
                let mut g1 = box_geo(r_out * 2.4, 0.0055, 0.0072, 0.0006, 1);
                translate(&mut g1, 0.0, 0.0, z);
                parts.push(g1);
            });
        }
        MuzzleKind::A2 => {
            // A2 birdcage: closed bottom, five slots.
            parts.push(lathe_z(
                &[
                    [0.0, r_barrel + 0.001],
                    [0.005, r_barrel + 0.002],
                    [0.007, r_out * 0.92],
                    [0.012, r_out],
                    [len - 0.004, r_out],
                    [len, r_out * 0.86],
                    [len, r_barrel * 0.6],
                    [len - 0.005, r_barrel * 0.58],
                ],
                20,
                0.0,
                std::f32::consts::TAU,
            ));
            (0u32..5).for_each(|i| {
                let a = -std::f32::consts::PI * 0.44 + (i as f32 / 4.0) * std::f32::consts::PI * 0.88;
                let mut slot = box_geo(0.0032, 0.0075, 0.021, 0.0005, 1);
                translate(&mut slot, 0.0, r_out * 0.82, 0.0);
                rotate_z(&mut slot, a);
                translate(&mut slot, 0.0, 0.0, 0.03);
                parts.push(slot);
            });
        }
        MuzzleKind::Comp => {
            // Linear compensator / blast can.
            parts.push(lathe_z(
                &[
                    [0.0, r_barrel + 0.0012],
                    [0.005, r_barrel + 0.003],
                    [0.008, r_out + 0.0016],
                    [0.03, r_out + 0.0016],
                    [0.031, r_out + 0.0022],
                    [len - 0.003, r_out + 0.0022],
                    [len, r_out + 0.0006],
                    [len, r_barrel * 0.7],
                    [len - 0.007, r_barrel * 0.66],
                ],
                20,
                0.0,
                std::f32::consts::TAU,
            ));
            let mut k = knurl_band(r_out + 0.0018, 0.018, 30, 0.0003, 4);
            translate(&mut k, 0.0, 0.0, 0.018);
            parts.push(k);
        }
        MuzzleKind::Trilug => {
            // Tri-lug / flash hider for the SMG class.
            parts.push(lathe_z(
                &[
                    [0.0, r_barrel + 0.0014],
                    [0.004, r_barrel + 0.0026],
                    [0.006, r_out],
                    [0.024, r_out],
                    [0.026, r_out - 0.0012],
                    [len - 0.002, r_out - 0.0012],
                    [len, r_out - 0.003],
                    [len, r_barrel * 0.62],
                    [len - 0.005, r_barrel * 0.6],
                ],
                18,
                0.0,
                std::f32::consts::TAU,
            ));
            (0u32..3).for_each(|i| {
                let a = (i as f32 / 3.0) * std::f32::consts::TAU;
                let mut lug = box_geo(0.0042, 0.0038, 0.012, 0.0005, 1);
                translate(&mut lug, 0.0, r_out + 0.0012, 0.0);
                rotate_z(&mut lug, a);
                translate(&mut lug, 0.0, 0.0, 0.008);
                parts.push(lug);
            });
        }
    }

    let g = merge_all(parts).expect("every MuzzleKind branch pushes at least one part");
    // Authored breech-to-crown along +Z, so flip it onto the muzzle.
    asm.add(
        g,
        mat_steel,
        Some(Xform {
            y,
            z: z_crown + len,
            ry: std::f32::consts::PI,
            ..Default::default()
        }),
    );

    // Crush washer.
    let washer = lathe_z(
        &[
            [0.0, r_barrel + 0.0012],
            [0.0, r_barrel + 0.0032],
            [0.0018, r_barrel + 0.0032],
            [0.0018, r_barrel + 0.0012],
        ],
        16,
        0.0,
        std::f32::consts::TAU,
    );
    asm.add(
        washer,
        mat_steel,
        Some(Xform {
            y,
            z: z_crown + len,
            ..Default::default()
        }),
    );

    // The bore itself, and the dark expansion chamber behind it.
    let bore = tube_z(r_barrel * 0.66, r_barrel * 0.4, len * 0.9, 14, 0.0002);
    asm.add(
        bore,
        mat_cavity,
        Some(Xform {
            y,
            z: z_crown + len * 0.5,
            ..Default::default()
        }),
    );

    MuzzleDeviceResult { len, crown_z: z_crown }
}
