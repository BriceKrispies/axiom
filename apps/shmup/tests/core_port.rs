//! The ported deterministic core, pinned against the JavaScript it came from.
//!
//! Every `expected` array in the RNG section was captured by running the
//! original `C:/dev/Claude-of-Duty/src/core/rng.js` under Node (v24) and
//! printing `toString(16)` / `toPrecision(17)`. They are golden values, not
//! recomputations: if a future edit to `rng.rs` changes one of them, the port
//! has silently stopped being the source's generator and every recoil pattern,
//! spread cone and scatter placement downstream has moved.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use axiom_shmup::config::{
    Config, Quality, FIXED_DT, FIXED_STEP, HIGH, LOW, MAX_SUBSTEPS, MEDIUM, PHYSICS_HZ, ULTRA,
    UNITS,
};
use axiom_shmup::engine::{Ctx, Engine, Time, CAPTURE_SEED};
use axiom_shmup::error::CoreError;
use axiom_shmup::events::EventBus;
use axiom_shmup::registry::{downcast, Phase, Registry, Subsystem};
use axiom_shmup::rng::Rng;
use axiom_kernel::Seconds;

// ---------------------------------------------------------------------------
// rng.js
// ---------------------------------------------------------------------------

fn take_u32(rng: &mut Rng, n: usize) -> Vec<u32> {
    (0..n).map(|_| rng.u32()).collect()
}

/// `ln`, `sin` and `cos` are not bit-guaranteed across libm implementations, so
/// the transcendental draws are compared to the JavaScript within an absolute
/// tolerance rather than exactly. Everything reachable by integer arithmetic and
/// exact division is compared exactly.
fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-12,
        "expected {expected:.17}, got {actual:.17}"
    );
}

#[test]
fn default_seed_reproduces_the_javascript_u32_sequence() {
    let mut rng = Rng::default();
    assert_eq!(
        take_u32(&mut rng, 12),
        vec![
            0x430c69e6, 0xd690e3cf, 0x955b817c, 0xb5c48450, 0xd49fe08c, 0xf905c64c, 0x5e2ab39d,
            0x2ab21dde, 0x2216dbf1, 0xd22e065a, 0xc37f1156, 0xae657640,
        ]
    );
    // The defaulted constructor argument is the golden-ratio constant itself.
    assert_eq!(Rng::DEFAULT_SEED, 0x9e37_79b9);
    assert_eq!(take_u32(&mut Rng::new(Rng::DEFAULT_SEED), 4), {
        let mut fresh = Rng::default();
        take_u32(&mut fresh, 4)
    });
}

#[test]
fn seed_one_reproduces_the_javascript_u32_sequence() {
    let mut rng = Rng::new(1);
    assert_eq!(
        take_u32(&mut rng, 12),
        vec![
            0x177119d4, 0x81962de5, 0xe3609ab3, 0x7cbcc17a, 0x6f2c548e, 0x81e3835f, 0xbf2cf5be,
            0xaa5f01ad, 0x2152cff3, 0xca63256a, 0xf0897759, 0x7300b898,
        ]
    );
}

#[test]
fn the_capture_seed_reproduces_the_javascript_sequence_and_state() {
    let mut rng = Rng::new(CAPTURE_SEED);
    assert_eq!(
        take_u32(&mut rng, 8),
        vec![
            0x83903eb0, 0x8418764e, 0x9f570b4d, 0x5b3797c8, 0x41ac68ff, 0xd9823dbc, 0x11a6b690,
            0x6ccf9960,
        ]
    );
    // The four state words after those eight draws, so a drift in the state
    // permutation is caught even where the scrambler output would still agree.
    assert_eq!(
        rng.state(),
        [0xfc2f74e4, 0x93fc7d8d, 0xd4f0eed1, 0xd85fa9c5]
    );
}

#[test]
fn splitmix32_expands_a_seed_into_the_javascript_state_words() {
    // Seed 0 exercises the expander alone: no draw has happened yet, so these
    // are exactly what SplitMix32 wrote.
    assert_eq!(
        Rng::new(0).state(),
        [0x64625032, 0xd9c0799c, 0xaf362e10, 0x7fa88912]
    );
}

#[test]
fn float_reproduces_the_javascript_doubles_exactly() {
    let mut rng = Rng::new(42);
    let expected = [
        0.15377165307290852,
        0.85048930067569017,
        0.018084544222801924,
        0.21193028264679015,
        0.53489062841981649,
        0.88151373830623925,
    ];
    for want in expected {
        assert_eq!(rng.float(), want);
    }
}

#[test]
fn range_and_signed_reproduce_the_javascript_doubles_exactly() {
    let mut rng = Rng::new(7);
    for want in [
        -1.6569328426849097,
        -0.84106352832168341,
        4.5089724585413933,
        4.6649143653921783,
    ] {
        assert_eq!(rng.range(-2.0, 5.0), want);
    }

    let mut rng = Rng::new(7);
    for want in [
        -0.90198081219568849,
        -0.66887529380619526,
        0.85970641672611237,
        0.90426124725490808,
    ] {
        assert_eq!(rng.signed(), want);
    }
}

