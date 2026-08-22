//! The CPU reference's own proof, and the source facts it pins.
//!
//! Two kinds of assertion live here, deliberately:
//!
//! 1. **A second transcription.** Where a number is asserted, the expected value
//!    is re-derived in the test from the GLSL text in `materialpatch.js`,
//!    longhand, with its own local `mix`/`smoothstep` — never by calling the
//!    module's helpers. That is the only check the port has against a
//!    misreading, and this port has measured what happens without it (ten
//!    defects in `sky/`, where one reading produced both sides).
//! 2. **Properties the algorithm must have** — a dark albedo occluding more
//!    than a bright one, the wall-skin feather separating a facade's two faces,
//!    a disabled feature being the exact identity. A shared misreading can
//!    survive a matching number; it rarely survives a property.
//!
//! Tests are exempt from the Branchless Law and keep their control flow. No
//! console-output or debug-print macros anywhere, tests included — the
//! architecture checker rejects them even under `#[cfg(test)]`, so every figure
//! rides an assertion message instead.

use super::{
    contact_shadow, direct_light, indirect, interior_gate, multi_bounce, sample_ao,
    specular_occlusion, ssr_blend, sun_bounce, IndirectIn, IndirectUniforms, AO_STRENGTH,
    FILL_DIR, INDIRECT_LIGHTING_WGSL, MAX_ROOMS, MICRO_SHADOW_FRACTION,
};

/// GLSL `mix(x, y, a) = x⋅(1−a)+y⋅a`, transcribed again from the spec text.
fn t_mix(x: f32, y: f32, a: f32) -> f32 {
    x * (1.0 - a) + y * a
}

/// GLSL `smoothstep(e0, e1, x)`, transcribed again from the spec text.
fn t_smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).max(0.0).min(1.0);
    t * t * (3.0 - 2.0 * t)
}

/// How close two `f32` results of the same arithmetic must land. Both sides are
/// evaluated on this CPU, so this is rounding-order slack only, not hardware
/// slack — the adapter proof carries the hardware number.
const EPS: f32 = 1.0e-6;

fn close(actual: f32, expected: f32, what: &str) {
    assert!(
        (actual - expected).abs() <= EPS,
        "{what}: got {actual}, expected {expected}, delta {}",
        (actual - expected).abs()
    );
}

// ---------------------------------------------------------------------------
// The source facts that read as uniforms and are not.
// ---------------------------------------------------------------------------

/// Grepped across the whole of `C:/dev/Claude-of-Duty/src`: nothing writes
/// `owAoStrength` after the constructor and nothing writes `owFillDir` at all.
/// If a future frame-graph slice starts driving either, this test is where the
/// claim in the module header stops being true.
#[test]
fn the_never_written_uniforms_are_the_values_every_frame_of_the_original_runs() {
    assert_eq!(AO_STRENGTH, [1.0, 0.6], "owAoStrength's constructor value");
    assert_eq!(
        FILL_DIR,
        [-0.95, 0.85, -0.05, 0.7],
        "owFillDir's constructor value"
    );
    // So the direct light's micro-shadow fraction is fixed at 1.0 * 0.35.
    assert_eq!(MICRO_SHADOW_FRACTION, 0.35);
    assert_eq!(
        AO_STRENGTH[0] * MICRO_SHADOW_FRACTION,
        0.35,
        "the micro-shadow the direct light actually receives"
    );
    // `#define OW_ROOMS ${MAX_ROOMS}` — the GLSL's loop bound is this constant.
    assert_eq!(MAX_ROOMS, 10);
}

/// `MaterialPatcher`'s constructor state, which is also the state every frame
/// runs in on a tier with `gtao`/`ssr` off. Every feature disabled, no rooms,
/// no fill, and both bands black.
#[test]
fn the_shipped_uniform_block_is_the_constructors_own_values() {
    let u = IndirectUniforms::shipped();
    assert_eq!(u.feat, [0.0, 0.0, 0.0, 1.0], "owFeat");
    assert_eq!(u.sky_fill, [0.0; 3], "owSkyFill");
    assert_eq!(u.ground_fill, [0.0; 3], "owGroundFill");
    assert_eq!(u.fill_gain, [1.0, 1.0], "owFillGain");
    assert_eq!(u.indirect, [1.0, 1.0, 0.0, 0.0], "owIndirect");
    assert_eq!(u.room_xf, [1.0, 0.0, 0.0, 0.0], "owRoomXf");
    assert_eq!(u.rooms, [[0.0; 4]; MAX_ROOMS], "owRooms");
    assert_eq!(u.rooms_y, [[0.0; 4]; MAX_ROOMS], "owRoomsY");
    assert_eq!(u.sun_dir_world, [0.0, 1.0, 0.0], "csm's owSunDirWorld");
    // The struct is comparable and printable, which is what lets a failure name
    // the block it disagreed about.
    assert_eq!(u, IndirectUniforms::shipped());
    assert!(format!("{u:?}").contains("IndirectUniforms"));
}

