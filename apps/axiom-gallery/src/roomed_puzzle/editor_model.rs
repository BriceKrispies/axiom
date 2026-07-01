//! The level-authoring (edit mode) model.
//!
//! [`EditorModel`] is a paintable grid of [`TileKind`] cells plus a wiring group
//! per cell. It is pure and browser-free: the browser shell turns clicks into
//! [`EditorModel::paint`] calls and reads back the live validation report and the
//! current TOML. The editor deliberately *allows* transient invalidity (zero or
//! several entrances, an empty group being typed) — that is exactly what the
//! validation report surfaces, and what gates the switch to playtest.

use crate::roomed_puzzle::coord::{GridCoord, GRID_HEIGHT, GRID_WIDTH};
use crate::roomed_puzzle::group_id::GroupId;
use crate::roomed_puzzle::level_codec::{self, LevelCodecError};
use crate::roomed_puzzle::level_definition::{Button, Door, LevelDefinition};
use crate::roomed_puzzle::level_validation::{validate_census, LevelCensus, LevelValidationReport};
use crate::roomed_puzzle::render_model::{RenderCell, RenderModel, RenderTile};
use crate::roomed_puzzle::tile_kind::TileKind;

/// A paintable level under construction.
#[derive(Debug, Clone)]
pub struct EditorModel {
    width: u32,
    height: u32,
    title: String,
    /// Row-major painted kinds.
    kinds: Vec<TileKind>,
    /// Row-major wiring groups (only meaningful for `Button`/`Door` cells).
    groups: Vec<GroupId>,
    /// The currently selected palette kind.
    selected: TileKind,
    /// The group new buttons/doors are painted with.
    paint_group: GroupId,
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

    /// Set the level title.
    pub fn set_title(&mut self, title: impl Into<String>) {
        self.title = title.into();
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
            self.kinds[i] = self.selected;
            if self.selected.has_group() {
                self.groups[i] = self.paint_group.clone();
            }
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
        };
        for y in 0..self.height {
            for x in 0..self.width {
                let coord = GridCoord::new(x as i32, y as i32);
                let i = (y * self.width + x) as usize;
                match self.kinds[i] {
                    TileKind::Floor => {}
                    TileKind::Wall => census.walls.push(coord),
                    TileKind::Entrance => census.entrances.push(coord),
                    TileKind::Exit => census.exits.push(coord),
                    TileKind::Button => census.buttons.push((coord, self.groups[i].clone())),
                    TileKind::Door => census.doors.push((coord, self.groups[i].clone())),
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
        self.resize(level.width, level.height);
        self.load_level(&level);
        Ok(())
    }

    /// Replace the editor's grid from a level (assumes the level's size).
    pub fn load_level(&mut self, level: &LevelDefinition) {
        self.resize(level.width, level.height);
        self.title = level.title.clone();
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
                    TileKind::Floor => RenderTile::Floor,
                    TileKind::Wall => RenderTile::Wall,
                    TileKind::Entrance => RenderTile::Entrance,
                    TileKind::Exit => RenderTile::Exit,
                    TileKind::Button => RenderTile::Button { pressed: false },
                    TileKind::Door => RenderTile::Door { open: false },
                };
                RenderCell { coord, tile }
            })
            .collect();
        RenderModel {
            width: self.width,
            height: self.height,
            cells,
            actors: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roomed_puzzle::level_validation::LevelError;

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