#[test]
fn int_reproduces_the_javascript_sequence_for_positive_and_signed_spans() {
    let mut rng = Rng::new(7);
    let rolls: Vec<i32> = (0..10).map(|_| rng.int(1, 6)).collect();
    assert_eq!(rolls, vec![6, 5, 3, 3, 2, 1, 1, 5, 6, 5]);

    let mut rng = Rng::new(7);
    let signed: Vec<i32> = (0..10).map(|_| rng.int(-3, 3)).collect();
    assert_eq!(signed, vec![1, -3, -2, 1, 2, 2, 3, -2, -3, 1]);

    // Both ends are inclusive, and a degenerate span is the constant.
    let mut rng = Rng::new(11);
    assert!((0..200).all(|_| (0..=1).contains(&rng.int(0, 1))));
    assert_eq!(rng.int(4, 4), 4);
}

#[test]
fn gauss_reproduces_the_javascript_pairs_and_caches_the_spare() {
    let mut rng = Rng::new(99);
    for want in [
        0.87737122226000475,
        0.59608940694124934,
        0.86030561221470525,
        -0.67761188796533822,
        -1.1471150044107319,
        -0.545133557928002,
    ] {
        assert_close(rng.gauss(), want);
    }

    // The cached spare is why two `gauss()` calls cost exactly two `float()`
    // draws: after an even number of them the stream is where a plain pair of
    // float draws would have left it.
    let mut paired = Rng::new(99);
    paired.gauss();
    paired.gauss();
    let mut plain = Rng::new(99);
    (0..2).for_each(|_| {
        plain.float();
    });
    assert_eq!(paired.state(), plain.state());
}

#[test]
fn reseed_keeps_the_cached_gauss_spare() {
    // The source's `seed()` touches only s0..s3, so a re-seed does not discard
    // the spare — the next `gauss()` returns the value cached before it. Not
    // elegant; it is what the source does, and a subsystem that re-seeds
    // mid-run depends on it.
    let mut rng = Rng::new(5);
    let first = rng.gauss();
    rng.seed(5);
    let after_reseed = rng.gauss();

    let mut fresh = Rng::new(5);
    assert_close(fresh.gauss(), first);
    assert_close(after_reseed, 0.22565725010667856);
    assert_ne!(after_reseed, first);
}

#[test]
fn pick_reproduces_the_javascript_choices() {
    let mut rng = Rng::new(1234);
    let arr = ['a', 'b', 'c', 'd', 'e'];
    let picked: String = (0..12).map(|_| *rng.pick(&arr)).collect();
    assert_eq!(picked, "edaddddeadbc");
}

#[test]
fn disc_reproduces_the_javascript_points_inside_the_unit_disc() {
    let mut rng = Rng::new(2024);
    let expected = [
        (-0.58913144107053439, 0.62628438082147908),
        (0.31250109459777842, 0.75926462722541854),
        (-0.26357235078297109, -0.010360462277742203),
        (-0.06021810065855733, -0.088997898295020195),
    ];
    for (want_x, want_y) in expected {
        let (x, y) = rng.disc();
        assert_close(x, want_x);
        assert_close(y, want_y);
        assert!(x * x + y * y <= 1.0);
    }
}

#[test]
fn fork_costs_the_parent_one_draw_and_seeds_the_child_from_it() {
    let mut parent = Rng::new(0xabcdef);
    let mut child = parent.fork();

    // The child is seeded from the parent's next u32 …
    let mut sibling = Rng::new(0xabcdef);
    let fork_seed = sibling.u32();
    assert_eq!(fork_seed, 0x1dcc177f);
    assert_eq!(take_u32(&mut child, 4), {
        let mut expected = Rng::new(fork_seed);
        take_u32(&mut expected, 4)
    });
    assert_eq!(
        take_u32(&mut Rng::new(fork_seed), 4),
        vec![0x3da60a85, 0xa45ed6e1, 0x505f2a82, 0x109193a1]
    );

    // … and the parent has advanced by exactly that one draw, so a subsystem
    // forking off it cannot perturb anyone else's sequence.
    assert_eq!(parent.u32(), 0x92d9d157);
    assert_eq!(sibling.u32(), 0x92d9d157);
}

#[test]
fn two_forks_of_the_same_parent_are_independent_streams() {
    let mut parent = Rng::new(CAPTURE_SEED);
    let mut a = parent.fork();
    let mut b = parent.fork();
    assert_ne!(take_u32(&mut a, 8), take_u32(&mut b, 8));

    // Draining one fork does not move the other, which is the whole point.
    let mut parent = Rng::new(CAPTURE_SEED);
    let mut first = parent.fork();
    let mut second = parent.fork();
    (0..1000).for_each(|_| {
        first.u32();
    });
    let mut control = Rng::new(CAPTURE_SEED);
    control.fork();
    let mut expected_second = control.fork();
    assert_eq!(take_u32(&mut second, 4), take_u32(&mut expected_second, 4));
}

// ---------------------------------------------------------------------------
// registry.js — the Registry
// ---------------------------------------------------------------------------

/// A subsystem that writes every call it receives into a shared log, so a test
/// can assert on order across systems.
struct Recorder {
    id: &'static str,
    deps: &'static [&'static str],
    phases: &'static [Phase],
    log: Rc<RefCell<Vec<String>>>,
    updates: u32,
}

