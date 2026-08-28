//! **The dev console — the route from a pixel back to a symbol.**
//!
//! An agent working on this game reads screenshots. A screenshot says "the flat
//! white thing beside the sandbags"; the codebase says `crate_c`,
//! `barrel_rust`, `plaster_cream`. Nothing connected the two, so every visual
//! defect began with somebody describing a shape in prose and somebody else
//! guessing which symbol it was. That guessing is where this port has lost the
//! most time, and it is what this removes: turn the overlay on, take a
//! screenshot, read the name off the thing.
//!
//! ## What it names
//!
//! [`axiom_introspect::WorldTag`] — the engine's own semantic noun: a stable
//! name, a coarse kind, a world position. It already existed and nothing used
//! it. The name a tag carries here is the **palette key**, which is the string
//! the material lookups key off (see `scene::install`), so a label on screen is
//! literally the identifier to search for: `ax q crate_c` lands somewhere
//! useful, and `ax refs plaster_cream` finds the material that drew it.
//!
//! ## Why a console and not a debug flag
//!
//! A flag is set before the build, so an agent that notices something
//! mid-session has to rebuild to look at it — three minutes here, and a rebuild
//! is exactly when a wasm-only break slips past. A console is a function call:
//! `window.__ax_console("ids on")` from a Playwright `eval`, then screenshot. No
//! rebuild, no source edit, and the same command works by hand in devtools.
//!
//! The command surface is deliberately tiny and text-in/text-out. Every reply is
//! a string an agent can read, and unknown input answers with the list of what
//! it does know rather than failing silently.
//!
//! ## Its second job: the parity instrument's grip on the game
//!
//! `scripts/parity_shot.py` photographs this port and the original browser FPS
//! (`apps/shmup`) under matched conditions and reports how far apart they are.
//! That number is worth nothing unless the two runs differ in **exactly one
//! thing**, and three axes had to be reachable from outside the binary before
//! that could be true:
//!
//! * `cam <x> <y> <z> <yaw> <pitch> [fov]` — stand the camera where the
//!   original's `shots.js` stands it. Without this the port photographs
//!   wherever the player happens to be.
//! * `freeze on` — stop live input fighting the scripted pose. `Input::frozen`
//!   was ported faithfully and then had no writer outside its own tests, so
//!   capture mode existed in the type and could not be entered.
//! * `dt <seconds>` — advance every accumulator, spring and particle by a fixed
//!   amount, so two runs of the same shot resolve identically.
//!
//! And one that makes a whole class of defect visible without a pixel at all:
//!
//! * `stats` — the level census, the uploaded geometry and the last frame's
//!   draw list. A port that generates a **different town** and a port that
//!   *lights the right town wrong* produce the same verdict from a screenshot
//!   ("the images differ") and completely different numbers here. The level
//!   fingerprint moves the moment a placement does.
//!
//! Every one of these reports whether the pin is **in force**, not merely
//! requested — `applied=yes`, `dt_used=…`, `UNOBSERVED`. That distinction is
//! the whole point: the frame loop that has to call [`DevConsole::resolve_camera`]
//! and [`DevConsole::frame_dt`] lives in `scene::boot`, which is `wasm32`-only
//! and therefore untested, so "the hook exists" and "the hook is wired" are
//! genuinely different facts and a harness must be able to tell them apart.

use std::collections::{BTreeMap, BTreeSet};

use axiom_introspect::WorldTag;

use crate::player::camera::Euler;
use crate::scene::game::CameraPose;

/// Micro-units per world unit — the fixed-point convention [`WorldTag`] stores
/// positions in.
const MICRO: f64 = 1_000_000.0;

/// Floats per vertex in the engine's static upload stream — position(3) +
/// normal(3) + uv(2) + colour(4). `RunningApp::mesh_set`'s own doc names it.
const MESH_VERTEX_FLOATS: usize = 12;

/// FNV-1a 64, the level fingerprint's mixer. Chosen because it is four lines
/// and because a fingerprint an agent cannot re-derive by hand is a fingerprint
/// nobody trusts.
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv(bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .fold(FNV_OFFSET, |h, b| (h ^ u64::from(*b)).wrapping_mul(FNV_PRIME))
}

/// One tag's contribution to the level fingerprint: its name and its position
/// **quantized to millimetres**.
///
/// Millimetres, not micrometres, on purpose. The two builds being compared are
/// the same Rust code compiled twice, so the quantum is not there to absorb
/// float noise between renderers — it is there so a fingerprint stays stable
/// across a harmless last-bit change in a trig call while still moving the
/// instant a prop lands somewhere else. A seed divergence relocates props by
/// metres, not by microns.
fn tag_hash(tag: &WorldTag) -> u64 {
    let mm = |v: i64| v / 1_000;
    let mut bytes = tag.name().as_bytes().to_vec();
    bytes.extend_from_slice(&mm(tag.x()).to_le_bytes());
    bytes.extend_from_slice(&mm(tag.y()).to_le_bytes());
    bytes.extend_from_slice(&mm(tag.z()).to_le_bytes());
    bytes.extend_from_slice(&tag.kind_code().to_le_bytes());
    fnv(&bytes)
}

/// Kind codes, so a filter can ask for one class of thing.
pub const KIND_STATIC: u16 = 1;
pub const KIND_PROP: u16 = 2;

/// The screen cell a label claims, in pixels — roughly one label's own ink, so
/// two kept labels never overlap.
const CELL_W: f64 = 104.0;
const CELL_H: f64 = 22.0;

/// A hard ceiling on labels, independent of the cell grid. A very wide view
/// still has a few hundred cells, and past ~60 names the overlay stops being
/// something you read and becomes something you decode.
const MAX_LABELS: usize = 60;

/// One label to draw: the name, and where it landed in pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct Label {
    pub name: String,
    pub x: f64,
    pub y: f64,
    /// Distance from the camera in world units, so a caller can fade or cull
    /// the far ones rather than painting a wall of text.
    pub depth: f64,
}

