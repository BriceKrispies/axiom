//! Driving Burnt Rubber with the reusable `axiom-agent` module.
//!
//! This compiles everywhere the app does, wasm included, because the browser
//! build genuinely needs it: the ghost you race is this driver, running live.
//! (An earlier version of this note claimed the module was "gated behind the
//! `agent` cargo feature". There is no such feature in this crate's manifest and
//! there never was — `cargo test --features agent` fails outright.)
//!
//! # What is the agent's, and what is the app's
//!
//! [`crate::script::autopilot`] is a *scripted* driver: one function that reads
//! the simulation and returns a finished [`DriveCommand`]. Nothing decides
//! anything; the app simply drives itself. This module is the other thing — an
//! embodied agent that perceives, decides, and emits player-equivalent actions:
//!
//! ```text
//! sim state --perceive--> Observation (integer facts)
//!            --axiom-agent decide--> move_axis intents
//!            --lower--> DriveCommand --> sim.step()
//! ```
//!
//! The split is real, not ceremonial. **The app owns perception**: reading the
//! road ahead, measuring the heading error to the racing line, choosing which
//! lane is clear of traffic, judging how fast the next corner can be taken.
//! That is the driver's *eyes*, and every part of it names a Burnt Rubber noun
//! (a track sample, a traffic car, a boost meter) that `axiom-agent` must never
//! learn. **The agent owns the control law**: a table of neutral bindings, each
//! turning a perceived scalar into a deflection of a control axis with a gain
//! and limits. That is the driver's *hands*, and it contains no racing concept
//! at all — the same table shape would fly a plane.
//!
//! Nothing here hand-rolls the decision: every steering input, every throttle
//! and brake application and every use of boost is emitted by
//! `AgentApi::step` as a `move_axis` intent and lowered back into the one
//! [`DriveCommand`] the simulation reads. Cut the agent out and the car does not
//! move.
//!
//! # Why the axis-map brain exists
//!
//! A car is analogue. The substrate's other brains emit *discrete* actions —
//! the scripted brain picks one intent, the hold-set brain holds a set of
//! controls — so driving through either would mean quantising a steering wheel
//! into a bitmask of buckets. `axiom-agent`'s action vocabulary always carried
//! `move_axis`; what it lacked was a brain that could emit one from an
//! observation. That gap was filled in the module (`AgentApi::axis_map_brain`),
//! at the lowest correct layer, rather than worked around up here.

use axiom_agent::AgentApi;
use axiom_kernel::{FrameIndex, Tick};
use axiom_runtime::RuntimeStep;

use crate::command::DriveCommand;
use crate::sim::{RaceEvent, RacePhase, RaceSim};
use crate::track::shortest_angle;

/// The app's control-axis vocabulary: the meaning this app assigns to a neutral
/// `move_axis` code. `axiom-agent` carries the `u32` opaquely.
const AXIS_STEER: u32 = 1;
const AXIS_THROTTLE: u32 = 2;
const AXIS_BRAKE: u32 = 3;
const AXIS_BOOST: u32 = 4;
/// The lane-hop axis — the phone game's entire lateral control. See
/// [`Perception::lane_intent`].
const AXIS_LANE: u32 = 5;

/// The app's observation-fact vocabulary: what the driver can *see*. Each fact's
/// `value` is a fixed-point micro-unit scalar (`axiom-agent` facts are integer
/// only), and a fact is present only when there is something to perceive — "I
/// have speed in hand" and "I am over the corner speed" are different sightings,
/// not one signed number, which is what lets the throttle and the brake be
/// driven by independent bindings.
const FACT_HEADING_ERROR: u16 = 1;
const FACT_YAW_RATE: u16 = 2;
const FACT_SPEED_HEADROOM: u16 = 3;
const FACT_SPEED_EXCESS: u16 = 4;
const FACT_BOOST_OPPORTUNITY: u16 = 5;
const FACT_LANE_INTENT: u16 = 6;

/// Fixed-point scale: one whole unit (a radian, a metre per second, a full axis
/// deflection) is a million.
const MICRO: f32 = 1_000_000.0;

/// The stable agent id this single-driver session uses.
const AGENT_RAW_ID: u64 = 1;

/// The engine's fixed 60 Hz step delta in integer nanoseconds, stamping the
/// `RuntimeStep` that drives one decision.
const FIXED_DELTA_NANOS: u64 = 16_666_667;

