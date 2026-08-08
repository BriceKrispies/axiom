//! Ordered, typed simulation events — the one-way channel from the
//! deterministic simulation to the camera director and presentation juice.
//! Events are emitted in a stable order within a tick and stamped with a
//! stable [`EventId`] (`tick << 8 | sequence`), which seeds presentation
//! variation (`config seed ^ event id`) without any ambient randomness.

use axiom::prelude::Vec3;

use crate::identity::{PlayId, PlayerId};

/// Stable identity of one emitted event: `tick << 8 | per-tick sequence`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EventId(pub u64);

impl EventId {
    /// Compose from a tick and the event's per-tick sequence number.
    pub fn new(tick: u64, seq: u8) -> Self {
        EventId((tick << 8) | u64::from(seq))
    }
}

/// Why the play ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayEndReason {
    /// The ball carrier was tackled to the ground.
    Tackled,
    /// The pass hit the ground uncaught.
    Incomplete,
    /// The carrier ran out of bounds.
    OutOfBounds,
    /// The carrier crossed the goal line untouched (no scoring rules yet —
    /// the play simply ends).
    BrokeFree,
    /// A defender intercepted the pass — a turnover. (For now the run ends here;
    /// the possession-flip hook lives in the drive layer.)
    Intercepted,
}

/// Which of the running back's three moves an event refers to. A plain code on
/// the event rather than a reference into [`crate::runback`], so the event
/// stream stays a leaf that presentation and the agent can read without
/// depending on the gameplay subsystem that produced it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunbackMoveCode {
    JukeLeft,
    JukeRight,
    Shoulder,
    Jump,
}

impl RunbackMoveCode {
    /// The short label the HUD and the agent trace print.
    pub fn label(self) -> &'static str {
        match self {
            RunbackMoveCode::JukeLeft => "JUKE LEFT",
            RunbackMoveCode::JukeRight => "JUKE RIGHT",
            RunbackMoveCode::Shoulder => "SHOULDER",
            RunbackMoveCode::Jump => "LEAP",
        }
    }
}