/// **A scripted camera** — what `cam` installs, and what the frame loop uses
/// instead of the pose the game resolved.
///
/// Angles are **radians**, the port's native unit ([`Euler`] carries radians and
/// `write_camera` feeds them straight to `Quat::from_axis_angle`). A harness
/// derives them from an eye/target pair; asking it to convert to degrees on the
/// way in and back on the way out is two chances to be wrong about a sign.
///
/// `roll` is not settable. A scripted parity shot is a `lookAt`, and `lookAt`
/// has no roll — offering one would only ever be used by accident.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraOverride {
    pub eye: [f64; 3],
    pub yaw: f64,
    pub pitch: f64,
    /// `None` keeps whatever vertical FOV the game's own rig resolved, so `cam`
    /// can pin the framing without also pinning the ADS/sprint FOV channel.
    pub fov_degrees: Option<f64>,
}

/// What one rendered frame actually contained. Written by
/// [`DevConsole::observe_draws`], read by `stats`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct FrameCensus {
    tick: u64,
    instances: usize,
    batches: usize,
    tris: u64,
    skinned: usize,
    lights: usize,
    clear: [f32; 4],
}

/// The uploaded static geometry inventory. Written once at bind by
/// [`DevConsole::observe_meshes`]; the per-mesh triangle counts are what turn a
/// frame's draw list into a triangle number.
#[derive(Debug, Clone, Default)]
struct MeshCensus {
    tris: BTreeMap<u64, u64>,
    total_tris: u64,
    total_verts: u64,
}

impl MeshCensus {
    const fn new() -> Self {
        MeshCensus {
            tris: BTreeMap::new(),
            total_tris: 0,
            total_verts: 0,
        }
    }
}

/// The console: the tag set, and whether the overlay is on.
#[derive(Debug)]
pub struct DevConsole {
    tags: Vec<WorldTag>,
    show_ids: bool,
    /// A line of live input state, rewritten by the frame loop.
    ///
    /// The console's second job. Pointer-lock state is the worst kind of
    /// invisible: when a lock is refused the game keeps responding to every
    /// button, so it looks healthy and simply will not turn. Something has to be
    /// able to *say* what the game thinks is true, and this is the surface that
    /// already reaches an agent and a human alike.
    status: String,
    /// Only tags within this many world units are labelled. The street holds
    /// several hundred props, and labelling all of them paints an unreadable
    /// screen — the failure mode of every debug overlay ever written.
    radius: f64,

    /* ==================================================================== */
    /* the capture harness's three pins                                     */
    /* ==================================================================== */
    //
    // A parity screenshot is only evidence if the two runs differ in exactly
    // one thing. Three axes have to be nailed down before the pixels mean
    // anything, and none of them could be reached from outside the binary:
    // where the camera is, whether live input is fighting it, and how long a
    // frame is. Each is one field here plus one getter the frame loop reads —
    // deliberately, because the frame loop lives in `scene::boot`, which is
    // `wasm32`-only and therefore untestable, and every line put there is a
    // line no test will ever run.
    /// `cam` — the scripted pose, or `None` for the game's own rig.
    camera: Option<CameraOverride>,
    /// The pose the last frame actually rendered with, override applied. Kept
    /// so `stats` can report the *effective* camera rather than the requested
    /// one: "I asked for this pose" and "the frame used this pose" are
    /// different claims, and only the second one is evidence.
    last_pose: Option<CameraPose>,
    /// `freeze` — drives `Input::frozen`, which zeroes the look delta.
    frozen: bool,
    /// `dt` — a fixed frame delta in seconds, or `None` for the wall clock.
    dt: Option<f64>,
    /// The delta the last frame **actually advanced by**, from
    /// [`Self::frame_dt`]. The same distinction `last_pose` draws for the
    /// camera: a `dt` sitting in the field above is a request, and only a value
    /// here is a pin in force.
    last_dt: Option<f64>,

    /// The uploaded geometry, from `observe_meshes`.
    meshes: MeshCensus,
    /// The last frame's contents, from `observe_draws`.
    frame: Option<FrameCensus>,
    /// How many frames have been observed. This doubles as the port's
    /// **readiness signal**: `__ax_console` is installed before the GPU binds,
    /// so its existence proves nothing, but a non-zero frame count means the
    /// engine has completed a frame.
    frames_observed: u64,
}

impl Default for DevConsole {
    fn default() -> Self {
        DevConsole::new()
    }
}

impl DevConsole {
    /// A console with no tags and the overlay off — what a normal run carries.
    pub const fn new() -> Self {
        DevConsole {
            tags: Vec::new(),
            show_ids: false,
            status: String::new(),
            radius: 40.0,
            camera: None,
            last_pose: None,
            frozen: false,
            dt: None,
            last_dt: None,
            meshes: MeshCensus::new(),
            frame: None,
            frames_observed: 0,
        }
    }

    /// Record a tagged point. Called once per installed batch.
    pub fn tag(&mut self, name: &str, kind: u16, position: [f64; 3]) {
        let id = self.tags.len() as u32;
        self.tags.push(WorldTag::new(
            id,
            name.to_owned(),
            kind,
            (position[0] * MICRO) as i64,
            (position[1] * MICRO) as i64,
            (position[2] * MICRO) as i64,
        ));
    }

    /// Every tag the console holds.
    pub fn tags(&self) -> &[WorldTag] {
        &self.tags
    }

    /// Report the live input state. Called once a frame; the console keeps
    /// only the latest.
    pub fn set_status(&mut self, status: String) {
        self.status = status;
    }

    /// Whether the id overlay is on.
    pub const fn show_ids(&self) -> bool {
        self.show_ids
    }

    /* ==================================================================== */
    /* what the frame loop reads and writes                                 */
    /* ==================================================================== */

    /// Whether live input should be frozen — `freeze on`.
    ///
    /// Assigned onto `Input::frozen`, which zeroes the frame's look delta and
    /// refuses the pointer lock. The port ported that field faithfully and then
    /// gave it no writer outside its own tests, so capture mode existed in the
    /// type and could not be entered.
    pub const fn frozen(&self) -> bool {
        self.frozen
    }