impl Recorder {
    fn new(
        id: &'static str,
        deps: &'static [&'static str],
        phases: &'static [Phase],
        log: &Rc<RefCell<Vec<String>>>,
    ) -> Self {
        Recorder {
            id,
            deps,
            phases,
            log: Rc::clone(log),
            updates: 0,
        }
    }

    fn note(&self, what: &str) {
        self.log.borrow_mut().push(format!("{}:{what}", self.id));
    }
}

const ALL_PHASES: &[Phase] = &[
    Phase::FixedUpdate,
    Phase::Update,
    Phase::LateUpdate,
    Phase::Resize,
    Phase::Render,
];

impl Subsystem for Recorder {
    fn id(&self) -> &'static str {
        self.id
    }
    fn deps(&self) -> &'static [&'static str] {
        self.deps
    }
    fn phases(&self) -> &'static [Phase] {
        self.phases
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn init(&mut self, _ctx: &Ctx<'_>) -> Result<(), CoreError> {
        self.note("init");
        Ok(())
    }
    fn fixed_update(&mut self, _h: Seconds, _ctx: &Ctx<'_>) {
        self.note("fixed");
    }
    fn update(&mut self, _dt: Seconds, _ctx: &Ctx<'_>) {
        self.updates += 1;
        self.note("update");
    }
    fn late_update(&mut self, _dt: Seconds, _ctx: &Ctx<'_>) {
        self.note("late");
    }
    fn resize(&mut self, width: u32, height: u32, _ctx: &Ctx<'_>) {
        self.log
            .borrow_mut()
            .push(format!("{}:resize {width}x{height}", self.id));
    }
    fn render(&mut self, _ctx: &Ctx<'_>) {
        self.note("render");
    }
    fn dispose(&mut self) {
        self.note("dispose");
    }
}

/// `Result::unwrap_err` needs `T: Debug`, and `T` here is a `SystemRef` —
/// `dyn Subsystem` cannot be `Debug` without forcing that bound on every system
/// the game will ever have, including ones holding GPU handles. Unwrapping the
/// error side directly costs nothing and asks for nothing.
fn err_of<T>(result: Result<T, CoreError>) -> CoreError {
    result.err().expect("expected an error")
}

fn ids_of(systems: &[axiom_shmup::registry::SystemRef]) -> Vec<&'static str> {
    systems.iter().map(|s| s.borrow().id()).collect()
}

#[test]
fn resolve_orders_dependencies_before_dependents() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut registry = Registry::new();
    // Registered in an order that is *not* the dependency order, so a pass-through
    // implementation could not accidentally look correct.
    registry
        .add(Recorder::new("render", &["world", "physics"], ALL_PHASES, &log))
        .unwrap();
    registry
        .add(Recorder::new("world", &["physics"], ALL_PHASES, &log))
        .unwrap();
    registry
        .add(Recorder::new("physics", &[], ALL_PHASES, &log))
        .unwrap();
    registry
        .add(Recorder::new("audio", &["world"], ALL_PHASES, &log))
        .unwrap();

    let order = registry.resolve().unwrap();
    assert_eq!(ids_of(&order), vec!["physics", "world", "render", "audio"]);
    assert_eq!(registry.len(), 4);
    assert!(!registry.is_empty());
}

#[test]
fn resolve_is_stable_on_registration_order_for_independent_systems() {
    // Two systems with no relationship keep their registration order — the
    // source iterates an insertion-ordered Map, and the init order of unrelated
    // systems is observable (they fork the shared Rng in that order).
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut registry = Registry::new();
    registry
        .add(Recorder::new("b", &[], ALL_PHASES, &log))
        .unwrap();
    registry
        .add(Recorder::new("a", &[], ALL_PHASES, &log))
        .unwrap();
    assert_eq!(ids_of(&registry.resolve().unwrap()), vec!["b", "a"]);
}

#[test]
fn resolve_reports_a_dependency_cycle() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut registry = Registry::new();
    registry
        .add(Recorder::new("a", &["b"], ALL_PHASES, &log))
        .unwrap();
    registry
        .add(Recorder::new("b", &["a"], ALL_PHASES, &log))
        .unwrap();
    let err = err_of(registry.resolve());
    assert_eq!(err.message(), "dependency cycle at \"a\" (via b)");
}

#[test]
fn resolve_reports_a_self_cycle() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut registry = Registry::new();
    registry
        .add(Recorder::new("a", &["a"], ALL_PHASES, &log))
        .unwrap();
    assert_eq!(
        err_of(registry.resolve()).message(),
        "dependency cycle at \"a\" (via a)"
    );
}

#[test]
fn resolve_reports_an_unregistered_dependency() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut registry = Registry::new();
    registry
        .add(Recorder::new("weapons", &["assets"], ALL_PHASES, &log))
        .unwrap();
    assert_eq!(
        err_of(registry.resolve()).message(),
        "\"weapons\" depends on unregistered subsystem \"assets\""
    );
}

#[test]
fn a_diamond_visits_the_shared_dependency_once() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut registry = Registry::new();
    registry
        .add(Recorder::new("base", &[], ALL_PHASES, &log))
        .unwrap();
    registry
        .add(Recorder::new("left", &["base"], ALL_PHASES, &log))
        .unwrap();
    registry
        .add(Recorder::new("right", &["base"], ALL_PHASES, &log))
        .unwrap();
    registry
        .add(Recorder::new("top", &["left", "right"], ALL_PHASES, &log))
        .unwrap();
    assert_eq!(
        ids_of(&registry.resolve().unwrap()),
        vec!["base", "left", "right", "top"]
    );
}

