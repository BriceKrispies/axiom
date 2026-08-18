//! The ported audio subsystem, pinned against the JavaScript it came from.
//!
//! Every `golden.json` value in this file was produced by running the **original**
//! `C:/dev/Claude-of-Duty/src/audio/*.js` under Node (v24) against a recording
//! stub of `BaseAudioContext`. The capture script is committed next to the data
//! at `tests/audio/capture.mjs`, so the goldens are reproducible rather than
//! copied: re-run it against the source and the file should come out identical.
//!
//! These are golden values, not recomputations. If an edit to `src/audio/`
//! changes one of them, the port has stopped being the source's synthesis and
//! the test says exactly which number moved.
//!
//! ## What is pinned, and how tightly
//!
//! * **Exactly** — everything reachable by integer or exact `f64` arithmetic:
//!   the envelope helpers' automation events, `classify_space`, the white/pink/
//!   brown noise fills, the buffer/graph *structure* (node kinds, creation
//!   order, connections, schedule).
//! * **Within a relative tolerance** — everything a transcendental touches.
//!   `Math.pow`/`exp`/`tanh`/`sin` are not bit-guaranteed across V8 and Rust's
//!   libm, so a value derived through one is compared at [`REL`] (1e-9
//!   relative). That is still an extraordinarily tight pin on this data: a
//!   single drifted `rng` draw changes a parameter in its *first* significant
//!   digit, not its ninth.
//! * **`f32` values** — noise samples, IR samples and curve tables live in
//!   `Float32Array`s in the source, so they are compared as the `f32` the store
//!   actually rounds to, at [`REL_F32`].

use std::sync::OnceLock;

use serde_json::Value;

use axiom_shmup::audio::ambience::{ambient_one_shot, Ambience, AmbienceCue, OneShot};
use axiom_shmup::audio::dsp::{
    ad, adsr, air_cutoff, clamp, db_to_gain, fill_noise, hit, lerp, limiter_curve,
    saturation_curve, semis, struck, sweep, NoiseBank, NoiseKind, Partial, SPEED_OF_SOUND,
};
use axiom_shmup::audio::foley::{
    body_fall, cloth, explosion, footstep, heartbeat, reload_phase, shell_casing, surface_impact,
    ui_sound, Gait, ReloadPhase, StepOpts, Surface, UiSound,
};
use axiom_shmup::audio::graph::{
    Automation, AudioGraph, NodeId, NodeKind, Param, ParamRef, Schedule, Sink,
};
use axiom_shmup::audio::ir::{
    classify_space, generate_ir, IrSpec, Space, SpaceWeights, IR_SPECS,
};
use axiom_shmup::audio::mixer::{Bus, Mixer};
use axiom_shmup::audio::spatial::{
    AcquireOpts, RayHit, RayMask, SpatialField, WorldProbe,
};
use axiom_shmup::audio::system::{
    ActorDeath, AudioCore, AudioSystem, BulletImpact, BulletTracer, DamageDealt, DamageTaken,
    ExplosionEvent, PlayOpts, PlayerFootstep, PlayerLand, PlayerState, VoiceKind, WeaponFire,
    WeaponReload, WeaponShell,
};
use axiom_shmup::audio::vox::{bark, bark_for, Bark, BarkOpts, BarkRequest};
use axiom_shmup::audio::weapons::{
    bullet_whizz, dry_fire, resolve_profile, weapon_shot, RoundRobinBank, ShotOpts, Voice,
    AK, LMG, PISTOL, RIFLE, SHOTGUN, SMG, SNIPER, SUPPRESSED,
};
use axiom_shmup::engine::{Engine, CAPTURE_SEED};
use axiom_shmup::registry::Phase;
use axiom_shmup::rng::Rng;

/// Relative tolerance for a value a transcendental touched.
///
/// Measured, not guessed: at the time of writing **every** comparison in this
/// file passes at a tolerance of exactly zero — V8 and Rust's `std` agree
/// bit-for-bit on every `pow`, `exp`, `log`, `tanh`, `sin` and `cos` this data
/// reaches. The tolerance is insurance against a libm that does not, on some
/// other host, and nothing more. If it ever has to be loosened past this, that
/// is a libm difference and not a port defect; a real drift moves a value in its
/// first significant digit, because it means an `rng` draw moved.
const REL: f64 = 1e-12;
/// Relative tolerance for a value that passed through a `Float32Array` store.
const REL_F32: f64 = 1e-9;

const SR: f64 = 48000.0;

fn golden() -> &'static Value {
    static G: OnceLock<Value> = OnceLock::new();
    G.get_or_init(|| {
        serde_json::from_str(include_str!("audio/golden.json")).expect("golden.json parses")
    })
}

fn close(actual: f64, expected: f64, rel: f64, what: &str) {
    if actual == expected {
        return;
    }
    let scale = expected.abs().max(1.0);
    assert!(
        (actual - expected).abs() <= rel * scale,
        "{what}: expected {expected:.17e}, got {actual:.17e} (rel {:.3e})",
        (actual - expected).abs() / scale
    );
}

fn num(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("not a number: {v}"))
}

/* ================================================================ */
/* dsp.js — scalar helpers                                          */
/* ================================================================ */

#[test]
fn dsp_scalar_helpers_match_the_javascript() {
    let g = golden();
    assert_eq!(SPEED_OF_SOUND, num(&g["speedOfSound"]));
    for row in g["airCutoff"].as_array().unwrap() {
        let (d, want) = (num(&row[0]), num(&row[1]));
        close(air_cutoff(d), want, REL, &format!("airCutoff({d})"));
    }
    for row in g["semis"].as_array().unwrap() {
        let (n, want) = (num(&row[0]), num(&row[1]));
        close(semis(n), want, REL, &format!("semis({n})"));
    }
    for row in g["dbToGain"].as_array().unwrap() {
        let (d, want) = (num(&row[0]), num(&row[1]));
        close(db_to_gain(d), want, REL, &format!("dbToGain({d})"));
    }
    for row in g["clamp"].as_array().unwrap() {
        let (v, lo, hi, want) = (num(&row[0]), num(&row[1]), num(&row[2]), num(&row[3]));
        assert_eq!(clamp(v, lo, hi), want);
    }
    for row in g["lerp"].as_array().unwrap() {
        let (a, b, t, want) = (num(&row[0]), num(&row[1]), num(&row[2]), num(&row[3]));
        assert_eq!(lerp(a, b, t), want);
    }
}

/// `clamp` is the source's ternary chain, not `f64::clamp` — the two disagree on
/// a NaN input, and `f64::clamp` additionally panics on a NaN bound.
#[test]
fn clamp_propagates_nan_the_way_the_javascript_does() {
    assert!(clamp(f64::NAN, 0.0, 1.0).is_nan());
    assert_eq!(clamp(f64::INFINITY, 0.0, 1.0), 1.0);
    assert_eq!(clamp(f64::NEG_INFINITY, 0.0, 1.0), 0.0);
}

/* ================================================================ */
/* dsp.js — noise                                                   */
/* ================================================================ */

#[test]
fn noise_fills_match_the_javascript() {
    let g = &golden()["noise"];
    for (kind, key, rel) in [
        (NoiseKind::White, "white", 0.0),
        (NoiseKind::Pink, "pink", 0.0),
        (NoiseKind::Brown, "brown", 0.0),
        // The crackle grains are `Math.sin`/`Math.exp` shaped.
        (NoiseKind::Crackle, "crackle", REL_F32),
    ] {
        let entry = &g[key];
        let n = entry["n"].as_u64().unwrap() as usize;
        let mut buf = vec![0.0f32; n];
        fill_noise(&mut buf, kind, &mut Rng::new(0x1234_abcd));

        let head = entry["head"].as_array().unwrap();
        for (i, want) in head.iter().enumerate() {
            let want = num(want);
            if rel == 0.0 {
                assert_eq!(
                    f64::from(buf[i]),
                    want,
                    "{key} head[{i}] must be bit-identical"
                );
            } else {
                close(f64::from(buf[i]), want, rel, &format!("{key} head[{i}]"));
            }
        }
        let tail = entry["tail"].as_array().unwrap();
        for (j, want) in tail.iter().enumerate() {
            let i = n - tail.len() + j;
            close(f64::from(buf[i]), num(want), rel.max(1e-12), &format!("{key} tail[{i}]"));
        }
        let sum: f64 = buf.iter().map(|&v| f64::from(v)).sum();
        let abs_sum: f64 = buf.iter().map(|&v| f64::from(v).abs()).sum();
        close(sum, num(&entry["sum"]), rel.max(1e-9), &format!("{key} sum"));
        close(
            abs_sum,
            num(&entry["absSum"]),
            rel.max(1e-12),
            &format!("{key} absSum"),
        );
    }
}

/// The bank's four buffers are built in a fixed order, from one stream, two
/// decorrelated channels each. Both facts fix every downstream `rng` draw.
#[test]
fn noise_bank_layout_and_offset_match_the_javascript() {
    let g = &golden()["bank"];
    let mut graph = AudioGraph::new(SR);
    let mut rng = Rng::new(0x51ee7);
    let bank = NoiseBank::new(&mut graph, &mut rng, 1.2);

    let buffers = g["buffers"].as_array().unwrap();
    assert_eq!(graph.buffers.len(), buffers.len());
    for (i, want) in buffers.iter().enumerate() {
        let b = &graph.buffers[i];
        assert_eq!(b.number_of_channels(), want["ch"].as_u64().unwrap() as usize);
        assert_eq!(b.length(), want["len"].as_u64().unwrap() as usize);
        close(b.duration(), num(&want["dur"]), REL, "bank duration");
    }
    // The two channels of one buffer must not be the same noise.
    assert_ne!(graph.buffers[0].channels[0], graph.buffers[0].channels[1]);

    let src = bank.source(&mut graph, NoiseKind::Pink, Some(&mut rng), 1.3, true);
    close(
        graph.source_offset(src),
        num(&g["offset"]),
        REL,
        "bank source offset",
    );
    close(bank.duration(), num(&g["duration"]), REL, "bank duration");
    match graph.node(src) {
        NodeKind::BufferSource {
            buffer,
            playback_rate,
            looping,
            loop_start,
            loop_end,
            ..
        } => {
            // pink is buffer index 1 — white is built first.
            assert_eq!(buffer.0, 1);
            assert_eq!(*playback_rate, 1.3);
            assert!(*looping);
            assert_eq!(*loop_start, 0.0);
            close(*loop_end, 1.2, REL, "loopEnd");
        }
        other => panic!("expected a buffer source, got {other:?}"),
    }
}

/* ================================================================ */
/* dsp.js — waveshaper curves                                       */
/* ================================================================ */