    /// The fixed frame delta in seconds `dt <s>` installed, or `None` for the
    /// wall clock.
    ///
    /// This is the whole app-tier half of a deterministic clock. `dt` is the
    /// only wall-clock read in the browser frame path that the app owns; the
    /// rAF cadence itself belongs to `axiom_windowing::run_web_multi_skinned`
    /// and is not reachable from here. Pinning this makes every accumulator,
    /// spring and particle age advance by the same amount on every run and on
    /// every machine — it does **not** make the frame *count* at the shutter a
    /// constant, which is the other half and needs a lockstep pump the engine
    /// does not offer.
    pub const fn dt_override(&self) -> Option<f64> {
        self.dt
    }

    /// **The delta the frame should advance by**, given the wall-clock one the
    /// loop measured — and a record of what it actually used.
    ///
    /// The recording is the whole reason this exists rather than the frame loop
    /// reading [`Self::dt_override`] itself. A `dt` can be *requested* from the
    /// console and silently never read, and from outside the binary that is
    /// indistinguishable from a pinned clock. Routing the frame's delta through
    /// here makes "the clock is pinned" an observable fact — `stats` reports
    /// `dt_used` — instead of a hope, and a harness that cannot verify its own
    /// pins is producing numbers whose provenance is unknown.
    pub fn frame_dt(&mut self, wall_clock: f64) -> f64 {
        let dt = self.dt.unwrap_or(wall_clock);
        self.last_dt = Some(dt);
        dt
    }

    /// **Apply the scripted camera** to the pose the game resolved, and record
    /// what the frame will actually render with.
    ///
    /// Called between `Game::frame` and `write_camera`. With no override
    /// installed this is the identity plus one recorded pose, which is exactly
    /// what makes `stats` able to answer "where is the camera" on an ordinary
    /// run as well as a scripted one.
    pub fn resolve_camera(&mut self, pose: CameraPose) -> CameraPose {
        let resolved = self.camera.map_or(pose, |c| CameraPose {
            eye: c.eye,
            rotation: Euler {
                pitch: c.pitch,
                yaw: c.yaw,
                roll: 0.0,
            },
            fov_degrees: c.fov_degrees.unwrap_or(pose.fov_degrees),
        });
        self.last_pose = Some(resolved);
        resolved
    }

    /// Record the static geometry the backend uploaded — `(mesh id, interleaved
    /// vertices, indices)`, the shape of `RunningApp::mesh_set`.
    ///
    /// Called once, at bind. The per-mesh triangle counts are the table
    /// [`Self::observe_draws`] needs to turn a frame's draw list into a
    /// triangle number, which is the only reason this is kept rather than
    /// summed and dropped.
    pub fn observe_meshes(&mut self, meshes: &[(u64, Vec<f32>, Vec<u32>)]) {
        self.meshes = MeshCensus::new();
        meshes.iter().for_each(|(id, vertices, indices)| {
            let tris = (indices.len() / 3) as u64;
            self.meshes.tris.insert(*id, tris);
            self.meshes.total_tris += tris;
            self.meshes.total_verts += (vertices.len() / MESH_VERTEX_FLOATS) as u64;
        });
    }

    /// Record one frame's draw list: `(mesh id, material id)` per instance.
    ///
    /// Takes the ids rather than the engine's `FrameOutcome` so it is reachable
    /// from a test — `FrameOutcome` cannot be constructed outside its own
    /// crate, and a census that only the browser can exercise is a census
    /// nothing checks. [`Self::observe_frame`] is the six-line adapter that
    /// feeds this from a real frame.
    pub fn observe_draws<I: IntoIterator<Item = (u64, u64)>>(
        &mut self,
        tick: u64,
        draws: I,
        skinned: usize,
        lights: usize,
        clear: [f32; 4],
    ) {
        let mut batches: BTreeSet<(u64, u64)> = BTreeSet::new();
        let mut instances = 0usize;
        let mut tris = 0u64;
        draws.into_iter().for_each(|(mesh, material)| {
            instances += 1;
            tris += self.meshes.tris.get(&mesh).copied().unwrap_or_default();
            batches.insert((mesh, material));
        });
        self.frame = Some(FrameCensus {
            tick,
            instances,
            batches: batches.len(),
            tris,
            skinned,
            lights,
            clear,
        });
        self.frames_observed += 1;
    }

    /// [`Self::observe_draws`] fed from the engine's own frame result. The one
    /// line the browser loop calls; every decision it makes is in the function
    /// above, where a test can reach it.
    pub fn observe_frame(&mut self, outcome: &axiom::prelude::FrameOutcome) {
        self.observe_draws(
            outcome.tick(),
            outcome
                .draws()
                .iter()
                .map(|d| (d.mesh_id(), d.material_id())),
            outcome.skinned_draws().len(),
            outcome.lights().len(),
            outcome.clear_color(),
        );
    }

    /// How many frames the engine has completed — **the readiness signal**.
    ///
    /// `window.__ax_console` is installed before the GPU binds, so a harness
    /// that waits for the global to exist is waiting for nothing. A non-zero
    /// count here is the first fact that means "a frame was rendered".
    pub const fn frames_observed(&self) -> u64 {
        self.frames_observed
    }

    /* ==================================================================== */
    /* `stats` — the town, compared without a pixel                         */
    /* ==================================================================== */

    /// **A numeric census of the level and the last frame.**
    ///
    /// This is the command that makes a parity claim falsifiable without a
    /// screenshot. A level-seed divergence — the port generating a *different
    /// town* from the original — shows up here as a different placement count
    /// and a different fingerprint, in one line, before anybody has argued
    /// about whether a wall looks too grey. The pixel comparison cannot make
    /// that distinction at all: two different towns and one badly-exposed town
    /// both come back as "the images differ".
    ///
    /// Every line is `key=value` pairs after a leading section word, so a
    /// harness parses it with a regex and a human reads it as prose.
    ///
    /// Sections that nothing has reported into say **UNOBSERVED** rather than
    /// zero. Zero is a measurement; UNOBSERVED is the absence of one, and a
    /// harness that cannot tell them apart will report a confident zero for a
    /// hook that was never wired.
    fn stats_report(&self) -> String {
        [self.level_line(), self.mesh_line(), self.frame_line(), self.pin_line()].join("\n")
    }