/// How the driver looks at the road and how hard it uses the car.
///
/// These are the numbers a driver would call *technique* — how far ahead to
/// look, how much of the car's cornering limit to actually use, when a lane is
/// worth changing to, when boost is worth spending. They are data rather than
/// constants so a run can be measured against a different technique without
/// touching the perception or the control law, which is what
/// [`DriverTuning::FAST`] was found with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DriverTuning {
    /// Metres of lookahead at a standstill, and extra metres per m/s — a fixed
    /// lookahead either wobbles at speed or cuts the corner at low speed.
    pub lookahead_base: f32,
    pub lookahead_per_speed: f32,
    /// The fraction of the car's true cornering limit the driver will use. Below
    /// `1.0` because the limit is a *steady-state* number and the road ahead
    /// keeps changing; this is the margin that keeps a corner from becoming an
    /// excursion.
    pub grip_usage: f32,
    /// The slowest the driver will talk itself into going (m/s).
    pub corner_speed_floor: f32,
    /// The fraction of the car's braking authority the driver plans on. Below
    /// `1.0` so that arriving slightly hot is recoverable rather than terminal.
    pub brake_usage: f32,
    /// How far over the planned speed the driver has to be before it brakes at
    /// all — the coast band between lifting and braking.
    pub brake_margin: f32,
    /// How far ahead corners are read, as seconds of travel and as a floor in
    /// metres.
    pub corner_horizon_seconds: f32,
    pub corner_horizon_floor: f32,
    /// How far ahead traffic is considered when choosing a lane (m).
    pub traffic_horizon: f32,
    /// Extra lateral margin (m) beyond the two half-widths before a pass counts
    /// as safe — the difference between threading a car and clipping it.
    pub touch_margin: f32,
    /// How far (m) the line is kept from the edge of the tarmac.
    pub edge_margin: f32,
    /// How heavily a candidate line is punished per metre it intrudes inside the
    /// touching width.
    ///
    /// **Ordinal, not a weight.** Since safety became lexicographic — any
    /// intruding candidate forfeits its near-miss reward outright — this no
    /// longer trades against anything: every clean candidate already outscores
    /// every dirty one regardless of its size, so scaling it cannot change which
    /// line wins. All it still does is rank the bad options against each other,
    /// for the case where every candidate intrudes and the driver has to pick
    /// the least-bad one. Sweeping it over 30..90 moves nothing, which is the
    /// measurement that says so.
    pub contact_penalty: f32,
    /// A mild pull back toward the centre of the road when nothing else decides
    /// the line.
    pub centre_pull: f32,
    /// How fast a car's influence on the line decays with the time until we
    /// reach it — a car half a second away matters, one four seconds away will
    /// be re-read many times before it counts.
    pub urgency_falloff: f32,
    /// How much a lane is penalised per metre of lateral movement to reach it —
    /// enough that the driver holds a line rather than weaving between equals.
    pub lane_change_cost: f32,
    /// Boost is spent when there is at least this much speed still to be gained
    /// (m/s): holding it against a limit the car is already at burns the meter
    /// for nothing.
    pub boost_min_headroom: f32,
    /// How full the meter must be before a *new* boost is started, `0..1`.
    ///
    /// The driver's one piece of patience, and it is worth more than any other
    /// number here. Without it the meter is a relaxation oscillator at the
    /// bottom of its range: the game lets boost start at 6% charge, the drain is
    /// 0.36/s, so engaging the instant it is legal buys 0.17 s and runs dry —
    /// measured, 699 separate boosts averaging **four steps each**. Four steps
    /// is nothing. Boost raises the speed ceiling by 22 m/s and adds 95 to
    /// acceleration, but acceleration takes *time*: a car cruising at 92 needs
    /// seconds to climb toward 114, so a four-step burst pays out almost none of
    /// what it costs, and the car sits at 103 having spent the whole meter.
    ///
    /// Banking to this level and then holding until dry converts the same charge
    /// into far more distance. It is also just what a person does — you save the
    /// bar for the straight instead of feathering it away.
    pub boost_start_charge: f32,
    /// How much a candidate line is *rewarded* for sitting in the lane next to a
    /// car it is about to overtake — the near-miss hunt.
    ///
    /// This is the one term that makes the driver seek traffic rather than
    /// merely survive it, and it is worth its own number because the payoff is
    /// real: a near miss is 0.13 of the boost meter, the meter drains at 0.36/s,
    /// so each one buys 0.36 s at the +22 m/s boost gives — about 7.9 m. Over a
    /// course with a hundred overtakes on it, that is the race.
    ///
    /// Scored against the *lane* the candidate falls in, because that is what
    /// the near-miss rule actually reads (see `sim::collision::is_near_miss`).
    /// A driver that threads *between* lanes — which an earlier version of this
    /// function deliberately did — rounds into one of them and is then either
    /// level with the car it is passing or two lanes off it, and is paid for
    /// neither.
    pub near_miss_reward: f32,
    /// Steering per radian of heading error, and per rad/s of the car's own yaw
    /// rate opposing it — the proportional and damping halves of the control law
    /// the agent is given, in thousandths.
    pub steer_gain_milli: i64,
    pub steer_damping_milli: i64,
}

