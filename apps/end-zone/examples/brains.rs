//! Headless AI-brain inspector for End Zone.
//!
//! Runs the deterministic play simulation with no browser and no scene attached
//! and prints, per tick, each player's brain decision (role state + typed
//! intent), the ball / possession state, and every contact event — so AI
//! behaviour and contact bugs can be read straight from a terminal instead of
//! being inferred from the camera.
//!
//! It can also STEER the ball carrier (a scripted "juke" of the user stick, to
//! force a defender's committed dive to whiff) and SCAN many seeds for the
//! *phantom dive tackle*: a dive that raises a `TackleContact` — which is what
//! makes the camera cut/shake AND ends the play — even though the diver never
//! actually reached the carrier (it sailed over the top, or "wrapped" from
//! farther than the two bodies could touch). `resolve_tackle` measures only the
//! horizontal distance to the carrier and never checks the diver's height.
//!
//! Usage:
//!   cargo run -p axiom-end-zone --example brains -- [OPTIONS]
//!
//!   --seed N           presentation seed (default 0)
//!   --ticks N          ticks to run (default 360)
//!   --start N          tick the play starts / forms up (default 0)
//!   --snap N           tick of the snap (default 80)
//!   --throw N          tick the QB throws (default 170)
//!   --window A:B       print the full per-player brain table only for ticks A..=B
//!   --team N           restrict the brain table to team N (0 or 1)
//!   --juke "T:x,y;..." set the user stick to (x,y) from tick T onward (repeatable
//!                      via ';') — steers whoever currently carries the ball
//!   --scan N           run seeds 0..N, report every phantom dive tackle, and exit
//!
//! Examples:
//!   cargo run -p axiom-end-zone --example brains -- --window 180:260
//!   cargo run -p axiom-end-zone --example brains -- --scan 200
//!   cargo run -p axiom-end-zone --example brains -- --seed 7 --juke "210:1,0.2"

use std::collections::HashMap;
use std::env;

use axiom::prelude::{Vec2, Vec3};
use axiom_end_zone::ai::{PlayerIntent, RoleState};
use axiom_end_zone::config::EndZoneConfig;
use axiom_end_zone::events::{PlayEndReason, SimEvent, StampedEvent};
use axiom_end_zone::identity::PlayerId;
use axiom_end_zone::launch::RunConfig;
use axiom_end_zone::player::AnimState;
use axiom_end_zone::presentation::snapshot::{capture, PlayerView, PresentationSnapshot};
use axiom_end_zone::showcase::{DiagnosticCommand, ShowcaseRun};
use axiom_end_zone::state::{SimCommand, SimState};

struct Opts {
    seed: u64,
    ticks: u64,
    start: u64,
    snap: u64,
    throw: u64,
    window: Option<(u64, u64)>,
    team: Option<u8>,
    jukes: Vec<(u64, f32, f32)>,
    scan: Option<u64>,
    drive: bool,
}

impl Opts {
    fn parse() -> Self {
        let mut o = Opts {
            seed: 0,
            ticks: 360,
            start: 0,
            snap: 80,
            throw: 170,
            window: None,
            team: None,
            jukes: Vec::new(),
            scan: None,
            drive: false,
        };
        let args: Vec<String> = env::args().skip(1).collect();
        let mut i = 0;
        while i < args.len() {
            let val = args.get(i + 1).cloned().unwrap_or_default();
            match args[i].as_str() {
                "--seed" => o.seed = val.parse().unwrap_or(0),
                "--ticks" => o.ticks = val.parse().unwrap_or(360),
                "--start" => o.start = val.parse().unwrap_or(0),
                "--snap" => o.snap = val.parse().unwrap_or(80),
                "--throw" => o.throw = val.parse().unwrap_or(170),
                "--team" => o.team = val.parse().ok(),
                "--scan" => o.scan = val.parse().ok(),
                "--window" => {
                    if let Some((a, b)) = val.split_once(':') {
                        o.window = Some((a.parse().unwrap_or(0), b.parse().unwrap_or(u64::MAX)));
                    }
                }
                "--juke" => o.jukes = parse_jukes(&val),
                "--drive" => {
                    o.drive = true;
                    i += 1;
                    continue;
                }
                _ => {
                    i += 1;
                    continue;
                }
            }
            i += 2;
        }
        o
    }
}