    /// The level census: how many placements, of what, where, and a fingerprint
    /// over the whole multiset.
    ///
    /// The fingerprint is **order-independent** (per-tag hashes summed). Install
    /// order is an artefact of how the assembler happens to iterate its
    /// prototypes; the *set of things placed in the world* is the fact worth
    /// pinning. A commutative combine moves when a prop appears, disappears, is
    /// renamed or is relocated by a millimetre, and stays put when the same
    /// world is merely built in a different order.
    fn level_line(&self) -> String {
        let mut names: BTreeSet<&str> = BTreeSet::new();
        let (mut statics, mut props) = (0usize, 0usize);
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        let mut fingerprint = 0u64;
        self.tags.iter().for_each(|tag| {
            names.insert(tag.name());
            statics += usize::from(tag.kind_code() == KIND_STATIC);
            props += usize::from(tag.kind_code() == KIND_PROP);
            let at = [
                tag.x() as f64 / MICRO,
                tag.y() as f64 / MICRO,
                tag.z() as f64 / MICRO,
            ];
            (0..3).for_each(|i| {
                lo[i] = lo[i].min(at[i]);
                hi[i] = hi[i].max(at[i]);
            });
            fingerprint = fingerprint.wrapping_add(tag_hash(tag));
        });
        let bounds = self.tags.is_empty().then(|| "min=- max=-".to_owned()).unwrap_or_else(|| {
            format!(
                "min={:.2},{:.2},{:.2} max={:.2},{:.2},{:.2}",
                lo[0], lo[1], lo[2], hi[0], hi[1], hi[2]
            )
        });
        format!(
            "level placements={} names={} static={} props={} fingerprint={fingerprint:#018x} {bounds}",
            self.tags.len(),
            names.len(),
            statics,
            props
        )
    }

    /// The uploaded geometry inventory — the *static* triangle budget, as
    /// distinct from the per-frame drawn one below.
    fn mesh_line(&self) -> String {
        self.meshes
            .tris
            .is_empty()
            .then(|| "meshes UNOBSERVED (boot never called observe_meshes)".to_owned())
            .unwrap_or_else(|| {
                format!(
                    "meshes count={} tris={} verts={}",
                    self.meshes.tris.len(),
                    self.meshes.total_tris,
                    self.meshes.total_verts
                )
            })
    }

    /// The last frame's contents — draw calls, instances, triangles actually
    /// submitted, skinned draws, lights, clear colour.
    fn frame_line(&self) -> String {
        self.frame.map_or_else(
            || "frame UNOBSERVED (boot never called observe_frame)".to_owned(),
            |f| {
                format!(
                    "frame tick={} observed={} draws={} instances={} tris={} skinned={} \
                     lights={} clear={:.4},{:.4},{:.4},{:.4}",
                    f.tick,
                    self.frames_observed,
                    f.batches,
                    f.instances,
                    f.tris,
                    f.skinned,
                    f.lights,
                    f.clear[0],
                    f.clear[1],
                    f.clear[2],
                    f.clear[3]
                )
            },
        )
    }

    /// The three pins, and — crucially — whether each is actually **in force**.
    ///
    /// `camera=override` says a pose was requested. `applied=yes` says a frame
    /// went through [`Self::resolve_camera`] carrying it. A harness that reads
    /// only the first will happily print a parity score for a run whose camera
    /// hook was never wired into the frame loop, which is worse than printing
    /// no score at all.
    fn pin_line(&self) -> String {
        let applied = ["no", "yes"][usize::from(self.last_pose.is_some())];
        let camera = self.last_pose.map_or_else(
            || "camera=UNPINNED applied=no".to_owned(),
            |p| {
                format!(
                    "camera={} applied={applied} eye={:.4},{:.4},{:.4} yaw={:.6} pitch={:.6} fov={:.3}",
                    ["rig", "override"][usize::from(self.camera.is_some())],
                    p.eye[0],
                    p.eye[1],
                    p.eye[2],
                    p.rotation.yaw,
                    p.rotation.pitch,
                    p.fov_degrees
                )
            },
        );
        format!(
            "pins {camera} frozen={} dt={} dt_used={}",
            ["off", "on"][usize::from(self.frozen)],
            self.dt
                .map_or_else(|| "wallclock".to_owned(), |d| format!("{d:.6}")),
            self.last_dt
                .map_or_else(|| "UNOBSERVED".to_owned(), |d| format!("{d:.6}"))
        )
    }

    /// `cam <x> <y> <z> <yaw> <pitch> [fov]` — install a scripted pose.
    fn set_camera(&mut self, arg: &str) -> String {
        let words: Vec<&str> = arg.split_whitespace().collect();
        let numbers: Option<Vec<f64>> = words.iter().map(|w| w.parse::<f64>().ok()).collect();
        match numbers.filter(|n| n.len() == 5 || n.len() == 6) {
            None => format!(
                "cam: expected `cam <x> <y> <z> <yaw> <pitch> [fov]` (angles in radians) \
                 or `cam off`, got {arg:?}"
            ),
            Some(n) => {
                let pose = CameraOverride {
                    eye: [n[0], n[1], n[2]],
                    yaw: n[3],
                    pitch: n[4],
                    fov_degrees: n.get(5).copied(),
                };
                self.camera = Some(pose);
                format!(
                    "cam eye={:.4},{:.4},{:.4} yaw={:.6} pitch={:.6} fov={}",
                    pose.eye[0],
                    pose.eye[1],
                    pose.eye[2],
                    pose.yaw,
                    pose.pitch,
                    pose.fov_degrees
                        .map_or_else(|| "rig".to_owned(), |f| format!("{f:.3}"))
                )
            }
        }
    }

