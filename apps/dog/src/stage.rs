//! The two **stages** the page can show, and everything that differs between
//! them.
//!
//! A stage is not a second scene. Geometry is uploaded once at bind (see
//! `NOTES.md` §8), so there is exactly one registered mesh set and exactly one
//! instance pool for the whole session — a stage is a *presentation* of it: how
//! many pool slots are drawn, whether they walk, and where the camera opens.
//! Both stages therefore cost the same nothing to switch between, and the study
//! is provably the same animal as the field rather than a second model that
//! could drift from it.
//!
//! Everything a stage decides is answered here, as data, so the browser edge
//! (`src/stage_input.rs`) is left doing nothing but turning a click into a value
//! — the same split the dial panel already has between [`crate::Dial`] and
//! `src/slider_input.rs`.

use crate::install::{CAMERA_EYE, CAMERA_TARGET};
use crate::study::{STUDY_EYE, STUDY_TARGET};

/// How many stages there are — one button on the page each.
pub const STAGE_COUNT: usize = 2;

/// Which presentation of the registered geometry the page is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// The whole field: every ring, every dog, walking on the terrain. The
    /// scene the app opens on.
    Field,
    /// One dog, held still and suspended at the origin, seen close up. The
    /// terrain and the rest of the crowd are retired; the camera still orbits.
    Study,
}

impl Stage {
    /// Every stage, in button order.
    pub const ALL: [Stage; STAGE_COUNT] = [Stage::Field, Stage::Study];

    /// The short stable identifier: the DOM `data-stage` attribute and the
    /// query parameter value.
    pub fn key(self) -> &'static str {
        ["field", "study"][self as usize]
    }

    /// The label printed on this stage's button.
    pub fn label(self) -> &'static str {
        ["the field", "one dog"][self as usize]
    }

    /// The stage `key` names, or the opening one for anything else.
    pub fn from_key(key: &str) -> Stage {
        Stage::ALL
            .into_iter()
            .find(|stage| stage.key() == key)
            .unwrap_or(Stage::Field)
    }

    /// Whether the crowd walks on this stage. The study is **still** — its pose
    /// is a pure function of the configuration and takes no tick at all, so a
    /// stopped dog is stopped by construction rather than by a paused clock.
    pub fn walks(self) -> bool {
        matches!(self, Stage::Field)
    }

    /// Whether the static half of the scene — the terrain — is drawn.
    pub fn shows_ground(self) -> bool {
        matches!(self, Stage::Field)
    }

    /// How many dogs this stage draws, given the crowd the layout asks for.
    pub fn crowd(self, field: usize) -> usize {
        [field, 1][self as usize]
    }

    /// This stage's authored camera framing, as an `(eye, target)` pair. One
    /// definition apiece, shared by the installed camera and by the orbit the
    /// user's gestures drive — see [`crate::OrbitState::for_stage`].
    pub fn framing(self) -> ([f32; 3], [f32; 3]) {
        [(CAMERA_EYE, CAMERA_TARGET), (STUDY_EYE, STUDY_TARGET)][self as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_stage_has_a_distinct_key_that_round_trips() {
        let mut keys: Vec<&str> = Stage::ALL.into_iter().map(Stage::key).collect();
        let count = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), count, "two stages share a key");
        for stage in Stage::ALL {
            assert_eq!(Stage::from_key(stage.key()), stage);
            assert!(!stage.label().is_empty());
        }
        // Anything unrecognised opens on the field, which is what a bare URL
        // and a junk one both have to do.
        assert_eq!(Stage::from_key(""), Stage::Field);
        assert_eq!(Stage::from_key("nonsense"), Stage::Field);
    }

    #[test]
    fn the_study_stops_the_field_and_keeps_exactly_one_dog() {
        assert!(Stage::Field.walks());
        assert!(!Stage::Study.walks());
        assert!(Stage::Field.shows_ground());
        assert!(!Stage::Study.shows_ground());
        assert_eq!(Stage::Field.crowd(104), 104);
        assert_eq!(Stage::Study.crowd(104), 1);
        // ...and one dog is still one dog on an empty field.
        assert_eq!(Stage::Study.crowd(0), 1);
    }

    #[test]
    fn the_two_stages_are_framed_from_different_cameras() {
        let (field_eye, field_target) = Stage::Field.framing();
        let (study_eye, study_target) = Stage::Study.framing();
        assert_eq!(field_eye, CAMERA_EYE);
        assert_eq!(field_target, CAMERA_TARGET);
        assert_ne!(study_eye, field_eye);
        // The study is much closer to what it is looking at than the field shot
        // is — that is the whole point of it.
        let span = |eye: [f32; 3], target: [f32; 3]| {
            ((eye[0] - target[0]).powi(2)
                + (eye[1] - target[1]).powi(2)
                + (eye[2] - target[2]).powi(2))
            .sqrt()
        };
        assert!(span(study_eye, study_target) * 4.0 < span(field_eye, field_target));
    }
}
