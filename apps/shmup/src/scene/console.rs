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

use axiom_introspect::WorldTag;

/// Micro-units per world unit — the fixed-point convention [`WorldTag`] stores
/// positions in.
const MICRO: f64 = 1_000_000.0;

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
                "  names           every distinct tag name in the level
",
                "  lock            live pointer-lock / input state"
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
    }
}