impl DriverTuning {
    /// The technique for the profile this race is being driven on.
    ///
    /// The two games do not share a lateral control, so they cannot share a
    /// technique. The wheel game steers continuously and rewards a short
    /// lookahead with a sharp, well-damped correction; the rails game commits a
    /// whole lane at a time and has to see far enough ahead to finish the move
    /// before it arrives. Driving the phone game on the wheel game's numbers is
    /// what left the ghost hitting cars there.
    pub const fn for_profile(profile: crate::PlayProfile) -> DriverTuning {
        [DriverTuning::FAST, DriverTuning::FAST_RAILS][profile.is_rails() as usize]
    }

    /// The phone game's technique: a longer look up the road, a heavier cost on
    /// changing lane, and a hungrier near-miss reward.
    ///
    /// Every difference from [`Self::FAST`] follows from the same fact — the
    /// rails car commits a whole lane at a time and crosses at a fixed 12 m/s,
    /// so a move takes about 0.3 s and roughly 30 m of road. It has to look
    /// further ahead to start one in time, and it must not start one lightly.
    /// The reward is larger because it converts better here: a rails car is
    /// always *exactly* on a lane centre, which is precisely what the near-miss
    /// rule pays for, so it captures passes the wheel car only approximates.
    pub const FAST_RAILS: DriverTuning = DriverTuning {
        lookahead_base: 9.1,
        lookahead_per_speed: 0.15,
        traffic_horizon: 60.0,
        touch_margin: 0.88,
        edge_margin: 0.93,
        centre_pull: 0.039,
        urgency_falloff: 0.85,
        lane_change_cost: 0.41,
        boost_start_charge: 0.17,
        near_miss_reward: 11.7,
        ..DriverTuning::FAST
    };

    /// The technique the agent races with.
    pub const FAST: DriverTuning = DriverTuning {
        lookahead_base: 2.5,
        lookahead_per_speed: 0.21,
        grip_usage: 0.92,
        corner_speed_floor: 18.0,
        brake_usage: 0.75,
        brake_margin: 1.02,
        corner_horizon_seconds: 2.6,
        corner_horizon_floor: 60.0,
        traffic_horizon: 46.0,
        touch_margin: 0.87,
        edge_margin: 0.94,
        contact_penalty: 30.0,
        centre_pull: 0.063,
        urgency_falloff: 0.36,
        lane_change_cost: 0.107,
        boost_min_headroom: 1.5,
        boost_start_charge: 0.345,
        near_miss_reward: 5.05,
        steer_gain_milli: 35_000,
        steer_damping_milli: 83,
    };
}

/// What the agent perceived this step, before it is encoded as integer facts.
///
/// Splitting this out is what makes perception testable on its own: it is a pure
/// function of `(car, track, traffic, boost)` with no agent machinery in it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Perception {
    /// Steering error to the aim point, in radians, already signed the way the
    /// steering axis wants it (steering right is a *decreasing* yaw — see the
    /// sign note in `sim::controller::rotate_chassis`).
    pub heading_error: f32,
    /// The car's own yaw rate (rad/s) — the derivative term the control law
    /// damps with.
    pub yaw_rate: f32,
    /// Speed in hand below the corner-limited target (m/s), or `0` if there is
    /// none.
    pub speed_headroom: f32,
    /// Speed over the braking threshold (m/s), or `0` if there is none.
    pub speed_excess: f32,
    /// Whether boost is charged *and* the road ahead can use it.
    pub boost_opportunity: bool,
    /// The lateral offset from the centreline the driver picked (m).
    pub target_lateral: f32,
    /// The lane hop the driver wants *this step*, in the same screen-direction
    /// units [`DriveCommand::lane_step`] uses, or `0` for "stay put".
    ///
    /// This exists because the two play profiles do not share a lateral control.
    /// The wheel game steers and its lateral position is emergent; the phone
    /// game is on rails and its lateral position is *driven* by discrete lane
    /// hops, and `sim::rails` reads `lane_step` and ignores `steer` completely.
    /// Emitting only `steer` therefore left the ghost unable to change lane at
    /// all on a phone — measured, it ploughed through 25 cars and took 96.45 s
    /// against the wheel game's 89.93 s. It was not driving badly; it was not
    /// steering.
    ///
    /// `lane_step` is a *relative* control — `sim::rails` retargets from the lane
    /// it currently holds on every step it sees a non-zero value — so this is
    /// computed against that committed lane ([`RaceSim::rails_lane`]) rather than
    /// against where the car happens to be. Doing it that way needs no pacing
    /// rule at all: once the committed lane equals the wanted one this is `0`, so
    /// there is nothing to march.
    ///
    /// The first version of this could not read the committed lane and so gated
    /// on "the car has settled in a lane centre" to avoid marching sixty lanes a
    /// second. That worked, and cost 15 collisions a race, because a driver that
    /// may only re-decide once it has *arrived* cannot abort a lane change into a
    /// car that appeared while it was crossing. The pacing hack was the bug.
    pub lane_intent: i8,
}