// ---------------------------------------------------------------------------
// owSampleAO
// ---------------------------------------------------------------------------

#[test]
fn sample_ao_is_the_exact_identity_with_the_feature_off() {
    // `if ( owFeat.x < 0.5 ) return 1.0;` — and the texel is not consulted.
    assert_eq!(sample_ao(0.0, 0.0, 1.0), 1.0);
    assert_eq!(sample_ao(0.49, 0.0, 1.0), 1.0);
    // The boundary belongs to the ENABLED arm: `< 0.5` returns early, `>= 0.5`
    // does not.
    assert_eq!(sample_ao(0.5, 1.0, 1.0), 1.0);
    assert!(sample_ao(0.5, 0.0, 1.0) < 1.0, "0.5 must sample, not skip");
}

#[test]
fn sample_ao_floors_visibility_at_a_quarter_and_lerps_by_strength() {
    // `mix( 1.0, max( ao, 0.25 ), owAoStrength.x )` at full strength.
    close(sample_ao(1.0, 0.4, 1.0), t_mix(1.0, 0.4, 1.0), "full strength");
    // The floor: a crevice reading 0.05 still returns a quarter of the light,
    // because a visibility term that reaches 0 is a dark halo, not occlusion.
    close(sample_ao(1.0, 0.05, 1.0), 0.25, "the 0.25 floor");
    close(sample_ao(1.0, 0.0, 1.0), 0.25, "the 0.25 floor at zero");
    // Partial strength lerps toward 1, so strength 0 disables occlusion rather
    // than blacking it out — the same shape `material_shader::masks` found for
    // the material's own `aoStrength`.
    close(sample_ao(1.0, 0.5, 0.6), t_mix(1.0, 0.5, 0.6), "0.6 strength");
    assert_eq!(sample_ao(1.0, 0.0, 0.0), 1.0, "strength 0 is no occlusion");
}

// ---------------------------------------------------------------------------
// owContactShadow
// ---------------------------------------------------------------------------

#[test]
fn the_contact_ray_reaches_the_sun_and_nothing_else() {
    // Feature off.
    assert_eq!(contact_shadow(0.0, 1.0, 0.2), 1.0);
    // On, but this light is not the sun: `dot( lightDirView, owSunDirView ) < 0.999`.
    assert_eq!(contact_shadow(1.0, 0.998, 0.2), 1.0);
    // On, and this light IS the sun.
    assert_eq!(contact_shadow(1.0, 0.999, 0.2), 0.2);
    assert_eq!(contact_shadow(1.0, 1.0, 0.2), 0.2);
    // Both conditions are required, not either.
    assert_eq!(contact_shadow(0.0, 1.0, 0.0), 1.0);
    assert_eq!(contact_shadow(1.0, 0.0, 0.0), 1.0);
}

// ---------------------------------------------------------------------------
// owMultiBounce
// ---------------------------------------------------------------------------

#[test]
fn multi_bounce_is_the_exact_identity_at_full_visibility() {
    // The fit evaluates to ~0.9998 at ao = 1, and the source's own lower clamp
    // — `vec3( ao )` — pulls it back to exactly 1. That is why the `owAo < 1.0`
    // guard costs nothing semantically.
    for albedo in [0.0_f32, 0.18, 0.5, 0.8, 1.0] {
        let out = multi_bounce(1.0, [albedo; 3]);
        assert_eq!(out, [1.0; 3], "albedo {albedo} at ao 1");
    }
}

#[test]
fn a_dark_albedo_occludes_more_than_a_bright_one() {
    let dark = multi_bounce(0.5, [0.0; 3]);
    let mid = multi_bounce(0.5, [0.5; 3]);
    let bright = multi_bounce(0.5, [1.0; 3]);
    assert!(
        dark[0] < mid[0] && mid[0] < bright[0],
        "monotone in albedo: dark {}, mid {}, bright {}",
        dark[0],
        mid[0],
        bright[0]
    );
    // Longhand, from the GLSL text, for the white case.
    let a = 2.0404_f32 * 1.0 - 0.3324;
    let b = -4.7951_f32 * 1.0 + 0.6417;
    let c = 2.7552_f32 * 1.0 + 0.6903;
    let ao = 0.5_f32;
    let expected = (ao * (ao * (ao * a + b) + c)).max(ao).min(1.0);
    close(bright[0], expected, "white albedo at ao 0.5");
    // A black albedo bounces nothing, so the fit falls below the raw visibility
    // and the source's lower clamp catches it at exactly `ao`.
    assert_eq!(dark, [0.5; 3], "black albedo clamps to the raw visibility");
}

