//! The agent-facing introspection facade.

use axiom_ecs::World;
use axiom_frame::EngineFrame;

use crate::frame_diff::FrameDiff;
use crate::frame_history::FrameHistory;
use crate::frame_report::FrameReport;
use crate::system_report::SystemReport;
use crate::world_report::WorldReport;
use crate::world_tag::WorldTag;

/// The query surface an agent uses to interrogate a running engine.
/// An owner feeds each completed [`EngineFrame`] to [`Self::observe`] (and,
/// optionally, the ECS [`World`] to [`Self::observe_world`]); the facade
/// projects frames into [`FrameReport`]s retained in a bounded [`FrameHistory`]
/// and keeps the latest [`WorldReport`]. Everything else is a read: describe a
/// frame by index, fetch the recent window, diff two frames, list recent
/// failures, or hand out a serialized snapshot of the latest frame / the whole
/// window / the world. The facade holds no engine state of its own and never
/// reads a clock — its answers are a pure function of what it has observed.
#[derive(Debug)]
pub struct IntrospectApi {
    history: FrameHistory,
    latest_world: Option<WorldReport>,
    tags: Vec<WorldTag>,
}

impl IntrospectApi {
    /// Create a facade retaining at most `capacity` recent frames.
    pub fn new(capacity: usize) -> Self {
        IntrospectApi {
            history: FrameHistory::new(capacity),
            latest_world: None,
            tags: Vec::new(),
        }
    }

    /// Record one completed engine frame.
    pub fn observe(&mut self, frame: &EngineFrame) {
        self.history.record(FrameReport::from_frame(frame));
    }

    /// Observe the ECS world, capturing its latest [`WorldReport`] (entity and
    /// system counts). The same facade now answers both "what did the engine
    /// just do" (frames) and "how big is the world right now" (world).
    pub fn observe_world<S>(&mut self, world: &World<S>) {
        self.latest_world = Some(WorldReport::observe(world));
    }

    /// The recorded report for the given engine frame index, if still retained.
    pub fn describe_frame(&self, engine_frame_index: u64) -> Option<&FrameReport> {
        self.history.describe(engine_frame_index)
    }

    /// The most recent `n` reports, in arrival order.
    pub fn recent(&self, n: usize) -> &[FrameReport] {
        self.history.recent(n)
    }

    /// The most recently observed report, if any.
    pub fn latest(&self) -> Option<&FrameReport> {
        self.history.recent(1).last()
    }

    /// How many frames are currently retained.
    pub fn frame_count(&self) -> usize {
        self.history.len()
    }

    /// The latest observed [`WorldReport`], if [`Self::observe_world`] has been
    /// called.
    pub fn latest_world(&self) -> Option<WorldReport> {
        self.latest_world
    }

    /// The structured delta between two retained frames, `from` → `to`.
    /// `None` if either index is no longer retained.
    pub fn diff(&self, from: u64, to: u64) -> Option<FrameDiff> {
        self.describe_frame(from)
            .zip(self.describe_frame(to))
            .map(|(a, b)| FrameDiff::between(a, b))
    }

    /// Every system that failed across the most recent `n` frames, each paired
    /// with the engine frame index it failed on, in frame-then-system order.
    pub fn failures(&self, n: usize) -> Vec<(u64, &SystemReport)> {
        self.recent(n)
            .iter()
            .flat_map(|report| {
                report
                    .systems()
                    .iter()
                    .filter(|system| !system.succeeded())
                    .map(move |system| (report.engine_frame_index(), system))
            })
            .collect()
    }

    /// A serialized snapshot of the **most recent frame** — the bytes an external
    /// agent reads for one frame. `None` until at least one frame has been
    /// observed. See [`Self::history_snapshot_bytes`] for the whole window and
    /// [`Self::world_snapshot_bytes`] for the world.
    pub fn snapshot_bytes(&self) -> Option<Vec<u8>> {
        self.latest().map(FrameReport::to_bytes)
    }