/// Look at the road, the traffic and the boost meter, and measure what a driver
/// would need to know. Pure, deterministic, and reads only simulation state.
pub fn perceive(sim: &RaceSim, driver: &DriverTuning) -> Perception {
    let car = sim.car();
    let track = sim.track();
    let lookahead = driver.lookahead_base + driver.lookahead_per_speed * car.speed();
    let aim_distance = car.distance + lookahead;
    let target_lateral = choose_line(sim, aim_distance, driver);

    let aim = track.interpolated_at(aim_distance).at_lateral(target_lateral);
    let to_aim = aim.subtract(car.position);
    let wanted_yaw = to_aim.x.atan2(to_aim.z);
    let heading_error = -shortest_angle(wanted_yaw - car.yaw);

    let target_speed = plan_speed(sim, driver);
    let headroom = (target_speed - car.speed()).max(0.0);
    // Boost is worth spending wherever there is speed still to be had — on a
    // straight, on the exit of a corner, anywhere the car is not already against
    // the limit the road imposes. Holding it into a corner the car must brake
    // for burns the meter to be braked straight back off.
    // Hysteresis, not a threshold: start only on a well-filled meter, then hold
    // until it is dry. Both halves are read off state the driver can genuinely
    // see — the boost bar on the HUD is exactly `charge`, and whether boost is
    // lit is exactly `active` — so this is technique, not privileged access.
    let charge = sim.boost().charge();
    let meter_says_go = sim
        .boost()
        .active()
        .then(|| charge > 0.0)
        .unwrap_or(charge >= driver.boost_start_charge);
    let boost_opportunity = sim.boost().ready(&sim.tuning().race)
        && meter_says_go
        && headroom > driver.boost_min_headroom
        && !car.surface.is_off_road();

    // The same chosen line, expressed as the phone game's control. `lane_step`
    // is a screen direction and the lane index runs the other way (see the sign
    // note in `sim::rails`), so wanting a *lower* lane index is a *positive*
    // step.
    let here = track.sample_at(car.distance);
    let lane_intent = sim
        .rails_lane()
        .map(|held| (held - track.lane_at_lateral(&here, target_lateral)).clamp(-1, 1) as i8)
        .unwrap_or(0);

    Perception {
        heading_error,
        yaw_rate: car.yaw_rate,
        speed_headroom: headroom,
        speed_excess: (car.speed() - target_speed * driver.brake_margin).max(0.0),
        boost_opportunity,
        target_lateral,
        lane_intent,
    }
}

/// The fastest the car may be going *here* to still make everything ahead.
///
/// This is the one measurement that decides a lap time, and it replaces the
/// scripted autopilot's guess — `speed = 150 / (1 + 190·curvature)`, a curve
/// fitted to nothing — with what the car can actually do.
///
/// For every point in the braking horizon it asks two questions. First, how fast
/// can the car get *round* that point: cornering at speed `v` on curvature `κ`
/// demands a yaw rate of `v·κ`, and the chassis can only supply
/// [`steering_authority`], so the limit is where the two meet (solving the
/// resulting quadratic, since authority itself falls with speed). Second, how
/// fast may the car be going *here* to still slow to that limit by then —
/// `v² = v_limit² + 2·a·d`, the braking parabola. The lowest answer over the
/// whole horizon wins.
///
/// Reading the *whole* horizon rather than its sharpest point is what lets the
/// driver brake late: a corner 120 m away stops mattering the moment the brakes
/// can still deal with it, so the throttle stays down until the parabola says
/// otherwise, instead of lifting the instant a bend appears.
fn plan_speed(sim: &RaceSim, driver: &DriverTuning) -> f32 {
    let track = sim.track();
    let car = sim.car();
    let vehicle = &sim.tuning().vehicle;
    let decel = (vehicle.brake_decel * driver.brake_usage).max(1.0);
    let horizon =
        (car.speed() * driver.corner_horizon_seconds).max(driver.corner_horizon_floor);
    let steps = ((horizon / track.spacing()).ceil().max(1.0) as usize).clamp(1, 512);

    (0..=steps)
        .map(|i| {
            let ahead = i as f32 * track.spacing();
            let sample = track.sample_at(car.distance + ahead);
            let limit = cornering_limit(sample.curvature.abs(), vehicle) * driver.grip_usage;
            // The speed the car may hold here and still be down to `limit` there.
            (limit * limit + 2.0 * decel * ahead).sqrt()
        })
        .fold(f32::INFINITY, f32::min)
        .max(driver.corner_speed_floor)
}