#[test]
fn multi_bounce_runs_per_channel_so_a_tinted_albedo_occludes_per_channel() {
    let out = multi_bounce(0.5, [0.0, 0.5, 1.0]);
    assert!(
        out[0] < out[1] && out[1] < out[2],
        "per-channel: {out:?} must be ordered like its albedo"
    );
}

// ---------------------------------------------------------------------------
// owSpecularOcclusion
// ---------------------------------------------------------------------------

#[test]
fn a_rougher_surface_gathers_from_a_wider_cone_and_sees_more_occlusion() {
    // rough 0 -> exponent 1 -> the visibility itself.
    close(specular_occlusion(0.5, 0.0), 0.5, "mirror");
    // rough 1 -> exponent 3.
    close(specular_occlusion(0.5, 1.0), 0.5_f32.powf(3.0), "fully rough");
    assert!(
        specular_occlusion(0.5, 1.0) < specular_occlusion(0.5, 0.0),
        "rougher must occlude more"
    );
    // Negative visibility is floored before the pow, so the term is finite for
    // any buffer content.
    assert_eq!(specular_occlusion(-1.0, 0.5), 0.0);
    assert_eq!(specular_occlusion(1.0, 0.5), 1.0);
}

// ---------------------------------------------------------------------------
// owSunBounce
// ---------------------------------------------------------------------------

#[test]
fn the_warm_bounce_comes_from_the_anti_sun_hemisphere_and_wraps_tightly() {
    // Sun overhead: the anti-sun vector is (~0, 1, ~0) after the 1e-4 nudge and
    // the normalize, so an up-facing normal takes the full term.
    let sun_up = [0.0, 1.0, 0.0];
    close(sun_bounce([0.0, 1.0, 0.0], sun_up), 1.0, "facing the band");
    // A down-facing normal is `(-1 + 0.12) / 1.12`, which clamps to zero.
    assert_eq!(sun_bounce([0.0, -1.0, 0.0], sun_up), 0.0, "facing away");
    // The wrap is 0.12, not 0.35: a normal perpendicular to the band gets
    // 0.12/1.12, a tenth of the term, not a third.
    let perpendicular = sun_bounce([1.0, 0.0, 0.0], sun_up);
    close(perpendicular, 0.12 / 1.12, "the tight 0.12 wrap");
    assert!(
        perpendicular < 0.35 / 1.35,
        "a 0.35 wrap would give {}, this gives {perpendicular}",
        0.35 / 1.35
    );
}

#[test]
fn the_anti_sun_vector_is_the_low_sun_mirrored_across_the_street() {
    // A low sun coming from +x: the bounce band sits toward -x and 0.28 up.
    let sun = [0.9, 0.2, 0.0];
    let raw = [-0.9_f32 + 1e-4, 0.28 + 1e-4, 0.0 + 1e-4];
    let len = (raw[0] * raw[0] + raw[1] * raw[1] + raw[2] * raw[2]).sqrt();
    let anti = [raw[0] / len, raw[1] / len, raw[2] / len];
    // A wall facing the shaded side receives the bounce.
    let toward = sun_bounce(anti, sun);
    close(toward, 1.0, "a normal along the band takes the full term");
    // A wall facing the sunlit side receives none of it, which is the whole
    // point of the tight wrap.
    let away = sun_bounce([-anti[0], -anti[1], -anti[2]], sun);
    assert_eq!(away, 0.0, "the sunlit-facing wall gets no street bounce");
    // The `y` component is a fixed 0.28 lift, NOT the sun's own elevation
    // mirrored: the street is below, so the band tilts up regardless.
    assert!(anti[1] > 0.0, "the band is above the horizon: {}", anti[1]);
}

// ---------------------------------------------------------------------------
// owInteriorGate
// ---------------------------------------------------------------------------

/// A single 10 x 10 m room centred on the level origin, floor slab at -0.8 and
/// roof deck at 6.0 — the shape `RenderSystem._updateRooms` publishes from a
/// building spec's footprint.
fn one_room(indirect_floor: f32) -> IndirectUniforms {
    let mut u = IndirectUniforms::shipped();
    u.indirect = [1.0, indirect_floor, 1.0, 0.0];
    u.rooms[0] = [0.0, 0.0, 5.0, 5.0];
    u.rooms_y[0] = [-0.8, 6.0, 0.0, 0.0];
    u
}