    /// A serialized snapshot of the **whole retained window** — every recent
    /// frame at once. Always returns bytes (an empty history is a valid empty
    /// payload).
    pub fn history_snapshot_bytes(&self) -> Vec<u8> {
        self.history.to_bytes()
    }

    /// A serialized snapshot of the latest **world** observation. `None` until
    /// [`Self::observe_world`] has been called.
    pub fn world_snapshot_bytes(&self) -> Option<Vec<u8>> {
        self.latest_world.map(|world| world.to_bytes())
    }

    /// Observe the world's semantic **tags** (its named points of interest),
    /// replacing the retained set. These are the nouns an agent resolves a
    /// high-level command against — "where is `mountaintop`". The facade now
    /// answers "what did the engine just do" (frames), "how big is the world"
    /// (world), and "what is *in* the world and what is it called" (tags).
    pub fn observe_tags(&mut self, tags: &[WorldTag]) {
        self.tags = tags.to_vec();
    }

    /// Every retained tag, in observation order.
    pub fn tags(&self) -> &[WorldTag] {
        &self.tags
    }

    /// The tag with the given name, if one is retained (first match).
    pub fn tag_by_name(&self, name: &str) -> Option<&WorldTag> {
        self.tags.iter().find(|tag| tag.name() == name)
    }

    /// Every retained tag of the given coarse kind, in observation order.
    pub fn tags_of_kind(&self, kind_code: u16) -> Vec<&WorldTag> {
        self.tags
            .iter()
            .filter(|tag| tag.kind_code() == kind_code)
            .collect()
    }

    /// The retained tag nearest the point `(x, y, z)` (squared Euclidean
    /// distance in micro-units), or `None` if no tags are retained.
    pub fn nearest_tag(&self, x: i64, y: i64, z: i64) -> Option<&WorldTag> {
        self.tags.iter().min_by_key(|tag| {
            let dx = i128::from(tag.x() - x);
            let dy = i128::from(tag.y() - y);
            let dz = i128::from(tag.z() - z);
            dx * dx + dy * dy + dz * dz
        })
    }