/// The steady-state speed (m/s) at which the car can hold curvature `curvature`.
///
/// A corner of curvature `κ` taken at `v` needs a yaw rate of `ω = v·κ`. The
/// chassis supplies `ω_max(v) = max_yaw_rate / (1 + v/falloff)` (with a floor),
/// which *falls* as the car speeds up — so the limit is the `v` where supply
/// meets demand, which is a quadratic: `κ·v² + κ·falloff·v − max_yaw·falloff = 0`.
/// Below the authority floor the supply stops falling and the limit is simply
/// `ω_floor / κ`; the larger of the two is the real limit. A straight road
/// (`κ → 0`) yields infinity, which the caller clamps against the car's top
/// speed by never having any more throttle to give.
fn cornering_limit(curvature: f32, vehicle: &crate::tuning::VehicleTuning) -> f32 {
    let falloff = vehicle.steer_falloff_speed.max(1.0e-3);
    let k = curvature.max(1.0e-6);
    // The hyperbolic branch: k·v² + k·falloff·v − max_yaw_rate·falloff = 0.
    let b = k * falloff;
    let hyperbolic =
        (-b + (b * b + 4.0 * k * vehicle.max_yaw_rate * falloff).sqrt()) / (2.0 * k);
    // The floor branch: authority stops falling, so ω_floor / κ.
    let floored = vehicle.max_yaw_rate * vehicle.steer_authority_floor / k;
    hyperbolic.max(floored)
}

/// Choose the line through the traffic — the driver's eyes, and the single
/// decision that decides this race.
///
/// The whole course is flat out (its sharpest corner is well inside what the
/// chassis can hold at top speed), so lap time is not made by braking later. It
/// is made by two things: never leaving the tarmac, and threading the traffic
/// rather than hitting it — because a hit costs speed *and* forfeits the boost a
/// clean pass pays.
///
/// So the line is picked by scoring candidate offsets across the usable road,
/// not by picking a lane. Lanes are where the *traffic* sits; a lane-centred
/// driver is therefore either behind a car or exactly 3.5 m from one — and the
/// near-miss window closes at 3.1 m, which is why a lane-disciplined driver
/// earns almost no boost. Threading between lanes is both faster and better
/// paid.
///
/// Each candidate is scored on:
///
/// * **contact** — anything inside the touching width plus a margin is fatal to
///   the score, weighted by how soon we arrive;
/// * **road room** — the tarmac narrows to 5.6 m half-width in places, so a
///   candidate is clamped to what is passable the whole way there, and pressed
///   away from the edge;
/// * **travel** — a small cost per metre of lateral movement, so the driver
///   holds a line rather than weaving between equals.
///
/// Traffic is projected to where it will be when we arrive, not read where it is
/// now: at 70 m/s of closing speed a car 90 m ahead has moved 40 m by the time we
/// get there, and which cars are actually in the way changes completely.
/// Deterministic throughout: a fixed candidate grid, fixed iteration order, ties
/// broken toward the lower offset.
fn choose_line(sim: &RaceSim, aim_distance: f32, driver: &DriverTuning) -> f32 {
    let track = sim.track();
    let car = sim.car();
    let vehicle = &sim.tuning().vehicle;
    let race = &sim.tuning().race;

    // How far off the centreline is still tarmac, the whole way to the aim
    // point — the road narrows, and a line that fits at the aim point but not
    // halfway there is an excursion.
    let steps = ((aim_distance - car.distance) / track.spacing()).ceil().max(1.0) as usize;
    let room = (0..=steps.min(64))
        .map(|i| {
            track
                .sample_at(car.distance + i as f32 * track.spacing())
                .half_width
        })
        .fold(f32::INFINITY, f32::min)
        - vehicle.half_width
        - driver.edge_margin;
    let room = room.max(0.5);

    // The traffic that will still be in front of us when we get there, each
    // projected forward by its own speed over the time it takes us to arrive.
    // `lane` and `unscored` ride along because the near-miss reward is a
    // *lane* question and pays only once per car.
    let closing_floor = 1.0f32;
    let ahead: Vec<(f32, f32, i32, bool)> = sim
        .traffic()
        .active()
        .filter_map(|other| {
            let gap = other.distance - car.distance;
            let closing = (car.speed() - other.speed).max(closing_floor);
            let time_to_reach = gap / closing;
            // Only a car we will actually go past can pay a near miss — being
            // overtaken scores nothing, by the same rule.
            let overtaking = car.speed() > other.speed;
            ((gap > -8.0) & (gap < driver.traffic_horizon)).then_some((
                other.lateral,
                time_to_reach.max(0.0),
                other.lane,
                !other.near_missed & overtaking,
            ))
        })
        .collect();

    // Which lane a candidate offset falls in, read at the point we are aiming
    // for. This is the same question `is_near_miss` asks of the player, so the
    // driver is scoring itself against the rule the game actually pays out on.
    let aim_sample = track.sample_at(aim_distance);

    // Anything closer than this laterally is a collision, not a pass.
    let touching = vehicle.half_width + race.traffic_half_width + driver.touch_margin;

    // The candidate set is the driver's **action space**, and the two profiles do
    // not have the same one. The wheel game can hold any offset, so it scores a
    // fine grid across the usable road. The rails game can only ever be at a lane
    // centre — so scoring a continuum and rounding the winner, which is what this
    // did first, evaluates lines the car cannot take and then takes a line it
    // never scored. Measured, that mismatch was worth 28 collisions a race.
    let reach = track.lane_reach(&aim_sample);
    let grid = 41;
    let offsets: Vec<f32> = sim
        .on_rails()
        .then(|| {
            (-reach..=reach)
                .map(|lane| track.lane_lateral(&aim_sample, lane))
                .collect()
        })
        .unwrap_or_else(|| {
            (0..grid)
                .map(|i| -room + 2.0 * room * (i as f32 / (grid - 1) as f32))
                .collect()
        });
    offsets
        .into_iter()
        .map(|lateral| {
            let lane = track.lane_at_lateral(&aim_sample, lateral);
            let (contact, reward) = ahead
                .iter()
                .map(|&(other_lateral, time_to_reach, other_lane, unscored)| {
                    let gap = (lateral - other_lateral).abs();
                    // A car we reach in half a second matters far more than one
                    // four seconds out, whose lateral we will re-read many times
                    // before it counts.
                    let urgency = 1.0 / (1.0 + time_to_reach * driver.urgency_falloff);
                    let contact = (touching - gap).max(0.0) * driver.contact_penalty * urgency;
                    // The hunt: one lane over from a car we are about to pass is
                    // where the boost is.
                    let adjacent = ((lane - other_lane).abs() == 1) & unscored;
                    let reward =
                        f32::from(u8::from(adjacent)) * driver.near_miss_reward * urgency;
                    (contact, reward)
                })
                .fold((0.0f32, 0.0f32), |(c, r), (dc, dr)| (c + dc, r + dr));
            // Safety is **lexicographic**, not weighted: a candidate that
            // intrudes on anything scores no reward at all, however many near
            // misses it would otherwise line up. Making it a large negative
            // weight instead — which is what this did first — lets a line that
            // clips one car outbid a clean one by being adjacent to three, and
            // the measured cost of that was two impacts in every run. A driver
            // that trades paint for boost has misunderstood the trade: the
            // collision costs more speed than the boost returns, and the near
            // miss it was reaching for is forfeited by the contact anyway.
            let score = -contact + (contact <= 0.0).then_some(reward).unwrap_or(0.0);
            let travel = (lateral - car.lateral).abs() * driver.lane_change_cost;
            // A mild pull back toward the middle of the road. With no traffic in
            // sight every candidate scores zero, and without this the driver
            // simply keeps whatever line it was left on — including one pinned
            // against the verge, from which the next car cannot be avoided at all.
            let centring = lateral.abs() * driver.centre_pull;
            (lateral, score - travel - centring)
        })
        .fold(
            (0.0f32, f32::NEG_INFINITY),
            |best, (lateral, score)| (score > best.1).then_some((lateral, score)).unwrap_or(best),
        )
        .0
}