#[test]
fn with_no_live_rooms_the_gate_is_the_ao_arm_alone() {
    let u = IndirectUniforms::shipped();
    // Fully visible, no rooms: full skylight, exactly.
    assert_eq!(interior_gate([0.0, 1.5, 0.0], 1.0, &u), 1.0);
    // The AO arm still bites in a pocket the room list does not know about —
    // arcades, stairwells, under-awning stalls. `owIndirect.y` is 1 here, so
    // the gate is still 1; the arm is exercised through a lowered floor below.
    let mut dim = u;
    dim.indirect[1] = 0.2;
    let pocket = interior_gate([0.0, 1.5, 0.0], 0.4, &dim);
    let ao_gate = t_mix(1.0, t_smoothstep(0.45, 0.98, 0.4), 0.6);
    close(pocket, t_mix(0.2, 1.0, ao_gate.min(1.0)), "the ao arm");
    assert!(pocket < 1.0, "an occluded pocket loses skylight: {pocket}");
}

#[test]
fn deep_inside_a_room_the_gate_falls_to_the_indirect_floor() {
    let u = one_room(0.15);
    // 2.3 m below the roof deck and 5 m from every wall: fully interior.
    close(interior_gate([0.0, 1.5, 0.0], 1.0, &u), 0.15, "room centre");
}

#[test]
fn the_feather_separates_the_two_faces_of_one_wall() {
    let u = one_room(0.15);
    // The facade's OUTER skin sits exactly on the footprint boundary: depth 0,
    // so `smoothstep( 0.06, 0.30, 0 )` is 0 and it keeps full skylight.
    close(interior_gate([5.0, 1.5, 0.0], 1.0, &u), 1.0, "outer skin");
    // Its INNER skin, one 35 cm wall thickness in, is past the 30 cm feather
    // and reads as fully interior. That is the whole trick: no per-room
    // geometry, four numbers per building.
    close(interior_gate([4.65, 1.5, 0.0], 1.0, &u), 0.15, "inner skin");
    // And the feather is a ramp, not a step, so the 18 cm mid-point is between.
    let mid = interior_gate([4.82, 1.5, 0.0], 1.0, &u);
    assert!(
        mid > 0.15 && mid < 1.0,
        "the 6..30 cm feather must ramp, got {mid}"
    );
}

#[test]
fn the_vertical_extent_gates_the_slab_and_the_roof() {
    let u = one_room(0.15);
    // Above the roof deck: outdoors.
    close(interior_gate([0.0, 6.5, 0.0], 1.0, &u), 1.0, "above the roof");
    // Below the floor slab: also outdoors, because `worldPos.y - ry.x` goes
    // negative and the min takes it.
    close(interior_gate([0.0, -1.5, 0.0], 1.0, &u), 1.0, "below the slab");
}

#[test]
fn the_room_transform_maps_world_into_the_levels_one_yaw() {
    // A level rotated a quarter turn: world -> level is `(cos, sin, tx, tz)`
    // with cos = 0, sin = 1, so world +x maps to level +z.
    let mut u = one_room(0.15);
    u.room_xf = [0.0, 1.0, 0.0, 0.0];
    // World (0, y, 0) -> level (0, 0): still the room centre.
    close(interior_gate([0.0, 1.5, 0.0], 1.0, &u), 0.15, "centre");
    // World (0, y, 20) -> level lx = 20, well outside the 5 m half-extent.
    close(interior_gate([0.0, 1.5, 20.0], 1.0, &u), 1.0, "outside");
    // World (20, y, 0) -> level lz = -20, also outside — which it would NOT be
    // if the two rows of the rotation were transposed.
    close(interior_gate([20.0, 1.5, 0.0], 1.0, &u), 1.0, "the other axis");
}

#[test]
fn the_live_count_bounds_the_loop_and_a_bogus_count_cannot_overrun_it() {
    let mut u = one_room(0.15);
    // A count of 1 reads room 0 only: room 1 is a second volume the gate must
    // not see.
    u.rooms[1] = [40.0, 0.0, 5.0, 5.0];
    u.rooms_y[1] = [-0.8, 6.0, 0.0, 0.0];
    close(interior_gate([40.0, 1.5, 0.0], 1.0, &u), 1.0, "room 1 is not live");
    u.indirect[2] = 2.0;
    close(interior_gate([40.0, 1.5, 0.0], 1.0, &u), 0.15, "now it is");
    // A count past the array is clamped by the GLSL's own `i < OW_ROOMS` bound.
    u.indirect[2] = 40.0;
    close(interior_gate([0.0, 1.5, 0.0], 1.0, &u), 0.15, "clamped count");
    // A negative count is not live at all.
    u.indirect[2] = -3.0;
    close(interior_gate([0.0, 1.5, 0.0], 1.0, &u), 1.0, "negative count");
    // `owIndirect.z > 0.5` is the liveness test, so a fractional count under it
    // reads as no rooms.
    u.indirect[2] = 0.4;
    close(interior_gate([0.0, 1.5, 0.0], 1.0, &u), 1.0, "sub-threshold count");
}

