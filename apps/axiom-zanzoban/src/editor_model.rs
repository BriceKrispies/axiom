//! The level-authoring (edit mode) model.
//!
//! [`EditorModel`] is a paintable grid of [`TileKind`] cells plus a wiring group
//! per cell. It is pure and browser-free: the browser shell turns clicks into
//! [`EditorModel::paint`] calls and reads back the live validation report and the
//! current TOML. The editor deliberately *allows* transient invalidity (zero or
//! several entrances, an empty group being typed) — that is exactly what the
//! validation report surfaces, and what gates the switch to playtest.

use crate::coord::{GridCoord, GRID_HEIGHT, GRID_WIDTH, MAX_DIMENSION};
use crate::group_id::GroupId;
use crate::level_codec::{self, LevelCodecError};
use crate::level_definition::{
    BudgetRule, Button, DecayRule, Door, LevelDefinition, RuleSet, Switch,
};
use crate::level_validation::{validate_census, LevelCensus, LevelValidationReport};
use crate::render_model::{RenderCell, RenderModel, RenderTile};
use crate::tile_kind::{Addon, TileKind};

/// How many undo steps the editor remembers.
const HISTORY_CAP: usize = 100;

/// An undoable snapshot of the whole editable state.
#[derive(Debug, Clone)]
struct Snapshot {
    width: u32,
    height: u32,
    title: String,
    kinds: Vec<TileKind>,
    groups: Vec<GroupId>,
    rules: RuleSet,
}

/// A paintable level under construction.
#[derive(Debug, Clone)]
pub struct EditorModel {
    width: u32,
    height: u32,
    title: String,
    /// Row-major painted kinds.
    kinds: Vec<TileKind>,
    /// Row-major wiring groups (only meaningful for `Button`/`Door`/`Switch` cells).
    groups: Vec<GroupId>,
    /// The currently selected palette kind.
    selected: TileKind,
    /// The group new buttons/doors/switches are painted with.
    paint_group: GroupId,
    /// The per-level configurable mechanics (add-ons) enabled for this level.
    rules: RuleSet,
    /// Undo/redo history of whole-state snapshots.
    undo_stack: Vec<Snapshot>,
    redo_stack: Vec<Snapshot>,
    /// Whether the last mutating op was a title edit (so title typing coalesces
    /// into one undo step instead of one-per-keystroke).
    last_edit_was_title: bool,
}

impl Default for EditorModel {
    fn default() -> Self {
        EditorModel::new(GRID_WIDTH, GRID_HEIGHT)
    }
}