/// Full throttle is reached with one m/s of headroom in hand.
const THROTTLE_GAIN_MILLI: i64 = 1_000;
/// Full brake is reached two m/s over the braking threshold.
const BRAKE_GAIN_MILLI: i64 = 500;
/// A seen boost opportunity is a full deflection of the boost axis.
const BOOST_GAIN_MILLI: i64 = 1_000;
/// A lane hop is passed through at unit gain — the fact is already `-1`, `0` or
/// `+1`, which is exactly the command's vocabulary.
const LANE_GAIN_MILLI: i64 = 1_000;

/// The outcome of one agent-driven run.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentRace {
    /// Whether the car crossed the finish line.
    pub finished: bool,
    /// Race time in seconds — a step count, not a clock reading.
    pub elapsed_seconds: f32,
    /// Fixed steps taken, including the countdown.
    pub steps: u32,
    /// Course progress, `0..1`.
    pub progress: f32,
    /// The highest ground speed reached (m/s).
    pub top_speed: f32,
    /// Traffic cars threaded.
    pub near_misses: u32,
    /// Things hit.
    pub impacts: u32,
    /// Of those, how many were traffic rather than scenery or a barrier.
    pub traffic_impacts: u32,
    /// Steps spent off the tarmac.
    pub offroad_steps: u32,
    /// Steps spent with the throttle backed off, and with the brake applied.
    pub lifted_steps: u32,
    pub braking_steps: u32,
    /// Mean ground speed across the run (m/s).
    pub mean_speed: f32,
    /// How many `observe -> decide -> emit` cycles ran.
    pub decisions: u64,
    /// How many `move_axis` intents the agent emitted across the run.
    pub axis_intents: u64,
    /// Steps spent on boost.
    pub boost_steps: u32,
    /// Section-by-section milestones, in order.
    pub milestones: Vec<String>,
}