// ---------------------------------------------------------------------------
// The directional-light injection
// ---------------------------------------------------------------------------

#[test]
fn a_light_that_does_not_receive_shadow_still_takes_the_micro_shadow() {
    // `receiveShadow ? … : 1.0` skips the sun shadow and the contact ray, but
    // the SECOND multiply is unconditional in the source.
    let out = direct_light([1.0; 3], false, 0.5, 0.5, 0.4, 1.0);
    let micro = t_mix(1.0, 0.4, 1.0 * 0.35);
    close(out[0], micro, "unshadowed light keeps the micro-shadow");
    assert_eq!(out[0], out[1], "the gain is achromatic");
    assert_eq!(out[1], out[2], "the gain is achromatic");
}

#[test]
fn the_shadow_and_the_micro_shadow_are_two_multiplies_in_the_sources_order() {
    let color = [0.9_f32, 0.7, 0.5];
    let out = direct_light(color, true, 0.5, 0.5, 0.4, 1.0);
    let micro = t_mix(1.0, 0.4, 1.0 * 0.35);
    for lane in 0..3 {
        // `(c * (s*k)) * m` — the shadow product is formed first, exactly as
        // `owSunShadow(…) * owContactShadow(…)` is, and applied before the
        // micro-shadow.
        let expected = (color[lane] * (0.5_f32 * 0.5)) * micro;
        assert_eq!(
            out[lane].to_bits(),
            expected.to_bits(),
            "lane {lane}: {} vs {expected}",
            out[lane]
        );
    }
}

#[test]
fn the_micro_shadow_is_a_third_of_the_key_in_a_crevice_and_a_few_percent_open() {
    // The source's own claim: "at 0.35 it costs 2-3% on an open surface and a
    // third of the key in a crevice."
    let open = direct_light([1.0; 3], false, 1.0, 1.0, 0.93, 1.0)[0];
    assert!(
        open > 0.97 && open < 0.98,
        "an open surface should lose 2-3%, lost {}",
        1.0 - open
    );
    // The AO buffer's own floor is 0.25, so the deepest crevice the direct
    // light ever sees costs `1 - mix(1, 0.25, 0.35)` = 26%.
    let crevice = direct_light([1.0; 3], false, 1.0, 1.0, 0.25, 1.0)[0];
    close(crevice, t_mix(1.0, 0.25, 0.35), "the deepest crevice");
    assert!(crevice < 0.75, "a crevice must lose a real fraction");
}

// ---------------------------------------------------------------------------
// The `lights_fragment_maps` injection, whole
// ---------------------------------------------------------------------------

/// A fragment on a shaded facade under a lit sky: the case the two-band fill
/// exists for.
fn facade_case() -> (IndirectIn, IndirectUniforms) {
    let mut u = one_room(0.15);
    u.indirect[2] = 0.0; // outdoors: the room list is not what is being tested
    u.sky_fill = [0.20, 0.31, 0.55];
    u.ground_fill = [0.33, 0.29, 0.225];
    u.fill_gain = [1.0, 0.5];
    u.sun_dir_world = [0.6, 0.4, 0.69];
    let input = IndirectIn {
        irradiance: [0.05, 0.05, 0.06],
        ibl_irradiance: [0.4, 0.45, 0.6],
        radiance: [0.2, 0.22, 0.3],
        diffuse_color: [0.62, 0.58, 0.5],
        roughness: 0.55,
        world_pos: [12.0, 2.5, -4.0],
        world_normal: [1.0, 0.0, 0.0],
        ao: 0.7,
    };
    (input, u)
}