impl EditorModel {
    /// A blank `width`×`height` editor: all floor, palette on `Wall`, default
    /// group `"main"`.
    pub fn new(width: u32, height: u32) -> Self {
        let count = (width * height) as usize;
        EditorModel {
            width,
            height,
            title: "Untitled".to_string(),
            kinds: vec![TileKind::Floor; count],
            groups: vec![GroupId::default_group(); count],
            selected: TileKind::Wall,
            paint_group: GroupId::default_group(),
            rules: RuleSet::default(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            last_edit_was_title: false,
        }
    }

    /// An editor pre-loaded with an existing level.
    pub fn from_level(level: &LevelDefinition) -> Self {
        let mut editor = EditorModel::new(level.width, level.height);
        editor.load_level(level);
        editor
    }

    /// Grid width.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Grid height.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The level title.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Set the level title. Consecutive title edits coalesce into a single undo
    /// step (so undo reverts a whole edit, not one keystroke).
    pub fn set_title(&mut self, title: impl Into<String>) {
        (!self.last_edit_was_title).then(|| self.push_undo());
        self.title = title.into();
        self.last_edit_was_title = true;
    }

    /// The selected palette kind.
    pub fn selected(&self) -> TileKind {
        self.selected
    }

    /// Select a palette kind to paint with.
    pub fn select(&mut self, kind: TileKind) {
        self.selected = kind;
    }

    /// The group new buttons/doors are painted with.
    pub fn paint_group(&self) -> &GroupId {
        &self.paint_group
    }

    /// Set the group new buttons/doors are painted with.
    pub fn set_paint_group(&mut self, group: GroupId) {
        self.paint_group = group;
    }

    /// Row-major index of `(x, y)`, if in range.
    fn index(&self, x: u32, y: u32) -> Option<usize> {
        (x < self.width && y < self.height).then(|| (y * self.width + x) as usize)
    }

    /// Paint the selected kind (and, for buttons/doors, the current group) onto
    /// cell `(x, y)`. Out-of-range coordinates are ignored.
    pub fn paint(&mut self, x: u32, y: u32) {
        if let Some(i) = self.index(x, y) {
            self.push_undo();
            self.kinds[i] = self.selected;
            if self.selected.has_group() {
                self.groups[i] = self.paint_group.clone();
            }
        }
    }

    /// Erase cell `(x, y)` back to bare floor — clearing both its kind and any
    /// wiring group (unlike painting `Floor`, which would leave a stale group).
    pub fn erase(&mut self, x: u32, y: u32) {
        if let Some(i) = self.index(x, y) {
            self.push_undo();
            self.kinds[i] = TileKind::Floor;
            self.groups[i] = GroupId::default_group();
        }
    }

    /// The kind painted at `(x, y)` (or `Floor` out of range).
    pub fn tile_at(&self, x: u32, y: u32) -> TileKind {
        self.index(x, y)
            .map(|i| self.kinds[i])
            .unwrap_or(TileKind::Floor)
    }

    /// The group painted at `(x, y)`.
    pub fn group_at(&self, x: u32, y: u32) -> GroupId {
        self.index(x, y)
            .map(|i| self.groups[i].clone())
            .unwrap_or_else(GroupId::default_group)
    }

    /// A census of everything currently painted (multiplicity-capable).
    pub fn census(&self) -> LevelCensus {
        let mut census = LevelCensus {
            width: self.width,
            height: self.height,
            entrances: Vec::new(),
            exits: Vec::new(),
            walls: Vec::new(),
            buttons: Vec::new(),
            doors: Vec::new(),
            wells: Vec::new(),
            switches: Vec::new(),
            crates: Vec::new(),
            hazards: Vec::new(),
        };
        for y in 0..self.height {
            for x in 0..self.width {
                let coord = GridCoord::new(x as i32, y as i32);
                let i = (y * self.width + x) as usize;
                // Add-on placements are emitted only while their rule is enabled,
                // so disabling an add-on hides its cells from the level without
                // wiping them from the grid (re-enabling brings them back).
                match self.kinds[i] {
                    TileKind::Floor => {}
                    TileKind::Wall => census.walls.push(coord),
                    TileKind::Entrance => census.entrances.push(coord),
                    TileKind::Exit => census.exits.push(coord),
                    TileKind::Button => census.buttons.push((coord, self.groups[i].clone())),
                    TileKind::Door => census.doors.push((coord, self.groups[i].clone())),
                    TileKind::Well => {
                        self.rules.decay.is_some().then(|| census.wells.push(coord));
                    }
                    TileKind::Switch => {
                        self.rules
                            .switches
                            .then(|| census.switches.push((coord, self.groups[i].clone())));
                    }
                    TileKind::Crate => {
                        self.rules.crates.then(|| census.crates.push(coord));
                    }
                    TileKind::Hazard => {
                        self.rules.hazards.then(|| census.hazards.push(coord));
                    }
                }
            }
        }
        census
    }

    /// Validate the current grid. Reachable here: the entrance/exit-count rules
    /// (the grid can hold zero or many of each).
    pub fn validate(&self) -> LevelValidationReport {
        validate_census(&self.census())
    }

    /// May the editor switch to playtest? Only when the level validates.
    pub fn can_playtest(&self) -> bool {
        self.validate().is_valid()
    }

    /// Best-effort conversion to a canonical [`LevelDefinition`]: the first
    /// entrance/exit in row-major order (or `(0, 0)` if none) plus every wall,
    /// button, and door. When the level is valid there is exactly one of each, so
    /// this is exact; when invalid, validation reports what is wrong.
    pub fn to_level_definition(&self) -> LevelDefinition {
        let census = self.census();
        LevelDefinition {
            title: self.title.clone(),
            width: self.width,
            height: self.height,
            entrance: census
                .entrances
                .first()
                .copied()
                .unwrap_or(GridCoord::new(0, 0)),
            exit: census
                .exits
                .first()
                .copied()
                .unwrap_or(GridCoord::new(0, 0)),
            walls: census.walls,
            buttons: census
                .buttons
                .into_iter()
                .map(|(position, group)| Button { position, group })
                .collect(),
            doors: census
                .doors
                .into_iter()
                .map(|(position, group)| Door { position, group })
                .collect(),
            wells: census.wells,
            switches: census
                .switches
                .into_iter()
                .map(|(position, group)| Switch { position, group })
                .collect(),
            crates: census.crates,
            hazards: census.hazards,
            rules: self.rules.clone(),
        }
    }

    /// The current level as hand-editable TOML.
    pub fn to_toml(&self) -> Result<String, LevelCodecError> {
        level_codec::to_toml(&self.to_level_definition())
    }

    /// Replace the editor's contents from TOML text. On a parse error the editor
    /// is left unchanged.
    pub fn import_toml(&mut self, text: &str) -> Result<(), LevelCodecError> {
        let level = level_codec::from_toml(text)?;
        self.push_undo();
        self.resize(level.width, level.height);
        self.load_level(&level);
        Ok(())
    }

    /// Replace the editor's grid from a level (assumes the level's size).
    pub fn load_level(&mut self, level: &LevelDefinition) {
        self.resize(level.width, level.height);
        self.title = level.title.clone();
        self.rules = level.rules.clone();
        self.kinds.iter_mut().for_each(|k| *k = TileKind::Floor);
        for &c in &level.walls {
            self.set_cell(c, TileKind::Wall, None);
        }
        for b in &level.buttons {
            self.set_cell(b.position, TileKind::Button, Some(&b.group));
        }
        for d in &level.doors {
            self.set_cell(d.position, TileKind::Door, Some(&d.group));
        }
        for &c in &level.wells {
            self.set_cell(c, TileKind::Well, None);
        }
        for s in &level.switches {
            self.set_cell(s.position, TileKind::Switch, Some(&s.group));
        }
        for &c in &level.crates {
            self.set_cell(c, TileKind::Crate, None);
        }
        for &c in &level.hazards {
            self.set_cell(c, TileKind::Hazard, None);
        }
        self.set_cell(level.entrance, TileKind::Entrance, None);
        self.set_cell(level.exit, TileKind::Exit, None);
    }

    /// Stamp one cell (skipping out-of-grid / negative coordinates).
    fn set_cell(&mut self, coord: GridCoord, kind: TileKind, group: Option<&GroupId>) {
        if coord.x < 0 || coord.y < 0 {
            return;
        }
        if let Some(i) = self.index(coord.x as u32, coord.y as u32) {
            self.kinds[i] = kind;
            if let Some(g) = group {
                self.groups[i] = g.clone();
            }
        }
    }

    /// Resize the grid (clearing it) if the dimensions changed.
    fn resize(&mut self, width: u32, height: u32) {
        if width != self.width || height != self.height {
            let count = (width * height) as usize;
            self.width = width;
            self.height = height;
            self.kinds = vec![TileKind::Floor; count];
            self.groups = vec![GroupId::default_group(); count];
        }
    }

    /// A render model of the painted grid (doors drawn closed, buttons released,
    /// no actors), so edit mode and playtest share the same depth-cue drawing.
    pub fn render_model(&self) -> RenderModel {
        let cells = (0..self.height)
            .flat_map(|y| (0..self.width).map(move |x| (x, y)))
            .map(|(x, y)| {
                let coord = GridCoord::new(x as i32, y as i32);
                let tile = match self.tile_at(x, y) {
                    // A crate is drawn as an overlay (below); its cell is floor.
                    TileKind::Floor | TileKind::Crate => RenderTile::Floor,
                    TileKind::Wall => RenderTile::Wall,
                    TileKind::Entrance => RenderTile::Entrance,
                    TileKind::Exit => RenderTile::Exit,
                    TileKind::Button => RenderTile::Button { pressed: false },
                    TileKind::Door => RenderTile::Door { open: false },
                    TileKind::Well => RenderTile::Well,
                    TileKind::Switch => RenderTile::Switch { latched: false },
                    TileKind::Hazard => RenderTile::Hazard,
                };
                RenderCell { coord, tile }
            })
            .collect();
        let crates = if self.rules.crates {
            (0..self.height)
                .flat_map(|y| (0..self.width).map(move |x| (x, y)))
                .filter(|&(x, y)| self.tile_at(x, y) == TileKind::Crate)
                .map(|(x, y)| GridCoord::new(x as i32, y as i32))
                .collect()
        } else {
            Vec::new()
        };
        RenderModel {
            width: self.width,
            height: self.height,
            cells,
            actors: Vec::new(),
            crates,
        }
    }

    // ---- Configurable mechanics (add-ons) --------------------------------------

    /// The mechanics currently enabled for this level.
    pub fn rules(&self) -> &RuleSet {
        &self.rules
    }

    /// Enable/disable the afterimage-decay add-on (`Some(lifetime_steps)` on).
    pub fn set_decay(&mut self, lifetime: Option<u32>) {
        self.push_undo();
        self.rules.decay = lifetime.map(|lifetime_steps| DecayRule { lifetime_steps });
        self.demote_selection_if_hidden();
    }

    /// Enable/disable the echo-budget add-on (`Some((max_ghosts, par))` on).
    pub fn set_budget(&mut self, budget: Option<(u32, Option<u32>)>) {
        self.push_undo();
        self.rules.budget = budget.map(|(max_ghosts, par)| BudgetRule { max_ghosts, par });
    }

    /// Enable/disable the latching-switches add-on.
    pub fn set_switches(&mut self, on: bool) {
        self.push_undo();
        self.rules.switches = on;
        self.demote_selection_if_hidden();
    }

    /// Enable/disable the pushable-crates add-on.
    pub fn set_crates(&mut self, on: bool) {
        self.push_undo();
        self.rules.crates = on;
        self.demote_selection_if_hidden();
    }

    /// Enable/disable the lethal-hazards add-on.
    pub fn set_hazards(&mut self, on: bool) {
        self.push_undo();
        self.rules.hazards = on;
        self.demote_selection_if_hidden();
    }

    /// The palette kinds available right now: the base six plus each add-on kind
    /// whose add-on is enabled.
    pub fn available_kinds(&self) -> Vec<TileKind> {
        TileKind::ALL
            .into_iter()
            .filter(|k| self.addon_enabled(k.required_addon()))
            .collect()
    }

    /// Is the add-on `addon` enabled (base kinds — `None` — are always available)?
    fn addon_enabled(&self, addon: Option<Addon>) -> bool {
        match addon {
            None => true,
            Some(Addon::Decay) => self.rules.decay.is_some(),
            Some(Addon::Switches) => self.rules.switches,
            Some(Addon::Crates) => self.rules.crates,
            Some(Addon::Hazards) => self.rules.hazards,
        }
    }

    /// If the selected kind's add-on just turned off, fall back to `Wall`.
    fn demote_selection_if_hidden(&mut self) {
        (!self.available_kinds().contains(&self.selected)).then(|| self.selected = TileKind::Wall);
    }

    /// Every cell wired to `group` (buttons, doors, switches) — for the editor's
    /// group-link highlight.
    pub fn cells_in_group(&self, group: &GroupId) -> Vec<GridCoord> {
        (0..self.height)
            .flat_map(|y| (0..self.width).map(move |x| (x, y)))
            .filter(|&(x, y)| {
                let i = (y * self.width + x) as usize;
                self.kinds[i].has_group() && &self.groups[i] == group
            })
            .map(|(x, y)| GridCoord::new(x as i32, y as i32))
            .collect()
    }

    // ---- Resize + undo/redo ----------------------------------------------------

    /// Resize the grid, preserving the overlapping top-left region. Clamped to
    /// `1..=MAX_DIMENSION`.
    pub fn resize_preserving(&mut self, width: u32, height: u32) {
        let width = width.clamp(1, MAX_DIMENSION);
        let height = height.clamp(1, MAX_DIMENSION);
        if width == self.width && height == self.height {
            return;
        }
        self.push_undo();
        let count = (width * height) as usize;
        let mut kinds = vec![TileKind::Floor; count];
        let mut groups = vec![GroupId::default_group(); count];
        let copy_w = self.width.min(width);
        let copy_h = self.height.min(height);
        for y in 0..copy_h {
            for x in 0..copy_w {
                let src = (y * self.width + x) as usize;
                let dst = (y * width + x) as usize;
                kinds[dst] = self.kinds[src];
                groups[dst] = self.groups[src].clone();
            }
        }
        self.width = width;
        self.height = height;
        self.kinds = kinds;
        self.groups = groups;
    }

    /// Snapshot the whole editable state.
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            width: self.width,
            height: self.height,
            title: self.title.clone(),
            kinds: self.kinds.clone(),
            groups: self.groups.clone(),
            rules: self.rules.clone(),
        }
    }

    /// Record the current state for undo (called at the start of a mutation).
    fn push_undo(&mut self) {
        self.undo_stack.push(self.snapshot());
        (self.undo_stack.len() > HISTORY_CAP).then(|| self.undo_stack.remove(0));
        self.redo_stack.clear();
        self.last_edit_was_title = false;
    }

    /// Overwrite the editable state from a snapshot.
    fn restore(&mut self, s: Snapshot) {
        self.width = s.width;
        self.height = s.height;
        self.title = s.title;
        self.kinds = s.kinds;
        self.groups = s.groups;
        self.rules = s.rules;
        self.demote_selection_if_hidden();
    }

    /// Undo the last mutation. Returns whether anything was undone.
    pub fn undo(&mut self) -> bool {
        match self.undo_stack.pop() {
            Some(prev) => {
                self.redo_stack.push(self.snapshot());
                self.restore(prev);
                self.last_edit_was_title = false;
                true
            }
            None => false,
        }
    }

    /// Redo the last undone mutation. Returns whether anything was redone.
    pub fn redo(&mut self) -> bool {
        match self.redo_stack.pop() {
            Some(next) => {
                self.undo_stack.push(self.snapshot());
                self.restore(next);
                self.last_edit_was_title = false;
                true
            }
            None => false,
        }
    }

    /// Is there anything to undo?
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    /// Is there anything to redo?
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level_validation::LevelError;

    #[test]
    fn painting_places_kinds_and_groups() {
        let mut e = EditorModel::new(10, 10);
        e.select(TileKind::Button);
        e.set_paint_group(GroupId::new("alt"));
        e.paint(4, 5);
        assert_eq!(e.tile_at(4, 5), TileKind::Button);
        assert_eq!(e.group_at(4, 5), GroupId::new("alt"));
        e.paint(99, 99);
        assert_eq!(e.tile_at(0, 0), TileKind::Floor);
    }

    #[test]
    fn zero_and_many_entrances_are_invalid() {
        let mut e = EditorModel::new(6, 1);
        let r = e.validate();
        assert!(r.contains(&LevelError::NoEntrance));
        assert!(r.contains(&LevelError::NoExit));
        assert!(!e.can_playtest());

        e.select(TileKind::Entrance);
        e.paint(0, 0);
        e.paint(1, 0);
        assert!(e.validate().contains(&LevelError::MultipleEntrances(2)));
    }

    #[test]
    fn a_complete_grid_validates_and_can_playtest() {
        let mut e = EditorModel::new(5, 1);
        e.select(TileKind::Entrance);
        e.paint(0, 0);
        e.select(TileKind::Exit);
        e.paint(4, 0);
        e.select(TileKind::Button);
        e.paint(1, 0);
        e.select(TileKind::Door);
        e.paint(3, 0);
        assert!(e.validate().is_valid(), "{:?}", e.validate().messages());
        assert!(e.can_playtest());
    }

    #[test]
    fn import_export_round_trips_through_the_editor() {
        let mut e = EditorModel::new(5, 1);
        e.set_title("corridor");
        e.select(TileKind::Entrance);
        e.paint(0, 0);
        e.select(TileKind::Exit);
        e.paint(4, 0);
        e.select(TileKind::Button);
        e.paint(1, 0);
        e.select(TileKind::Door);
        e.paint(3, 0);

        let toml = e.to_toml().expect("exports");
        let mut e2 = EditorModel::new(1, 1);
        e2.import_toml(&toml).expect("imports");
        assert_eq!(e2.tile_at(1, 0), TileKind::Button);
        assert_eq!(e2.tile_at(3, 0), TileKind::Door);
        assert_eq!(e2.title(), "corridor");
        assert_eq!(e2.to_level_definition(), e.to_level_definition());
    }

    #[test]
    fn render_model_draws_doors_closed_and_has_no_actors() {
        let mut e = EditorModel::new(3, 1);
        e.select(TileKind::Door);
        e.paint(1, 0);
        let m = e.render_model();
        assert!(m.actors.is_empty());
        assert_eq!(
            m.cell_at(GridCoord::new(1, 0)).unwrap().tile,
            RenderTile::Door { open: false }
        );
    }
}
