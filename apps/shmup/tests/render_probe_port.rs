//! `render/probe.js` — the renderer validation blockout.
//!
//! No golden capture: this port dropped the JavaScript captures deliberately
//! (see the commit that removed them), so these test the *properties* the
//! source's own comments claim, and the two stream-order rules that a natural
//! transcription gets wrong. Those two are the whole risk in this file.

use axiom_shmup::render::probe::{
    build_scene, fbm, footprint_clear, make_surface, ProbeMaps, SurfaceOpts, GROUND_SIZE,
    SHOT_KEEPOUT, SPHERE_RADIUS,
};
use axiom_shmup::rng::Rng;

/// The source's three surfaces, in the order `build()` bakes them
/// (`probe.js:157-180`). Order is load-bearing: each spends 256 draws.
fn concrete() -> SurfaceOpts {
    SurfaceOpts {
        base: [0.42, 0.41, 0.39],
        rough: 0.82,
        rough_var: 0.3,
        cracks: true,
        bump: 2.0,
        scale: 5.0,
        ..SurfaceOpts::default()
    }
}

fn asphalt() -> SurfaceOpts {
    SurfaceOpts {
        base: [0.16, 0.16, 0.17],
        rough: 0.62,
        rough_var: 0.42,
        bump: 3.0,
        scale: 9.0,
        ..SurfaceOpts::default()
    }
}

/// **fBm is bounded to `[0, 1]` and is not constant.**
///
/// It is a normalised sum of hashes that are each `perm[..] / 255`, so it
/// cannot leave the unit range however the octaves stack. A constant result
/// would mean the permutation lookup collapsed — which is exactly what a
/// mis-masked negative index produces.
#[test]
fn fbm_stays_in_the_unit_range_and_varies() {
    let mut rng = Rng::new(9);
    let mut perm = [0_u8; 256];
    perm.iter_mut()
        .for_each(|p| *p = u8::try_from(rng.int(0, 255)).expect("in range"));

    let samples: Vec<f64> = (0..400)
        .map(|i| {
            let t = f64::from(i) * 0.37;
            fbm(t, t * 0.61, &perm)
        })
        .collect();
    let lo = samples.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = samples.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    assert!(lo >= 0.0 && hi <= 1.0, "fbm left [0,1]: {lo}..{hi}");
    assert!(hi - lo > 0.1, "fbm is nearly constant: {lo}..{hi}");
}

/// **Negative coordinates are as alive as positive ones.**
///
/// `Math.floor` goes negative left of the origin and the source relies on
/// `-1 & 255 == 255` to wrap the lookup. A port that clamped instead — or that
/// used a Rust `%` on a negative index — would flatten the whole `x < 0` half
/// of every surface, and the ground plane is 120 m wide and centred, so half
/// the probe scene would be plain.
#[test]
fn the_permutation_lookup_wraps_on_negative_coordinates() {
    let mut rng = Rng::new(4);
    let mut perm = [0_u8; 256];
    perm.iter_mut()
        .for_each(|p| *p = u8::try_from(rng.int(0, 255)).expect("in range"));

    let negative: Vec<f64> = (1..120)
        .map(|i| fbm(-f64::from(i) * 0.41, -f64::from(i) * 0.23, &perm))
        .collect();
    let lo = negative.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = negative.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    assert!(lo >= 0.0 && hi <= 1.0, "negative side left [0,1]: {lo}..{hi}");
    assert!(
        hi - lo > 0.1,
        "the negative half is flat, so the index did not wrap: {lo}..{hi}"
    );
}

/// **A surface spends exactly 256 draws on its permutation table**, before it
/// reads a single texel.
///
/// Pinned because the bake order is part of the level: `build()` makes
/// concrete, then asphalt, then rust metal, and the placements that follow are
/// 768 draws downstream of where they would otherwise be.
#[test]
fn a_surface_bake_costs_two_hundred_and_fifty_six_draws() {
    let mut counted = Rng::new(17);
    let _ = make_surface(&mut counted, 8, concrete());

    let mut manual = Rng::new(17);
    (0..256).for_each(|_| {
        let _ = manual.int(0, 255);
    });
    assert_eq!(
        counted.float(),
        manual.float(),
        "a bake must consume 256 draws and no more — the texel loops draw nothing"
    );
}