fn parse_jukes(spec: &str) -> Vec<(u64, f32, f32)> {
    spec.split(';')
        .filter_map(|entry| {
            let (t, xy) = entry.split_once(':')?;
            let (x, y) = xy.split_once(',')?;
            Some((t.trim().parse().ok()?, x.trim().parse().ok()?, y.trim().parse().ok()?))
        })
        .collect()
}

/// The user stick in effect at `tick`: the last juke whose tick has arrived.
fn active_stick(jukes: &[(u64, f32, f32)], tick: u64) -> Vec2 {
    jukes
        .iter()
        .filter(|(t, _, _)| *t <= tick)
        .last()
        .map(|(_, x, y)| Vec2::new(*x, *y))
        .unwrap_or(Vec2::ZERO)
}

fn xz_distance(a: Vec3, b: Vec3) -> f32 {
    Vec3::new(a.x - b.x, 0.0, a.z - b.z).length()
}

fn role_tag(role: RoleState) -> String {
    format!("{role:?}")
        .split_whitespace()
        .next()
        .unwrap_or("?")
        .replace('{', "")
}

fn intent_tag(intent: &PlayerIntent) -> String {
    match intent {
        PlayerIntent::Hold => "hold".into(),
        PlayerIntent::Face { .. } => "face".into(),
        PlayerIntent::MoveToward { sprint, .. } => {
            format!("move{}", if *sprint { "!" } else { "" })
        }
        PlayerIntent::DropBack { .. } => "dropback".into(),
        PlayerIntent::Block { target, .. } => format!("block→#{}", target.0),
        PlayerIntent::Pursue { target, .. } => format!("pursue→#{}", target.0),
        PlayerIntent::PrepareCatch { .. } => "catch-prep".into(),
        PlayerIntent::Throw => "throw".into(),
        PlayerIntent::Carry { .. } => "carry".into(),
        PlayerIntent::Tackle { target, .. } => format!("tackle→#{}", target.0),
        PlayerIntent::Recover => "recover".into(),
    }
}

fn player_line(p: &PlayerView) -> String {
    format!(
        "  p{:<2}(#{:<2} T{}) {:<9} {:<11} {:<12} pos=({:>6.2},{:>5.2},{:>6.2}) v={:>4.1}",
        p.id.0,
        p.jersey,
        p.team.0,
        role_tag(p.role),
        intent_tag(&p.intent),
        format!("{:?}", p.anim),
        p.pos.x,
        p.pos.y,
        p.pos.z,
        p.speed,
    )
}

/// One dive's peak arc height and the closest it ever came to the carrier —
/// tracked across the airborne ticks so a landed tackle can be judged.
#[derive(Clone, Copy)]
struct DivePeak {
    max_y: f32,
    min_xz_to_carrier: f32,
}

/// Verdict on a `TackleContact` raised by a diving tackler.
struct DiveVerdict {
    phantom: bool,
    detail: String,
}

/// Judge a dive tackle: bodies only actually touch within the sum of the two
/// body radii, and a real wrap happens near ground height. A `TackleContact`
/// whose diver peaked well above the carrier, or never came within body-contact
/// range, is a phantom — a visual miss the sim scored as a hit. With the
/// physics-contact tackle gate in place this count should be zero; it stays here
/// as the standing regression check.
fn judge_dive(peak: DivePeak, tackler: &PlayerView, carrier: &PlayerView) -> DiveVerdict {
    let touch = tackler.body_radius + carrier.body_radius;
    let flew_over = peak.max_y > 0.5;
    let never_touched = peak.min_xz_to_carrier > touch;
    let phantom = flew_over || never_touched;
    DiveVerdict {
        phantom,
        detail: format!(
            "peak_y={:.2} closest_xz={:.2} (bodies touch<{:.2}){}{}",
            peak.max_y,
            peak.min_xz_to_carrier,
            touch,
            if flew_over { " OVER-THE-TOP" } else { "" },
            if never_touched { " NEVER-TOUCHED" } else { "" },
        ),
    }
}