#[test]
fn duplicate_ids_are_rejected() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut registry = Registry::new();
    registry
        .add(Recorder::new("hud", &[], ALL_PHASES, &log))
        .unwrap();
    assert_eq!(
        err_of(registry.add(Recorder::new("hud", &[], ALL_PHASES, &log))).message(),
        "duplicate subsystem id \"hud\""
    );
    assert_eq!(registry.len(), 1);
}

#[test]
fn get_peek_and_has_agree_about_what_is_registered() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut registry = Registry::new();
    registry
        .add(Recorder::new("hud", &[], ALL_PHASES, &log))
        .unwrap();
    assert!(registry.has("hud"));
    assert_eq!(registry.get("hud").unwrap().borrow().id(), "hud");
    assert!(registry.peek("hud").is_some());

    assert!(!registry.has("nope"));
    assert!(registry.peek("nope").is_none());
    assert_eq!(
        err_of(registry.get("nope")).message(),
        "subsystem \"nope\" not registered"
    );
}

#[test]
fn with_filters_by_declared_phase_and_caches_the_result() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut registry = Registry::new();
    registry
        .add(Recorder::new("physics", &[], &[Phase::FixedUpdate], &log))
        .unwrap();
    registry
        .add(Recorder::new(
            "camera",
            &["physics"],
            &[Phase::Update, Phase::LateUpdate],
            &log,
        ))
        .unwrap();
    registry
        .add(Recorder::new("hud", &[], &[Phase::Update], &log))
        .unwrap();

    assert_eq!(
        ids_of(&registry.with(Phase::FixedUpdate).unwrap()),
        vec!["physics"]
    );
    assert_eq!(
        ids_of(&registry.with(Phase::Update).unwrap()),
        vec!["camera", "hud"]
    );
    assert_eq!(
        ids_of(&registry.with(Phase::LateUpdate).unwrap()),
        vec!["camera"]
    );
    assert!(registry.with(Phase::Render).unwrap().is_empty());

    // Second call is the cache; same content, and still correct after an
    // explicit invalidate.
    assert_eq!(
        ids_of(&registry.with(Phase::Update).unwrap()),
        vec!["camera", "hud"]
    );
    registry.invalidate();
    assert_eq!(
        ids_of(&registry.with(Phase::Update).unwrap()),
        vec!["camera", "hud"]
    );
}

#[test]
fn adding_after_a_resolve_re_resolves_rather_than_dropping_the_new_system() {
    // The one deliberate divergence from the source, which leaves a stale order
    // behind and silently never steps the late arrival.
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut registry = Registry::new();
    registry
        .add(Recorder::new("a", &[], &[Phase::Update], &log))
        .unwrap();
    assert_eq!(ids_of(&registry.with(Phase::Update).unwrap()), vec!["a"]);
    registry
        .add(Recorder::new("b", &["a"], &[Phase::Update], &log))
        .unwrap();
    assert_eq!(
        ids_of(&registry.with(Phase::Update).unwrap()),
        vec!["a", "b"]
    );
}

#[test]
fn ordered_resolves_on_first_use_and_reuses_the_result() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut registry = Registry::new();
    registry
        .add(Recorder::new("b", &["a"], ALL_PHASES, &log))
        .unwrap();
    registry
        .add(Recorder::new("a", &[], ALL_PHASES, &log))
        .unwrap();
    assert_eq!(ids_of(&registry.ordered().unwrap()), vec!["a", "b"]);
    assert_eq!(ids_of(&registry.ordered().unwrap()), vec!["a", "b"]);
}

#[test]
fn downcast_recovers_the_concrete_system_behind_an_id() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut registry = Registry::new();
    registry
        .add(Recorder::new("hud", &[], &[Phase::Update], &log))
        .unwrap();
    let handle = registry.get("hud").unwrap();
    let typed = downcast::<Recorder>(&handle).expect("hud is a Recorder");
    assert_eq!(typed.updates, 0);
    drop(typed);

    struct Other;
    impl Subsystem for Other {
        fn id(&self) -> &'static str {
            "other"
        }
        fn phases(&self) -> &'static [Phase] {
            &[]
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
    }
    assert!(downcast::<Other>(&handle).is_none());
}

// ---------------------------------------------------------------------------
// registry.js — the EventBus
// ---------------------------------------------------------------------------

fn record_into(log: &Rc<RefCell<Vec<String>>>, what: &'static str) -> impl Fn(&dyn Any) -> Result<(), CoreError> {
    let log = Rc::clone(log);
    move |payload: &dyn Any| {
        let n = payload.downcast_ref::<u32>().copied().unwrap_or(0);
        log.borrow_mut().push(format!("{what}({n})"));
        Ok(())
    }
}

#[test]
fn emit_dispatches_synchronously_in_subscription_order() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let bus = EventBus::new();
    bus.on("shot", record_into(&log, "a"));
    bus.on("shot", record_into(&log, "b"));

    assert!(bus.emit("shot", &7u32).is_empty());
    // Synchronous: the log is already complete when `emit` returns.
    assert_eq!(*log.borrow(), vec!["a(7)", "b(7)"]);
    // An event nobody listens to is a no-op, not an error.
    assert!(bus.emit("nothing", &0u32).is_empty());
}