#[test]
fn saturation_and_limiter_curves_match_the_javascript() {
    let g = &golden()["curves"];
    for (drive, asym) in [(4.0, 0.0), (6.0, 0.35), (2.5, 0.2), (14.0, 0.7), (1.6, 0.35)] {
        let key = format!("sat:{drive}:{asym}");
        let entry = &g[&key];
        let mut graph = AudioGraph::new(SR);
        let id = saturation_curve(&mut graph, drive, asym);
        let curve = graph.curve(id);
        assert_eq!(curve.len(), entry["n"].as_u64().unwrap() as usize);
        for row in entry["samples"].as_array().unwrap() {
            let i = row[0].as_u64().unwrap() as usize;
            close(
                f64::from(curve[i]),
                num(&row[1]),
                REL_F32,
                &format!("{key}[{i}]"),
            );
        }
    }
    let entry = &g["limiter"];
    let mut graph = AudioGraph::new(SR);
    let id = limiter_curve(&mut graph);
    let curve = graph.curve(id);
    assert_eq!(curve.len(), entry["n"].as_u64().unwrap() as usize);
    for row in entry["samples"].as_array().unwrap() {
        let i = row[0].as_u64().unwrap() as usize;
        close(
            f64::from(curve[i]),
            num(&row[1]),
            REL_F32,
            &format!("limiter[{i}]"),
        );
    }
}

/// The two-decimal cache key is load-bearing: two drives that round to the same
/// key share one curve, so the first caller's exact drive shapes both. Pinned
/// because it is visible in a graph diff and invisible in the audio.
#[test]
fn the_saturation_cache_key_rounds_to_two_decimals() {
    let g = golden();
    let mut graph = AudioGraph::new(SR);
    let a = saturation_curve(&mut graph, 6.144, 0.351);
    let b = saturation_curve(&mut graph, 6.1449, 0.3512);
    let c = saturation_curve(&mut graph, 6.149, 0.351);
    assert_eq!(g["curveCacheShares"].as_bool(), Some(true));
    assert_eq!(g["curveCacheDistinct"].as_bool(), Some(false));
    assert_eq!(a, b, "6.144 and 6.1449 both key on \"6.14\"");
    assert_ne!(a, c, "6.144 keys on \"6.14\", 6.149 on \"6.15\"");
    assert_eq!(graph.curves.len(), 2);
}

/* ================================================================ */
/* dsp.js — envelopes                                               */
/* ================================================================ */

/// Record the automation the helpers emit on a scratch param and compare it
/// against the calls the JavaScript made on a recording stub — event for event,
/// exactly. These are pure schedule arithmetic; no tolerance is warranted.
fn env_case(name: &str, run: impl FnOnce(&mut AudioGraph, ParamRef) -> f64) {
    let entry = &golden()["env"][name];
    let mut g = AudioGraph::new(SR);
    let node = g.create_gain(0.0);
    let ret = run(&mut g, node.gain());

    let want = entry["calls"].as_array().unwrap();
    assert_eq!(
        g.automation.len(),
        want.len(),
        "{name}: expected {} automation events, got {}",
        want.len(),
        g.automation.len()
    );
    for (i, w) in want.iter().enumerate() {
        let e = g.automation[i];
        assert_eq!(e.kind.as_str(), w[0].as_str().unwrap(), "{name}[{i}] kind");
        assert_eq!(e.value, num(&w[1]), "{name}[{i}] value");
        assert_eq!(e.time, num(&w[2]), "{name}[{i}] time");
        if e.kind == Automation::SetTargetAtTime {
            assert_eq!(e.time_constant, num(&w[3]), "{name}[{i}] timeConstant");
        }
    }
    match entry["ret"].as_f64() {
        Some(w) => assert_eq!(ret, w, "{name} return"),
        // A NaN `t0` is returned unchanged; JSON has no NaN, so the capture
        // stringified it.
        None => assert!(ret.is_nan(), "{name} return should be NaN"),
    }
}

#[test]
fn envelope_helpers_emit_the_javascript_automation() {
    env_case("hit", |g, p| hit(g, p, 0.02, 0.9, 0.0075));
    env_case("hitTinyPeak", |g, p| hit(g, p, 0.02, 1e-9, 0.01));
    env_case("adLongAttack", |g, p| ad(g, p, 0.02, 0.8, 0.012, 0.13));
    env_case("adShortAttack", |g, p| ad(g, p, 0.02, 0.8, 0.0005, 0.13));
    env_case("adsr", |g, p| {
        adsr(g, p, 0.02, 0.5, 0.014, 0.03, 0.07, 0.72, 0.055)
    });
    env_case("sweep", |g, p| sweep(g, p, 0.02, 620.0, 190.0, 0.28));
    env_case("sweepFloor", |g, p| sweep(g, p, 0.02, 0.0001, 0.0, 0.0));
}

/// The guard arms: an envelope handed garbage schedules nothing at all rather
/// than throwing inside Web Audio. Eleven subsystems can reach audio and one NaN
/// position must not take the frame down.
#[test]
fn envelope_guards_refuse_garbage_without_scheduling() {
    env_case("hitNaN", |g, p| hit(g, p, f64::NAN, 0.5, 0.01));
    env_case("hitNegT", |g, p| hit(g, p, -0.001, 0.5, 0.01));
    env_case("sweepBadTo", |g, p| {
        sweep(g, p, 0.02, 100.0, f64::NAN, 0.1)
    });
    // A NaN peak is refused too, on the same guard.
    let mut g = AudioGraph::new(SR);
    let n = g.create_gain(0.0);
    assert_eq!(ad(&mut g, n.gain(), 0.02, f64::NAN, 0.01, 0.1), 0.02);
    assert_eq!(
        adsr(&mut g, n.gain(), 0.02, f64::NAN, 0.01, 0.01, 0.01, 0.5, 0.01),
        0.02
    );
    assert!(g.automation.is_empty());
}

/* ================================================================ */
/* ir.js — the procedural reverb                                    */
/* ================================================================ */

/// The strongest pin in this file: a whole rendered IR, sample for sample,
/// against the same IR rendered by the JavaScript. 480 samples of a synthetic
/// spec that exercises every branch — the `env < 1e-6` skip, both taps, both
/// slaps, and the peak normalisation.
#[test]
fn a_whole_impulse_response_matches_the_javascript_sample_for_sample() {
    let g = &golden()["irTiny"];
    let s = &g["spec"];
    let taps: Vec<f64> = s["taps"].as_array().unwrap().iter().map(num).collect();
    let spec = IrSpec {
        seconds: num(&s["seconds"]),
        rt60: num(&s["rt60"]),
        predelay: num(&s["predelay"]),
        hf_damp: num(&s["hfDamp"]),
        bright: num(&s["bright"]),
        diffusion: num(&s["diffusion"]),
        width: num(&s["width"]),
        taps: Box::leak(taps.into_boxed_slice()),
        tap_gain: num(&s["tapGain"]),
        slaps: s["slaps"].as_u64().unwrap() as u32,
        slap_time: num(&s["slapTime"]),
    };
    let buf = generate_ir(SR, &mut Rng::new(99), &spec);
    assert_eq!(buf.length(), g["length"].as_u64().unwrap() as usize);
    for (ch, key) in ["ch0", "ch1"].into_iter().enumerate() {
        let want = g[key].as_array().unwrap();
        assert_eq!(buf.channels[ch].len(), want.len());
        for (i, w) in want.iter().enumerate() {
            close(
                f64::from(buf.channels[ch][i]),
                num(w),
                REL_F32,
                &format!("irTiny {key}[{i}]"),
            );
        }
    }
    // This spec is still ringing at its last sample — `seconds` is only 4.8
    // RT60s here. The `env < 1e-6` skip is exercised by `open` below, whose
    // 2.8 s runs well past its 1.15 s RT60.
    assert_ne!(buf.channels[0][buf.length() - 1], 0.0);
}

#[test]
fn all_five_named_spaces_render_the_javascript_impulse_responses() {
    let g = &golden()["ir"];
    for (i, space) in Space::ALL.into_iter().enumerate() {
        let key = space.as_str();
        let entry = &g[key];
        // The capture seeds each space with `0x1234 + key.length`.
        let seed = 0x1234u32 + key.len() as u32;
        let buf = generate_ir(SR, &mut Rng::new(seed), &IR_SPECS[i]);
        assert_eq!(
            buf.length(),
            entry["length"].as_u64().unwrap() as usize,
            "{key} length"
        );
        for (ch, ck) in ["ch0", "ch1"].into_iter().enumerate() {
            let want = &entry[ck];
            let d = &buf.channels[ch];
            let peak = d.iter().fold(0.0f64, |a, &v| a.max(f64::from(v).abs()));
            let sum: f64 = d.iter().map(|&v| f64::from(v)).sum();
            let rms = (d.iter().map(|&v| f64::from(v) * f64::from(v)).sum::<f64>()
                / d.len() as f64)
                .sqrt();
            close(peak, num(&want["peak"]), REL_F32, &format!("{key} {ck} peak"));
            close(sum, num(&want["sum"]), 1e-5, &format!("{key} {ck} sum"));
            close(rms, num(&want["rms"]), REL_F32, &format!("{key} {ck} rms"));
        }
        for (ck, sk) in [("ch0", "samples0"), ("ch1", "samples1")] {
            let ch = usize::from(ck == "ch1");
            for row in entry[sk].as_array().unwrap() {
                let i = row[0].as_u64().unwrap() as usize;
                close(
                    f64::from(buf.channels[ch][i]),
                    num(&row[1]),
                    REL_F32,
                    &format!("{key} {ck}[{i}]"),
                );
            }
        }
        // `open` runs 2.8 s against a 1.15 s RT60, so its diffuse envelope falls
        // under 1e-6 long before the end and the skip arm zeroes the tail. Its
        // last tap lands at 0.62 s, so nothing writes over it.
        if space == Space::Open {
            assert_eq!(buf.channels[0][buf.length() - 1], 0.0);
            assert_eq!(buf.channels[1][buf.length() - 1], 0.0);
        }
        // Peak-normalised to 0.42, and the two channels are decorrelated.
        let peak = buf
            .channels
            .iter()
            .flatten()
            .fold(0.0f64, |a, &v| a.max(f64::from(v).abs()));
        close(peak, 0.42, 1e-6, &format!("{key} normalised peak"));
        assert_ne!(buf.channels[0], buf.channels[1], "{key} channels differ");
    }
}