/// Drive the shipping course with the agent until it finishes or `limit` steps
/// elapse.
///
/// The agent's identity, profile, brain and memory live across the whole race —
/// one driver, not a fresh one each step — and every step runs the full
/// `observe -> decide -> emit` cycle through `AgentApi::step`. The emitted
/// intents are lowered back into a [`DriveCommand`] and that is the *only* thing
/// the simulation is given.
pub fn race_to_the_finish(limit: u32) -> AgentRace {
    race(RaceSim::shipping(), &DriverTuning::FAST, limit)
}

/// Drive `sim` with the agent, on `driver`'s technique, until it finishes or
/// `limit` steps elapse.

/// One full `observe → decide → emit → lower` cycle: what the agent does with
/// one step of the world.
///
/// This is the whole driver, and it is the single path both users of the agent
/// take — the offline [`race`] loop and the live ghost that races the player in
/// the browser. Returns the command to hand the simulation and how many
/// `move_axis` intents the agent emitted to produce it.
///
/// The brain and memory are built per call rather than held across steps. That
/// is not laziness: the axis-map brain is a stateless *table* (its decision is a
/// pure function of the observation), and memory only records each step's
/// decision reason, which nothing reads back. Building them here is what lets
/// the caller be a plain `RaceSim` owner — `axiom-agent`'s brain types are sealed
/// behind the facade and cannot be named in a struct field, which is exactly the
/// module boundary working as intended.
pub fn drive_one_step(sim: &RaceSim, driver: &DriverTuning, tick: u64) -> (DriveCommand, usize) {
    let agent_id = AgentApi::create_agent_id(AGENT_RAW_ID);
    let profile = AgentApi::debug_perfect_profile();
    let perception = perceive(sim, driver);

    // Observe. A fact is added only when there is something to see: no headroom
    // fact when the car is already at the corner speed, no excess fact when it
    // is not over it, no boost fact when boost is not worth spending. An absent
    // fact drives no axis, which is how "lift off" and "do not brake" are
    // expressed without the control law needing a conditional.
    let mut builder = AgentApi::observation_builder(agent_id, Tick::new(tick), 2, 6, 0);
    builder
        .add_channel(AgentApi::channel_geometric())
        .expect("one channel within the channel bound");
    builder
        .add_channel(AgentApi::channel_semantic())
        .expect("two channels within the channel bound");
    [
        Some((FACT_HEADING_ERROR, perception.heading_error)),
        Some((FACT_YAW_RATE, perception.yaw_rate)),
        (perception.speed_headroom > 0.0).then_some((FACT_SPEED_HEADROOM, perception.speed_headroom)),
        (perception.speed_excess > 0.0).then_some((FACT_SPEED_EXCESS, perception.speed_excess)),
        perception
            .boost_opportunity
            .then_some((FACT_BOOST_OPPORTUNITY, 1.0)),
        (perception.lane_intent != 0)
            .then_some((FACT_LANE_INTENT, f32::from(perception.lane_intent))),
    ]
    .into_iter()
    .flatten()
    .for_each(|(kind, value)| {
        builder
            .add_fact(AgentApi::observation_fact(
                kind,
                0,
                0,
                0,
                0,
                (value * MICRO) as i64,
            ))
            .expect("at most six facts, within the fact bound");
    });
    let observation = builder.build();

    // Decide. The control law: a perceived scalar in, an axis deflection out.
    // Steering takes two bindings (proportional + damping) onto one axis; the
    // queue sums them.
    // The control law: a table of neutral bindings. There is not one racing
    // noun in it — a perceived scalar, a proportional gain in thousandths, and
    // the limits the axis is held inside. Two bindings drive the steering axis
    // (a proportional term on heading error and a damping term on the car's own
    // yaw rate) and the queue sums them, which is the difference between a
    // controller that converges and one that oscillates into the guardrail. It
    // is written inline because `axiom-agent`'s binding type is sealed behind
    // the facade and so cannot be named as a helper's return type.
    let mut brain = AgentApi::axis_map_brain(vec![
        AgentApi::axis_binding(
            FACT_HEADING_ERROR,
            AXIS_STEER,
            driver.steer_gain_milli,
            0,
            -1_000_000,
            1_000_000,
        ),
        AgentApi::axis_binding(
            FACT_YAW_RATE,
            AXIS_STEER,
            driver.steer_damping_milli,
            0,
            -1_000_000,
            1_000_000,
        ),
        AgentApi::axis_binding(
            FACT_SPEED_HEADROOM,
            AXIS_THROTTLE,
            THROTTLE_GAIN_MILLI,
            0,
            0,
            1_000_000,
        ),
        AgentApi::axis_binding(
            FACT_SPEED_EXCESS,
            AXIS_BRAKE,
            BRAKE_GAIN_MILLI,
            0,
            0,
            1_000_000,
        ),
        AgentApi::axis_binding(
            FACT_BOOST_OPPORTUNITY,
            AXIS_BOOST,
            BOOST_GAIN_MILLI,
            0,
            0,
            1_000_000,
        ),
        AgentApi::axis_binding(
            FACT_LANE_INTENT,
            AXIS_LANE,
            LANE_GAIN_MILLI,
            0,
            -1_000_000,
            1_000_000,
        ),
    ]);
    let mut memory = AgentApi::empty_memory(1);
    let step = RuntimeStep::new(
        FrameIndex::new(tick),
        Tick::new(tick),
        FIXED_DELTA_NANOS,
        0,
    );
    let (report, queue) = AgentApi::step(
        agent_id,
        profile,
        &mut brain,
        &observation,
        &mut memory,
        step,
    );

    // Lower. Axis deflections come back in micro-units, already folded per axis
    // by the queue — so the two steering bindings arrive here summed. This is
    // the only place a command reaches a simulation.
    let command = DriveCommand {
        throttle: deflection(queue.axis_value(AXIS_THROTTLE)).clamp(0.0, 1.0),
        brake: deflection(queue.axis_value(AXIS_BRAKE)).clamp(0.0, 1.0),
        steer: deflection(queue.axis_value(AXIS_STEER)).clamp(-1.0, 1.0),
        boost: deflection(queue.axis_value(AXIS_BOOST)) > 0.5,
        // Inert on the wheel profile — `sim::rails` is the only reader, and a
        // wheel race has no rails state — so this needs no profile branch here.
        lane_step: deflection(queue.axis_value(AXIS_LANE)).round().clamp(-1.0, 1.0) as i8,
        ..DriveCommand::IDLE
    };
    (command, report.emitted_action_count())
}