#[test]
fn off_removes_a_handler_and_a_stale_id_is_a_no_op() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let bus = EventBus::new();
    let a = bus.on("shot", record_into(&log, "a"));
    bus.on("shot", record_into(&log, "b"));
    assert_eq!(bus.handler_count("shot"), 2);

    bus.off("shot", a);
    bus.off("shot", a); // already gone
    bus.off("never-used", a); // never existed
    bus.emit("shot", &1u32);
    assert_eq!(*log.borrow(), vec!["b(1)"]);
    assert_eq!(bus.handler_count("shot"), 1);
}

#[test]
fn a_handler_may_unsubscribe_during_dispatch() {
    // The source copies the handler set before iterating so this is safe. The
    // exact consequence it preserves: the handler removed mid-dispatch was
    // already in the copy, so it still runs *this* time and never again.
    let log = Rc::new(RefCell::new(Vec::new()));
    let bus = EventBus::new();
    let victim = Rc::new(RefCell::new(None));

    let killer_bus = bus.clone();
    let killer_victim = Rc::clone(&victim);
    let killer_log = Rc::clone(&log);
    bus.on("tick", move |_| {
        killer_log.borrow_mut().push("killer".to_string());
        if let Some(id) = *killer_victim.borrow() {
            killer_bus.off("tick", id);
        }
        Ok(())
    });
    *victim.borrow_mut() = Some(bus.on("tick", record_into(&log, "victim")));

    bus.emit("tick", &0u32);
    assert_eq!(*log.borrow(), vec!["killer", "victim(0)"]);
    assert_eq!(bus.handler_count("tick"), 1);

    log.borrow_mut().clear();
    bus.emit("tick", &0u32);
    assert_eq!(*log.borrow(), vec!["killer"]);
}

#[test]
fn a_handler_may_unsubscribe_itself_during_dispatch() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let bus = EventBus::new();
    let slot: Rc<RefCell<Option<_>>> = Rc::new(RefCell::new(None));
    let inner_bus = bus.clone();
    let inner_slot = Rc::clone(&slot);
    let inner_log = Rc::clone(&log);
    let id = bus.on("boom", move |_| {
        inner_log.borrow_mut().push("self".to_string());
        inner_bus.off("boom", inner_slot.borrow().unwrap());
        Ok(())
    });
    *slot.borrow_mut() = Some(id);

    bus.emit("boom", &0u32);
    bus.emit("boom", &0u32);
    assert_eq!(*log.borrow(), vec!["self"]);
}

#[test]
fn once_fires_exactly_once() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let bus = EventBus::new();
    bus.once("ready", record_into(&log, "once"));
    bus.on("ready", record_into(&log, "always"));

    bus.emit("ready", &1u32);
    bus.emit("ready", &2u32);
    assert_eq!(*log.borrow(), vec!["once(1)", "always(1)", "always(2)"]);
    assert_eq!(bus.handler_count("ready"), 1);
}

#[test]
fn once_can_be_cancelled_before_it_fires() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let bus = EventBus::new();
    let id = bus.once("ready", record_into(&log, "once"));
    bus.off("ready", id);
    bus.emit("ready", &1u32);
    assert!(log.borrow().is_empty());
}

#[test]
fn a_failing_handler_does_not_stop_the_rest_of_the_dispatch() {
    // Property 3 of the source's try/catch, as returned values: every handler
    // runs, and the failure comes back to the caller instead of vanishing into
    // console.error.
    let log = Rc::new(RefCell::new(Vec::new()));
    let bus = EventBus::new();
    bus.on("frame", record_into(&log, "before"));
    let bad = bus.on("frame", |_| Err(CoreError::new("handler exploded")));
    bus.on("frame", record_into(&log, "after"));

    let failures = bus.emit("frame", &3u32);
    assert_eq!(*log.borrow(), vec!["before(3)", "after(3)"]);
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].event, "frame");
    assert_eq!(failures[0].subscription, bad);
    assert_eq!(failures[0].error.message(), "handler exploded");
}

#[test]
fn clear_drops_every_subscription() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let bus = EventBus::new();
    bus.on("a", record_into(&log, "a"));
    bus.on("b", record_into(&log, "b"));
    bus.clear();
    bus.emit("a", &0u32);
    bus.emit("b", &0u32);
    assert!(log.borrow().is_empty());
    assert_eq!(bus.handler_count("a"), 0);
}

#[test]
fn a_clone_of_the_bus_is_the_same_bus() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let bus = EventBus::new();
    let other = bus.clone();
    other.on("shot", record_into(&log, "via-clone"));
    bus.emit("shot", &5u32);
    assert_eq!(*log.borrow(), vec!["via-clone(5)"]);
}

// ---------------------------------------------------------------------------
// engine.js
// ---------------------------------------------------------------------------

/// One frame's worth of milliseconds at 60 Hz.
const FRAME_MS: f64 = 1000.0 / 60.0;