#[test]
fn full_visibility_leaves_every_occlusion_term_untouched() {
    // The `owAo < 1.0` guard. At ao == 1 the whole block is the identity, and
    // the test says so on the VALUES rather than arguing it from the algebra.
    let (mut input, u) = facade_case();
    input.ao = 1.0;
    let out = indirect(input, &u);
    // The fill still runs — it is outside the guard — so `irradiance` moves.
    // What must not move is the occlusion applied to the three inputs.
    let unoccluded = indirect(
        IndirectIn {
            diffuse_color: [1.0; 3],
            ..input
        },
        &u,
    );
    assert_eq!(
        out.irradiance, unoccluded.irradiance,
        "at ao 1 the multi-bounce is albedo-independent because it is skipped"
    );
    assert_eq!(out.radiance, input.radiance, "radiance untouched at ao 1");
    // And `iblIrradiance` takes only the budget multiply, not the bounce.
    for lane in 0..3 {
        close(
            out.ibl_irradiance[lane],
            input.ibl_irradiance[lane] * (u.indirect[0] * out.indoor),
            "ibl at ao 1",
        );
    }
}

#[test]
fn the_whole_composition_matches_a_second_transcription_of_the_glsl() {
    let (input, u) = facade_case();
    let out = indirect(input, &u);
    let ao = input.ao;

    // --- `if ( owAo < 1.0 ) { … }`, longhand from the GLSL.
    let bounce: Vec<f32> = (0..3)
        .map(|lane| {
            let alb = input.diffuse_color[lane];
            let a = 2.0404_f32 * alb - 0.3324;
            let b = -4.7951_f32 * alb + 0.6417;
            let c = 2.7552_f32 * alb + 0.6903;
            (ao * (ao * (ao * a + b) + c)).max(ao).min(1.0)
        })
        .collect();
    let mut irradiance: Vec<f32> = (0..3).map(|l| input.irradiance[l] * bounce[l]).collect();
    let mut ibl: Vec<f32> = (0..3).map(|l| input.ibl_irradiance[l] * bounce[l]).collect();
    let spec_occ = (ao.max(0.0)).powf(1.0 + (input.roughness * input.roughness) * 2.0);
    let radiance: Vec<f32> = (0..3)
        .map(|l| input.radiance[l] * t_mix(1.0, spec_occ.max(0.0).min(1.0), u.ao_strength[1]))
        .collect();

    // --- `owInteriorGate`, longhand. No live rooms here, so `indoor` is 0 and
    // only the AO arm runs.
    let ao_gate = t_mix(1.0, t_smoothstep(0.45, 0.98, ao), 0.6);
    let g = (1.0_f32 - 0.0).min(ao_gate);
    let indoor = t_mix(u.indirect[1], 1.0, g.max(0.0).min(1.0));
    close(out.indoor, indoor, "the interior gate");

    // --- the two-band fill, longhand.
    let fill_ao = ao.max(0.0).sqrt();
    let up = input.world_normal[1].max(-1.0).min(1.0);
    let sky_g = t_smoothstep(u.fill_dir[0], u.fill_dir[1], up) * indoor;
    let gnd_g = t_smoothstep(u.fill_dir[2], u.fill_dir[3], -up);
    for lane in 0..3 {
        irradiance[lane] += (u.sky_fill[lane] * sky_g + u.ground_fill[lane] * gnd_g * indoor)
            * (fill_ao * u.fill_gain[0]);
    }
    for lane in 0..3 {
        ibl[lane] *= u.indirect[0] * indoor;
    }

    // --- `owSunBounce`, longhand.
    let raw = [
        -u.sun_dir_world[0] + 1e-4,
        0.28_f32 + 1e-4,
        -u.sun_dir_world[2] + 1e-4,
    ];
    let len = (raw[0] * raw[0] + raw[1] * raw[1] + raw[2] * raw[2]).sqrt();
    let anti = [raw[0] / len, raw[1] / len, raw[2] / len];
    let facing = input.world_normal[0] * anti[0]
        + input.world_normal[1] * anti[1]
        + input.world_normal[2] * anti[2];
    let wrap = ((facing + 0.12) / 1.12).max(0.0).min(1.0);
    for lane in 0..3 {
        irradiance[lane] += u.ground_fill[lane] * (wrap * u.fill_gain[1] * fill_ao * indoor);
    }

    for lane in 0..3 {
        close(out.irradiance[lane], irradiance[lane], "irradiance");
        close(out.ibl_irradiance[lane], ibl[lane], "ibl irradiance");
        close(out.radiance[lane], radiance[lane], "radiance");
    }
}