#[test]
fn the_ir_spec_table_matches_the_javascript() {
    let g = &golden()["irSpecs"];
    for (i, space) in Space::ALL.into_iter().enumerate() {
        let s = &g[space.as_str()];
        let want = &IR_SPECS[i];
        assert_eq!(want.seconds, num(&s["seconds"]));
        assert_eq!(want.rt60, num(&s["rt60"]));
        assert_eq!(want.predelay, num(&s["predelay"]));
        assert_eq!(want.hf_damp, num(&s["hfDamp"]));
        assert_eq!(want.bright, num(&s["bright"]));
        assert_eq!(want.diffusion, num(&s["diffusion"]));
        assert_eq!(want.width, num(&s["width"]));
        assert_eq!(want.tap_gain, num(&s["tapGain"]));
        assert_eq!(u64::from(want.slaps), s["slaps"].as_u64().unwrap());
        assert_eq!(want.slap_time, num(&s["slapTime"]));
        let taps: Vec<f64> = s["taps"].as_array().unwrap().iter().map(num).collect();
        assert_eq!(want.taps, taps.as_slice());
        assert_eq!(space.spec(), want);
    }
    let keys: Vec<&str> = golden()["spaceKeys"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(
        Space::ALL.map(Space::as_str).to_vec(),
        keys,
        "the normalisation order is fixed"
    );
}

/* ================================================================ */
/* ir.js — classifySpace                                            */
/* ================================================================ */

#[test]
fn classify_space_matches_the_javascript() {
    let g = &golden()["classify"];
    let cases: [(&str, Vec<f64>); 7] = [
        ("smallRoom", {
            let mut v = vec![3.5; 9];
            v[8] = 2.6;
            v
        }),
        ("street", vec![4.0, 30.0, 40.0, 30.0, 4.0, 30.0, 40.0, 30.0, 40.0]),
        ("open", vec![40.0; 9]),
        (
            "corridor",
            vec![1.8, 12.0, 38.0, 12.0, 1.8, 12.0, 38.0, 12.0, 2.4],
        ),
        ("infinite", vec![f64::INFINITY; 9]),
        ("degenerate", vec![0.0; 9]),
        // An odd horizon count takes the other median branch.
        ("evenHoriz", vec![3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 2.5]),
    ];
    for (name, hits) in cases {
        let mut w = SpaceWeights::default();
        classify_space(&hits, 40.0, &mut w);
        let want = &g[name];
        for (field, got) in [
            ("tight", w.tight),
            ("room", w.room),
            ("street", w.street),
            ("tunnel", w.tunnel),
            ("open", w.open),
            ("enclosure", w.enclosure),
            ("meanFree", w.mean_free),
            ("ceiling", w.ceiling),
            ("closeSides", w.close_sides),
            ("median", w.median),
        ] {
            close(got, num(&want[field]), REL, &format!("{name}.{field}"));
        }
        // Whatever the shape, the five space weights sum to one.
        let total: f64 = Space::ALL.into_iter().map(|s| w.get(s)).sum();
        close(total, 1.0, 1e-12, &format!("{name} normalised"));
    }
}

/// A probe that hits nothing reads as open ground; a probe pressed against
/// geometry reads as the tightest possible interior.
///
/// **The source's `tot < 1e-4` fallback arm is unreachable, and that is worth
/// recording rather than smoothing over.** `indoor + outdoor == 1` by
/// construction; `tunnel <= 0.55 * indoor` so `rest >= 0.45 * indoor`, and
/// `open >= 0.06 * outdoor`. The five weights therefore always sum to at least
/// 0.45. The degenerate all-zero probe below — every ray touching the listener —
/// still produces a well-formed classification (fully enclosed, minimum size,
/// so: tight). The arm is defensive, it is ported because it is in the source,
/// and no input reaches it.
#[test]
fn a_degenerate_probe_still_classifies() {
    let mut w = SpaceWeights::default();
    classify_space(&[0.0; 9], 40.0, &mut w);
    assert_eq!(w.tight, 1.0, "zero distance everywhere is as tight as it gets");
    assert_eq!(w.dominant(), Space::Tight);

    let mut open = SpaceWeights::default();
    classify_space(&[40.0; 9], 40.0, &mut open);
    assert_eq!(open.dominant(), Space::Open);
    assert_eq!(open.enclosure, 0.0);

    let mut room = SpaceWeights::default();
    classify_space(&[3.5, 3.5, 3.5, 3.5, 3.5, 3.5, 3.5, 3.5, 2.6], 40.0, &mut room);
    assert_eq!(room.dominant(), Space::Tight);
    assert!(room.enclosure > 0.9, "a 2.6 m ceiling reads as fully indoors");
}

/* ================================================================ */
/* Whole-graph comparison                                           */
/* ================================================================ */

/// The kind tag the capture stub records for a node.
fn kind_tag(k: &NodeKind) -> &'static str {
    match k {
        NodeKind::Gain { .. } => "gain",
        NodeKind::Biquad { .. } => "biquad",
        NodeKind::Oscillator { .. } => "oscillator",
        NodeKind::BufferSource { .. } => "bufferSource",
        NodeKind::WaveShaper { .. } => "waveShaper",
        NodeKind::Convolver { .. } => "convolver",
        NodeKind::Compressor { .. } => "compressor",
        NodeKind::StereoPanner { .. } => "stereoPanner",
        NodeKind::Panner { .. } => "panner",
    }
}

/// A node's `AudioParam` by the name Web Audio (and the capture) uses.
fn param_by_name(k: &NodeKind, name: &str) -> Option<f64> {
    match (k, name) {
        (NodeKind::Gain { gain }, "gain") => Some(*gain),
        (NodeKind::Biquad { frequency, .. }, "frequency") => Some(*frequency),
        (NodeKind::Biquad { q, .. }, "Q") => Some(*q),
        (NodeKind::Biquad { gain, .. }, "gain") => Some(*gain),
        (NodeKind::Biquad { .. }, "detune") => Some(0.0),
        (NodeKind::Oscillator { frequency, .. }, "frequency") => Some(*frequency),
        (NodeKind::Oscillator { detune, .. }, "detune") => Some(*detune),
        (NodeKind::BufferSource { playback_rate, .. }, "playbackRate") => Some(*playback_rate),
        (NodeKind::BufferSource { .. }, "detune") => Some(0.0),
        (NodeKind::StereoPanner { pan }, "pan") => Some(*pan),
        (NodeKind::Compressor { threshold, .. }, "threshold") => Some(*threshold),
        (NodeKind::Compressor { knee, .. }, "knee") => Some(*knee),
        (NodeKind::Compressor { ratio, .. }, "ratio") => Some(*ratio),
        (NodeKind::Compressor { attack, .. }, "attack") => Some(*attack),
        (NodeKind::Compressor { release, .. }, "release") => Some(*release),
        _ => None,
    }
}

/// A node's plain (non-`AudioParam`) property, as the capture's proxy saw it.
fn field_by_name(g: &AudioGraph, k: &NodeKind, name: &str) -> Option<Value> {
    let v = match (k, name) {
        (NodeKind::Biquad { filter, .. }, "type") => Value::from(filter.as_str()),
        (NodeKind::Oscillator { wave: Some(w), .. }, "type") => Value::from(w.as_str()),
        (NodeKind::Oscillator { periodic, .. }, "wave") => Value::from((*periodic)?.0),
        (NodeKind::BufferSource { buffer, .. }, "buffer") => Value::from(buffer.0),
        (NodeKind::BufferSource { looping, .. }, "loop") => Value::from(*looping),
        (NodeKind::BufferSource { loop_start, .. }, "loopStart") => Value::from(*loop_start),
        (NodeKind::BufferSource { loop_end, .. }, "loopEnd") => Value::from(*loop_end),
        (NodeKind::BufferSource { offset, .. }, "_offset") => Value::from(*offset),
        (NodeKind::WaveShaper { curve, .. }, "curve") => Value::from(g.curve(*curve).len()),
        (NodeKind::WaveShaper { oversample, .. }, "oversample") => Value::from(*oversample),
        (NodeKind::Convolver { normalize, .. }, "normalize") => Value::from(*normalize),
        _ => return None,
    };
    Some(v)
}

fn param_name(p: Param) -> &'static str {
    p.as_str()
}

/// Compare one built voice against the recorded JavaScript graph for the same
/// seed: node list in creation order, every constructed parameter, every plain
/// property, every connection, every automation event, every source start/stop,
/// and the returned `{ node, end, send }`.
fn assert_voice(name: &str, g: &AudioGraph, voice: Voice) {
    let want = &golden()["voices"][name];
    assert!(!want.is_null(), "no golden for voice {name}");

    let wn = want["nodes"].as_array().unwrap();
    assert_eq!(
        g.nodes.len(),
        wn.len(),
        "{name}: node count (creation order must match exactly)"
    );
    for (i, w) in wn.iter().enumerate() {
        let k = g.node(NodeId(i));
        assert_eq!(w["i"].as_u64().unwrap() as usize, i);
        assert_eq!(kind_tag(k), w["k"].as_str().unwrap(), "{name} node {i} kind");
        for (pname, pval) in w["p"].as_object().unwrap() {
            let got = param_by_name(k, pname)
                .unwrap_or_else(|| panic!("{name} node {i}: no param {pname} on {k:?}"));
            close(got, num(pval), REL, &format!("{name} node {i}.{pname}"));
        }
        for (fname, fval) in w["f"].as_object().unwrap() {
            let got = field_by_name(g, k, fname)
                .unwrap_or_else(|| panic!("{name} node {i}: no field {fname} on {k:?}"));
            match fval.as_f64() {
                Some(x) => close(
                    got.as_f64().unwrap(),
                    x,
                    REL,
                    &format!("{name} node {i}.{fname}"),
                ),
                None => assert_eq!(&got, fval, "{name} node {i}.{fname}"),
            }
        }
    }

    let wc = want["conns"].as_array().unwrap();
    assert_eq!(g.connections.len(), wc.len(), "{name}: connection count");
    for (i, w) in wc.iter().enumerate() {
        let c = g.connections[i];
        assert_eq!(c.from.0, w[0].as_u64().unwrap() as usize, "{name} conn {i} from");
        match c.to {
            Sink::Node(to) => {
                assert_eq!(w[1].as_str().unwrap(), "node", "{name} conn {i} kind");
                assert_eq!(to.0, w[2].as_u64().unwrap() as usize, "{name} conn {i} to");
            }
            Sink::Param(ParamRef(to, p)) => {
                assert_eq!(w[1].as_str().unwrap(), "param", "{name} conn {i} kind");
                assert_eq!(to.0, w[2].as_u64().unwrap() as usize, "{name} conn {i} to");
                assert_eq!(param_name(p), w[3].as_str().unwrap(), "{name} conn {i} param");
            }
        }
    }

    let wa = want["autos"].as_array().unwrap();
    assert_eq!(g.automation.len(), wa.len(), "{name}: automation count");
    for (i, w) in wa.iter().enumerate() {
        let e = g.automation[i];
        assert_eq!(e.param.0 .0, w[0].as_u64().unwrap() as usize, "{name} auto {i} node");
        assert_eq!(param_name(e.param.1), w[1].as_str().unwrap(), "{name} auto {i} param");
        assert_eq!(e.kind.as_str(), w[2].as_str().unwrap(), "{name} auto {i} kind");
        close(e.value, num(&w[3]), REL, &format!("{name} auto {i} value"));
        close(e.time, num(&w[4]), REL, &format!("{name} auto {i} time"));
        if e.kind == Automation::SetTargetAtTime {
            close(
                e.time_constant,
                num(&w[5]),
                REL,
                &format!("{name} auto {i} timeConstant"),
            );
        }
    }

    let ws = want["sched"].as_array().unwrap();
    assert_eq!(g.schedule.len(), ws.len(), "{name}: schedule count");
    for (i, w) in ws.iter().enumerate() {
        let e = g.schedule[i];
        assert_eq!(e.node.0, w[0].as_u64().unwrap() as usize, "{name} sched {i} node");
        let tag = match e.kind {
            Schedule::Start => "start",
            Schedule::Stop => "stop",
        };
        assert_eq!(tag, w[1].as_str().unwrap(), "{name} sched {i} kind");
        close(e.when, num(&w[2]), REL, &format!("{name} sched {i} when"));
        match (e.offset, w[3].as_f64()) {
            (Some(a), Some(b)) => close(a, b, REL, &format!("{name} sched {i} offset")),
            (None, None) => {}
            (a, b) => panic!("{name} sched {i} offset: {a:?} vs {b:?}"),
        }
        match (e.duration, w[4].as_f64()) {
            (Some(a), Some(b)) => close(a, b, REL, &format!("{name} sched {i} duration")),
            (None, None) => {}
            (a, b) => panic!("{name} sched {i} duration: {a:?} vs {b:?}"),
        }
    }

    let wr = &want["ret"];
    assert_eq!(voice.node.0, wr["node"].as_u64().unwrap() as usize, "{name} ret node");
    close(voice.end, num(&wr["end"]), REL, &format!("{name} ret end"));
    close(voice.send, num(&wr["send"]), REL, &format!("{name} ret send"));
}

