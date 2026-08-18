//! Ported from Claude-of-Duty `src/weapons/parts.js:1215-1636` — `buildOptic`,
//! the tube red-dot sight (T2 pattern) on a cantilever mount.
//!
//! Weapon-local space is `+X` right, `+Y` up, `-Z` toward the muzzle, origin
//! at the shooting hand's anchor — the convention `geometry.js:28-30`
//! documents and the `geometry` module (`03-weapon-geometry-api.md`) carries
//! forward. Optic space here is centred on `(0, 0, 0)`; the caller positions
//! it via `y`/`z`.
//!
//! This is app code (`apps/`), outside the Branchless Law — plain `if`/`for`
//! throughout. Rust has no default arguments, so every JS `?? value` default
//! is documented on [`OpticOpts`] and callers pass it explicitly.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use axiom_math::{Mat4, Vec3};

use crate::weapons::geometry::primitives::{box_geo, extrude, knurl_band, lathe_z, tube_z, ExtrudeOpts};
use crate::weapons::geometry::{merge_all, Assembly, Geo, Xform};
use crate::weapons::parts::hardware::{add_screw, MountAxis};

/// `BufferGeometry.translate(x, y, z)`, applied to a not-yet-added piece
/// (the turret's knurl band, each of the 12 click-mark dashes) before it
/// reaches `mergeAll`/`Assembly.add`. Reuses [`Geo::apply`] — the same
/// normal-matrix-correct transform `Assembly::add` uses.
fn translate(g: &mut Geo, x: f32, y: f32, z: f32) {
    g.apply(&Mat4::translation(Vec3::new(x, y, z)));
}

/// `BufferGeometry.rotateZ(angle)`, the other direct-geometry op the click-
/// mark loop needs. `angle` is `f64` and the rotation is built directly from
/// `f64`-computed `sin`/`cos` — matching `THREE.Matrix4.makeRotationZ`, which
/// takes a full-precision `f64` angle throughout — rather than truncating the
/// angle to `f32` before the trigonometry. Mirrors `parts::magazine`'s
/// `rotate_x`/`rotate_y`, which fixed a real second-weld tie-break mismatch
/// this same rounding-order issue caused.
fn rotate_z(g: &mut Geo, angle: f64) {
    let (s, c) = (angle.sin() as f32, angle.cos() as f32);
    let m = Mat4::from_cols_array([
        c, s, 0.0, 0.0, //
        -s, c, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, //
        0.0, 0.0, 0.0, 1.0, //
    ]);
    g.apply(&m);
}

/// `new THREE.RingGeometry(innerRadius, outerRadius, thetaSegments,
/// phiSegments)` (`three/src/geometries/RingGeometry.js`, MIT licensed,
/// Three.js authors), ported directly because [`build_optic`]'s inner-edge
/// reflection ring (`parts.js:1394`) calls the *raw Three primitive*, not
/// `geometry.js`'s own (differently shaped, toroidal-tube) `ring()`
/// primitive — the two are unrelated despite the shared name. `thetaStart`
/// is always `0` and `thetaLength` always `TAU` at this kit's one call site,
/// so those two Three parameters are not exposed.
fn ring_geometry(inner_radius: f64, outer_radius: f64, theta_segments: u32, phi_segments: u32) -> Geo {
    let theta_segments = theta_segments.max(3);
    let phi_segments = phi_segments.max(1);

    let mut pos = Vec::new();
    let mut normal = Vec::new();
    let mut uv = Vec::new();

    let radius_step = (outer_radius - inner_radius) / f64::from(phi_segments);
    (0..=phi_segments).for_each(|j| {
        let radius = inner_radius + radius_step * f64::from(j);
        (0..=theta_segments).for_each(|i| {
            let segment = f64::from(i) / f64::from(theta_segments) * std::f64::consts::TAU;
            let x = radius * segment.cos();
            let y = radius * segment.sin();
            pos.extend_from_slice(&[x as f32, y as f32, 0.0]);
            normal.extend_from_slice(&[0.0, 0.0, 1.0]);
            uv.extend_from_slice(&[((x / outer_radius + 1.0) / 2.0) as f32, ((y / outer_radius + 1.0) / 2.0) as f32]);
        });
    });

    let mut index = Vec::new();
    (0..phi_segments).for_each(|j| {
        let theta_segment_level = j * (theta_segments + 1);
        (0..theta_segments).for_each(|i| {
            let segment = i + theta_segment_level;
            let (a, b, c, d) = (segment, segment + theta_segments + 1, segment + theta_segments + 2, segment + 1);
            index.extend_from_slice(&[a, b, d]);
            index.extend_from_slice(&[b, c, d]);
        });
    });

    Geo { pos, normal, uv, index }
}

