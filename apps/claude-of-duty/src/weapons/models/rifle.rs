//! Ported from Claude-of-Duty `src/weapons/models/rifle.js` (468 lines) —
//! `buildRifle()`, the AR-15/M4 pattern carbine.
//!
//! Layout (weapon-local metres, origin at the shooting hand's thumb web),
//! carried over verbatim from `rifle.js:28-41` because the numbers below only
//! make sense with the sheet that produced them:
//!
//! ```text
//!   bore axis        y = +0.075
//!   rail deck        y = +0.1036   (28.6 mm over bore, as on a real flat-top)
//!   optic centre     y = +0.142    (67 mm over bore, absolute co-witness)
//!   receiver         z = +0.055 .. -0.143
//!   handguard        z = -0.145 .. -0.385
//!   muzzle crown     z = -0.502
//!   butt pad         z = +0.245
//! ```
//!
//! This is app code (`apps/`), outside the Branchless Law and the Coverage
//! Law — plain `if`/`for` throughout, matching the source's own control flow.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use crate::weapons::geometry::primitives::{blob, extrude, lathe_z, ExtrudeOpts};
use crate::weapons::geometry::{Assembly, Xform};
use crate::weapons::parts::barrel::{add_barrel, add_gas_block, add_muzzle_device, BarrelOpts, GasBlockOpts, MuzzleKind};
use crate::weapons::parts::controls::{
    add_carbine_stock, add_pistol_grip, charging_handle_part, selector_part, trigger_part, CarbineStockOpts, PistolGripOpts,
};
use crate::weapons::parts::hardware::{add_pin, add_qd_socket, add_rail, add_sling_loop, cartridge, MountAxis, RailOpts};
use crate::weapons::parts::magazine::{
    add_front_sight, add_rear_sight, add_rollmark, build_magazine, MagazineDims, MagazineOpts, RollmarkOpts,
};
use crate::weapons::parts::optics::{build_optic, OpticOpts, OpticResult};
use crate::weapons::parts::receiver::{
    add_bolt_carrier, add_handguard, add_lower_receiver, add_upper_receiver, BoltCarrierOpts, HandguardOpts, LowerReceiverOpts,
    UpperReceiverOpts,
};

use super::{GripTarget, HandguardProfile, PosRot, ShellDims};

/// `moving: { magazine, charging, bolt, trigger, selector }` (`rifle.js:319`).
pub struct RifleMoving {
    pub magazine: Assembly,
    pub charging: Assembly,
    pub bolt: Assembly,
    pub trigger: Assembly,
    pub selector: Assembly,
}

/// `nodes` (`rifle.js:320-463`) — every attachment point the (not-yet-ported)
/// animation rig reads.
pub struct RifleNodes {
    pub muzzle: [f32; 3],
    pub chamber: [f32; 3],
    pub eject: [f32; 3],
    pub eject_dir: [f32; 3],
    pub sight: [f32; 3],
    pub sight_axis: [f32; 3],
    pub iron_sight: [f32; 3],
    /// Shooting hand. See the source's long derivation
    /// (`rifle.js:328-352`, carried below in [`build_rifle`]): the target is
    /// a WRIST, not a palm — `knuckle - 0.098 * fingerDir` — and the
    /// metacarpals run DOWN the grip's own raked axis, not forward along the
    /// receiver.
    pub grip_r: GripTarget,
    /// Support hand, solved as a C-clamp AGAINST THE CAMERA rather than
    /// eyeballed — see [`build_rifle`]'s carried-over derivation
    /// (`rifle.js:363-434`): clock angle 250 deg under the handguard (not the
    /// geometrically-obvious 140 deg, which parks the hand directly over the
    /// muzzle in the hipfire projection), with the wrist standoff tuned to
    /// 6.5 mm so the PALM — not just the fingertips — reads as touching.
    pub grip_l: GripTarget,
    pub handguard: HandguardProfile,
    pub mag_seat: PosRot,
    pub mag_drop: [f32; 3],
    pub charge_rest: PosRot,
    pub charge_pull: [f32; 3],
    pub bolt_rest: PosRot,
    pub bolt_travel: [f32; 3],
    pub trigger_pivot: PosRot,
    pub trigger_pull: f32,
    pub selector_pivot: PosRot,
    pub optic_glass: OpticResult,
}