fn engine_with_recorders(log: &Rc<RefCell<Vec<String>>>) -> Engine {
    let mut engine = Engine::new(Config::default(), CAPTURE_SEED);
    engine
        .add(Recorder::new("physics", &[], &[Phase::FixedUpdate], log))
        .unwrap();
    engine
        .add(Recorder::new(
            "camera",
            &["physics"],
            &[Phase::Update, Phase::LateUpdate],
            log,
        ))
        .unwrap();
    engine
        .add(Recorder::new("render", &["camera"], ALL_PHASES, log))
        .unwrap();
    engine
}

#[test]
fn init_runs_every_system_in_dependency_order() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut engine = engine_with_recorders(&log);
    engine.init().unwrap();
    assert_eq!(
        *log.borrow(),
        vec!["physics:init", "camera:init", "render:init"]
    );
}

#[test]
fn init_propagates_a_system_failure() {
    struct Broken;
    impl Subsystem for Broken {
        fn id(&self) -> &'static str {
            "broken"
        }
        fn phases(&self) -> &'static [Phase] {
            &[]
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
        fn init(&mut self, _ctx: &Ctx<'_>) -> Result<(), CoreError> {
            Err(CoreError::new("no device"))
        }
    }
    let mut engine = Engine::new(Config::default(), CAPTURE_SEED);
    engine.add(Broken).unwrap();
    assert_eq!(engine.init().unwrap_err().message(), "no device");
}

#[test]
fn a_frame_runs_the_phases_in_the_documented_order() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut engine = engine_with_recorders(&log);
    engine.init().unwrap();
    log.borrow_mut().clear();

    // Exactly one fixed step's worth of wall time.
    engine.step(FIXED_DT * 1000.0).unwrap();
    assert_eq!(
        *log.borrow(),
        vec![
            // Both fixed-phase systems step, in dependency order, before any
            // `update` runs — the frame order is per phase, not per system.
            "physics:fixed",
            "render:fixed",
            "camera:update",
            "render:update",
            "camera:late",
            "render:late",
            "render:render",
        ]
    );
}

#[test]
fn the_accumulator_runs_one_fixed_step_per_fixed_dt_of_wall_time() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut engine = engine_with_recorders(&log);
    engine.init().unwrap();
    log.borrow_mut().clear();

    // A 60 Hz frame at a 120 Hz fixed rate is two substeps.
    engine.step(FRAME_MS).unwrap();
    let fixed = log.borrow().iter().filter(|e| *e == "physics:fixed").count();
    assert_eq!(fixed, 2);

    // Sub-step frames bank time rather than stepping.
    log.borrow_mut().clear();
    engine.step(FRAME_MS + 1.0).unwrap();
    assert_eq!(
        log.borrow().iter().filter(|e| *e == "physics:fixed").count(),
        0
    );
}

#[test]
fn alpha_is_the_leftover_accumulator_as_a_fraction_of_the_fixed_step() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut engine = engine_with_recorders(&log);
    engine.init().unwrap();

    // Half a fixed step of wall time: no substep, and alpha lands on 0.5.
    engine.step(FIXED_DT * 500.0).unwrap();
    assert!((engine.time().alpha - 0.5).abs() < 1e-9);
    assert_eq!(engine.time().frame, 1);

    // Another half: one substep, and alpha returns to (near) zero.
    engine.step(FIXED_DT * 1000.0).unwrap();
    assert!(engine.time().alpha.abs() < 1e-9);
    assert_eq!(engine.time().frame, 2);
}

#[test]
fn a_long_stall_is_clamped_and_the_backlog_is_shed_rather_than_spiralling() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut engine = engine_with_recorders(&log);
    engine.init().unwrap();
    log.borrow_mut().clear();

    // Ten seconds of wall time — a tab switch. The raw delta clamps to 0.1s,
    // which is 12 fixed steps' worth, so the substep cap bites …
    engine.step(10_000.0).unwrap();
    assert_eq!(
        log.borrow().iter().filter(|e| *e == "physics:fixed").count(),
        MAX_SUBSTEPS as usize
    );
    assert!((engine.time().dt - 0.1).abs() < 1e-12);
    assert!((engine.time().raw - 0.1).abs() < 1e-12);

    // … and the leftover is dropped, not carried, so the next frame starts
    // clean instead of owing another 4 steps.
    assert_eq!(engine.time().alpha, 0.0);
    log.borrow_mut().clear();
    engine.step(10_000.0 + FIXED_DT * 1000.0).unwrap();
    assert_eq!(
        log.borrow().iter().filter(|e| *e == "physics:fixed").count(),
        1
    );
}

#[test]
fn time_never_runs_backwards_and_a_repeated_timestamp_is_a_zero_delta() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut engine = engine_with_recorders(&log);
    engine.init().unwrap();
    engine.step(100.0).unwrap();
    let after = engine.time();
    // A timestamp that went backwards clamps to zero rather than rewinding.
    engine.step(50.0).unwrap();
    assert_eq!(engine.time().dt, 0.0);
    assert_eq!(engine.time().elapsed, after.elapsed);
    assert_eq!(engine.time().raw, after.raw);
    assert_eq!(engine.time().frame, after.frame + 1);
}