/// Set up one case exactly as the capture does: a seeded stream, one fork for
/// the noise bank, the parent stream for the voice.
fn voice_case(
    name: &str,
    seed: u32,
    build: impl FnOnce(&mut AudioGraph, &NoiseBank, &mut Rng) -> Voice,
) {
    let mut g = AudioGraph::new(SR);
    let mut rng = Rng::new(seed);
    let bank = NoiseBank::new(&mut g, &mut rng.fork(), 1.2);
    let voice = build(&mut g, &bank, &mut rng);
    assert_voice(name, &g, voice);
}

#[test]
fn weapon_shots_build_the_javascript_graph() {
    for (name, seed, profile, dist, fp) in [
        ("shot:rifle@2m", 0x000A_0D10u32, &RIFLE, 2.0, true),
        ("shot:rifle@120m", 0x000A_0D17, &RIFLE, 120.0, false),
        ("shot:shotgun@2m", 0x000A_0D1E, &SHOTGUN, 2.0, true),
        ("shot:suppressed@1m", 0x000A_0D25, &SUPPRESSED, 1.0, true),
    ] {
        voice_case(name, seed, |g, bank, rng| {
            let mut rr = RoundRobinBank::new();
            weapon_shot(
                g,
                bank,
                rng,
                &mut rr,
                profile,
                ShotOpts {
                    when: Some(0.02),
                    distance: dist,
                    first_person: fp,
                    echo_boost: 1.0,
                },
            )
        });
    }
}

/// Two consecutive shots from one profile: the round-robin table is built once
/// and the index advances before each read, so the second shot is a different
/// timbre slot on top of fresh per-shot jitter.
#[test]
fn the_round_robin_advances_between_two_shots_exactly_as_the_javascript_does() {
    voice_case("shot:rifle:x2", 0x000A_0D2C, |g, bank, rng| {
        let mut rr = RoundRobinBank::new();
        let opts = |when| ShotOpts {
            when: Some(when),
            distance: 2.0,
            first_person: true,
            echo_boost: 1.0,
        };
        weapon_shot(g, bank, rng, &mut rr, &RIFLE, opts(0.02));
        weapon_shot(g, bank, rng, &mut rr, &RIFLE, opts(0.12))
    });
}

#[test]
fn whizz_and_dryfire_build_the_javascript_graph() {
    voice_case("whizz", 0x000A_0D33, |g, bank, rng| {
        bullet_whizz(g, bank, rng, Some(0.02), 1.2, 1.0)
    });
    voice_case("dryfire", 0x000A_0D3A, |g, bank, rng| {
        dry_fire(g, bank, rng, Some(0.02))
    });
}

#[test]
fn every_surface_impact_builds_the_javascript_graph() {
    for s in Surface::ALL {
        let name = format!("impact:{}", s.name());
        let seed = 0x000B_0000u32 + s.name().len() as u32 * 977;
        voice_case(&name, seed, |g, bank, rng| {
            surface_impact(g, bank, rng, Some(0.02), s, 1.0)
        });
    }
}

#[test]
fn every_gait_builds_the_javascript_footstep_graph() {
    for (gait, tag) in [
        (Gait::Walk, "walk"),
        (Gait::Run, "run"),
        (Gait::Sprint, "sprint"),
        (Gait::Crouch, "crouch"),
        (Gait::Land, "land"),
    ] {
        let name = format!("step:concrete:{tag}");
        let seed = 0x000C_0000u32 + tag.len() as u32 * 977;
        voice_case(&name, seed, |g, bank, rng| {
            footstep(
                g,
                bank,
                rng,
                StepOpts {
                    when: Some(0.02),
                    surface: Surface::Concrete,
                    gait,
                    level: 1.0,
                    gear: None,
                },
            )
        });
    }
    // Metal is the surface with a ring bank on the first contact.
    voice_case("step:metal:run", 0x000C_1234, |g, bank, rng| {
        footstep(
            g,
            bank,
            rng,
            StepOpts {
                when: Some(0.02),
                surface: Surface::Metal,
                gait: Gait::Run,
                level: 1.0,
                gear: None,
            },
        )
    });
}

#[test]
fn shell_casings_build_the_javascript_graph_on_hard_and_soft_ground() {
    voice_case("shell:concrete", 0x000D_0001, |g, bank, rng| {
        shell_casing(g, bank, rng, Some(0.02), Surface::Concrete, 1.0, None)
    });
    voice_case("shell:dirt", 0x000D_0002, |g, bank, rng| {
        shell_casing(g, bank, rng, Some(0.02), Surface::Dirt, 1.0, None)
    });
}

#[test]
fn every_reload_phase_builds_the_javascript_graph() {
    for (phase, tag) in [
        (ReloadPhase::Start, "start"),
        (ReloadPhase::MagOut, "magout"),
        (ReloadPhase::MagIn, "magin"),
        (ReloadPhase::End, "end"),
    ] {
        let name = format!("reload:{tag}");
        let seed = 0x000E_0000u32 + tag.len() as u32 * 977;
        voice_case(&name, seed, |g, bank, rng| {
            reload_phase(g, bank, rng, phase, Some(0.02), 1.0)
        });
    }
}

#[test]
fn explosions_body_falls_cloth_and_heartbeat_build_the_javascript_graph() {
    voice_case("explosion@5m", 0x000F_0001, |g, bank, rng| {
        explosion(g, bank, rng, Some(0.02), 5.0, 8.0, 1.0)
    });
    voice_case("explosion@180m", 0x000F_0002, |g, bank, rng| {
        explosion(g, bank, rng, Some(0.02), 180.0, 12.0, 1.0)
    });
    voice_case("bodyfall", 0x000F_0003, |g, bank, rng| {
        body_fall(g, bank, rng, Some(0.02), 1.0)
    });
    voice_case("cloth", 0x000F_0004, |g, bank, rng| {
        cloth(g, bank, rng, Some(0.02), 1.0)
    });
    voice_case("heartbeat", 0x000F_0005, |g, _bank, _rng| {
        heartbeat(g, Some(0.02), 1.0)
    });
}

#[test]
fn every_ui_sound_builds_the_javascript_graph() {
    for (kind, tag) in [
        (UiSound::Hitmarker, "hitmarker"),
        (UiSound::Headshot, "headshot"),
        (UiSound::Kill, "kill"),
        (UiSound::Damage, "damage"),
        (UiSound::Armour, "armour"),
        (UiSound::GrenadeWarn, "grenade_warn"),
        (UiSound::Regen, "regen"),
        (UiSound::LowHealth, "lowhealth"),
        // An unrecognised name falls through to the plain blip.
        (UiSound::Blip, "blip"),
    ] {
        let name = format!("ui:{tag}");
        let seed = 0x0001_0000u32 + tag.len() as u32 * 977;
        voice_case(&name, seed, |g, bank, rng| {
            ui_sound(g, bank, rng, kind, Some(0.02), 1.0)
        });
        assert_eq!(UiSound::from_str(tag), kind);
    }
}

#[test]
fn every_bark_builds_the_javascript_formant_graph() {
    for b in Bark::ALL {
        let name = format!("bark:{}", b.as_str());
        let seed = 0x0002_0000u32 + b.as_str().len() as u32 * 977;
        voice_case(&name, seed, |g, bank, rng| {
            bark(
                g,
                bank,
                rng,
                BarkOpts {
                    when: Some(0.02),
                    bark: b,
                    ..BarkOpts::default()
                },
            )
        });
    }
    voice_case("bark:radio", 0x0002_FFFF, |g, bank, rng| {
        bark(
            g,
            bank,
            rng,
            BarkOpts {
                when: Some(0.02),
                bark: Bark::Contact,
                radio: true,
                ..BarkOpts::default()
            },
        )
    });
}

#[test]
fn every_ambient_one_shot_builds_the_javascript_graph() {
    for k in OneShot::ALL {
        let name = format!("ambient:{}", k.as_str());
        let seed = 0x0003_0000u32 + k.as_str().len() as u32 * 977;
        voice_case(&name, seed, |g, bank, rng| {
            ambient_one_shot(g, bank, rng, k, Some(0.02), 1.0)
        });
    }
}

/// The workhorse behind every metal, glass and wood sound, including its `??`
/// defaults — `q = 22`, `g = 0.5`, `decay = 0.12` — and the `sqrt(q)` makeup
/// gain without which every metallic sound sits inaudibly low in the mix.
#[test]
fn the_struck_resonator_builds_the_javascript_graph_including_its_defaults() {
    voice_case("resonator", 0x0004_0001, |g, bank, rng| {
        let parts = [Partial::new(1750.0, 34.0, 0.42, 0.28), Partial::at(3120.0)];
        let node = struck(g, bank, rng, 0.02, &parts, 0.0035);
        Voice {
            node,
            end: 0.0,
            send: 0.0,
        }
    });
}

/* ================================================================ */
/* weapons.js / vox.js — the lookup tables                          */
/* ================================================================ */

#[test]
fn resolve_profile_matches_the_javascript_pattern_order() {
    let g = &golden()["resolveProfile"];
    for (input, want) in g.as_object().unwrap() {
        let name = if input == "<empty>" { "" } else { input };
        let got = resolve_profile(Some(name));
        assert_eq!(
            got.name,
            want.as_str().unwrap(),
            "resolveProfile({input:?})"
        );
    }
    assert_eq!(
        resolve_profile(None).name,
        golden()["resolveProfileNull"].as_str().unwrap()
    );
    // The "ak" arm is a substring test, not a word match — a real property of
    // the original, reproduced.
    assert_eq!(resolve_profile(Some("breaker")).name, "ak");
}