#[test]
fn the_fill_bands_are_gated_by_the_normal_and_not_lerped_between() {
    let (input, u) = facade_case();
    // A vertical wall sees half the sky dome, and the -0.95 lower edge is what
    // lets it: a narrow band here is what made a shaded facade read neutral.
    let wall = indirect(
        IndirectIn {
            world_normal: [1.0, 0.0, 0.0],
            ..input
        },
        &u,
    );
    // A soffit — a downward face — is the ground band's, not the sky's.
    let soffit = indirect(
        IndirectIn {
            world_normal: [0.0, -1.0, 0.0],
            ..input
        },
        &u,
    );
    // The sky band is cool (blue-dominant) and the ground band is warm
    // (red-dominant). If the two were LERPED rather than gated, the wall would
    // carry the street's warmth — which is the defect the source's comment
    // names.
    let wall_cool = wall.irradiance[2] - wall.irradiance[0];
    let soffit_cool = soffit.irradiance[2] - soffit.irradiance[0];
    assert!(
        wall_cool > soffit_cool,
        "the wall must be cooler than the soffit: {wall_cool} vs {soffit_cool}"
    );
    // And an up-facing surface takes the sky band at full strength.
    let up = indirect(
        IndirectIn {
            world_normal: [0.0, 1.0, 0.0],
            ..input
        },
        &u,
    );
    assert!(
        up.irradiance[2] > wall.irradiance[2],
        "up-facing must see more sky than a wall: {} vs {}",
        up.irradiance[2],
        wall.irradiance[2]
    );
}

#[test]
fn the_fill_is_occluded_by_the_root_of_ao_so_it_can_never_reach_zero() {
    let (input, u) = facade_case();
    let bright = indirect(IndirectIn { ao: 1.0, ..input }, &u);
    let dark = indirect(IndirectIn { ao: 0.25, ..input }, &u);
    // sqrt(0.25) is 0.5, so the darkest fragment the AO buffer can produce
    // still receives half the fill — not a quarter, and never zero.
    assert!(
        dark.irradiance[2] > 0.0,
        "a fill AO can drive to zero is not a fill: {}",
        dark.irradiance[2]
    );
    assert!(
        dark.irradiance[2] < bright.irradiance[2],
        "but it must still shade: {} vs {}",
        dark.irradiance[2],
        bright.irradiance[2]
    );
}

#[test]
fn an_interior_fragment_loses_the_skylight_the_bands_and_the_street_bounce() {
    let (input, u) = facade_case();
    let mut inside = u;
    inside.indirect[2] = 1.0; // the room is live
    let outdoors = indirect(
        IndirectIn {
            world_pos: [40.0, 1.5, 0.0],
            ..input
        },
        &u,
    );
    let indoors = indirect(
        IndirectIn {
            world_pos: [0.0, 1.5, 0.0],
            ..input
        },
        &inside,
    );
    assert!(
        indoors.indoor < outdoors.indoor,
        "the gate must bite: {} vs {}",
        indoors.indoor,
        outdoors.indoor
    );
    // All three gated terms shrink together — the image-based budget, the sky
    // band and the warm street bounce. That last one is the source's own
    // bug-fix note: without the gate, the inside of every room received the
    // street's bounce at full strength through a metre of masonry.
    assert!(indoors.ibl_irradiance[2] < outdoors.ibl_irradiance[2]);
    assert!(indoors.irradiance[2] < outdoors.irradiance[2]);
    assert!(indoors.irradiance[0] < outdoors.irradiance[0]);
}

// ---------------------------------------------------------------------------
// SSR
// ---------------------------------------------------------------------------

#[test]
fn a_reflection_replaces_the_image_based_specular_rather_than_adding_to_it() {
    let radiance = [0.4_f32, 0.4, 0.4];
    let ssr = [1.0_f32, 0.5, 0.25, 1.0];
    let out = ssr_blend(radiance, 1.0, 0.0, ssr);
    // At roughness 0 the ramp is saturated and confidence is 1, so the mix is
    // total: the reflection REPLACES the cubemap. Adding would give 1.4.
    close(out[0], 1.0, "a mirror takes the reflection whole");
    assert!(out[0] <= 1.0, "energy replaced, not added: {}", out[0]);
}

#[test]
fn the_reflection_fades_out_by_confidence_and_by_roughness() {
    let radiance = [0.0_f32; 3];
    let ssr = [1.0_f32, 1.0, 1.0, 1.0];
    let mirror = ssr_blend(radiance, 1.0, 0.05, ssr)[0];
    let satin = ssr_blend(radiance, 1.0, 0.4, ssr)[0];
    assert!(
        satin < mirror,
        "a rougher surface reflects less: {satin} vs {mirror}"
    );
    // The `smoothstep( 0.62, 0.14, r )` is DESCENDING — e0 > e1 on purpose —
    // and it is exhausted at exactly the roughness the outer test uses.
    let expected = 1.0_f32 * t_smoothstep(0.62, 0.14, 0.4);
    close(satin, expected.max(0.0).min(1.0), "the descending ramp");
    // Low confidence keeps the cubemap.
    let unsure = ssr_blend([0.3; 3], 1.0, 0.05, [1.0, 1.0, 1.0, 0.1])[0];
    assert!(unsure < 0.4, "a low-confidence trace barely moves: {unsure}");
}