/// **The maps are RGBA8, square, and opaque**, and the ORM map puts roughness
/// in **G** with metalness an exact zero in B.
///
/// The channel assignment is the thing worth pinning: three reads one texture
/// as both `roughnessMap` and `metalnessMap`, so a port that wrote roughness
/// into R would silently make everything metal.
#[test]
fn the_orm_map_carries_roughness_in_green_and_no_metal() {
    let mut rng = Rng::new(3);
    let maps: ProbeMaps = make_surface(&mut rng, 16, asphalt());
    let texels = 16 * 16;
    assert_eq!(maps.albedo.len(), texels * 4);
    assert_eq!(maps.normal.len(), texels * 4);
    assert_eq!(maps.orm.len(), texels * 4);

    (0..texels).for_each(|i| {
        assert_eq!(maps.orm[i * 4], 255, "occlusion is a constant 1");
        assert_eq!(maps.orm[i * 4 + 2], 0, "metalness must be zero");
        assert_eq!(maps.orm[i * 4 + 3], 255);
        assert_eq!(maps.albedo[i * 4 + 3], 255);
        assert_eq!(maps.normal[i * 4 + 3], 255);
    });
    // Roughness varies around the authored 0.62 with the height.
    let g: Vec<u8> = (0..texels).map(|i| maps.orm[i * 4 + 1]).collect();
    assert!(
        g.iter().copied().max() > g.iter().copied().min(),
        "roughness must track the height field, not sit flat"
    );
}

/// **The normal map's z lane is biased low by half a code value**, because the
/// source writes it `(1 / len) * 0.5 * 255 + 127` while x and y encode as
/// `(v * 0.5 + 0.5) * 255`, which is `… + 127.5`.
///
/// A flat texel — where the height gradient is zero, so `len == 1` — therefore
/// lands on **254**, not 255. Asserted because it is exactly the kind of
/// asymmetry a tidy-up removes without anyone noticing, and it would shift
/// every normal in every map this bakes.
#[test]
fn the_normal_z_lane_keeps_the_sources_half_step_bias() {
    // A zero-relief surface: `bump = 0` kills the gradient, so `len` is exactly
    // 1 at every texel and z is the encode's fixed point.
    let mut rng = Rng::new(11);
    let flat = make_surface(
        &mut rng,
        8,
        SurfaceOpts {
            bump: 0.0,
            ..asphalt()
        },
    );
    (0..8 * 8).for_each(|i| {
        assert_eq!(flat.normal[i * 4], 127, "x centres at 127.5, truncating to 127");
        assert_eq!(flat.normal[i * 4 + 1], 127, "y likewise");
        assert_eq!(
            flat.normal[i * 4 + 2],
            254,
            "z is `0.5 * 255 + 127` = 254.5 -> 254, NOT the 255 a symmetric \
             encode would give"
        );
    });
}

/// **Every shot camera is left standing in clear air.**
///
/// The keep-out list is why the probe is usable as a capture target at all: a
/// block spawned on a camera fills the frame with a wall, and the capture reads
/// as a broken renderer rather than as a bad seed.
#[test]
fn no_block_or_crate_encloses_a_shot_camera() {
    (0..40_u64).for_each(|seed| {
        let mut rng = Rng::new(seed as u32);
        let scene = build_scene(&mut rng);
        scene.blocks.iter().for_each(|b| {
            assert!(
                footprint_clear(b.position[0], b.position[2], b.scale[0] / 2.0, b.scale[2] / 2.0, 1.5),
                "seed {seed}: a block encloses a shot camera at {:?}",
                b.position
            );
        });
        scene.crates.iter().for_each(|c| {
            assert!(
                footprint_clear(c.position[0], c.position[2], c.scale[0] * 0.71, c.scale[0] * 0.71, 0.9),
                "seed {seed}: a crate encloses a shot camera at {:?}",
                c.position
            );
        });
    });
}