#[test]
fn the_time_scale_scales_elapsed_but_not_raw() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut engine = engine_with_recorders(&log);
    engine.init().unwrap();
    engine.set_time_scale(0.25);
    engine.step(80.0).unwrap();
    let t = engine.time();
    assert!((t.raw - 0.08).abs() < 1e-12);
    assert!((t.dt - 0.02).abs() < 1e-12);
    assert!((t.elapsed - 0.02).abs() < 1e-12);
    assert_eq!(t.dt_seconds(), Seconds::finite_or_zero(0.02));
    assert!((t.elapsed_seconds().get() - 0.02).abs() < 1e-6);
    assert_eq!(t.fixed, FIXED_DT);
}

#[test]
fn reset_clock_re_anchors_the_frame_delta() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut engine = engine_with_recorders(&log);
    engine.init().unwrap();
    engine.reset_clock(50_000.0);
    engine.step(50_000.0 + FRAME_MS).unwrap();
    assert!((engine.time().raw - FRAME_MS / 1000.0).abs() < 1e-12);
}

#[test]
fn resize_reaches_every_resize_system_and_then_the_bus() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut engine = engine_with_recorders(&log);
    engine.init().unwrap();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let bus_seen = Rc::clone(&seen);
    engine.events().on("resize", move |payload| {
        let size = payload.downcast_ref::<(u32, u32)>().copied().unwrap();
        bus_seen.borrow_mut().push(size);
        Ok(())
    });
    log.borrow_mut().clear();

    engine.resize(1920, 1080).unwrap();
    assert_eq!(*log.borrow(), vec!["render:resize 1920x1080"]);
    assert_eq!(*seen.borrow(), vec![(1920, 1080)]);

    // A zero dimension is floored to 1 — the source's `Math.max(1, …)`.
    engine.resize(0, 0).unwrap();
    assert_eq!(*seen.borrow(), vec![(1920, 1080), (1, 1)]);
}

#[test]
fn dispose_tears_systems_down_in_reverse_dependency_order() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut engine = engine_with_recorders(&log);
    engine.init().unwrap();
    let fired = Rc::new(RefCell::new(0u32));
    let bus_fired = Rc::clone(&fired);
    engine.events().on("resize", move |_| {
        *bus_fired.borrow_mut() += 1;
        Ok(())
    });
    log.borrow_mut().clear();

    engine.dispose().unwrap();
    assert_eq!(
        *log.borrow(),
        vec!["render:dispose", "camera:dispose", "physics:dispose"]
    );
    // The bus is cleared too, so a stray post-teardown emit reaches nobody.
    engine.resize(800, 600).unwrap();
    assert_eq!(*fired.borrow(), 0);
}

#[test]
fn a_system_reaches_another_system_through_the_context() {
    struct Looker {
        found: bool,
        missing: bool,
        has_render: bool,
    }
    impl Subsystem for Looker {
        fn id(&self) -> &'static str {
            "looker"
        }
        fn deps(&self) -> &'static [&'static str] {
            &["physics"]
        }
        fn phases(&self) -> &'static [Phase] {
            &[]
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
        fn init(&mut self, ctx: &Ctx<'_>) -> Result<(), CoreError> {
            self.found = ctx.get("physics").is_ok();
            self.missing = ctx.peek("nope").is_none() && ctx.get("nope").is_err();
            self.has_render = ctx.has("render");
            // The shared root stream is what every subsystem forks off, once.
            let seeded = ctx.rng.borrow_mut().fork().u32();
            assert_ne!(seeded, 0);
            Ok(())
        }
    }

    let log = Rc::new(RefCell::new(Vec::new()));
    let mut engine = engine_with_recorders(&log);
    let looker = engine
        .add(Looker {
            found: false,
            missing: false,
            has_render: false,
        })
        .unwrap();
    engine.init().unwrap();

    let typed = downcast::<Looker>(&looker).unwrap();
    assert!(typed.found);
    assert!(typed.missing);
    assert!(typed.has_render);
}

#[test]
fn a_frame_is_reproducible_from_the_root_seed() {
    // The whole point of the port's determinism contract: two engines built
    // from the same seed and pumped with the same timestamps draw the same
    // numbers.
    struct Drawer {
        draws: Vec<u32>,
        stream: Rng,
    }
    impl Subsystem for Drawer {
        fn id(&self) -> &'static str {
            "drawer"
        }
        fn phases(&self) -> &'static [Phase] {
            &[Phase::Update]
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
        fn init(&mut self, ctx: &Ctx<'_>) -> Result<(), CoreError> {
            self.stream = ctx.rng.borrow_mut().fork();
            Ok(())
        }
        fn update(&mut self, _dt: Seconds, _ctx: &Ctx<'_>) {
            self.draws.push(self.stream.u32());
        }
    }

    let run = |seed: u32| {
        let mut engine = Engine::new(Config::default(), seed);
        let handle = engine
            .add(Drawer {
                draws: Vec::new(),
                stream: Rng::new(0),
            })
            .unwrap();
        engine.init().unwrap();
        for frame in 1..=10 {
            engine.step(f64::from(frame) * FRAME_MS).unwrap();
        }
        let draws = downcast::<Drawer>(&handle).unwrap().draws.clone();
        draws
    };

    assert_eq!(run(CAPTURE_SEED), run(CAPTURE_SEED));
    assert_ne!(run(CAPTURE_SEED), run(CAPTURE_SEED.wrapping_add(1)));
    assert_eq!(run(CAPTURE_SEED).len(), 10);
}