/// `buildRifle()`'s full return value (`rifle.js:314-467`).
pub struct RifleModel {
    pub id: &'static str,
    pub label: &'static str,
    pub fx_class: &'static str,
    pub body: Assembly,
    pub moving: RifleMoving,
    pub nodes: RifleNodes,
    pub shell: ShellDims,
    pub mag_size: MagazineDims,
}

/// The assault rifle — an AR-15/M4 pattern carbine with a free-float
/// handguard, a 14.5" barrel, a three-port brake, a collapsible stock and a
/// tube red dot on a cantilever mount (`buildRifle`, `rifle.js:42-468`).
pub fn build_rifle() -> RifleModel {
    let bore: f32 = 0.075;
    let r_upper: f32 = 0.0192;
    let rail_top: f32 = bore + 0.0286;
    let z_upper_rear: f32 = 0.055;
    let z_upper_front: f32 = -0.143;
    let port_z: f32 = -0.052;
    let mag_z: f32 = -0.058;
    let mag_tilt: f32 = 0.08;
    let hg_z0: f32 = -0.145;
    let hg_z1: f32 = -0.385;
    let hg_r: f32 = 0.0235;
    let z_breech: f32 = -0.1;
    let z_barrel_end: f32 = -0.44;
    let optic_y: f32 = bore + 0.067;
    let optic_z: f32 = -0.022;

    let mut body = Assembly::new("rifle-body");

    // ---- receivers ---------------------------------------------------------
    add_upper_receiver(
        &mut body,
        "alu",
        "steel",
        "cavity",
        UpperReceiverOpts {
            z_rear: z_upper_rear,
            z_front: z_upper_front,
            bore,
            r: r_upper,
            port_z,
            rail_top,
        },
    );

    // `lower` (`rifle.js:71`) is captured but never read again in the
    // source — a preserved source quirk, not silently dropped, same
    // convention as `parts::receiver::add_lower_receiver`'s own dead
    // `matSteel` and `parts::magazine::build_magazine`'s dead `mats`.
    let _lower = add_lower_receiver(
        &mut body,
        "alu",
        "steel",
        LowerReceiverOpts {
            bore,
            z_rear: z_upper_rear + 0.004,
            z_front: -0.088,
            w: 0.0245,
            mag_w: 0.0292,
            mag_d: 0.0672,
            mag_top: Some(0.049),
            mag_bottom: Some(0.008),
            mag_z,
            mag_tilt,
            trigger_z: -0.012,
            grip_angle: 0.38,
        },
    );

    // Bolt catch (left), magazine release in its fence (right), takedown pins.
    let catch_paddle = extrude(
        &[[-0.012, -0.0035], [0.012, -0.0045], [0.014, 0.0035], [-0.012, 0.0045]],
        0.0042,
        ExtrudeOpts {
            bevel: 0.0007,
            ..Default::default()
        },
    );
    body.add(
        catch_paddle,
        "steel",
        Some(Xform {
            x: -0.0135,
            y: 0.0545,
            z: -0.018,
            ry: FRAC_PI_2,
            ..Default::default()
        }),
    );
    let catch_boss = blob(0.006, 0.011, 0.014, 0.0018, 2);
    body.add(
        catch_boss,
        "alu",
        Some(Xform {
            x: -0.0128,
            y: 0.0555,
            z: -0.0085,
            ..Default::default()
        }),
    );

    let rel_fence = blob(0.0075, 0.016, 0.019, 0.0022, 2);
    body.add(
        rel_fence,
        "alu",
        Some(Xform {
            x: 0.0132,
            y: 0.0505,
            z: -0.0295,
            ..Default::default()
        }),
    );
    let rel_button = lathe_z(
        &[[0.0, 0.0], [0.0, 0.0048], [0.0016, 0.0052], [0.0042, 0.0052], [0.0042, 0.0]],
        14,
        0.0,
        TAU,
    );
    body.add(
        rel_button,
        "steel",
        Some(Xform {
            x: 0.0158,
            y: 0.0505,
            z: -0.0295,
            ry: FRAC_PI_2,
            ..Default::default()
        }),
    );
    add_pin(&mut body, "steel", 0.0, 0.0555, -0.083, 0.0028, 0.0252); // front takedown
    add_pin(&mut body, "steel", 0.0, 0.0555, 0.0455, 0.0028, 0.0252); // rear takedown

    // Rollmark + calibre stamp on the left of the magwell — the side that
    // faces the camera in the hipfire pose, engraved as geometry so it
    // cannot swim.
    add_rollmark(
        &mut body,
        "cavity",
        RollmarkOpts {
            x: Some(-0.0149),
            y: Some(0.0355),
            z: Some(-0.031),
            h: 0.0036,
            ..Default::default()
        },
    );
    add_rollmark(
        &mut body,
        "cavity",
        RollmarkOpts {
            x: Some(-0.0149),
            y: Some(0.0272),
            z: Some(-0.033),
            h: 0.0024,
            pitch: 0.0014,
            pattern: vec![2, 3, 1, 0, 2, 2, 3, 0, 3, 2],
            ..Default::default()
        },
    );

    // ---- barrel, gas system, muzzle -----------------------------------------
    let _barrel = add_barrel(
        &mut body,
        "steel",
        "cavity",
        BarrelOpts {
            y: bore,
            z_breech,
            z_muzzle: z_barrel_end,
            r_chamber: 0.0112,
            r_barrel: 0.0077,
            r_gas: 0.0098,
            gas_at: Some(-0.3),
            ..Default::default()
        },
    );
    // Soot: the gas block vents combustion products by design and the brake
    // is 20 mm from the crown. See `steel_soot` in materials.js.
    add_gas_block(
        &mut body,
        "steel_soot",
        GasBlockOpts {
            y: bore,
            z: -0.3,
            r_barrel: 0.0077,
            tube_to: -0.15,
            w: 0.021,
            h: 0.0195,
            ..Default::default()
        },
    );
    let muzzle = add_muzzle_device(&mut body, "steel_soot", "cavity", MuzzleKind::Brake, z_barrel_end, 0.0077, bore);

    // ---- handguard + rails ---------------------------------------------------
    //
    // The handguard is an aluminium chassis (barrel nut, braces, end cap)
    // carrying POLYMER panels. That is what gives the gun its second
    // material class: a warm, 0.023-albedo, 0.65-rough moulded shell bolted
    // to a cool, 0.033-albedo, 0.40-rough anodised receiver, with phosphate
    // steel forward of both.
    //
    // `topFrom/topTo` closes the top of the handguard over the section the
    // support hand grips, and the top rail is split around it — see gripL
    // below. A hand cannot close over a Picatinny rail without the fingers
    // passing through the teeth, and a support hand that does not close is
    // the reason the glove read as detached slabs floating beside the gun.

    // Where the support hand's knuckles cross the handguard. Moved 10 mm
    // rearward (was -0.245) when the hipfire pose pushed the weapon out to
    // 300 mm: the support arm is reach-limited, and every 10 mm off the
    // contact is elbow bend recovered. 150 mm of handguard remains ahead of
    // the hand.
    let hand_z: f32 = -0.235;
    add_handguard(
        &mut body,
        "alu",
        HandguardOpts {
            mat_panel: Some("polymer"),
            y: bore,
            z0: hg_z0,
            z1: hg_z1,
            r: hg_r,
            sides: 8,
            slat_w: 0.0166,
            slat_t: 0.0036,
            slots: 4,
            braces: 3,
            top_from: Some(hand_z + 0.048),
            top_to: Some(hg_z1 + 0.056),
        },
    );
    // ONE continuous top rail over the whole handguard, as a free-float tube
    // actually has. It used to be split around the support hand's knuckles,
    // because a hand cannot close over Picatinny teeth without the fingers
    // passing through them — but the hand now grips UNDER the handguard (see
    // gripL), so the split left a 138 mm bare gap in the middle of the deck
    // for no reason.
    add_rail(&mut body, "alu", hg_z1 + 0.004, hg_z0 - 0.002, rail_top, 0.0, RailOpts::default());
    add_qd_socket(&mut body, "alu", "steel", -hg_r + 0.001, bore - 0.008, hg_z0 - 0.035, MountAxis::X, 0.005);
    add_sling_loop(
        &mut body,
        "steel",
        0.0,
        bore - hg_r - 0.0015,
        hg_z1 + 0.03,
        0.0075,
        Xform {
            rx: FRAC_PI_2,
            ry: FRAC_PI_2,
            ..Default::default()
        },
    );

    // ---- furniture -----------------------------------------------------------
    add_pistol_grip(
        &mut body,
        "polymer",
        "rubber",
        PistolGripOpts {
            y: 0.035,
            z: 0.015,
            angle: 0.38,
            len: 0.108,
            w: 0.031,
        },
    );
    // Buffer tube stays aluminium (it is a machined extrusion); the cheek
    // riser and butt stock are the polymer class, the pad is rubber. Three
    // classes, one part.
    add_carbine_stock(
        &mut body,
        "alu",
        "polymer",
        "rubber",
        CarbineStockOpts {
            bore,
            z_front: z_upper_rear + 0.003,
            z_rear: 0.245,
            y: Some(bore - 0.012),
        },
    );

    // ---- sights --------------------------------------------------------------
    //
    // 31 mm tube, 52 mm long, a 33 mm belled objective with a 7 mm shade.
    //
    // The length is the number that matters and 70 mm was the wrong one. The
    // visible sight picture in ADS is the objective bore subtended at
    // (eyeRelief + len), so every millimetre of tube shrinks it; at 70 mm
    // the objective stopped the train down to 34% of the housing radius and
    // the ADS frame was a quarter-height ring of dark tube wall — "a length
    // of drainpipe", measured. 52 mm plus the flared bore in `build_optic`
    // gets it to 69%.
    let optic = build_optic(
        &mut body,
        OpticOpts {
            r_tube: 0.0155,
            len: 0.052,
            hood: 0.007,
            y: optic_y,
            z: optic_z,
            rail_top,
            mat_body: "alu_fine",
            mat_steel: "steel",
        },
    );
    // BACK-UP IRON SIGHTS IN POLYMER, not steel.
    //
    // MEASURED: as `steel`/`steel_black` the folded leaves and the windage
    // drum rendered at L=188-192 — the brightest objects on the front half
    // of the weapon and the last of the "bright cream blocky bits". They are
    // METALS, so `specularIntensity` does not apply to them (three folds the
    // albedo into F0 at metalness 1) and halving F0 twice moved the display
    // value by a fifth of a stop.
    //
    // A folding BUIS is a Magpul MBUS or a Troy: glass-filled polymer or
    // black anodised aluminium, never bright phosphate. `polymer` is both
    // the honest material and a DIELECTRIC, so it takes the 0.13 specular
    // clamp with the rest of the gun and carries the moulding stipple as
    // well.
    add_front_sight(&mut body, "polymer", "alu", 0.0, rail_top, -0.358, false);
    // BACK-UP REAR SIGHT — MOVED, and the move is a composition fix, not a
    // taste one.
    //
    // At z = +0.038 (the classic flat-top position, right at the back of
    // the upper) the folded BUIS sits 75 mm from the eye in ADS. That is
    // closer than any other part of the weapon — closer than the optic
    // itself — so it rendered as a pale cream slab with a chrome drum
    // filling the bottom 180 px of the ADS frame at L=207-224, the
    // brightest object on screen. Measured by raycasting the ADS frame:
    // every one of those pixels came back `rifle-body-steel` at d=0.072-0.076.
    //
    // No material change fixes an object that large and that close (the F0
    // was already halved twice and it moved the display value by 0.15 of a
    // stop — it is sitting flat on the tone curve's shoulder). Mounting the
    // BUIS forward of the optic on the free-float rail is both a completely
    // standard configuration with a cantilever mount and puts it 224 mm out
    // instead of 75, i.e. a ninth of the screen area, behind and below the
    // sight picture where it belongs. Its steel also becomes nitride black
    // rather than bright phosphate, which is what a folding BUIS is
    // actually finished in.
    add_rear_sight(&mut body, "polymer", "alu", 0.0, rail_top, -0.112, false);

    // ---- moving parts --------------------------------------------------------
    let mut magazine = Assembly::new("rifle-mag");
    let mag = build_magazine(
        &mut magazine,
        (),
        MagazineOpts {
            w: 0.0255,
            d: 0.0655,
            len: 0.212,
            curve: 0.03,
            segs: 8,
            witness: 4,
            poly: "polymer",
            ..MagazineOpts::default()
        },
    );

    let mut charging = Assembly::new("rifle-charging");
    let ch_g = charging_handle_part();
    // An AR charging handle is a black-anodised ALUMINIUM extrusion, not
    // bright steel. Measured as `steel_bright` it was a 30 x 15 px cream
    // plate at L=170-184 sitting on the receiver flank in hipfire — one of
    // the "untextured white blocks". As `alu` it is the same class as the
    // receiver it slides in, which is correct, and it picks up the specular
    // clamp and the anodising grain.
    charging.add(ch_g, "alu", Some(Xform::default()));

    let mut bolt = Assembly::new("rifle-bolt");
    add_bolt_carrier(
        &mut bolt,
        "steel_bright",
        BoltCarrierOpts {
            r: 0.0152,
            len: 0.092,
            z: 0.0,
            ..BoltCarrierOpts::default()
        },
    );
    // A round in the chamber. It lies ALONG the bore (the cartridge is
    // authored base-at-0 running +Z, so ry=PI turns it muzzle-forward) and
    // is pushed far enough forward that only the case head shows in the
    // ejection port. Left where it is easy to put it, a chambered round
    // spears out through the receiver wall and reads as a bug.
    let chamber_round = cartridge(0.0446, 0.00495, 0.019);
    bolt.add(
        chamber_round.brass,
        "brass",
        Some(Xform {
            z: -0.09,
            ry: PI,
            y: 0.0,
            ..Default::default()
        }),
    );

    let mut trigger = Assembly::new("rifle-trigger");
    let trg = trigger_part("steel_bright");
    trigger.add(trg.geo, "steel_bright", Some(Xform::default()));

    let mut selector = Assembly::new("rifle-selector");
    let sel = selector_part("alu", "steel", 0.006);
    selector.add(sel.geo, "alu", Some(Xform::default()));
    let sel_r = selector_part("alu", "steel", 0.006);
    selector.add(
        sel_r.geo,
        "alu",
        Some(Xform {
            sx: -1.0,
            ..Default::default()
        }),
    );

    // Hand targets are WRISTS, not palms: the glove is modelled from the
    // wrist forward, with the knuckle line 98 mm along the hand's -Z. So
    // each target is derived as `knuckle - 0.098 * fingerDir` from the
    // contact point we actually want on the weapon. Authoring the palm
    // position directly is what buries the hand inside the handguard.
    //
    // Shooting hand: knuckles on the front strap 52 mm below the origin,
    // web of the thumb at the top-rear of the grip tang.
    //
    // The metacarpals run DOWN the grip, not forward along the receiver.
    // The old finger direction (-0.05,-0.42,-0.906) was 65 deg off the
    // grip's own axis, which threw the knuckle line 40 mm forward of the
    // front strap: the fingers closed on air inside the trigger guard and
    // the whole hand read as a slab parked next to the gun. The grip rakes
    // 0.38 rad, so the hand rides it at (0.02,-0.90,-0.44) — a shade more
    // forward than the strap, which is what wraps the fingertips around
    // onto the far side where the camera can see them.
    let grip_r = GripTarget {
        pos: [0.0251, 0.06, 0.1223],
        finger: [0.05, -0.55, -0.833],
        back: [1.0, 0.03, 0.04],
    };

    // Support hand: knuckles over the lower-left of the handguard, wrist low
    // and outboard, so the fingers close around a 47 mm tube.
    //
    // It grips the REAR third of the rail, not the far end. That is both
    // how a carbine is actually driven with a modern grip and a hard
    // constraint: a 0.57 m arm measured from a shoulder 0.2 m off the eye
    // cannot reach a hand 0.55 m downrange, and when the two-bone solve
    // clamps, the elbow locks dead straight and the arm reads as a
    // broomstick.
    //
    // Support hand: a C-clamp, SOLVED against the handguard cylinder rather
    // than eyeballed.
    //
    // The handguard is a 47 mm tube on the axis (0, bore). Pick the contact
    // clock angle phi = 140 deg (upper left), then:
    //   back   = the outward surface normal there, tilted +0.30 rearward so
    //            the dorsal knuckle line turns to face the camera instead of
    //            presenting edge-on.
    //   finger = the tangent at phi, rolled 0.35 forward, so the fingers
    //            wrap clockwise over the top of the handguard and down the
    //            far side.
    //   pos    = knuckleContact - 0.098 * finger   (targets are WRISTS)
    // with the knuckle contact pushed 14.5 mm off the surface — half a palm
    // thickness is 16 mm, so the glove interpenetrates the handguard by
    // 1.5 mm and there is no daylight anywhere along the contact.
    //
    // The old target put the knuckle line 14 mm clear of the tube on the
    // wrong side of it entirely, so the fingers closed in mid-air below the
    // handguard.
    //
    // SOLVED AGAINST WHAT THE CAMERA CAN SEE, not just against the tube.
    //
    // The C-clamp above is a correct grip and it was the wrong one here.
    // With the hipfire pose derived from the bore axis (defs.js) the barrel
    // is only 4 deg off the view axis, so the muzzle projects to (1065,698)
    // — up and LEFT of the handguard — and a C-clamp puts the knuckles at
    // clock angle 140 deg, which projects to (1104,701). 40 px apart, with a
    // hand 160 px wide: the hand sat exactly on top of the muzzle, the
    // barrel, the gas block and the front sight, and every one of them was
    // invisible. Measured, not guessed: see the marked captures.
    //
    // So the support hand goes UNDER the handguard — clock angle 250 deg,
    // the classic grip — and wraps counter-clockwise up the far side. That
    // puts the knuckle contact at (1117,818) and the wrist at (978,752),
    // i.e. 130 px below the muzzle, and the whole muzzle end of the weapon
    // is clear. Dead bottom (270 deg) clears it by another 20 px but drops
    // the whole hand into the handguard's own cast shadow, where the only
    // light left is blue sky fill and the warm glove measures COOLER than
    // the receiver — the exact defect the retint was supposed to cure.
    // 250 deg keeps the dorsum in the viewmodel key.
    //
    // Derivation, with the handguard a 54.2 mm tube on the bore axis
    // (23.5 mm chassis + 3.6 mm panels) and the knuckle line 14.5 mm off the
    // surface, so a 16 mm half-palm interpenetrates by 1.5 mm and there is
    // no daylight:
    //   phi     = 250 deg                       (below and slightly
    //             near-side)
    //   finger  = tangent at phi rolled 0.30 rad forward  -> wraps CCW
    //   back    = surface normal tilted 0.62 rad REARWARD. This is the one
    //             number that is about the camera and not the grip: it
    //             rolls the dorsum to face the shooter, which recovers all
    //             of the knuckle read the C-clamp was there to provide (dot
    //             with the view direction 0.40, against the C-clamp's
    //             0.385).
    //   pos     = contact - 0.098 * finger      (targets are WRISTS)
    // Reach from the support shoulder is 94% of a 630 mm arm — the elbow
    // keeps a visible bend. The distal joints are then fitted per-fingertip
    // against this same cylinder at build time; see Arm.fitToCylinder.
    //
    // The wrist target is 8 mm CLOSER to the tube than the derivation above
    // gives (14.5 mm of knuckle standoff -> 6.5 mm).
    //
    // MEASURED with the build-time contact solve: at 14.5 mm the four
    // fingertips landed 0.4-0.7 mm off the handguard — a real grip — but
    // the PALM stood 29 mm clear of it, and the palm is the part of the
    // support hand the camera actually sees. On screen that is a hand held
    // next to the handguard with daylight behind it, which is precisely the
    // "they float beside it with a visible gap" complaint, even though
    // every fingertip is touching.
    //
    // A 16 mm half-palm at 6.5 mm of standoff interpenetrates the tube by
    // ~9 mm at the heel, which is what a glove does when it is squeezing
    // something. The per-fingertip solve re-runs against this target at
    // build time and just uses less curl, so the contact is preserved.
    let grip_l = GripTarget {
        pos: [-0.1, 0.0734, hand_z + 0.0252],
        finger: [0.8977, -0.3267, -0.2955],
        back: [-0.2784, -0.7648, 0.581],
    };

    // The handguard's collision profile, for the build-time fingertip
    // contact solve (`Arm.fitToCylinder`). The handguard is genuinely a
    // cylinder on the bore axis, so the profile is exact — `r` is the outer
    // radius of the POLYMER panels (the slats stand 3.6 mm off the 23.5 mm
    // chassis), which is the surface a hand actually touches.
    let handguard = HandguardProfile {
        axis: [0.0, bore, 0.0],
        dir: [0.0, 0.0, 1.0],
        r: hg_r + 0.0036,
        z0: hg_z0,
        z1: hg_z1,
    };

    RifleModel {
        id: "rifle",
        label: "M4A1",
        fx_class: "carbine",
        body,
        moving: RifleMoving {
            magazine,
            charging,
            bolt,
            trigger,
            selector,
        },
        nodes: RifleNodes {
            muzzle: [0.0, bore, muzzle.crown_z],
            chamber: [0.0, bore, port_z],
            eject: [r_upper + 0.008, bore + 0.003, port_z],
            eject_dir: [0.86, 0.44, 0.26],
            sight: [0.0, optic_y, optic.lens_z],
            sight_axis: [0.0, 0.0, -1.0],
            iron_sight: [0.0, rail_top + 0.026, 0.038],
            grip_r,
            grip_l,
            handguard,
            mag_seat: PosRot {
                pos: [0.0, 0.061, mag_z],
                rot: [mag_tilt, 0.0, 0.0],
            },
            mag_drop: [0.0, -0.4, 0.02],
            charge_rest: PosRot {
                pos: [0.0, bore + r_upper - 0.0075, z_upper_rear - 0.024],
                rot: [0.0, 0.0, 0.0],
            },
            charge_pull: [0.0, 0.0, 0.082],
            bolt_rest: PosRot {
                pos: [0.0, bore, 0.021],
                rot: [0.0, 0.0, 0.0],
            },
            bolt_travel: [0.0, 0.0, 0.062],
            trigger_pivot: PosRot {
                pos: [0.0, 0.0455, -0.0055],
                rot: [0.0, 0.0, 0.0],
            },
            trigger_pull: -0.34,
            selector_pivot: PosRot {
                pos: [0.0, 0.0525, 0.0205],
                rot: [0.0, 0.0, 0.0],
            },
            optic_glass: optic,
        },
        shell: ShellDims {
            case_len: 0.0446,
            rim_r: 0.00495,
        },
        mag_size: mag,
    }
}