#[test]
fn ssr_is_the_exact_identity_when_disabled_or_too_rough() {
    let radiance = [0.4_f32, 0.4, 0.4];
    let ssr = [1.0_f32, 1.0, 1.0, 1.0];
    assert_eq!(ssr_blend(radiance, 0.0, 0.0, ssr), radiance, "feature off");
    assert_eq!(ssr_blend(radiance, 0.5, 0.0, ssr), radiance, "0.5 is off");
    // `material.roughness < 0.62` — the boundary belongs to the off arm.
    assert_eq!(ssr_blend(radiance, 1.0, 0.62, ssr), radiance, "at the edge");
    assert_eq!(ssr_blend(radiance, 1.0, 0.9, ssr), radiance, "past the edge");
    assert_ne!(ssr_blend(radiance, 1.0, 0.61, ssr), radiance, "under it");
}

// ---------------------------------------------------------------------------
// The WGSL text
// ---------------------------------------------------------------------------

#[test]
fn the_wgsl_declares_every_function_the_cpu_reference_defines() {
    // If a function is added on one side and not the other, the adapter proof
    // is comparing something to nothing.
    for name in [
        "axiom_indirect_mix",
        "axiom_indirect_mix3",
        "axiom_indirect_clamp",
        "axiom_indirect_smoothstep",
        "axiom_indirect_sample_ao",
        "axiom_indirect_contact_shadow",
        "axiom_indirect_multi_bounce",
        "axiom_indirect_specular_occlusion",
        "axiom_indirect_sun_bounce",
        "axiom_indirect_interior_gate",
        "axiom_indirect_direct_light",
        "axiom_indirect_apply",
        "axiom_indirect_ssr_blend",
    ] {
        assert!(
            INDIRECT_LIGHTING_WGSL.contains(&format!("fn {name}(")),
            "the WGSL must declare {name}"
        );
    }
    assert!(INDIRECT_LIGHTING_WGSL.contains("struct AxiomIndirectU"));
    assert!(INDIRECT_LIGHTING_WGSL.contains("struct AxiomIndirectOut"));
}

#[test]
fn the_wgsl_keeps_the_sources_own_factoring() {
    // The three shapes this port has been bitten by. If any of them is tidied
    // for readability, the parity proof stops proving what it claims.
    assert!(
        INDIRECT_LIGHTING_WGSL.contains("return x * (1.0 - a) + y * a;"),
        "mix must be the GLSL spec factoring, not x + (y - x) * a"
    );
    assert!(
        INDIRECT_LIGHTING_WGSL.contains("(facing + 0.12) / 1.12"),
        "the sun-bounce wrap is a DIVISION, not a reciprocal multiply"
    );
    assert!(
        INDIRECT_LIGHTING_WGSL.contains("ao * (ao * (ao * a + b) + c)"),
        "the multi-bounce fit is Horner-nested in the source"
    );
    assert!(
        INDIRECT_LIGHTING_WGSL.contains("return (color * shadowed) * micro;"),
        "the direct light takes TWO multiplies, in the source's order"
    );
    // The uniform array bound is the source's `#define OW_ROOMS 10`.
    assert!(INDIRECT_LIGHTING_WGSL.contains("array<vec4<f32>, 10>"));
    assert!(INDIRECT_LIGHTING_WGSL.contains("i < 10;"));
}

/// The spec factoring is not a stylistic preference — it is a different number.
/// This test exists so that if someone "simplifies" [`super::mix`], the diff
/// carries the evidence with it.
#[test]
fn the_two_lerp_factorings_are_not_the_same_number() {
    let pairs = [(1.0_f32, 0.3_f32), (0.62, 0.58), (1.0, 0.25), (0.05, 0.9)];
    let disagreements = pairs
        .iter()
        .flat_map(|(x, y)| {
            (0..4096).map(move |step| {
                let a = step as f32 / 4096.0;
                let spec = x * (1.0 - a) + y * a;
                let other = x + (y - x) * a;
                usize::from(spec.to_bits() != other.to_bits())
            })
        })
        .sum::<usize>();
    assert!(
        disagreements > 0,
        "over {} sampled lerps the two factorings never disagreed, which would \
         make the port's mix discipline moot",
        pairs.len() * 4096
    );
}