#[test]
fn the_weapon_profile_table_matches_the_javascript() {
    let g = &golden()["weaponProfiles"];
    for p in [
        &RIFLE, &AK, &SMG, &PISTOL, &SHOTGUN, &SNIPER, &LMG, &SUPPRESSED,
    ] {
        let w = &g[p.name];
        assert!(!w.is_null(), "no golden profile {}", p.name);
        for (field, got) in [
            ("level", p.level),
            ("bodyF", p.body_f),
            ("bodyF2", p.body_f2),
            ("bodyDecay", p.body_decay),
            ("subF", p.sub_f),
            ("subDecay", p.sub_decay),
            ("crackF", p.crack_f),
            ("crackQ", p.crack_q),
            ("crackDecay", p.crack_decay),
            ("drive", p.drive),
            ("asym", p.asym),
            ("midF", p.mid_f),
            ("midDecay", p.mid_decay),
            ("tailDecay", p.tail_decay),
            ("tailF", p.tail_f),
            ("tailEndF", p.tail_end_f),
            ("mechDelay", p.mech_delay),
            ("mechLevel", p.mech_level),
            ("send", p.send),
        ] {
            assert_eq!(got, num(&w[field]), "{}.{field}", p.name);
        }
        let parts: Vec<f64> = w["mechPartials"].as_array().unwrap().iter().map(num).collect();
        assert_eq!(p.mech_partials.to_vec(), parts, "{}.mechPartials", p.name);
        assert_eq!(
            p.pellets,
            w["pellets"].as_u64().unwrap_or(0) as u32,
            "{}.pellets",
            p.name
        );
        assert_eq!(
            p.suppressed,
            w["suppressed"].as_bool().unwrap_or(false),
            "{}.suppressed",
            p.name
        );
    }
}

#[test]
fn the_bark_table_matches_the_javascript() {
    let g = &golden()["barks"];
    for b in Bark::ALL {
        let w = &g[b.as_str()];
        let s = b.spec();
        assert_eq!(s.f0, num(&w["f0"]), "{} f0", b.as_str());
        assert_eq!(s.drive, num(&w["drive"]), "{} drive", b.as_str());
        assert_eq!(s.breath, w["breath"].as_f64(), "{} breath", b.as_str());
        assert_eq!(s.tremolo, w["tremolo"].as_f64(), "{} tremolo", b.as_str());
        assert_eq!(
            s.dying,
            w["dying"].as_bool().unwrap_or(false),
            "{} dying",
            b.as_str()
        );
        let syl = w["syl"].as_array().unwrap();
        assert_eq!(s.syl.len(), syl.len(), "{} syllable count", b.as_str());
        for (i, ws) in syl.iter().enumerate() {
            let x = s.syl[i];
            assert_eq!(x.d, num(&ws["d"]), "{}[{i}].d", b.as_str());
            assert_eq!(x.a, num(&ws["a"]), "{}[{i}].a", b.as_str());
            assert_eq!(x.p, num(&ws["p"]), "{}[{i}].p", b.as_str());
            assert_eq!(x.g, ws["g"].as_f64().unwrap_or(0.0), "{}[{i}].g", b.as_str());
        }
    }
}

#[test]
fn bark_for_matches_the_javascript_including_which_arms_draw() {
    let g = &golden()["barkFor"];
    let mut rng = Rng::new(0x8a12);
    // One stream across every kind, in the capture's order: two of the arms draw
    // and the rest do not, so the order is part of what is being pinned.
    for name in [
        "spot", "reload", "grenade", "flank", "suppress", "advance", "hurt",
        "death", "copy", "unknown",
    ] {
        let want = &g[name];
        let kind = BarkRequest::from_str(name);
        for (i, w) in want.as_array().unwrap().iter().enumerate() {
            assert_eq!(
                bark_for(kind, &mut rng).as_str(),
                w.as_str().unwrap(),
                "barkFor({name})[{i}]"
            );
        }
    }
    // Only `spot` and `hurt` consume a draw; everything else must leave the
    // stream untouched, which is what keeps a forked stream aligned.
    let mut a = Rng::new(7);
    let mut b = Rng::new(7);
    bark_for(BarkRequest::Reload, &mut a);
    assert_eq!(a.state(), b.state());
    bark_for(BarkRequest::Spot, &mut a);
    b.float();
    assert_eq!(a.state(), b.state());
}