    /// **Run one command.** Text in, text out — the whole agent surface.
    pub fn exec(&mut self, command: &str) -> String {
        let line = command.trim();
        let (verb, rest) = line.split_once(' ').unwrap_or((line, ""));
        let arg = rest.trim();
        match (verb, arg) {
            ("ids", "on") => {
                self.show_ids = true;
                format!("ids on - {} tagged entities", self.tags.len())
            }
            ("ids", "off") => {
                self.show_ids = false;
                "ids off".to_owned()
            }
            ("ids", "") => format!(
                "ids are {} - {} tagged entities, radius {} m",
                ["off", "on"][usize::from(self.show_ids)],
                self.tags.len(),
                self.radius
            ),
            ("radius", value) => value.parse::<f64>().map_or_else(
                |_| format!("radius: expected a number, got {value:?}"),
                |r| {
                    self.radius = r;
                    format!("radius {r} m")
                },
            ),
            ("find", needle) if !needle.is_empty() => {
                let hits: Vec<&str> = self
                    .tags
                    .iter()
                    .filter(|t| t.name().contains(needle))
                    .map(WorldTag::name)
                    .collect::<std::collections::BTreeSet<_>>()
                    .into_iter()
                    .collect();
                match hits.is_empty() {
                    true => format!("find {needle}: nothing"),
                    false => format!("find {needle}: {}", hits.join(", ")),
                }
            }
            ("lock" | "input", "") => match self.status.is_empty() {
                true => "input: the frame loop has not reported yet".to_owned(),
                false => self.status.clone(),
            },
            ("cam", "off") => {
                self.camera = None;
                "cam off - the game's own rig drives the camera".to_owned()
            }
            ("cam", "") => self.pin_line(),
            ("cam", arg) => self.set_camera(arg),
            ("freeze", "on") => {
                self.frozen = true;
                "freeze on - look input is zeroed".to_owned()
            }
            ("freeze", "off") => {
                self.frozen = false;
                "freeze off".to_owned()
            }
            ("freeze", "") => format!("freeze is {}", ["off", "on"][usize::from(self.frozen)]),
            ("dt", "off") => {
                self.dt = None;
                "dt off - frames advance on the wall clock".to_owned()
            }
            ("dt", "") => self
                .dt
                .map_or_else(|| "dt wallclock".to_owned(), |d| format!("dt {d}")),
            ("dt", value) => value
                .parse::<f64>()
                .ok()
                .filter(|d| d.is_finite() && *d > 0.0)
                .map_or_else(
                    || format!("dt: expected a positive number of seconds, got {value:?}"),
                    |d| {
                        self.dt = Some(d);
                        format!("dt {d} s per frame")
                    },
                ),
            ("stats", "") => self.stats_report(),
            ("names", "") => {
                let names: Vec<&str> = self
                    .tags
                    .iter()
                    .map(WorldTag::name)
                    .collect::<std::collections::BTreeSet<_>>()
                    .into_iter()
                    .collect();
                format!("{} distinct: {}", names.len(), names.join(", "))
            }
            _ => concat!(
                "commands:\n",
                "  ids on|off      label every tagged entity on screen\n",
                "  ids             report the overlay state\n",
                "  radius <m>      how far to label (default 40)\n",
                "  find <text>     which tag names contain <text>\n",
                "  names           every distinct tag name in the level\n",
                "  lock            live pointer-lock / input state\n",
                "  stats           level census, geometry and last frame\n",
                "  cam <x> <y> <z> <yaw> <pitch> [fov]   scripted camera (radians)\n",
                "  cam off         hand the camera back to the game's rig\n",
                "  cam             report the camera the last frame used\n",
                "  freeze on|off   zero the look input, so a shot holds still\n",
                "  dt <seconds>    fixed frame delta; `dt off` for the wall clock"
            )
            .to_owned(),
        }
    }