/// **A rejected block costs four draws; a rejected crate costs five.**
///
/// The two loops reject on opposite sides of their last draw — the block's
/// `continue` fires before the yaw, the crate's after everything — and this is
/// the one property of this file that a careful, natural transcription still
/// gets wrong. If either is wrong the stream desynchronises and every value
/// after it in the level differs.
///
/// Measured against the arithmetic rather than a captured number. A block
/// draws five before its reject check (w, h, d, the z jitter, x) and a sixth,
/// the yaw, only if it survives; a crate draws five and rejects after all of
/// them. So the total is `14 * 5 + accepted_blocks + 22 * 5`.
#[test]
fn a_rejected_block_and_a_rejected_crate_cost_the_stream_differently() {
    (0..24_u64).for_each(|seed| {
        let mut rng = Rng::new(seed as u32);
        let scene = build_scene(&mut rng);
        let after_scene = rng.float();

        let expected_draws = 14 * 5 + scene.blocks.len() + 22 * 5;
        let mut manual = Rng::new(seed as u32);
        (0..expected_draws).for_each(|_| {
            let _ = manual.float();
        });
        assert_eq!(
            after_scene,
            manual.float(),
            "seed {seed}: the scene consumed the wrong number of draws — \
             {} blocks accepted of 14, so it should be {expected_draws}",
            scene.blocks.len()
        );
    });
}

/// **The blockout is a street: blocks alternate sides and march down `-z`.**
///
/// `side = i % 2 === 0 ? -1 : 1` with `x = side * range(9, 13)`, so a block is
/// never nearer the centreline than 9 m and the two rows face each other. If
/// the sign convention inverted, the "street" would be one row of fourteen.
#[test]
fn the_blocks_form_two_rows_facing_a_street() {
    let mut rng = Rng::new(5);
    let scene = build_scene(&mut rng);
    assert!(!scene.blocks.is_empty(), "some blocks must survive the keep-out");
    let left = scene.blocks.iter().filter(|b| b.position[0] < 0.0).count();
    let right = scene.blocks.len() - left;
    assert!(left > 0 && right > 0, "both sides of the street must be built");
    scene.blocks.iter().for_each(|b| {
        assert!(
            b.position[0].abs() >= 9.0,
            "a block stands in the road at x = {}",
            b.position[0]
        );
        // The source sets `position.y = h / 2`, so a box of full height `h`
        // rests ON the ground rather than sinking half into it.
        assert!(
            (b.position[1] - b.scale[1] / 2.0).abs() < 1e-9,
            "a block is not resting on the ground"
        );
    });
}

/// **The four spheres walk the roughness range and draw no RNG.**
///
/// They exist so a reflection pass can be judged from mirror to near-diffuse in
/// one frame, which only works if they are the same four every run.
#[test]
fn the_spheres_are_a_fixed_roughness_ramp() {
    let a = build_scene(&mut Rng::new(1));
    let b = build_scene(&mut Rng::new(9_999));
    assert_eq!(a.spheres, b.spheres, "the ramp must not depend on the seed");
    assert_eq!(a.lamps, b.lamps, "nor the lamps");

    let rough: Vec<f64> = a.spheres.iter().map(|s| s.roughness).collect();
    assert_eq!(rough.len(), 4);
    rough.windows(2).for_each(|w| {
        assert!(w[1] > w[0], "roughness must increase across the row");
    });
    assert!(rough[0] < 0.1, "the first sphere is a mirror: {}", rough[0]);
    assert!(rough[3] > 0.4, "the last is near-diffuse: {}", rough[3]);
    a.spheres.iter().for_each(|s| {
        assert!(
            (s.position[1] - SPHERE_RADIUS).abs() < 1e-9,
            "a sphere is not resting on the ground"
        );
    });
}

/// The keep-out list and the ground agree: every shot camera stands **on** the
/// plate it is meant to be looking across.
#[test]
fn every_shot_camera_stands_on_the_ground_plate() {
    let half = GROUND_SIZE / 2.0;
    SHOT_KEEPOUT.iter().for_each(|k| {
        assert!(
            k[0].abs() < half && k[1].abs() < half,
            "shot camera {k:?} is off the {GROUND_SIZE} m ground plate"
        );
    });
}