    /// A serialized snapshot of the retained **tag set** — the world's nouns an
    /// external agent reads over the same byte channel as the frame and world
    /// snapshots. Always returns bytes (an empty set is a valid empty payload).
    pub fn tags_snapshot_bytes(&self) -> Vec<u8> {
        WorldTag::encode_set(&self.tags)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures;

    #[test]
    fn fresh_facade_is_empty() {
        let api = IntrospectApi::new(4);
        assert_eq!(api.frame_count(), 0);
        assert!(api.latest().is_none());
        assert!(api.snapshot_bytes().is_none());
        assert_eq!(api.recent(3).len(), 0);
        assert!(api.describe_frame(0).is_none());
    }

    #[test]
    fn observing_records_and_answers_queries() {
        let mut api = IntrospectApi::new(8);
        let frames = fixtures::active_engine_frames(3);
        for frame in &frames {
            api.observe(frame);
        }
        assert_eq!(api.frame_count(), 3);

        let indices: Vec<u64> = api
            .recent(3)
            .iter()
            .map(FrameReport::engine_frame_index)
            .collect();
        assert!(indices.windows(2).all(|w| w[0] < w[1]));

        let known = frames[1].engine_frame_index();
        assert_eq!(
            api.describe_frame(known).unwrap().engine_frame_index(),
            known
        );
        assert!(api.describe_frame(1_000_000).is_none());

        let last = frames[2].engine_frame_index();
        assert_eq!(api.latest().unwrap().engine_frame_index(), last);
    }

    #[test]
    fn snapshot_bytes_round_trip_to_the_latest_report() {
        let mut api = IntrospectApi::new(8);
        api.observe(&fixtures::failing_engine_frame());
        let bytes = api.snapshot_bytes().expect("a frame was observed");
        let decoded = FrameReport::from_bytes(&bytes).unwrap();
        assert_eq!(&decoded, api.latest().unwrap());
        assert_eq!(decoded.systems().len(), 1);
    }

    #[test]
    fn observation_sequence_is_deterministic() {
        let build = || {
            let mut api = IntrospectApi::new(8);
            for frame in &fixtures::active_engine_frames(2) {
                api.observe(frame);
            }
            api.recent(2).to_vec()
        };
        assert_eq!(build(), build());
    }

    #[derive(Default)]
    struct Storage;

    struct Noop;
    impl axiom_ecs::WorldSystem<Storage> for Noop {
        fn run(&self, _: &axiom_ecs::WorldStep, _: &axiom_ecs::EntityRegistry, _: &mut Storage) {}
    }

    #[test]
    fn world_observation_flows_through_the_facade() {
        let mut api = IntrospectApi::new(4);
        assert!(api.latest_world().is_none());
        assert!(api.world_snapshot_bytes().is_none());

        let mut world: World<Storage> = World::new();
        world.register_system(Box::new(Noop));
        world.spawn();
        world.spawn();
        let frame = &fixtures::active_engine_frames(1)[0];
        world.advance(0, &axiom_frame::FrameContext::new(frame));
        api.observe_world(&world);

        let report = api.latest_world().expect("a world was observed");
        assert_eq!(report.entities(), 2);
        assert_eq!(report.systems(), 1);

        let bytes = api.world_snapshot_bytes().expect("world observed");
        assert_eq!(WorldReport::from_bytes(&bytes).unwrap(), report);
    }

    #[test]
    fn tags_flow_through_the_facade_and_answer_queries() {
        let mut api = IntrospectApi::new(4);
        assert!(api.tags().is_empty());
        assert!(api.tag_by_name("mountaintop").is_none());
        assert!(api.nearest_tag(0, 0, 0).is_none());
        assert!(api.tags_of_kind(7).is_empty());

        let summit = WorldTag::new(1, "mountaintop".to_string(), 7, 100, 8_000, 100);
        let ground = WorldTag::new(2, "ground".to_string(), 3, 10, 0, 10);
        let camp = WorldTag::new(3, "basecamp".to_string(), 7, 50, 2_000, 50);
        api.observe_tags(&[summit.clone(), ground.clone(), camp.clone()]);

        assert_eq!(api.tags().len(), 3);
        assert_eq!(api.tag_by_name("ground"), Some(&ground));
        assert!(api.tag_by_name("nope").is_none());
        let summits = api.tags_of_kind(7);
        assert_eq!(summits, vec![&summit, &camp]);
        assert_eq!(api.nearest_tag(11, 1, 11), Some(&ground));

        let decoded = WorldTag::decode_set(&api.tags_snapshot_bytes()).unwrap();
        assert_eq!(decoded, vec![summit, ground, camp]);

        api.observe_tags(&[]);
        assert!(api.tags().is_empty());
    }

    #[test]
    fn diff_compares_two_retained_frames() {
        let mut api = IntrospectApi::new(8);
        let frames = fixtures::active_engine_frames(2);
        for frame in &frames {
            api.observe(frame);
        }
        let (a, b) = (
            frames[0].engine_frame_index(),
            frames[1].engine_frame_index(),
        );
        let diff = api.diff(a, b).expect("both frames retained");
        assert_eq!(diff.from_index(), a);
        assert_eq!(diff.to_index(), b);
        assert!(api.diff(a, 9_999).is_none());
        assert!(api.diff(9_999, b).is_none());
    }

    #[test]
    fn failures_lists_failed_systems_across_the_window() {
        let mut api = IntrospectApi::new(8);
        api.observe(&fixtures::active_engine_frames(1)[0]);
        api.observe(&fixtures::failing_engine_frame());
        let failures = api.failures(8);
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].1.name(), "fail");
        assert!(api.failures(0).is_empty());
    }

    #[test]
    fn history_snapshot_bytes_round_trip_to_the_window() {
        use crate::FrameHistory;
        let mut api = IntrospectApi::new(8);
        for frame in &fixtures::active_engine_frames(3) {
            api.observe(frame);
        }
        let decoded = FrameHistory::from_bytes(&api.history_snapshot_bytes()).unwrap();
        assert_eq!(decoded.len(), 3);
        assert_eq!(decoded.recent(3), api.recent(3));
    }
}