/// One typed simulation event. Payload floats are exact sim values, so replays
/// compare bit-for-bit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SimEvent {
    /// The showcase play began (players placed in formation).
    PlayStarted { play: PlayId },
    /// The snap: the ball leaves the snapper toward the quarterback.
    Snap {
        snapper: PlayerId,
        quarterback: PlayerId,
    },
    /// The quarterback began the drop-back.
    DropBack { quarterback: PlayerId },
    /// The exchange began: the ball is leaving the quarterback's hands for the
    /// back's. Possession changes when it lands, not here.
    Handoff {
        quarterback: PlayerId,
        back: PlayerId,
        point: Vec3,
    },
    /// The running back committed to one of his three moves this tick. The
    /// *attempt*, not the outcome — the outcome is one of the three success
    /// events below, or nothing at all.
    RunbackMove {
        runner: PlayerId,
        move_code: RunbackMoveCode,
        /// Ground speed at the moment of commitment, yd/s. Every move's worth
        /// is a function of it, so it is the number worth recording.
        speed: f32,
    },
    /// **A successful dodge.** A defender who had a credible imminent tackle on
    /// the runner's pre-juke line was left behind by the juke, and the play went
    /// on. `gap` is how far the defender ended up off the runner when he was
    /// beaten, yd.
    TackleDodged {
        runner: PlayerId,
        defender: PlayerId,
        gap: f32,
    },
    /// **A successful jump over a defender.** The runner was airborne, the
    /// defender passed through the horizontal encounter region beneath him with
    /// `clearance` yards of daylight above the defender's tackling reach, and
    /// the runner landed still carrying.
    DefenderHurdled {
        runner: PlayerId,
        defender: PlayerId,
        clearance: f32,
    },
    /// **A successful shoulder charge.** The contact resolved in the runner's
    /// favour: `impulse` (his momentum into the hit) beat `resistance` (the
    /// defender's braced anchor), the defender was displaced, and the runner
    /// kept the ball. Both terms ride along so the reason is inspectable.
    TackleBroken {
        runner: PlayerId,
        defender: PlayerId,
        impulse: f32,
        resistance: f32,
    },
    /// A shoulder charge that **lost** — the same contest, decided the other
    /// way. Not a success signal; it exists so the failure is as legible as the
    /// win, to the player and to a test.
    ChargeStuffed {
        runner: PlayerId,
        defender: PlayerId,
        impulse: f32,
        resistance: f32,
    },
    /// The forward pass was released.
    Throw {
        quarterback: PlayerId,
        release: Vec3,
        velocity: Vec3,
        target: Vec3,
        eta_ticks: u32,
    },
    /// A receiver is attempting the catch this tick.
    CatchAttempt { player: PlayerId },
    /// The catch completed; possession transfers this tick.
    CatchCompleted { player: PlayerId },
    /// Possession changed hands (including snap and catch).
    PossessionChanged {
        from: Option<PlayerId>,
        to: Option<PlayerId>,
    },
    /// A defender broke up the pass at the catch point (a contested incompletion
    /// — the defender got a hand on it but could not secure it).
    PassBrokenUp { defender: PlayerId, position: Vec3 },
    /// A defender intercepted the pass — a clean play on the ball. Possession
    /// passes to the defender (the run ends on it for now).
    Intercepted { defender: PlayerId, position: Vec3 },
    /// The ball is live on the ground with no possessor.
    BallLoose { position: Vec3 },
    /// The ball settled on the turf.
    BallGrounded { position: Vec3 },
    /// A blocker engaged a defender.
    BlockEngaged {
        blocker: PlayerId,
        defender: PlayerId,
    },
    /// A tackle landed. `strength` is normalized `0..=1`.
    TackleContact {
        tackler: PlayerId,
        target: PlayerId,
        contact_point: Vec3,
        contact_direction: Vec3,
        relative_speed: f32,
        strength: f32,
        target_airborne: bool,
    },
    /// **A tackle the carrier shed.** A defender reached him and lost the
    /// contest: not enough closing speed, a bad angle, or simply outmuscled.
    /// Both terms ride along so the reason is inspectable.
    ///
    /// Not a player *move* — the runner did nothing but stay on his feet — so it
    /// is its own event rather than one of the three success signals. It is what
    /// makes contact a question the game asks rather than an answer it gives.
    TackleShed {
        tackler: PlayerId,
        runner: PlayerId,
        impulse: f32,
        resistance: f32,
        /// The carrier's balance AFTER paying for it: sheds are cumulative, and
        /// this is the number that says how close the next one is to landing.
        balance_left: f32,
    },
    /// A player left the ground (big hit).
    PlayerAirborne { player: PlayerId },
    /// A falling player hit the turf. `strength` is normalized `0..=1`.
    GroundImpact {
        player: PlayerId,
        position: Vec3,
        strength: f32,
    },
    /// The play is over.
    PlayEnded { reason: PlayEndReason },
    /// All showcase state was reset to formation.
    PlayReset,
}

/// An emitted event with its stamp: the tick it happened on and its stable id.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StampedEvent {
    pub id: EventId,
    pub tick: u64,
    pub event: SimEvent,
}

/// Per-tick ordered event sink with a hard bound (drops beyond capacity are a
/// sim bug caught in tests; nothing here allocates unboundedly).
#[derive(Debug, Default)]
pub struct EventSink {
    events: Vec<StampedEvent>,
    seq: u8,
    tick: u64,
}

/// The most events one tick may emit.
pub const MAX_EVENTS_PER_TICK: usize = 32;

impl EventSink {
    /// Begin a new tick: clear the buffer, reset the sequence counter.
    pub fn begin_tick(&mut self, tick: u64) {
        self.events.clear();
        self.seq = 0;
        self.tick = tick;
    }

    /// Emit `event` in order. Silently ignores emissions past the per-tick cap
    /// (bounded by construction; the cap is asserted in tests).
    pub fn emit(&mut self, event: SimEvent) {
        if self.events.len() < MAX_EVENTS_PER_TICK {
            self.events.push(StampedEvent {
                id: EventId::new(self.tick, self.seq),
                tick: self.tick,
                event,
            });
            self.seq = self.seq.saturating_add(1);
        }
    }

    /// The events emitted this tick, in emission order.
    pub fn events(&self) -> &[StampedEvent] {
        &self.events
    }
}