/// Run one play. Returns the number of phantom dive tackles observed.
/// `verbose` controls whether per-tick brain/event lines are printed.
fn run(opts: &Opts, verbose: bool) -> usize {
    let mut sim = SimState::new(EndZoneConfig::with_seed(opts.seed));
    let mut prev: Option<PresentationSnapshot> = None;
    let mut peaks: HashMap<u8, DivePeak> = HashMap::new();
    let mut phantoms = 0usize;

    for tick in 0..opts.ticks {
        sim.user_stick = active_stick(&opts.jukes, tick);
        let mut commands = Vec::new();
        (tick == opts.start).then(|| commands.push(SimCommand::BeginPlay));
        (tick == opts.snap).then(|| commands.push(SimCommand::Snap));
        (tick == opts.throw).then(|| commands.push(SimCommand::ThrowNow));
        let events: Vec<StampedEvent> = sim.step(&commands).to_vec();
        let snap = capture(&sim);

        // Contact events — the camera/game's tackle signal is TackleContact.
        for ev in &events {
            if let Some(line) = event_line(&ev.event, &snap) {
                if verbose {
                    println!("[t{tick:>3}] {line}");
                }
            }
            if let SimEvent::TackleContact { tackler, target, .. } = ev.event {
                if let (Some(prev_snap), Some(peak)) = (prev.as_ref(), peaks.get(&tackler.0)) {
                    let t = prev_snap.player(tackler);
                    let c = prev_snap.player(target);
                    let verdict = judge_dive(*peak, t, c);
                    let tag = if verdict.phantom { "PHANTOM DIVE TACKLE" } else { "dive tackle (ok)" };
                    if verbose || verdict.phantom {
                        println!(
                            "[t{tick:>3}] {tag}: {} tackled {} — {}",
                            who(prev_snap, tackler),
                            who(prev_snap, target),
                            verdict.detail
                        );
                    }
                    phantoms += usize::from(verdict.phantom);
                }
            }
        }

        // Track every airborne diver's arc peak + closest horizontal approach.
        let carrier_pos = snap.carrier().map(|c| c.pos);
        for p in &snap.players {
            if p.anim == AnimState::Dive {
                let dist = carrier_pos.map(|cp| xz_distance(p.pos, cp)).unwrap_or(f32::MAX);
                let entry = peaks.entry(p.id.0).or_insert(DivePeak {
                    max_y: 0.0,
                    min_xz_to_carrier: f32::MAX,
                });
                entry.max_y = entry.max_y.max(p.pos.y);
                entry.min_xz_to_carrier = entry.min_xz_to_carrier.min(dist);
                if verbose {
                    println!(
                        "[t{tick:>3}] DIVE  #{:<2} pos=({:>6.2},{:>5.2},{:>6.2}) xz_to_carrier={:.2}",
                        p.jersey, p.pos.x, p.pos.y, p.pos.z, dist
                    );
                }
            } else {
                peaks.remove(&p.id.0);
            }
        }

        // Full brain table, only inside the requested window.
        let in_window = opts.window.map(|(a, b)| tick >= a && tick <= b).unwrap_or(false);
        if verbose && in_window {
            println!(
                "── t{tick} phase={:?} ball={:?} possession={:?}",
                snap.phase,
                ball_tag(&snap),
                snap.possession.map(|id| id.0)
            );
            for p in &snap.players {
                if opts.team.map(|t| p.team.0 == t).unwrap_or(true) {
                    println!("{}", player_line(p));
                }
            }
        }

        prev = Some(snap);
    }
    phantoms
}