/// `new THREE.CircleGeometry(radius, segments)`
/// (`three/src/geometries/CircleGeometry.js`, MIT licensed, Three.js
/// authors), the tube vignette disc (`parts.js:1418`) — again the raw Three
/// primitive, not a `geometry.js` wrapper. `thetaStart`/`thetaLength` are
/// always `0`/`TAU` at this kit's one call site.
fn circle_geometry(radius: f64, segments: u32) -> Geo {
    let segments = segments.max(3);

    let mut pos = vec![0.0f32, 0.0, 0.0];
    let mut normal = vec![0.0f32, 0.0, 1.0];
    let mut uv = vec![0.5f32, 0.5];

    (0..=segments).for_each(|s| {
        let segment = f64::from(s) / f64::from(segments) * std::f64::consts::TAU;
        let x = radius * segment.cos();
        let y = radius * segment.sin();
        pos.extend_from_slice(&[x as f32, y as f32, 0.0]);
        normal.extend_from_slice(&[0.0, 0.0, 1.0]);
        uv.extend_from_slice(&[((x / radius + 1.0) / 2.0) as f32, ((y / radius + 1.0) / 2.0) as f32]);
    });

    let mut index = Vec::new();
    (1..=segments).for_each(|i| index.extend_from_slice(&[i, i + 1, 0]));

    Geo { pos, normal, uv, index }
}

/// `o` on `buildOptic(asm, o)` (`parts.js:1236-1242`). `rail_top` (`o.railTop`)
/// has no JS default — every real caller sets it; `Default` zeroes it only so
/// struct-update syntax works. `hood` is `o.hood ?? 0.009` (`parts.js:1524`).
#[derive(Clone, Copy, Debug)]
pub struct OpticOpts {
    pub r_tube: f32,
    pub len: f32,
    pub mat_body: &'static str,
    pub mat_steel: &'static str,
    pub y: f32,
    pub z: f32,
    pub rail_top: f32,
    pub hood: f32,
}

impl Default for OpticOpts {
    fn default() -> Self {
        OpticOpts {
            r_tube: 0.0155,
            len: 0.068,
            mat_body: "alu",
            mat_steel: "steel",
            y: 0.0,
            z: 0.0,
            rail_top: 0.0,
            hood: 0.009,
        }
    }
}

/// `buildOptic`'s return (`parts.js:1631-1637`): the reticle plane's local
/// position (so the rig can align it to screen centre in ADS), the aperture
/// radius for the vignette, the tube radius, and the overall length.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OpticResult {
    pub center: [f32; 3],
    pub lens_z: f32,
    pub aperture_r: f32,
    pub tube_r: f32,
    pub len: f32,
}