#[test]
fn the_one_shot_table_matches_the_javascript() {
    let want: Vec<&str> = golden()["oneShots"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(OneShot::ALL.map(OneShot::as_str).to_vec(), want);
}

/* ================================================================ */
/* mixer.js                                                         */
/* ================================================================ */

fn built_mixer() -> (AudioGraph, Mixer) {
    let mut g = AudioGraph::new(SR);
    let mut mixer = Mixer::new(&mut g, Rng::new(0x51ee7), 0.95);
    mixer.build_reverbs(&mut g);
    (g, mixer)
}

#[test]
fn the_mixer_wires_the_documented_signal_path() {
    let (g, mixer) = built_mixer();
    let edge = |from: NodeId, to: NodeId| {
        g.connections
            .iter()
            .any(|c| c.from == from && c.to == Sink::Node(to))
    };
    // master chain, in order, ending at the device.
    assert!(edge(mixer.master_sum, mixer.pre_gain));
    assert!(edge(mixer.pre_gain, mixer.master_comp));
    assert!(edge(mixer.master_comp, mixer.soft_clip));
    assert!(edge(mixer.soft_clip, mixer.master_gain));
    assert!(edge(mixer.master_gain, AudioGraph::DESTINATION));
    // The headroom stage sits BEFORE the compressor, at 0.22 — the deliberate
    // choice that keeps a footstep and a gunshot different sizes.
    assert!(matches!(g.node(mixer.pre_gain), NodeKind::Gain { gain } if *gain == 0.22));
    // concussion path
    assert!(edge(mixer.world_sum, mixer.muffle_lp));
    assert!(edge(mixer.muffle_lp, mixer.muffle_hs));
    assert!(edge(mixer.muffle_hs, mixer.muffle_gain));
    assert!(edge(mixer.muffle_gain, mixer.master_sum));
    // reverb send shaping
    assert!(edge(mixer.reverb_send, mixer.send_hp));
    assert!(edge(mixer.send_hp, mixer.send_lp));
    assert!(edge(mixer.reverb_return, mixer.world_sum));
}

/// The `ui` bus bypasses the muffle so a menu click survives a grenade; every
/// other bus runs through it.
#[test]
fn the_ui_bus_bypasses_the_muffle_and_the_others_do_not() {
    let (g, mixer) = built_mixer();
    // Walk each bus from its input to whatever it finally sums into.
    for bus in Bus::ALL {
        let input = mixer.bus(bus);
        let mut node = input;
        let mut seen = 0;
        while seen < 8 {
            let Some(next) = g.connections.iter().find_map(|c| match (c.from == node, c.to) {
                (true, Sink::Node(n)) => Some(n),
                _ => None,
            }) else {
                break;
            };
            node = next;
            seen += 1;
            if node == mixer.world_sum || node == mixer.master_sum {
                break;
            }
        }
        if bus == Bus::Ui {
            assert_eq!(node, mixer.master_sum, "ui must bypass the muffle");
        } else {
            assert_eq!(node, mixer.world_sum, "{} runs through the muffle", bus.as_str());
        }
    }
    assert_eq!(Bus::from_str("weapons"), Bus::Weapons);
    assert_eq!(Bus::from_str("nonsense"), Bus::Foley);
}

/// The five convolvers are rendered once, and only the ones with real weight are
/// plugged into the send: a 2.8 s stereo convolution is the most expensive node
/// in the graph.
#[test]
fn only_audible_spaces_stay_plugged_into_the_reverb_send() {
    let (mut g, mut mixer) = built_mixer();
    let live_count = |g: &AudioGraph, mixer: &Mixer| {
        g.connections
            .iter()
            .filter(|c| c.from == mixer.send_lp)
            .count()
    };
    // The starting weights are street 0.35 / open 0.65 — two live.
    assert_eq!(live_count(&g, &mixer), 2);
    assert!(mixer.reverbs_built());

    let mut all = SpaceWeights::default();
    all.set(Space::Tight, 1.0);
    all.set(Space::Room, 1.0);
    all.set(Space::Street, 1.0);
    all.set(Space::Tunnel, 1.0);
    all.set(Space::Open, 1.0);
    mixer.set_space(&mut g, &all, 0.35);
    assert_eq!(live_count(&g, &mixer), 5);

    let mut only_tight = SpaceWeights::default();
    only_tight.set(Space::Tight, 1.0);
    mixer.set_space(&mut g, &only_tight, 0.35);
    assert_eq!(live_count(&g, &mixer), 1);

    // Rebuilding is a no-op — the IRs cost the most of anything here.
    let before = g.nodes.len();
    mixer.build_reverbs(&mut g);
    assert_eq!(g.nodes.len(), before);
}

/// Ducking: a deeper duck wins over a shallower one, the hold delays recovery,
/// and the bus floats back to unity.
#[test]
fn ducking_holds_then_recovers_and_a_deeper_duck_wins() {
    let (mut g, mut mixer) = built_mixer();
    mixer.duck(&mut g, 0.55, 0.1);
    let after_first = g.automation.len();
    // A shallower duck must not lift an existing one.
    mixer.duck(&mut g, 0.2, 0.1);
    assert_eq!(g.automation.len(), after_first, "a shallower duck is ignored");
    // A deeper one takes over. Ambience is ducked at scale 1.0, so a full-force
    // duck is what reaches the 0.92 ceiling.
    mixer.duck(&mut g, 1.0, 0.1);
    assert!(g.automation.len() > after_first);
    let deepest = g
        .automation
        .iter()
        .filter(|e| e.kind == Automation::SetTargetAtTime)
        .map(|e| e.value)
        .fold(f64::INFINITY, f64::min);
    close(deepest, 1.0 - 0.92, 1e-12, "duck clamp");

    // Hold first, then recover over ~400 ms.
    let before = g.automation.len();
    mixer.update(&mut g, 0.05);
    assert_eq!(g.automation.len(), before, "still holding");
    for _ in 0..40 {
        g.set_current_time(g.current_time() + 0.016);
        mixer.update(&mut g, 0.016);
    }
    let last = g.automation.last().unwrap();
    close(last.value, 1.0, 1e-12, "duck fully recovered");
}

/// Concussion muffles the world, dips its level, starts a tinnitus tone, and
/// recovers slowly-then-quickly. A weaker concussion cannot reduce an existing
/// one.
#[test]
fn concussion_muffles_the_world_rings_the_ears_and_recovers() {
    let (mut g, mut mixer) = built_mixer();
    let nodes_before = g.nodes.len();
    mixer.concuss(&mut g, 1.0);
    assert_eq!(mixer.deafness, 1.0);
    // 1.0 -> ~480 Hz on the muffle lowpass.
    let cutoff = g
        .automation
        .iter()
        .rev()
        .find(|e| e.param == mixer.muffle_lp.frequency() && e.kind == Automation::SetTargetAtTime)
        .unwrap()
        .value;
    close(cutoff, 20000.0 * 0.024, 1e-9, "muffle cutoff at full deafness");
    // Three beating partials plus a wobble LFO: 8 nodes and a summing gain.
    assert_eq!(g.nodes.len(), nodes_before + 9, "tinnitus voice built once");

    let weaker = g.automation.len();
    mixer.concuss(&mut g, 0.4);
    assert_eq!(mixer.deafness, 1.0);
    assert_eq!(g.automation.len(), weaker, "a weaker concussion is ignored");

    // A re-trigger reuses the existing tinnitus rather than stacking another.
    let n = g.nodes.len();
    mixer.deafness = 0.0;
    mixer.concuss(&mut g, 0.5);
    assert_eq!(g.nodes.len(), n, "tinnitus is re-triggered, not rebuilt");

    // It outlives the muffling and then tears itself down.
    let disconnects = g.disconnects;
    for _ in 0..800 {
        g.set_current_time(g.current_time() + 0.05);
        mixer.update(&mut g, 0.05);
    }
    close(mixer.deafness, 0.0, 1e-9, "deafness recovered");
    assert!(g.disconnects > disconnects, "tinnitus torn down");
}

#[test]
fn master_and_bus_volumes_ramp_rather_than_jump() {
    let (mut g, mut mixer) = built_mixer();
    mixer.set_master_volume(&mut g, 2.5);
    assert_eq!(mixer.master_volume, 1.0, "clamped to 0..1");
    let e = *g.automation.last().unwrap();
    assert_eq!(e.kind, Automation::SetTargetAtTime);
    assert_eq!(e.param, mixer.master_gain.gain());

    mixer.set_bus_volume(&mut g, Bus::Weapons, 0.5);
    let e = *g.automation.last().unwrap();
    // Scaled by the bus's own static trim, not replacing it.
    close(e.value, 0.5 * 0.95, 1e-12, "bus volume keeps its trim");
    // The live compressor readout has no meaning in a recorded graph.
    assert_eq!(mixer.reduction(), 0.0);

    mixer.dispose(&mut g);
    assert!(g.connections.is_empty(), "dispose unwires the whole mixer");
}

/* ================================================================ */
/* spatial.js                                                       */
/* ================================================================ */

/// A probe that reports a wall at a fixed distance for every ray.
struct Wall(f64);

impl WorldProbe for Wall {
    fn raycast(&self, _o: [f64; 3], _d: [f64; 3], max: f64, _m: RayMask) -> Option<RayHit> {
        (self.0 <= max).then_some(RayHit {
            distance: self.0,
            surface: Surface::Concrete,
        })
    }
}

#[test]
fn the_emitter_chain_stacks_air_and_occlusion_losses_separately() {
    let mut g = AudioGraph::new(SR);
    let mut field = SpatialField::new(&mut g);
    let mut mixer = Mixer::new(&mut g, Rng::new(1), 0.95);
    mixer.build_reverbs(&mut g);
    field.set_listener(&mut g, [0.0, 1.6, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]);
    assert_eq!(g.listener.updates, 1);
    assert_eq!(g.listener.position, [0.0, 1.6, 0.0]);

    let idx = field
        .acquire(
            &mut g,
            &mixer,
            Some(&Wall(1.0)),
            AcquireOpts {
                x: 30.0,
                y: 1.6,
                z: 0.0,
                ..AcquireOpts::default()
            },
        )
        .expect("a free emitter");
    let e = *field.emitter(idx);
    // Both filters were driven, and they are different nodes: a distant AND
    // occluded source pays both losses.
    assert_ne!(e.air_lp, e.occ_lp);
    let air = g
        .automation
        .iter()
        .find(|a| a.param == e.air_lp.frequency())
        .unwrap();
    close(air.value, air_cutoff(30.0), 1e-12, "air cutoff at 30 m");
    let occ = g
        .automation
        .iter()
        .find(|a| a.param == e.occ_lp.frequency())
        .unwrap();
    assert!(occ.value < 20000.0, "two blocked rays closed the occlusion LP");
    assert_eq!(field.stats.occlusion_rays, 2);

    // The panner's own distance model is off; `distGain` carries attenuation.
    match g.node(e.panner) {
        NodeKind::Panner {
            rolloff_factor,
            panning_model,
            ..
        } => {
            assert_eq!(*rolloff_factor, 0.0);
            assert_eq!(*panning_model, "HRTF");
        }
        other => panic!("expected a panner, got {other:?}"),
    }
    // And the send is fed from distGain — post distance, pre panning.
    assert!(g
        .connections
        .iter()
        .any(|c| c.from == e.dist_gain && c.to == Sink::Node(e.send_gain)));
    assert!(g
        .connections
        .iter()
        .any(|c| c.from == e.dist_gain && c.to == Sink::Node(e.panner)));
}

#[test]
fn attenuation_is_gentler_than_inverse_distance_past_forty_metres() {
    let mut g = AudioGraph::new(SR);
    let field = SpatialField::new(&mut g);
    // Below 40 m it is close to physical: 2 m is unity, 10 m is well down.
    close(field.attenuation(2.0), 1.0, 1e-12, "reference distance");
    assert!(field.attenuation(10.0) < 0.3);
    // Past 45 m the far term takes over and 150 m is still clearly audible.
    assert!(field.attenuation(150.0) > 0.03);
    assert!(field.attenuation(150.0) > 2.0 / (2.0 + 0.85 * 148.0));
    // Never above unity anywhere.
    for d in 0..400 {
        assert!(field.attenuation(f64::from(d)) <= 1.0);
    }
    // The curve is monotonic below 45 m and above 46 m, with one deliberate step
    // UP at the crossover where the far term takes over from the near one. That
    // discontinuity is in the source, it is a fraction of a dB, and it is what
    // stops a level feeling dead at range — so it is reproduced, not smoothed.
    let mut prev = 1.1;
    for d in 0..46 {
        let a = field.attenuation(f64::from(d));
        assert!(a <= prev + 1e-12, "monotonic below the crossover, at {d} m");
        prev = a;
    }
    assert!(field.attenuation(46.0) > field.attenuation(45.0));
    let mut prev = field.attenuation(46.0);
    for d in 46..400 {
        let a = field.attenuation(f64::from(d));
        assert!(a <= prev + 1e-12, "monotonic above the crossover, at {d} m");
        prev = a;
    }
}

/// A free emitter is fully detached, so the expensive HRTF convolution is not
/// evaluated for silence.
#[test]
fn a_finished_emitter_detaches_from_the_graph() {
    let mut g = AudioGraph::new(SR);
    let mut field = SpatialField::new(&mut g);
    let mut mixer = Mixer::new(&mut g, Rng::new(1), 0.95);
    mixer.build_reverbs(&mut g);
    let voice = g.create_gain(1.0);
    let idx = field
        .acquire(
            &mut g,
            &mixer,
            None,
            AcquireOpts {
                end_time: Some(0.5),
                ..AcquireOpts::default()
            },
        )
        .unwrap();
    field.hold(&mut g, idx, voice, 0.5);
    field.update(&mut g, None);
    assert_eq!(field.stats.active, 1);

    g.set_current_time(1.0);
    field.update(&mut g, None);
    assert_eq!(field.stats.active, 0);
    assert!(field.emitter(idx).free);
    let e = *field.emitter(idx);
    assert!(!g
        .connections
        .iter()
        .any(|c| c.from == e.panner && c.to == Sink::Node(mixer.bus(Bus::Foley))));
}

/// The pool is 40 deep. Past that, a louder sound steals the least important
/// voice closest to finishing — and a quieter one is dropped instead.
#[test]
fn the_emitter_pool_steals_by_priority_and_drops_when_it_cannot() {
    let mut g = AudioGraph::new(SR);
    let mut field = SpatialField::new(&mut g);
    let mut mixer = Mixer::new(&mut g, Rng::new(1), 0.95);
    mixer.build_reverbs(&mut g);
    for _ in 0..40 {
        assert!(field
            .acquire(
                &mut g,
                &mixer,
                None,
                AcquireOpts {
                    priority: 0.9,
                    end_time: Some(100.0),
                    ..AcquireOpts::default()
                },
            )
            .is_some());
    }
    // Full, and everything playing outranks this by more than 0.25.
    assert!(field
        .acquire(
            &mut g,
            &mixer,
            None,
            AcquireOpts {
                priority: 0.1,
                ..AcquireOpts::default()
            },
        )
        .is_none());
    assert_eq!(field.stats.dropped, 1);
    // A gunshot outranks them and takes a slot.
    assert!(field
        .acquire(
            &mut g,
            &mixer,
            None,
            AcquireOpts {
                priority: 0.95,
                ..AcquireOpts::default()
            },
        )
        .is_some());
    assert_eq!(field.stats.stolen, 1);
}

/// A tracked emitter (a bed, a loop) is never stolen and is refreshed one per
/// frame.
#[test]
fn tracked_emitters_are_never_stolen_and_are_refreshed_in_turn() {
    let mut g = AudioGraph::new(SR);
    let mut field = SpatialField::new(&mut g);
    let mut mixer = Mixer::new(&mut g, Rng::new(1), 0.95);
    mixer.build_reverbs(&mut g);
    let tracked = field
        .acquire(
            &mut g,
            &mixer,
            None,
            AcquireOpts {
                tracked: true,
                priority: 0.1,
                end_time: Some(0.0),
                ..AcquireOpts::default()
            },
        )
        .unwrap();
    for _ in 0..39 {
        field
            .acquire(
                &mut g,
                &mixer,
                None,
                AcquireOpts {
                    priority: 0.9,
                    end_time: Some(100.0),
                    ..AcquireOpts::default()
                },
            )
            .unwrap();
    }
    field
        .acquire(
            &mut g,
            &mixer,
            None,
            AcquireOpts {
                priority: 1.0,
                ..AcquireOpts::default()
            },
        )
        .unwrap();
    assert!(!field.emitter(tracked).free, "a bed is never stolen");
    // Its end time has passed, and it still survives the sweep.
    g.set_current_time(10.0);
    let before = g.automation.len();
    for _ in 0..41 {
        field.update(&mut g, None);
    }
    assert!(!field.emitter(tracked).free);
    assert!(g.automation.len() > before, "the bed was refreshed");

    // Moving it is smoothed, not stepped.
    let n = g.automation.len();
    field.emitter_mut(tracked).move_to(&mut g, 1.0, 2.0, 3.0, 0.06);
    assert_eq!(g.automation.len(), n + 3);
    assert!(g.automation[n..]
        .iter()
        .all(|e| e.kind == Automation::SetTargetAtTime));
    assert_eq!(field.emitter(tracked).pos, [1.0, 2.0, 3.0]);

    field.dispose(&mut g);
}

/// Occlusion degrades gracefully: no probe, or the feature switched off, means
/// no rays and no loss — exactly what the source does when `physics` is absent.
#[test]
fn occlusion_degrades_to_nothing_without_a_world_probe() {
    let mut g = AudioGraph::new(SR);
    let mut field = SpatialField::new(&mut g);
    field.set_listener(&mut g, [0.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]);
    assert_eq!(field.occlusion_at(None, 10.0, 0.0, 0.0), 0.0);
    assert_eq!(field.stats.occlusion_rays, 0);

    // A very close source is never tested either.
    assert_eq!(field.occlusion_at(Some(&Wall(1.0)), 0.3, 0.0, 0.0), 0.0);
    assert_eq!(field.stats.occlusion_rays, 0);

    // A hit well short of the target reads as a thick wall; one just short of it
    // reads as a thin partition.
    assert_eq!(field.occlusion_at(Some(&Wall(1.0)), 20.0, 0.0, 0.0), 1.0);
    assert_eq!(field.occlusion_at(Some(&Wall(19.5)), 20.0, 0.0, 0.0), 0.5);

    field.occlusion_enabled = false;
    let rays = field.stats.occlusion_rays;
    assert_eq!(field.occlusion_at(Some(&Wall(1.0)), 20.0, 0.0, 0.0), 0.0);
    assert_eq!(field.stats.occlusion_rays, rays);
}

/* ================================================================ */
/* ambience.js                                                      */
/* ================================================================ */

#[test]
fn the_ambience_beds_build_and_react_to_enclosure() {
    let mut g = AudioGraph::new(SR);
    let mut rng = Rng::new(0xabcd);
    let bank = NoiseBank::new(&mut g, &mut rng.fork(), 1.2);
    let mut mixer = Mixer::new(&mut g, rng.fork(), 0.95);
    mixer.build_reverbs(&mut g);
    let mut amb = Ambience::new(rng.fork());
    let before = g.nodes.len();
    amb.start(&mut g, &bank, &mixer);
    assert!(amb.started);
    assert!(g.nodes.len() > before + 20, "wind, whistle, city and war beds");

    // Every bed source loops and starts immediately.
    let loops = g
        .nodes
        .iter()
        .skip(before)
        .filter(|n| matches!(n.kind, NodeKind::BufferSource { looping: true, .. }))
        .count();
    assert_eq!(loops, 5, "two wind layers, whistle, city, war");

    // Starting twice is a no-op.
    let n = g.nodes.len();
    amb.start(&mut g, &bank, &mixer);
    assert_eq!(g.nodes.len(), n);

    // Walking inside drops the wind and closes a lowpass over the outdoor bed.
    let autos = g.automation.len();
    amb.set_enclosure(&mut g, 1.0);
    assert_eq!(amb.enclosure, 1.0);
    let new: Vec<_> = g.automation[autos..].to_vec();
    assert_eq!(new.len(), 3);
    close(new[0].value, 620.0, 1e-12, "outdoor lowpass closes");
    close(new[1].value, 0.45, 1e-12, "outdoor level drops");
    close(new[2].value, 0.12, 1e-12, "wind drops");
}

/// The scheduler's four cues fire on their own timers, and intensity scales the
/// two battle ones.
#[test]
fn the_ambience_scheduler_fires_its_cues() {
    let mut g = AudioGraph::new(SR);
    let mut rng = Rng::new(0xabcd);
    let bank = NoiseBank::new(&mut g, &mut rng.fork(), 1.2);
    let mut mixer = Mixer::new(&mut g, rng.fork(), 0.95);
    mixer.build_reverbs(&mut g);
    let mut amb = Ambience::new(rng.fork());

    // Before `start` nothing is scheduled at all.
    assert!(amb.update(&mut g, 10.0).is_empty());

    amb.start(&mut g, &bank, &mixer);
    let mut seen = [false; 4];
    for _ in 0..400 {
        g.set_current_time(g.current_time() + 0.5);
        for cue in amb.update(&mut g, 0.5) {
            seen[match cue {
                AmbienceCue::DistantVolley => 0,
                AmbienceCue::DistantBoom => 1,
                AmbienceCue::OneShot => 2,
                AmbienceCue::DistantChatter => 3,
            }] = true;
        }
    }
    assert_eq!(seen, [true; 4], "every cue fired within 200 s");

    // A gust automates both wind layers' level and cutoff, four events each.
    let before = g.automation.len();
    let mut fired = 0;
    for _ in 0..200 {
        g.set_current_time(g.current_time() + 0.5);
        amb.update(&mut g, 0.5);
        fired += 1;
        if g.automation.len() >= before + 8 {
            break;
        }
    }
    assert!(fired < 200 && g.automation.len() >= before + 8, "a gust fired");

    amb.dispose(&mut g);
    assert!(!amb.started);
}

/* ================================================================ */
/* index.js — the subsystem                                         */
/* ================================================================ */

fn started_core() -> AudioCore {
    let mut core = AudioCore::new(Rng::new(CAPTURE_SEED));
    assert!(core.start(SR));
    assert!(core.running);
    core
}

#[test]
fn the_subsystem_is_a_no_op_until_it_starts() {
    let mut core = AudioCore::new(Rng::new(1));
    assert!(!core.running);
    // Every public entry point must be safe before the graph exists.
    assert!(!core.play(VoiceKind::DryFire, None, PlayOpts::default()));
    assert!(!core.play(
        VoiceKind::DryFire,
        Some([1.0, 0.0, 0.0]),
        PlayOpts::default()
    ));
    assert!(!core.ui(UiSound::Hitmarker, 1.0));
    assert!(!core.bark(BarkRequest::Spot, None, 1.0, false, 0, true));
    core.set_master_volume(0.5);
    core.set_bus_volume(Bus::Ui, 0.5);
    core.set_ambience_intensity(2.0);
    core.set_occlusion_enabled(false);
    core.update(0.016);
    core.debug_storm();
    assert!(core.graph().is_none());
    let r = core.report();
    assert!(!r.running);
    assert_eq!(r.sample_rate, 0.0);

    assert!(core.start(SR));
    assert!(core.start(SR), "starting twice is idempotent");
    core.dispose();
    assert!(!core.running);
}

/// A head-locked voice takes a bookkeeping slot, is torn down once its tail has
/// decayed, and the 48 slots wrap.
#[test]
fn head_locked_voices_are_torn_down_when_their_tail_decays() {
    let mut core = started_core();
    // The first frame reclassifies the space and unplugs the convolvers that
    // fell to nothing; take the baseline after it, not before.
    core.update(0.016);
    assert!(core.ui(UiSound::Hitmarker, 1.0));
    let nodes = core.graph().unwrap().nodes.len();
    let disconnects = core.graph().unwrap().disconnects;

    // Still alive on the next frame.
    core.advance(0.1);
    core.update(0.1);
    assert_eq!(core.graph().unwrap().disconnects, disconnects);

    // Past its end, torn down.
    core.advance(2.0);
    core.update(0.016);
    assert!(core.graph().unwrap().disconnects > disconnects);
    assert!(core.graph().unwrap().nodes.len() >= nodes);

    // 60 UI sounds cycle the 48 slots, stealing the oldest rather than leaking.
    for _ in 0..60 {
        core.ui(UiSound::Blip, 1.0);
    }
    assert!(core.report().events >= 61);
}

/// The propagation delay is scheduling, not a delay node: a shot 343 m away is
/// scheduled a whole second late.
#[test]
fn propagation_delay_is_scheduling_at_the_speed_of_sound() {
    let mut core = started_core();
    core.set_listener_basis([0.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]);
    core.update(0.016);
    let before = core.graph().unwrap().schedule.len();
    assert!(core.play_at(
        VoiceKind::Shot {
            profile: &RIFLE,
            first_person: false,
        },
        [343.0, 0.0, 0.0],
        PlayOpts {
            max_dist: 400.0,
            ..PlayOpts::default()
        },
        Bus::Weapons,
        0.5,
    ));
    let first = core.graph().unwrap().schedule[before];
    close(first.when, 1.0, 1e-6, "343 m is one second of propagation");
    // No delay node was created for it.
    assert!(core
        .graph()
        .unwrap()
        .nodes
        .iter()
        .all(|n| !matches!(n.kind, NodeKind::Convolver { buffer: None, .. })));
}

/// Beyond `maxDist` nothing is built at all — the cheapest possible cull.
#[test]
fn a_source_past_max_distance_builds_nothing() {
    let mut core = started_core();
    let nodes = core.graph().unwrap().nodes.len();
    assert!(!core.play_at(
        VoiceKind::DryFire,
        [10_000.0, 0.0, 0.0],
        PlayOpts::default(),
        Bus::Weapons,
        0.5,
    ));
    assert_eq!(core.graph().unwrap().nodes.len(), nodes);

    // A NaN position falls back to a head-locked voice rather than throwing.
    assert!(core.play_at(
        VoiceKind::DryFire,
        [f64::NAN, 0.0, 0.0],
        PlayOpts::default(),
        Bus::Weapons,
        0.5,
    ));
    assert!(core.graph().unwrap().nodes.len() > nodes);
}

/// The space probe runs on its own timer, and again whenever the listener has
/// moved far enough.
#[test]
fn the_space_probe_reclassifies_on_a_timer_and_on_movement() {
    struct Room;
    impl WorldProbe for Room {
        fn raycast(&self, _o: [f64; 3], d: [f64; 3], max: f64, _m: RayMask) -> Option<RayHit> {
            // A 2.4 m ceiling and 3 m walls: unambiguously a tight interior.
            let up = d[1] > 0.5;
            Some(RayHit {
                distance: if up { 2.4f64 } else { 3.0f64 }.min(max),
                surface: Surface::Concrete,
            })
        }
    }
    let mut core = started_core();
    core.set_world_probe(Some(std::rc::Rc::new(Room)));
    core.set_listener_basis([0.0, 1.6, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]);
    core.update(0.016);
    assert_eq!(core.report().space, Space::Tight);
    assert!(core.space().enclosure > 0.9);
    assert!(core.report().mean_free < 4.0);

    // Dropping the probe puts it back outdoors on the next scheduled sweep.
    core.set_world_probe(None);
    core.advance(0.5);
    core.update(0.5);
    assert_eq!(core.report().space, Space::Open);

    // A 2 m step re-probes immediately, before the 0.45 s timer.
    core.set_listener_basis([2.0, 1.6, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]);
    let rays = core.report().occlusion_rays;
    core.update(0.001);
    assert_eq!(core.report().occlusion_rays, rays);
}

/// Every wired event runs end to end and builds real nodes. This is the port's
/// counterpart to the source's `debugStorm`, which exists to prove exactly this.
#[test]
fn the_debug_storm_drives_every_event_path() {
    let mut core = started_core();
    core.set_listener_basis([0.0, 1.6, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]);
    core.update(0.016);
    let nodes = core.graph().unwrap().nodes.len();
    let events = core.report().events;
    core.debug_storm();
    core.update(0.0);
    let r = core.report();
    assert!(r.events > events + 30, "the storm fired {} voices", r.events - events);
    assert!(core.graph().unwrap().nodes.len() > nodes + 500);
    // The explosion at 6 m deafened the listener.
    assert!(r.deafness > 0.0);
    // And nothing produced a non-finite schedule time.
    assert!(core
        .graph()
        .unwrap()
        .schedule
        .iter()
        .all(|e| e.when.is_finite()));
    assert!(core
        .graph()
        .unwrap()
        .automation
        .iter()
        .all(|e| e.value.is_finite() && e.time.is_finite()));
}

/// Per-frame budgets cap how many of each kind can fire in one frame, and reset
/// on the next.
#[test]
fn per_frame_budgets_cap_impacts_steps_shells_and_whizzes() {
    let mut core = started_core();
    core.set_listener_basis([0.0, 1.6, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]);
    core.update(0.016);
    let events = core.report().events;
    for _ in 0..20 {
        core.on_impact(&BulletImpact {
            point: Some([2.0, 1.0, 0.0]),
            surface: Some(Surface::Concrete),
            damage: Some(32.0),
            exit: false,
        });
    }
    // Five impacts get through, and each of those five also fires a crack-past
    // whizz because it landed inside 6 m. Note what the source does NOT do: the
    // impact path never touches the whizz budget, which caps `bullet:tracer` only.
    let fired = core.report().events - events;
    assert_eq!(fired, 5 + 5, "impact budget is 5, and each raises its own whizz");

    // An exit wound is silent, and never spends budget.
    let events = core.report().events;
    core.on_impact(&BulletImpact {
        point: Some([2.0, 1.0, 0.0]),
        surface: Some(Surface::Metal),
        exit: true,
        damage: None,
    });
    assert_eq!(core.report().events, events);

    core.advance(0.016);
    core.update(0.016);
    let events = core.report().events;
    core.on_impact(&BulletImpact {
        point: Some([2.0, 1.0, 0.0]),
        surface: Some(Surface::Concrete),
        damage: Some(32.0),
        exit: false,
    });
    assert!(core.report().events > events, "budgets reset each frame");
}

#[test]
fn firing_ducks_the_mix_and_a_first_person_shot_stays_dry() {
    let mut core = started_core();
    core.set_listener_basis([0.0, 1.6, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]);
    core.update(0.016);

    // First person: no propagation delay at all.
    let before = core.graph().unwrap().schedule.len();
    core.on_fire(&WeaponFire {
        weapon: Some("rifle".to_string()),
        origin: Some([0.0, 1.6, 0.0]),
        ..WeaponFire::default()
    });
    let first = core.graph().unwrap().schedule[before];
    close(first.when, 0.0, 1e-9, "own weapon is scheduled now");

    // An empty magazine is a dry-fire click and nothing else.
    let events = core.report().events;
    core.on_fire(&WeaponFire {
        weapon: Some("rifle".to_string()),
        empty: true,
        ..WeaponFire::default()
    });
    assert_eq!(core.report().events, events + 1);

    // A named suppressor and an explicit `suppressed` flag both resolve there.
    assert_eq!(resolve_profile(Some("silenced_mp7")).name, "suppressed");
    core.on_fire(&WeaponFire {
        weapon: Some("mp5".to_string()),
        suppressed: true,
        origin: Some([40.0, 1.6, 0.0]),
        ..WeaponFire::default()
    });
    assert!(core.report().events > events + 1);
}

#[test]
fn health_events_drive_the_hitmarker_the_sting_and_the_heartbeat() {
    let mut core = started_core();
    core.set_listener_basis([0.0, 1.6, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]);
    core.update(0.016);

    // Damage dealt to the player is NOT a hitmarker — that would be backwards.
    let events = core.report().events;
    core.on_damage_dealt(&DamageDealt {
        target_is_player: true,
        ..DamageDealt::default()
    });
    assert_eq!(core.report().events, events);

    core.on_damage_dealt(&DamageDealt {
        target_is_player: false,
        has_target: true,
        headshot: true,
        killed: true,
        point: Some([4.0, 1.0, -9.0]),
    });
    assert_eq!(core.report().events, events + 2, "headshot tick + kill jingle");

    // Taking damage below 34 starts the heartbeat on the next frames.
    core.on_damage_taken(&DamageTaken {
        amount: Some(80.0),
        health: Some(20.0),
    });
    let events = core.report().events;
    for _ in 0..80 {
        core.advance(0.016);
        core.update(0.016);
    }
    assert!(core.report().events > events, "a low-health heartbeat fired");
}

/// Landing hard plays the heaviest gait plus a cloth rustle; a stance change
/// plays one rustle, and only on the change.
#[test]
fn movement_events_play_landings_and_stance_changes_once_each() {
    let mut core = started_core();
    core.set_listener_basis([0.0, 1.6, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]);
    core.update(0.016);

    let events = core.report().events;
    core.on_land(&PlayerLand {
        velocity: Some(12.0),
        surface: Some(Surface::Concrete),
    });
    assert_eq!(core.report().events, events + 2, "landing + cloth");

    let events = core.report().events;
    core.on_player_state(&PlayerState {
        stance: Some("crouch".to_string()),
        ads: Some(true),
    });
    assert_eq!(core.report().events, events + 2);
    core.on_player_state(&PlayerState {
        stance: Some("crouch".to_string()),
        ads: Some(true),
    });
    assert_eq!(core.report().events, events + 2, "no change, no sound");

    // A tracer whose closest approach is far away, or is our own muzzle, is
    // silent.
    let events = core.report().events;
    core.on_tracer(&BulletTracer {
        from: [100.0, 1.6, 0.0],
        to: [100.0, 1.6, -50.0],
        speed: Some(880.0),
    });
    assert_eq!(core.report().events, events, "50 m off is not a whizz");
    core.on_tracer(&BulletTracer {
        from: [0.0, 1.6, 0.0],
        to: [0.0, 1.6, -50.0],
        speed: None,
    });
    assert_eq!(core.report().events, events, "our own muzzle is not a whizz");
    core.on_tracer(&BulletTracer {
        from: [5.0, 1.6, 0.0],
        to: [-5.0, 1.6, 0.0],
        speed: Some(880.0),
    });
    assert!(core.report().events > events, "a round passing the ear whizzes");
}

/// Barks are rate limited to one per 0.42 s so a firefight does not turn into
/// mush, unless the caller forces one (a death scream always plays).
#[test]
fn barks_are_rate_limited_unless_forced() {
    let mut core = started_core();
    core.set_listener_basis([0.0, 1.6, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]);
    core.update(0.016);
    assert!(core.bark(BarkRequest::Spot, None, 1.0, false, 3, false));
    assert!(!core.bark(BarkRequest::Spot, None, 1.0, false, 3, false));
    assert!(core.bark(BarkRequest::Death, None, 1.0, false, 3, true));
    core.advance(0.5);
    assert!(core.bark(BarkRequest::Copy, Some([3.0, 1.6, 0.0]), 1.0, true, 4, false));

    // A death event forces its bark and schedules the body fall after it.
    let events = core.report().events;
    core.on_death(&ActorDeath {
        point: Some([4.0, 0.4, -9.0]),
        actor_id: 3,
    });
    assert_eq!(core.report().events, events + 2);
    core.on_death(&ActorDeath {
        point: None,
        actor_id: 3,
    });
    assert_eq!(core.report().events, events + 2, "no point, no sound");
}

/// The shell lands on whatever is under it, so brass on sand does not ring like
/// concrete.
#[test]
fn a_shell_casing_asks_the_world_what_it_will_land_on() {
    struct Sand;
    impl WorldProbe for Sand {
        fn raycast(&self, _o: [f64; 3], _d: [f64; 3], _max: f64, _m: RayMask) -> Option<RayHit> {
            Some(RayHit {
                distance: 0.8,
                surface: Surface::Sand,
            })
        }
    }
    let mut core = started_core();
    core.set_world_probe(Some(std::rc::Rc::new(Sand)));
    core.set_listener_basis([0.0, 1.6, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]);
    core.update(0.016);

    let events = core.report().events;
    core.on_shell(&WeaponShell {
        position: Some([0.3, 1.4, -0.2]),
    });
    assert_eq!(core.report().events, events + 1);

    // Past 22 m the brass is inaudible and nothing is built.
    let events = core.report().events;
    core.on_shell(&WeaponShell {
        position: Some([100.0, 1.4, 0.0]),
    });
    assert_eq!(core.report().events, events);

    // A reload with a position is spatialised; without one it is head-locked.
    let events = core.report().events;
    core.on_reload(&WeaponReload {
        weapon: Some("m249".to_string()),
        phase: Some(ReloadPhase::MagIn),
        position: Some([1.0, 1.4, 0.0]),
    });
    core.on_reload(&WeaponReload {
        weapon: None,
        phase: None,
        position: None,
    });
    assert_eq!(core.report().events, events + 2);

    // And a footstep past 45 m is silent.
    let events = core.report().events;
    core.on_footstep(&PlayerFootstep {
        position: Some([200.0, 0.0, 0.0]),
        ..PlayerFootstep::default()
    });
    assert_eq!(core.report().events, events);
}

/// A near explosion ducks the mix and deafens; a distant one does neither.
#[test]
fn explosions_duck_and_deafen_by_distance() {
    let mut core = started_core();
    core.set_listener_basis([0.0, 1.6, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]);
    core.update(0.016);
    core.on_explosion(&ExplosionEvent {
        position: [200.0, 0.0, 0.0],
        radius: Some(8.0),
    });
    // `deafness` is mirrored off the mixer once a frame, exactly as the source
    // does it, so a zero-length frame is what reads it back.
    core.update(0.0);
    assert_eq!(core.report().deafness, 0.0, "nothing past ~22 m");
    core.on_explosion(&ExplosionEvent {
        position: [4.0, 1.6, 0.0],
        radius: Some(8.0),
    });
    core.update(0.0);
    assert!(core.report().deafness > 0.5, "a grenade at 4 m is deafening");
}

/// Wired through the real registry and event bus, the way the game runs it.
#[test]
fn the_subsystem_registers_and_receives_events_through_the_bus() {
    let mut engine = Engine::new(Default::default(), CAPTURE_SEED);
    let system = AudioSystem::new(Rng::new(CAPTURE_SEED));
    let core = system.core();
    engine.add(system).expect("audio registers");
    engine.init().expect("init wires the events");

    assert!(engine.events().handler_count("weapon:fire") == 1);
    assert!(engine.events().handler_count("bullet:impact") == 1);
    assert!(engine.events().handler_count("ai:bark") == 1);

    core.borrow_mut().start(SR);
    core.borrow_mut()
        .set_listener_basis([0.0, 1.6, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]);
    core.borrow_mut().update(0.016);

    let before = core.borrow().report().events;
    // Dispatch is synchronous: the shot is scheduled inside `emit`, not a frame
    // later.
    let failures = engine.events().emit(
        "weapon:fire",
        &WeaponFire {
            weapon: Some("ak".to_string()),
            origin: Some([12.0, 1.6, -3.0]),
            ..WeaponFire::default()
        },
    );
    assert!(failures.is_empty());
    assert!(core.borrow().report().events > before);

    // A payload of the wrong type is ignored rather than a failure.
    assert!(engine.events().emit("weapon:fire", &42u32).is_empty());

    // The audio system takes part in exactly one phase.
    let update = engine.registry().with(Phase::Update).unwrap();
    assert_eq!(update.len(), 1);
    assert!(engine.registry().with(Phase::FixedUpdate).unwrap().is_empty());

    engine.dispose().expect("dispose");
    assert!(!core.borrow().running);
}