fn ball_tag(snap: &PresentationSnapshot) -> &'static str {
    use axiom_end_zone::football::BallState;
    match snap.ball.state {
        BallState::Dead => "dead",
        BallState::Held { .. } => "held",
        BallState::Snap { .. } => "snap",
        BallState::Airborne { .. } => "airborne",
        BallState::Loose => "loose",
        BallState::Grounded => "grounded",
    }
}

/// Unambiguous identity for logs: jersey numbers are NOT unique across the two
/// teams, so always show the PlayerId index and team alongside.
fn who(snap: &PresentationSnapshot, id: PlayerId) -> String {
    let p = snap.player(id);
    format!("p{}(#{} T{})", id.0, p.jersey, p.team.0)
}

fn event_line(ev: &SimEvent, snap: &PresentationSnapshot) -> Option<String> {
    Some(match ev {
        SimEvent::PlayStarted { .. } => "PLAY STARTED".into(),
        SimEvent::Snap { quarterback, .. } => format!("SNAP → QB {}", who(snap, *quarterback)),
        SimEvent::DropBack { .. } => "QB DROP-BACK".into(),
        SimEvent::Throw { target, eta_ticks, .. } => {
            format!("THROW → target=({:.1},{:.1}) eta={eta_ticks}", target.x, target.z)
        }
        SimEvent::CatchAttempt { player } => format!("catch attempt by {}", who(snap, *player)),
        SimEvent::CatchCompleted { player } => format!("CATCH by {}", who(snap, *player)),
        SimEvent::PossessionChanged { from, to } => format!(
            "possession {:?} → {:?}",
            from.map(|i| i.0),
            to.map(|i| i.0)
        ),
        SimEvent::PassBrokenUp { defender, .. } => {
            format!("PASS BROKEN UP by {}", who(snap, *defender))
        }
        SimEvent::Intercepted { defender, .. } => {
            format!("INTERCEPTED by {}", who(snap, *defender))
        }
        SimEvent::BallLoose { .. } => "ball loose".into(),
        SimEvent::BallGrounded { .. } => "ball grounded".into(),
        SimEvent::BlockEngaged { blocker, defender } => {
            format!("block {} vs {}", who(snap, *blocker), who(snap, *defender))
        }
        SimEvent::TackleContact { tackler, target, strength, target_airborne, .. } => format!(
            "TACKLE CONTACT {} → {} strength={strength:.2} airborne={target_airborne}",
            who(snap, *tackler),
            who(snap, *target),
        ),
        SimEvent::PlayerAirborne { player } => format!("{} airborne", who(snap, *player)),
        SimEvent::GroundImpact { player, strength, .. } => {
            format!("{} ground impact strength={strength:.2}", who(snap, *player))
        }
        SimEvent::PlayEnded { reason } => format!("PLAY ENDED: {}", end_reason(*reason)),
        SimEvent::PlayReset => "play reset".into(),
    })
}

fn end_reason(reason: PlayEndReason) -> &'static str {
    match reason {
        PlayEndReason::Tackled => "tackled",
        PlayEndReason::Incomplete => "incomplete",
        PlayEndReason::OutOfBounds => "out of bounds",
        PlayEndReason::BrokeFree => "broke free",
        PlayEndReason::Intercepted => "intercepted",
    }
}