    /// Project every in-range tag through `view_proj` into pixel positions,
    /// **declustered**: at most one label per screen cell, nearest wins.
    ///
    /// Returns nothing at all when the overlay is off, so a caller can call this
    /// unconditionally every frame and pay one branch for it.
    ///
    /// # Why declustering is not optional
    ///
    /// Every *placement* is tagged, not every *name* — pointing at one crate is
    /// the whole point, and a per-name tag could not do that. But a street of
    /// 8,000 placements puts ~750 labels on a 1280×720 view, which is a green
    /// smear: strictly less legible than no overlay, because it also hides the
    /// thing it is naming. Nearest-first-wins per cell is what turns the tag set
    /// back into something a screenshot can be read off, and it is the correct
    /// place for the rule — a caller that had to decluster for itself would be
    /// re-deriving this in every consumer.
    pub fn labels(
        &self,
        view_proj: [f32; 16],
        width: f64,
        height: f64,
        eye: [f64; 3],
    ) -> Vec<Label> {
        self.show_ids
            .then(|| {
                let mut all: Vec<Label> = self
                    .tags
                    .iter()
                    .filter_map(|tag| self.project(tag, view_proj, width, height, eye))
                    .collect();
                // Nearest first, so the label a cell keeps is the thing actually
                // in front. Ties break on the name, so the same view always
                // produces the same overlay — a screenshot an agent compares
                // against another screenshot has to be stable.
                all.sort_by(|a, b| {
                    a.depth
                        .partial_cmp(&b.depth)
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then_with(|| a.name.cmp(&b.name))
                });
                let mut taken = std::collections::BTreeSet::new();
                all.into_iter()
                    .filter(|l| {
                        taken.insert((
                            (l.x / CELL_W) as i64,
                            (l.y / CELL_H) as i64,
                        ))
                    })
                    .take(MAX_LABELS)
                    .collect()
            })
            .unwrap_or_default()
    }
    /// One tag to a pixel position, or `None` when it is behind the camera, off
    /// screen, or beyond the radius.
    fn project(
        &self,
        tag: &WorldTag,
        m: [f32; 16],
        width: f64,
        height: f64,
        eye: [f64; 3],
    ) -> Option<Label> {
        let (x, y, z) = (
            tag.x() as f64 / MICRO,
            tag.y() as f64 / MICRO,
            tag.z() as f64 / MICRO,
        );
        let depth = ((x - eye[0]).powi(2) + (y - eye[1]).powi(2) + (z - eye[2]).powi(2)).sqrt();
        (depth <= self.radius).then_some(())?;
        // Column-major, the convention every matrix in this port uses.
        let f = |r: usize| {
            f64::from(m[r]) * x
                + f64::from(m[4 + r]) * y
                + f64::from(m[8 + r]) * z
                + f64::from(m[12 + r])
        };
        let (cx, cy, cw) = (f(0), f(1), f(3));
        // `w <= 0` is behind the eye; dividing anyway wraps the label round onto
        // the view upside down, which is the classic debug-overlay ghost.
        (cw > 1e-6).then_some(())?;
        let (ndc_x, ndc_y) = (cx / cw, cy / cw);
        let on_screen = (-1.0..=1.0).contains(&ndc_x) && (-1.0..=1.0).contains(&ndc_y);
        on_screen.then(|| Label {
            name: tag.name().to_owned(),
            x: (ndc_x * 0.5 + 0.5) * width,
            y: (1.0 - (ndc_y * 0.5 + 0.5)) * height,
            depth,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Column-major view-projection mapping world `(x, y, z)` to clip
    /// `(x, y, z, 1)` — an orthographic identity, so NDC *is* the world position
    /// and the pixel arithmetic is checkable by hand.
    const IDENTITY: [f32; 16] = [
        1.0, 0.0, 0.0, 0.0, //
        0.0, 1.0, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, //
        0.0, 0.0, 0.0, 1.0,
    ];

    fn console() -> DevConsole {
        let mut c = DevConsole::new();
        c.tag("plaster_cream", KIND_STATIC, [0.0, 0.0, 0.0]);
        c.tag("crate_c", KIND_PROP, [0.5, 0.5, 0.0]);
        // Off-centre on purpose: at the origin it would share a screen cell
        // with `plaster_cream` and be declustered away, which would let the
        // radius test pass for the wrong reason.
        c.tag("barrel_rust", KIND_PROP, [0.5, -0.5, 1000.0]);
        c
    }

    #[test]
    fn the_overlay_is_off_until_a_command_turns_it_on() {
        let mut c = console();
        assert!(!c.show_ids());
        assert!(c.labels(IDENTITY, 1280.0, 720.0, [0.0; 3]).is_empty());
        let reply = c.exec("ids on");
        assert!(c.show_ids(), "`ids on` did not turn the overlay on");
        assert!(reply.contains('3'), "the reply should say how many: {reply}");
    }

    /// **A tag lands where the maths says.** World origin under an identity
    /// view-projection is dead centre, and `y` is flipped because pixels count
    /// down while NDC counts up — the slip that puts every label on the wrong
    /// half of the screen.
    #[test]
    fn a_tag_projects_to_the_pixel_it_should() {
        let mut c = console();
        c.exec("ids on");
        let labels = c.labels(IDENTITY, 1280.0, 720.0, [0.0; 3]);
        let origin = labels
            .iter()
            .find(|l| l.name == "plaster_cream")
            .expect("the origin tag is on screen");
        assert!((origin.x - 640.0).abs() < 1e-9);
        assert!((origin.y - 360.0).abs() < 1e-9);
        let offset = labels
            .iter()
            .find(|l| l.name == "crate_c")
            .expect("on screen");
        assert!(offset.x > origin.x, "+x is right");
        assert!(offset.y < origin.y, "+y is UP the screen, so fewer pixels");
    }

    /// **The overlay declusters, and it is the nearest thing that survives.**
    ///
    /// This is the test the first working build failed: 8,164 tags put 747
    /// labels on the view and the screenshot was a green smear. Two tags in the
    /// same screen cell must collapse to one, and the one kept must be the near
    /// one — a label naming something hidden behind a wall is worse than no
    /// label, because it reads as truth.
    #[test]
    fn two_tags_in_one_screen_cell_collapse_to_the_nearer() {
        let mut c = DevConsole::new();
        // Same world x/y, so they project to the same pixel; different z, so one
        // is nearer the eye at the origin.
        c.tag("far_thing", KIND_PROP, [0.0, 0.0, 5.0]);
        c.tag("near_thing", KIND_PROP, [0.0, 0.0, 1.0]);
        c.exec("ids on");
        let labels = c.labels(IDENTITY, 1280.0, 720.0, [0.0; 3]);
        assert_eq!(labels.len(), 1, "one cell must keep one label");
        assert_eq!(labels[0].name, "near_thing");
    }

    /// Tags far enough apart on screen both survive — declustering must not be
    /// a blanket cull.
    #[test]
    fn tags_in_different_cells_both_survive() {
        let mut c = DevConsole::new();
        c.tag("left", KIND_PROP, [-0.5, 0.0, 1.0]);
        c.tag("right", KIND_PROP, [0.5, 0.0, 1.0]);
        c.exec("ids on");
        let labels = c.labels(IDENTITY, 1280.0, 720.0, [0.0; 3]);
        assert_eq!(labels.len(), 2);
    }

    /// The hard ceiling holds even when every label lands in its own cell.
    #[test]
    fn the_label_count_is_capped() {
        let mut c = DevConsole::new();
        // 200 tags strung across the view, each far enough apart to own a cell.
        (0..200).for_each(|i| {
            let t = f64::from(i) / 200.0 * 1.8 - 0.9;
            c.tag(&format!("thing_{i}"), KIND_PROP, [t, t, 1.0]);
        });
        c.exec("ids on");
        assert_eq!(c.labels(IDENTITY, 20000.0, 20000.0, [0.0; 3]).len(), 60);
    }
    /// Distant tags are dropped rather than painted as an unreadable wall.
    #[test]
    fn a_tag_beyond_the_radius_is_not_labelled() {
        let mut c = console();
        c.exec("ids on");
        let labels = c.labels(IDENTITY, 1280.0, 720.0, [0.0; 3]);
        assert!(
            !labels.iter().any(|l| l.name == "barrel_rust"),
            "a tag 1000 m away was labelled"
        );
        assert_eq!(c.exec("radius 2000"), "radius 2000 m");
        let labels = c.labels(IDENTITY, 1280.0, 720.0, [0.0; 3]);
        assert!(labels.iter().any(|l| l.name == "barrel_rust"));
    }

    /// **Behind the camera is not on screen.** A `w <= 0` divided anyway wraps
    /// the label round onto the view, and it is the most common defect in a
    /// hand-rolled overlay.
    #[test]
    fn a_tag_behind_the_eye_is_dropped() {
        let behind: [f32; 16] = [
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            0.0, 0.0, 0.0, -1.0,
        ];
        let mut c = console();
        c.exec("ids on");
        assert!(c.labels(behind, 1280.0, 720.0, [0.0; 3]).is_empty());
    }

    /// The agent-facing half: every reply is readable text, and an unknown
    /// command answers with what it does know instead of nothing.
    #[test]
    fn every_command_answers_in_text() {
        let mut c = console();
        assert!(c.exec("find crate").contains("crate_c"));
        assert!(c.exec("find nothing_like_this").contains("nothing"));
        assert!(c.exec("names").contains("barrel_rust"));
        assert!(c.exec("ids").contains("off"));
        let help = c.exec("wat");
        assert!(help.contains("ids on|off") && help.contains("find"));
        assert!(help.contains("stats") && help.contains("cam ") && help.contains("freeze"));
    }

    /* ==================================================================== */
    /* the capture pins                                                     */
    /* ==================================================================== */

    fn pose() -> CameraPose {
        CameraPose {
            eye: [1.0, 2.0, 3.0],
            rotation: Euler {
                pitch: 0.1,
                yaw: 0.2,
                roll: 0.3,
            },
            fov_degrees: 90.0,
        }
    }

    /// **`cam` replaces the pose the game resolved, and `cam off` gives it
    /// back.** The whole point of the command: a parity shot has to be able to
    /// stand somewhere the player is not.
    #[test]
    fn a_scripted_camera_replaces_the_rigs_pose() {
        let mut c = console();
        assert_eq!(c.resolve_camera(pose()), pose(), "no override is the identity");

        let reply = c.exec("cam 12 1.75 18 -0.5 -0.1 75");
        assert!(reply.starts_with("cam eye=12.0000,1.7500,18.0000"), "{reply}");
        let out = c.resolve_camera(pose());
        assert_eq!(out.eye, [12.0, 1.75, 18.0]);
        assert!((out.rotation.yaw + 0.5).abs() < 1e-12);
        assert!((out.rotation.pitch + 0.1).abs() < 1e-12);
        assert!((out.fov_degrees - 75.0).abs() < 1e-12);
        assert_eq!(out.rotation.roll, 0.0, "a lookAt shot has no roll");

        assert!(c.exec("cam off").contains("own rig"));
        assert_eq!(c.resolve_camera(pose()), pose());
    }

    /// The FOV is optional, because a shot may want to pin the framing and
    /// leave the ADS/sprint FOV channel alone.
    #[test]
    fn a_scripted_camera_without_a_fov_keeps_the_rigs_fov() {
        let mut c = console();
        c.exec("cam 0 0 0 0 0");
        assert!((c.resolve_camera(pose()).fov_degrees - 90.0).abs() < 1e-12);
    }

    /// A malformed `cam` must not half-apply. Nothing is worse in a measuring
    /// instrument than a pin that silently took on some axes and not others.
    #[test]
    fn a_malformed_cam_changes_nothing_and_says_so() {
        let mut c = console();
        c.exec("cam 1 2 3 4 5 6");
        ["cam", "cam 1 2 3", "cam a b c d e", "cam 1 2 3 4 5 6 7"]
            .into_iter()
            .for_each(|bad| {
                let reply = c.exec(bad);
                assert!(
                    reply.contains("cam:") || reply.contains("pins "),
                    "{bad} -> {reply}"
                );
            });
        // The good pose from the top of the test is still the one in force.
        assert_eq!(c.resolve_camera(pose()).eye, [1.0, 2.0, 3.0]);
        assert!((c.resolve_camera(pose()).fov_degrees - 6.0).abs() < 1e-12);
    }

    #[test]
    fn freeze_and_dt_are_settable_and_reportable() {
        let mut c = console();
        assert!(!c.frozen());
        assert_eq!(c.dt_override(), None);

        assert!(c.exec("freeze on").contains("zeroed"));
        assert!(c.frozen());
        assert!(c.exec("freeze").contains("on"));
        assert!(c.exec("freeze off").contains("off"));
        assert!(!c.frozen());

        assert!(c.exec("dt 0.0166666667").contains("per frame"));
        assert_eq!(c.dt_override(), Some(0.016_666_666_7));
        assert!(c.exec("dt").contains("0.0166"));
        // A zero or negative step is a hang, not a clock.
        ["dt 0", "dt -1", "dt banana"].into_iter().for_each(|bad| {
            assert!(c.exec(bad).contains("expected a positive"), "{bad}");
            assert_eq!(c.dt_override(), Some(0.016_666_666_7), "{bad} changed it");
        });
        assert!(c.exec("dt off").contains("wall clock"));
        assert_eq!(c.dt_override(), None);
    }

    /// **A requested `dt` and a `dt` in force are different claims.** Until a
    /// frame has been through `frame_dt`, `stats` says `dt_used=UNOBSERVED`,
    /// which is what tells a harness the boot wiring is missing.
    #[test]
    fn the_clock_pin_is_only_in_force_once_a_frame_used_it() {
        let mut c = console();
        c.exec("dt 0.02");
        assert!(c.exec("stats").contains("dt=0.020000 dt_used=UNOBSERVED"));
        assert!((c.frame_dt(0.007) - 0.02).abs() < 1e-12, "the pin wins");
        assert!(c.exec("stats").contains("dt=0.020000 dt_used=0.020000"));
        c.exec("dt off");
        assert!((c.frame_dt(0.007) - 0.007).abs() < 1e-12, "the wall clock");
        assert!(c.exec("stats").contains("dt=wallclock dt_used=0.007000"));
    }

    /* ==================================================================== */
    /* `stats`                                                              */
    /* ==================================================================== */

    /// **An unwired hook says UNOBSERVED, not zero.** The single most important
    /// property of this whole file: a harness must be able to tell "the port
    /// drew nothing" from "nobody told me what the port drew", because the
    /// second one invalidates the measurement and the first one *is* the
    /// measurement.
    #[test]
    fn an_unreported_frame_reads_unobserved_rather_than_zero() {
        let c = console();
        let stats = c.exec_ref();
        assert!(stats.contains("frame UNOBSERVED"), "{stats}");
        assert!(stats.contains("meshes UNOBSERVED"), "{stats}");
        assert!(stats.contains("camera=UNPINNED applied=no"), "{stats}");
        assert!(!stats.contains("draws=0"), "zero is a measurement: {stats}");
    }

    /// The frame census counts instances, distinct `(mesh, material)` batches,
    /// and triangles looked up through the uploaded mesh table.
    #[test]
    fn the_frame_census_counts_batches_instances_and_triangles() {
        let mut c = console();
        // Two meshes: 2 triangles and 4 triangles, 12 floats per vertex.
        c.observe_meshes(&[
            (7, vec![0.0; 12 * 6], vec![0; 6]),
            (9, vec![0.0; 12 * 12], vec![0; 12]),
        ]);
        // Five instances over three distinct (mesh, material) pairs.
        c.observe_draws(
            42,
            [(7, 1), (7, 1), (7, 2), (9, 1), (9, 1)],
            3,
            16,
            [0.1, 0.2, 0.3, 1.0],
        );
        let stats = c.exec("stats");
        assert!(stats.contains("meshes count=2 tris=6 verts=18"), "{stats}");
        assert!(stats.contains("tick=42"), "{stats}");
        assert!(stats.contains("draws=3"), "three distinct batches: {stats}");
        assert!(stats.contains("instances=5"), "{stats}");
        // 3 instances of the 2-tri mesh + 2 of the 4-tri mesh.
        assert!(stats.contains("tris=14"), "{stats}");
        assert!(stats.contains("skinned=3") && stats.contains("lights=16"), "{stats}");
        assert!(stats.contains("clear=0.1000,0.2000,0.3000,1.0000"), "{stats}");
        assert_eq!(c.frames_observed(), 1);
    }

    /// A draw against a mesh nobody uploaded contributes no triangles rather
    /// than panicking — the skinned soldiers are exactly that case, and they
    /// are counted separately.
    #[test]
    fn a_draw_against_an_unknown_mesh_costs_no_triangles() {
        let mut c = console();
        c.observe_meshes(&[(7, vec![0.0; 12 * 3], vec![0; 3])]);
        c.observe_draws(1, [(7, 0), (999, 0)], 0, 1, [0.0; 4]);
        let stats = c.exec("stats");
        assert!(stats.contains("instances=2") && stats.contains("tris=1"), "{stats}");
    }

    /// **The level fingerprint is the seed-divergence detector.**
    ///
    /// Two builds that placed the same things in the same places agree; move
    /// one prop, rename one key, or add one placement and the number moves.
    /// This is the check that would have caught the port generating a
    /// different town without anyone taking a screenshot.
    #[test]
    fn the_level_fingerprint_moves_when_the_level_does() {
        let fingerprint = |c: &DevConsole| {
            c.level_line()
                .split_whitespace()
                .find_map(|w| w.strip_prefix("fingerprint="))
                .expect("the level line carries a fingerprint")
                .to_owned()
        };
        let base = fingerprint(&console());
        assert_eq!(base, fingerprint(&console()), "same level, same number");

        let mut moved = DevConsole::new();
        moved.tag("plaster_cream", KIND_STATIC, [0.0, 0.0, 0.0]);
        moved.tag("crate_c", KIND_PROP, [0.5, 0.5, 0.0]);
        moved.tag("barrel_rust", KIND_PROP, [0.5, -0.5, 1000.5]);
        assert_ne!(base, fingerprint(&moved), "a relocated prop must show");

        let mut renamed = DevConsole::new();
        renamed.tag("plaster_cream", KIND_STATIC, [0.0, 0.0, 0.0]);
        renamed.tag("crate_d", KIND_PROP, [0.5, 0.5, 0.0]);
        renamed.tag("barrel_rust", KIND_PROP, [0.5, -0.5, 1000.0]);
        assert_ne!(base, fingerprint(&renamed), "a renamed key must show");

        let mut extra = console();
        extra.tag("crate_c", KIND_PROP, [9.0, 0.0, 9.0]);
        assert_ne!(base, fingerprint(&extra), "an extra placement must show");
    }

    /// Install order is an artefact of how the assembler iterates; the set of
    /// things placed is the fact. Two builds that placed the same world in a
    /// different order must agree.
    #[test]
    fn the_level_fingerprint_ignores_install_order() {
        let line = |c: &DevConsole| c.level_line();
        let mut reversed = DevConsole::new();
        reversed.tag("barrel_rust", KIND_PROP, [0.5, -0.5, 1000.0]);
        reversed.tag("crate_c", KIND_PROP, [0.5, 0.5, 0.0]);
        reversed.tag("plaster_cream", KIND_STATIC, [0.0, 0.0, 0.0]);
        assert_eq!(line(&console()), line(&reversed));
    }

    /// The level line carries the census a numeric town comparison needs.
    #[test]
    fn the_level_line_counts_placements_names_kinds_and_bounds() {
        let line = console().level_line();
        assert!(line.contains("placements=3"), "{line}");
        assert!(line.contains("names=3"), "{line}");
        assert!(line.contains("static=1") && line.contains("props=2"), "{line}");
        assert!(line.contains("min=0.00,-0.50,0.00"), "{line}");
        assert!(line.contains("max=0.50,0.50,1000.00"), "{line}");
        assert!(DevConsole::new().level_line().contains("min=- max=-"));
    }

    /// `stats` reports the camera the frame **used**, and marks whether the
    /// override was actually applied by a frame — the difference between a pin
    /// requested and a pin in force.
    #[test]
    fn stats_reports_the_camera_a_frame_actually_used() {
        let mut c = console();
        c.exec("cam 12 1.75 18 -0.5 -0.1 75");
        assert!(
            c.exec("stats").contains("camera=UNPINNED applied=no"),
            "requested is not applied until a frame runs it"
        );
        c.resolve_camera(pose());
        let stats = c.exec("stats");
        assert!(stats.contains("camera=override applied=yes"), "{stats}");
        assert!(stats.contains("eye=12.0000,1.7500,18.0000"), "{stats}");
        assert!(stats.contains("fov=75.000"), "{stats}");
        c.exec("cam off");
        c.resolve_camera(pose());
        let stats = c.exec("stats");
        assert!(stats.contains("camera=rig applied=yes"), "{stats}");
        assert!(stats.contains("frozen=off dt=wallclock"), "{stats}");
        // `cam` on its own is the same line, so an agent can ask just that.
        assert_eq!(c.exec("cam"), c.pin_line());
    }

    impl DevConsole {
        /// `stats` without needing a `&mut` — the tests read it constantly.
        fn exec_ref(&self) -> String {
            self.stats_report()
        }
    }
}