/// Tube red-dot sight (T2 pattern) on a cantilever mount. Built centred on
/// `(0, 0, 0)` in optic space; the caller positions it via `o.y`/`o.z`
/// (`buildOptic`, `parts.js:1215-1637`).
pub fn build_optic(asm: &mut Assembly, o: OpticOpts) -> OpticResult {
    let OpticOpts {
        r_tube,
        len,
        mat_body,
        mat_steel,
        y,
        z,
        rail_top,
        hood: hood_len,
    } = o;

    // SEGMENT BUDGET (`parts.js:1231-1234`). In ADS the objective ring is
    // ~250 px across and it is the single largest curve on screen, so it is
    // the one place in the whole game where a low-poly ring is COUNTABLE.
    // 56 segments would put the facet sagitta under the AA threshold; this
    // kit uses 72/80, comfortably past that floor. The interior rings matter
    // as much as the outer one, because a hard dark/light boundary shows
    // faceting far more readily than a shaded exterior does.
    const SEG: u32 = 72;
    const SEG_IN: u32 = 80;

    // THE APERTURE BUDGET (`parts.js:1236-1264`) — the whole reason the ADS
    // frame used to read as a drainpipe.
    //
    // Looking down a tube from a fixed eye point, the visible sight picture
    // is the SMALLER of two cones: the ocular bore subtended at the eye
    // relief, and the objective bore subtended at (relief + length). A
    // straight-bore tube loses that contest to the objective by a wide
    // margin and the sight picture shrinks to about a third of the housing.
    //
    // The fix is not a material and it is not a segment count: a real red
    // dot beats this by having an objective lens BIGGER than its exit
    // aperture, so the bore flares and the front of the housing carries an
    // objective bell:
    //   bore   12.2 mm radius at the ocular, flaring to 16.5 mm at the objective
    //   shell  15.5 mm radius at the ocular, belling to 19.0 mm at the objective
    // which lands both cones' subtended angles on nearly the same number —
    // the mark of a correctly stopped optical train, and the reason a real
    // sight has no visible second vignette.
    let r_bore_oc = r_tube * 0.787; // 12.2 mm on a 15.5 mm tube
    let r_bore_ob = r_tube * 1.065; // flared to 16.5 mm at the objective
    let r_bell_ob = r_tube * 1.226; // 19.0 mm objective bell
    let z_oc = len / 2.0;
    let z_ob = -len / 2.0;

    // Main tube: a straight section at the ocular, a conical flare, then the
    // objective bell. Every rim carries a chamfer face — the only thing on
    // the silhouette that can catch a specular line and say the rim has
    // thickness. The bell is deliberately SMALLER on screen than the ocular
    // rim, so it never breaks the housing's outer circle: from behind the
    // sight the silhouette is one clean ring, and from the side in hipfire
    // the bell is what makes the optic read as a red dot rather than a pipe.
    let tube = lathe_z(
        &[
            [z_ob, r_bore_ob * 0.995],
            [z_ob + 0.0004, r_bell_ob * 0.99],
            [z_ob, r_bell_ob * 1.008],
            [z_ob + 0.0022, r_bell_ob],
            [z_ob + 0.008, r_bell_ob * 0.995],
            [z_ob + 0.014, r_tube * 1.1],
            [z_ob + 0.022, r_tube * 1.01],
            [z_ob + 0.03, r_tube],
            [z_oc - 0.012, r_tube],
            [z_oc - 0.01, r_tube * 1.05],
            [z_oc - 0.002, r_tube * 1.05],
            [z_oc - 0.0003, r_tube * 1.02],
            [z_oc, r_tube * 0.995],
            [z_oc, r_bore_oc * 1.02],
        ],
        SEG,
        0.0,
        TAU,
    );
    asm.add(tube, mat_body, Some(Xform { y, z, ..Default::default() }));

    // Interior: a LIGHT TRAP, not a black hole — a CONE rather than a
    // cylinder, so the wall is seen at a much shallower angle than a
    // cylinder's would be and occupies a thin annulus instead of a wide
    // band. NO INTERNAL BAFFLE STEPS: shallow rings down the bore were
    // tried, on the theory each would shade the one behind it, but each
    // step's inner lip is an annulus facing the eye and they rendered as
    // concentric light-grey rings instead. The gradient has to come from
    // the wall material, not from geometry in the bore.
    let baffle = lathe_z(
        &[
            [z_ob + 0.001, r_bore_ob],
            [z_ob + 0.001, r_bore_ob * 0.985],
            [z_oc - 0.009, r_bore_oc * 0.985],
            [z_oc - 0.009, r_bore_oc],
        ],
        SEG_IN,
        0.0,
        TAU,
    );
    asm.add(baffle, "optic_tube", Some(Xform { y, z, ..Default::default() }));

    // The ocular clear aperture — everything downstream (vignette, edge
    // ring, reticle vignette) is derived from this one number.
    let lens_r = r_bore_oc * 0.99;

    // EYE-RELIEF RING (`parts.js:1358-1370`): a real sight has a black field
    // stop right behind the ocular lens. Without it the aperture edge is the
    // tube's own lit inner wall and the "glass" reads as a drilled hole.
    // Kept short (1.2 mm) — anything longer becomes another concentric ring.
    let relief = lathe_z(
        &[
            [0.0, lens_r * 0.998],
            [0.0012, lens_r * 1.012],
            [0.0034, r_bore_oc * 1.01],
            [0.0038, r_tube],
            [0.0038, r_bore_oc],
            [0.0, r_bore_oc],
        ],
        SEG_IN,
        0.0,
        TAU,
    );
    asm.add(relief, "optic_tube", Some(Xform { y, z: z + z_oc - 0.0045, ..Default::default() }));

    // Lens elements — AR-coated glass, both ends, slightly dished. The
    // coating's angle-dependent hue lives on the material. The objective
    // element is the big one, as it is on the real product.
    let lens_oc = lathe_z(&[[0.0, 0.0], [-0.0009, lens_r * 0.6], [-0.0014, lens_r]], SEG_IN, 0.0, TAU);
    let lens_ob = lathe_z(&[[0.0, 0.0], [-0.0012, r_bore_ob * 0.58], [-0.0019, r_bore_ob * 0.985]], SEG_IN, 0.0, TAU);
    asm.add(lens_ob, "glass", Some(Xform { y, z: z + z_ob + 0.0055, ..Default::default() }));
    asm.add(lens_oc, "glass", Some(Xform { y, z: z + z_oc - 0.007, ry: PI, ..Default::default() }));

    // INNER-EDGE REFLECTION RING (`parts.js:1373-1397`). The unmistakable
    // cue that a tube contains glass rather than air is a thin, very bright
    // arc a millimetre inside the objective rim — the inside of the bezel
    // reflected in the front surface of the lens. It is a property of the
    // LENS, so it is a thin additive ring sitting on the glass, not a bright
    // band painted onto the bezel — painting it on the bezel is exactly the
    // failure mode that produced a cream ring around the front lip. HAIRLINE
    // width, and on the ocular only: the objective's ring is behind two
    // lenses and a light trap, so it can only add haze.
    let edge = ring_geometry(f64::from(lens_r) * 0.965, f64::from(lens_r) * 0.99, SEG_IN, 1);
    asm.add(edge, "lens_ring", Some(Xform { y, z: z + z_oc - 0.0066, ..Default::default() }));

    // TUBE VIGNETTE: 6-8% darkening toward the rim of the exit pupil, from
    // the field stop and the tube wall eating the outer rays. A flat disc
    // with a radial alpha ramp (material-side) sitting just inside the
    // ocular glass.
    let vig = circle_geometry(f64::from(lens_r) * 0.995, SEG_IN);
    asm.add(vig, "lens_vig", Some(Xform { y, z: z + z_oc - 0.0085, ..Default::default() }));

    // Turrets: windage on the right, elevation on top, each a knurled cap
    // with an engraved click scale. The scale is real geometry in the
    // part's own local space rather than a projected decal, so it can never
    // swim as the viewmodel animates.
    let mut turret_knurl = knurl_band(0.0072, 0.0052, 26, 0.00032, 3);
    translate(&mut turret_knurl, 0.0, 0.0, 0.0102);
    let turret = merge_all(vec![
        lathe_z(
            &[
                [0.0, 0.0062],
                [0.004, 0.0075],
                [0.0075, 0.0075],
                [0.0085, 0.0068],
                [0.0125, 0.0068],
                [0.0128, 0.006],
                [0.0128, 0.0],
            ],
            32,
            0.0,
            TAU,
        ),
        turret_knurl,
    ])
    .expect("turret: lathe body + knurl band both always present");

    // Engraved click marks around the turret skirt: 12 short recessed
    // dashes and one long index, cut in the cavity material so each reads
    // as a dark line.
    let marks_parts: Vec<Geo> = (0u32..12)
        .map(|i| {
            let a = (f64::from(i) / 12.0) * std::f64::consts::TAU;
            let long = i == 0;
            let h: f32 = if long { 0.0026 } else { 0.0014 };
            let mut t = box_geo(0.00035, h, 0.0006, 0.00008, 1);
            rotate_z(&mut t, a);
            let offset = 0.0075 - f64::from(h) * 0.42;
            translate(&mut t, (a.cos() * offset) as f32, (a.sin() * offset) as f32, 0.0);
            t
        })
        .collect();
    let marks = merge_all(marks_parts).expect("marks: 12 click-dash parts always present");

    // Elevation on top (its local +Z ends up along +Y), windage on the
    // right (+X). The marks sit up each turret's own axis, on the skirt
    // below the knurl.
    let elev = Xform {
        y: y + r_tube * 0.9,
        z: z + 0.004,
        rx: -FRAC_PI_2,
        ..Default::default()
    };
    let wind = Xform {
        x: r_tube * 0.9,
        y,
        z: z + 0.004,
        ry: FRAC_PI_2,
        ..Default::default()
    };
    asm.add(turret.clone(), mat_body, Some(elev));
    asm.add(turret, mat_body, Some(wind));
    asm.add(marks.clone(), "cavity", Some(Xform { y: elev.y + 0.0055, ..elev }));
    asm.add(marks, "cavity", Some(Xform { x: wind.x + 0.0055, ..wind }));

    // Battery cap / brightness dial on the left.
    let dial = lathe_z(
        &[[0.0, 0.008], [0.005, 0.0092], [0.0125, 0.0092], [0.0128, 0.008], [0.0128, 0.0]],
        32,
        0.0,
        TAU,
    );
    asm.add(
        dial,
        mat_body,
        Some(Xform { x: -r_tube * 0.9, y, z: z - 0.006, ry: -FRAC_PI_2, ..Default::default() }),
    );
    let dial_knurl = knurl_band(0.0094, 0.006, 26, 0.00028, 3);
    asm.add(
        dial_knurl,
        mat_body,
        Some(Xform { x: -r_tube * 0.9 - 0.008, y, z: z - 0.006, ry: -FRAC_PI_2, ..Default::default() }),
    );

    // Mount (`parts.js:1497-1524`): a slim cantilever riser clamped to the
    // rail with two crossbolts. The riser is NARROW and waisted — a
    // full-width block under the tube is the single thing that makes a red
    // dot read as a plumbing fixture when you are looking straight down it
    // in ADS.
    //
    // `mount_top` is TANGENT to the tube's outer wall. It used to sit above
    // the floor of the tube bore, which in ADS put a lit grey slab clean
    // across the bottom third of the sight picture — the riser must never
    // enter the bore.
    let mount_top = y - r_tube;
    let mount_h = mount_top - rail_top;
    let base = extrude(
        &[
            [-0.0092, 0.0],
            [0.0092, 0.0],
            [0.0105, -0.0025],
            [0.0072, -f64::from(mount_h) * 0.45],
            [0.0072, -f64::from(mount_h) + 0.005],
            [0.013, -f64::from(mount_h) + 0.0018],
            [0.013, -f64::from(mount_h)],
            [-0.013, -f64::from(mount_h)],
            [-0.013, -f64::from(mount_h) + 0.0018],
            [-0.0072, -f64::from(mount_h) + 0.005],
            [-0.0072, -f64::from(mount_h) * 0.45],
            [-0.0105, -0.0025],
        ],
        0.03,
        ExtrudeOpts { bevel: 0.0008, ..Default::default() },
    );
    asm.add(base, mat_body, Some(Xform { y: mount_top, z: z + 0.002, ..Default::default() }));

    // ring clamp around the tube
    let clamp = lathe_z(
        &[[0.0, r_tube], [0.0, r_tube + 0.0035], [0.0055, r_tube + 0.0035], [0.0055, r_tube]],
        SEG,
        0.0,
        TAU,
    );
    asm.add(clamp.clone(), mat_body, Some(Xform { y, z: z - 0.014, ..Default::default() }));
    asm.add(clamp, mat_body, Some(Xform { y, z: z + 0.012, ..Default::default() }));
    [z - 0.0115, z + 0.0145]
        .into_iter()
        .for_each(|cz| add_screw(asm, mat_steel, 0.0135, mount_top - 0.004, cz, 0.0028, MountAxis::X, 0.01));

    // recoil lug + rail clamp bolts
    let clamp_bar = box_geo(0.032, 0.006, 0.03, 0.0008, 1);
    asm.add(clamp_bar, mat_body, Some(Xform { y: rail_top + 0.001, z: z + 0.002, ..Default::default() }));
    add_screw(asm, mat_steel, 0.0165, rail_top + 0.001, z - 0.008, 0.003, MountAxis::X, 0.012);
    add_screw(asm, mat_steel, 0.0165, rail_top + 0.001, z + 0.012, 0.003, MountAxis::X, 0.012);

    // RUBBER EYEPIECE BEZEL (`parts.js:1554-1580`) — the fix for a cream
    // ring measured at screen radius 225-262 px in ADS: the tube's own rear
    // rim chamfer and outer flank, nearly edge-on to the eye, sitting right
    // in the reflection path of the viewmodel's warm rim light. Clamping the
    // aluminium material's specular alone was not enough — as long as an
    // ALUMINIUM surface is what the eye is looking at, something lights up
    // at grazing incidence. So the rear of the sight stops being aluminium
    // at all: a rubber bezel covers the bore lip, the rear annulus, the rim
    // chamfer, and wraps down the outside of the flank past the widest point
    // of the housing, so the entire outer circle of the optic in ADS is
    // moulded rubber — which is also where a real sight's rubber bumper is.
    // `rubber` rather than `cavity`: cavity is unlit and reads as a hole;
    // moulded rubber takes the mask bake and a faint shading gradient, so
    // the bezel reads as a surface.
    let cup = lathe_z(
        &[
            [0.0, r_bore_oc * 0.995],
            [0.0004, r_bore_oc * 1.03],
            [0.0009, r_tube * 1.02],
            [0.0018, r_tube * 1.075],
            [0.0055, r_tube * 1.1],
            [0.0072, r_tube * 1.09],
            [-0.0042, r_tube * 1.085],
            [-0.0048, r_tube * 1.03],
        ],
        SEG,
        0.0,
        TAU,
    );
    asm.add(cup, "rubber", Some(Xform { y, z: z + z_oc - 0.0012, ..Default::default() }));

    // Objective shade. It rides on the bell now, so it is wider than the
    // tube and, like the bell, still projects inside the ocular rim in ADS —
    // it can never break the housing silhouette. The inside is the
    // light-trap material for the same reason the bore is: a near-
    // cylindrical anodised wall pointed at the sky is the other place the
    // cream ring used to come from.
    let hood = lathe_z(
        &[
            [0.0, r_bell_ob],
            [0.0, r_bell_ob * 1.05],
            [hood_len - 0.0003, r_bell_ob * 1.05],
            [hood_len, r_bell_ob * 1.035],
            [hood_len, r_bell_ob * 0.99],
        ],
        SEG,
        0.0,
        TAU,
    );
    asm.add(hood, mat_body, Some(Xform { y, z: z + z_ob - hood_len + 0.0015, ..Default::default() }));
    let hood_liner = tube_z(r_bell_ob * 1.035, r_bell_ob * 0.998, hood_len - 0.0008, SEG, 0.0002);
    asm.add(hood_liner, "optic_tube", Some(Xform { y, z: z + z_ob - hood_len * 0.5 + 0.0015, ..Default::default() }));

    // A rubber bumper on the objective rim too — same argument as the
    // eyepiece, and it is the part of the optic that faces the camera in
    // hipfire.
    let ob_bumper = lathe_z(
        &[
            [0.0, r_bell_ob * 1.01],
            [0.0006, r_bell_ob * 1.075],
            [0.0038, r_bell_ob * 1.08],
            [0.005, r_bell_ob * 1.03],
        ],
        SEG,
        0.0,
        TAU,
    );
    asm.add(ob_bumper, "rubber", Some(Xform { y, z: z + z_ob - hood_len - 0.0035, ..Default::default() }));

    OpticResult {
        center: [0.0, y, z],
        lens_z: z + z_oc - 0.007,
        // The exit pupil the reticle vignettes against is the ocular clear aperture.
        aperture_r: lens_r * 0.94,
        tube_r: r_tube,
        len,
    }
}