// ---------------------------------------------------------------------------
// config.js
// ---------------------------------------------------------------------------

#[test]
fn the_fixed_step_constants_match_the_source() {
    assert_eq!(PHYSICS_HZ, 120);
    assert_eq!(FIXED_DT, 1.0 / 120.0);
    assert_eq!(MAX_SUBSTEPS, 8);
    assert!((FIXED_STEP.get() - 1.0 / 120.0).abs() < 1e-9);
}

#[test]
fn the_units_block_matches_the_source() {
    // Gravity is deliberately 2.1x real — CoD-like feel, not physical accuracy.
    // Exact, not within 1e-5: `UNITS` is `f64`, the width `config.js` authors
    // and the simulation integrates in. A tolerance here would hide exactly
    // the `f32` narrowing that broke `tests/player_system_port.rs` — see
    // `config.rs`'s module doc comment.
    assert_eq!(UNITS.gravity, -9.81 * 2.1);
    assert_eq!(UNITS.player_height, 1.78);
    assert_eq!(UNITS.player_crouch_height, 1.12);
    assert_eq!(UNITS.player_radius, 0.32);
    assert_eq!(UNITS.eye_offset, 0.12);
    // The eye sits below the top of the capsule, never above it.
    assert!(UNITS.eye_offset < UNITS.player_crouch_height);
}

#[test]
fn the_quality_presets_match_the_source() {
    assert_eq!(LOW.render_scale.get(), 0.72);
    assert_eq!(LOW.shadow_map_size, 1024);
    assert_eq!(LOW.cascades, 3);
    assert_eq!(LOW.shadow_distance.get(), 60.0);
    assert!(!LOW.taa && !LOW.gtao && !LOW.ssr && !LOW.volumetrics && !LOW.motion_blur);
    assert!(LOW.bloom);
    assert_eq!((LOW.anisotropy, LOW.particle_budget, LOW.decal_budget), (4, 2000, 64));

    assert_eq!(MEDIUM.render_scale.get(), 0.85);
    assert!(MEDIUM.taa && MEDIUM.gtao && !MEDIUM.ssr && MEDIUM.volumetrics);
    assert_eq!((MEDIUM.shadow_map_size, MEDIUM.cascades), (2048, 3));
    assert_eq!((MEDIUM.anisotropy, MEDIUM.particle_budget, MEDIUM.decal_budget), (8, 6000, 128));

    assert_eq!((HIGH.shadow_map_size, HIGH.cascades), (2048, 4));
    assert_eq!(HIGH.shadow_distance.get(), 140.0);
    assert!(HIGH.ssr);
    assert_eq!((HIGH.anisotropy, HIGH.particle_budget, HIGH.decal_budget), (16, 12000, 256));

    assert_eq!((ULTRA.shadow_map_size, ULTRA.cascades), (4096, 4));
    assert_eq!(ULTRA.shadow_distance.get(), 200.0);
    assert_eq!(ULTRA.render_scale.get(), 1.0);
    assert_eq!((ULTRA.anisotropy, ULTRA.particle_budget, ULTRA.decal_budget), (16, 24000, 512));

    // Every preset is reachable by the name the source keys it under.
    for quality in Quality::ALL {
        assert_eq!(Quality::from_name(quality.name()).unwrap(), quality);
        assert_eq!(quality.preset(), quality.preset());
    }
    assert_eq!(
        Quality::from_name("uhltra").unwrap_err().message(),
        "unknown quality preset \"uhltra\""
    );
}

#[test]
fn the_defaults_match_the_source() {
    let cfg = Config::default();
    assert_eq!(cfg.quality, Quality::Ultra);
    assert_eq!(cfg.fov, 80.0);
    assert_eq!(cfg.ads_fov_scale, 0.72);
    assert_eq!(cfg.sensitivity, 0.0022);
    assert_eq!(cfg.ads_sens_scale, 0.65);
    assert!(!cfg.invert_y);
    assert_eq!(cfg.exposure, 1.0);
    assert!(!cfg.deterministic);
    assert_eq!(cfg.q, ULTRA);
}

#[test]
fn set_quality_replaces_the_live_preset_copy() {
    let mut cfg = Config::with_quality(Quality::Low);
    assert_eq!(cfg.q, LOW);

    // The quality scaler nudges one knob on the live copy …
    cfg.q.render_scale = axiom_kernel::Ratio::finite_or_zero(0.5);
    assert_ne!(cfg.q, LOW);
    assert_eq!(LOW.render_scale.get(), 0.72, "the preset table is untouched");

    // … and switching preset restores it wholesale.
    cfg.set_quality(Quality::High);
    assert_eq!(cfg.quality, Quality::High);
    assert_eq!(cfg.q, HIGH);
}

#[test]
fn the_engine_clock_starts_at_the_source_defaults() {
    let engine = Engine::new(Config::default(), CAPTURE_SEED);
    let t: Time = engine.time();
    assert_eq!(t.frame, 0);
    assert_eq!(t.scale, 1.0);
    assert_eq!(t.fixed, FIXED_DT);
    assert_eq!((t.elapsed, t.raw, t.dt, t.alpha), (0.0, 0.0, 0.0, 0.0));
    assert!(engine.registry().is_empty());
}