/// Drive a real score-attack run, steering the ball carrier STRAIGHT DOWNFIELD
/// every tick, and log the drive state + play-end reasons — to see what happens
/// when the carrier runs far down the field (does the drive keep advancing, or
/// stall / lock out?).
fn run_drive(opts: &Opts) {
    let config = RunConfig {
        seed: opts.seed,
        ..RunConfig::default()
    };
    let mut run = ShowcaseRun::new_run(&config);
    let mut prev_drive = run.drive_state();
    if let Some(d) = prev_drive {
        println!(
            "start: down={} los={:.0} first_down={:.0} heat={}",
            d.down, d.los_yard, d.first_down_yard, d.heat
        );
    }
    let mut throw_at: Option<u64> = None;
    for tick in 0..opts.ticks {
        // Steer a RECEIVER (post-catch carrier) straight at the end zone; leave
        // the quarterback alone so he can throw instead of scrambling into a sack.
        let steer_downfield = run
            .sim
            .possession
            .map_or(false, |p| p != run.sim.quarterback);
        run.sim.user_stick = if steer_downfield {
            Vec2::new(0.0, 1.0)
        } else {
            Vec2::ZERO
        };
        // Throw ~90 ticks after each snap so a receiver catches downfield and
        // then runs (the QB alone just gets sacked at the line).
        let commands: &[DiagnosticCommand] = if throw_at == Some(tick) {
            throw_at = None;
            &[DiagnosticCommand::PrimaryAction]
        } else {
            &[]
        };
        let out = run.step(commands);

        for ev in &out.events {
            match ev.event {
                SimEvent::PlayStarted { .. } => println!("[t{tick}] PLAY STARTED"),
                SimEvent::Snap { .. } => {
                    println!("[t{tick}] SNAP");
                    throw_at = Some(tick + 90);
                }
                SimEvent::CatchCompleted { .. } => println!("[t{tick}] CATCH"),
                SimEvent::PlayEnded { reason } => {
                    println!("[t{tick}] PLAY ENDED: {reason:?}")
                }
                _ => {}
            }
        }

        let drive = run.drive_state();
        if drive != prev_drive {
            if let Some(d) = drive {
                println!(
                    "[t{tick}] DRIVE down={} los={:.0} first_down={:.0} td={} first_downs={} over={} phase={:?}",
                    d.down, d.los_yard, d.first_down_yard, d.touchdowns, d.first_downs, d.over, out.snapshot.phase
                );
            }
            prev_drive = drive;
        }

        if tick % 60 == 0 {
            let carrier = out
                .snapshot
                .carrier()
                .map(|c| format!("z={:.1}", c.pos.z))
                .unwrap_or_else(|| "none".into());
            println!(
                "[t{tick}] phase={:?} possession={:?} carrier {carrier}",
                out.snapshot.phase,
                out.snapshot.possession.map(|id| id.0)
            );
        }
    }
    if let Some(d) = run.drive_state() {
        println!(
            "\nfinal: down={} los={:.0} td={} first_downs={} over={}",
            d.down, d.los_yard, d.touchdowns, d.first_downs, d.over
        );
    }
}

fn main() {
    let opts = Opts::parse();

    if opts.drive {
        run_drive(&opts);
        return;
    }

    if let Some(n) = opts.scan {
        println!("scanning seeds 0..{n} for phantom dive tackles …");
        let mut total = 0usize;
        let mut hit_seeds = Vec::new();
        for seed in 0..n {
            let scan_opts = Opts { seed, ..copy_opts(&opts) };
            let count = run(&scan_opts, false);
            if count > 0 {
                hit_seeds.push((seed, count));
                total += count;
            }
        }
        println!(
            "\n{} phantom dive tackle(s) across {} seed(s):",
            total,
            hit_seeds.len()
        );
        for (seed, count) in &hit_seeds {
            println!("  seed {seed}: {count}");
        }
        if let Some((seed, _)) = hit_seeds.first() {
            println!(
                "\nreplay one with:\n  cargo run -p axiom-end-zone --example brains -- --seed {seed}"
            );
        }
        return;
    }

    println!(
        "seed={} ticks={} start={} snap={} throw={}{}",
        opts.seed,
        opts.ticks,
        opts.start,
        opts.snap,
        opts.throw,
        opts.window.map(|(a, b)| format!(" window={a}:{b}")).unwrap_or_default(),
    );
    let phantoms = run(&opts, true);
    println!("\n{phantoms} phantom dive tackle(s) this run.");
}

fn copy_opts(o: &Opts) -> Opts {
    Opts {
        seed: o.seed,
        ticks: o.ticks,
        start: o.start,
        snap: o.snap,
        throw: o.throw,
        window: o.window,
        team: o.team,
        jukes: o.jukes.clone(),
        scan: None,
        drive: false,
    }
}