/// Drive `sim` with the agent, on `driver`'s technique, until it finishes or
/// `limit` steps elapse.
pub fn race(mut sim: RaceSim, driver: &DriverTuning, limit: u32) -> AgentRace {
    let mut decisions = 0u64;
    let mut axis_intents = 0u64;
    let mut boost_steps = 0u32;
    let mut traffic_impacts = 0u32;
    let mut offroad_steps = 0u32;
    let mut lifted_steps = 0u32;
    let mut braking_steps = 0u32;
    let mut speed_sum = 0f64;
    let mut milestones = Vec::new();
    let mut seen_section = None;
    let mut steps = 0u32;

    while (sim.phase() != RacePhase::Finished) & (steps < limit) {
        let (command, intents) = drive_one_step(&sim, driver, u64::from(steps));
        decisions += 1;
        axis_intents += intents as u64;

        boost_steps += u32::from(command.boost);
        lifted_steps += u32::from(command.throttle < 0.999);
        braking_steps += u32::from(command.brake > 0.001);
        offroad_steps += u32::from(sim.car().surface.is_off_road());
        speed_sum += f64::from(sim.car().speed());

        let section = sim.section();
        (seen_section != Some(section)).then(|| {
            seen_section = Some(section);
            milestones.push(format!(
                "{:>6.1}s  {:>7.0}m  {:?} at {:.0} km/h",
                sim.elapsed_seconds(),
                sim.car().distance,
                section,
                sim.car().speed() * 3.6
            ));
        });

        sim.step(command);
        sim.take_events().into_iter().for_each(|event| {
            matches!(event, RaceEvent::Impact { traffic: true, .. }).then(|| {
                traffic_impacts += 1;
            });
            matches!(event, RaceEvent::Finished { .. }).then(|| {
                milestones.push(format!(
                    "{:>6.1}s  {:>7.0}m  crossed the line",
                    sim.elapsed_seconds(),
                    sim.car().distance
                ));
            });
        });
        steps += 1;
    }

    AgentRace {
        finished: sim.phase() == RacePhase::Finished,
        elapsed_seconds: sim.elapsed_seconds(),
        steps,
        progress: sim.progress(),
        top_speed: sim.top_speed_seen(),
        near_misses: sim.near_miss_count(),
        impacts: sim.impact_count(),
        traffic_impacts,
        offroad_steps,
        lifted_steps,
        braking_steps,
        mean_speed: (speed_sum / f64::from(steps.max(1))) as f32,
        decisions,
        axis_intents,
        boost_steps,
        milestones,
    }
}

/// A micro-unit axis deflection as the float the command wants.
fn deflection(micro_units: i64) -> f32 {
    micro_units as f32 / MICRO
}
