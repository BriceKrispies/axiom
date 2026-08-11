//! **Road-mesh preparation** — cutting the road's geometry from the compiled
//! track before the race starts.
//!
//! This is the largest count of anything the phase produces: over the shipping
//! course's 9 270 m it builds **24** drawn spans (`DRAW_SPAN` = 400 m, four
//! material parts each) and **927** fine paint chunks (`PAINT_CHUNK_LENGTH` =
//! 10 m). Do not confuse either number with the **93** scenery cells
//! (`CHUNK_LENGTH` = 100 m) or with `Effects::install`'s 92 entities; three
//! different chunkings and an unrelated coincidence all live within a few
//! hundred lines of each other.
//!
//! # Why this task reads the course cell
//!
//! Road geometry is a pure function of `(track, index, tuning)`, and the track
//! comes from the course task pushed before it. The schedule is assembled
//! *before* `Runtime::prepare` runs, so there is no `Track` to hand this task at
//! construction time — it only exists once `CourseTask` has executed inside that
//! same `prepare()` call. So the task holds the course **cell** and reads it in
//! its own `prepare`, failing the phase if it is empty. That is the
//! read-before-write protocol the runtime's own
//! `a_task_that_reads_an_unwritten_product_fails_the_phase` pins.
//!
//! # What is deliberately NOT prepared here
//!
//! `PlayerCar::install`, `TrafficVisuals::install`, `PickupVisuals::install`,
//! `Effects::install`, `install_finish_arch`, `install_lights`,
//! `DebugView::install` and the three prop surfaces all build from engine
//! primitives (`Mesh::cube()`, `Mesh::cylinder()`) or from a handful of
//! hand-authored quads. There is nothing expensive to prepare, and moving them
//! would be churn carrying golden risk for no measurable benefit — the same
//! reasoning that keeps dynamic traffic in the frame loop.

use std::cell::RefCell;
use std::rc::Rc;

use axiom::prelude::MeshData;
use axiom_runtime::{PreparationTask, RuntimeError, RuntimeErrorCode, RuntimeResult};

use crate::preparation::course;
use crate::render::road_mesh::{
    build_draw_mesh, build_paint_chunk, draw_count, paint_chunk_count, ChunkMeshes,
};
use crate::tuning::CourseTuning;

/// Every piece of road geometry, cut from the compiled track.
///
/// Deliberately not `Default`: an empty road is not a road.
#[derive(Debug, Clone)]
pub struct PreparedMeshes {
    draw_chunks: Vec<ChunkMeshes>,
    paint_chunks: Vec<MeshData>,
}

impl PreparedMeshes {
    /// Cut every span and every paint chunk, in ascending index order.
    ///
    /// Ascending order is not incidental. `RoadChunks::install` registers the
    /// meshes in exactly this sequence, `add_mesh_data` mints
    /// `id = meshes.len() + 1`, and those ids are encoded in the committed
    /// golden artifacts.
    pub fn cut(track: &crate::track::Track, tuning: &CourseTuning) -> PreparedMeshes {
        PreparedMeshes {
            draw_chunks: (0..draw_count(track))
                .map(|index| build_draw_mesh(track, index, tuning))
                .collect(),
            paint_chunks: (0..paint_chunk_count(track))
                .map(|index| build_paint_chunk(track, index, tuning))
                .collect(),
        }
    }

    /// The drawn spans, in registration order.
    pub fn draw_chunks(&self) -> &[ChunkMeshes] {
        &self.draw_chunks
    }

    /// The fine paint chunks, in registration order.
    pub fn paint_chunks(&self) -> &[MeshData] {
        &self.paint_chunks
    }

    /// Consume into `(draw spans, paint chunks)`.
    ///
    /// By value on purpose: `add_mesh_data` takes its `MeshData` by value, so
    /// registering from a borrow would clone every one of the ~951 meshes and
    /// hold the whole road twice at the seam. Handing ownership over means the
    /// prepared store is emptied into the scene exactly once.
    pub fn into_parts(self) -> (Vec<ChunkMeshes>, Vec<MeshData>) {
        (self.draw_chunks, self.paint_chunks)
    }
}

/// Cuts the road's geometry at startup into [`PreparedMeshes`].
#[derive(Debug)]
pub struct MeshTask {
    /// READ cell — the course task, pushed earlier, fills this.
    pub course: Rc<RefCell<Option<course::PreparedCourse>>>,
    /// The course tuning the strip geometry is cut against.
    pub tuning: CourseTuning,
    /// The cell this task writes its product into.
    pub out: Rc<RefCell<Option<PreparedMeshes>>>,
}

impl PreparationTask for MeshTask {
    fn prepare(&mut self) -> RuntimeResult<()> {
        // Read-before-write: if the course task has not run, this is `None`.
        // Failing the phase is the only correct response — `.expect()` here
        // would panic through `Runtime::prepare` and bypass the failure
        // protocol entirely.
        let plan = self.course.borrow().as_ref().map(course::PreparedCourse::plan);
        let plan = plan.ok_or_else(|| {
            RuntimeError::new(
                RuntimeErrorCode::PreparationFailed,
                "burnt-rubber/meshes needs the compiled course",
            )
        })?;
        *self.out.borrow_mut() = Some(PreparedMeshes::cut(plan.track(), &self.tuning));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tuning::Tuning;
    use crate::DEFAULT_SEED;

    fn prepared_course() -> course::PreparedCourse {
        let out = Rc::new(RefCell::new(None));
        let mut task = course::CourseTask {
            seed: DEFAULT_SEED,
            tuning: Tuning::DEFAULT,
            out: Rc::clone(&out),
        };
        task.prepare().expect("the shipping course compiles");
        let product = out.borrow_mut().take();
        product.expect("the course task wrote its product")
    }

    fn prepared_meshes() -> (PreparedMeshes, course::PreparedCourse) {
        let course = prepared_course();
        let cell = Rc::new(RefCell::new(Some(course.clone())));
        let out = Rc::new(RefCell::new(None));
        let mut task = MeshTask {
            course: cell,
            tuning: Tuning::DEFAULT.course,
            out: Rc::clone(&out),
        };
        task.prepare().expect("the road cuts");
        let product = out.borrow_mut().take();
        (product.expect("the task wrote its product"), course)
    }

    /// Every span and every paint chunk the course calls for.
    #[test]
    fn preparing_cuts_every_chunk_of_the_road() {
        let (meshes, course) = prepared_meshes();
        let plan = course.plan();
        assert_eq!(meshes.draw_chunks().len(), draw_count(plan.track()));
        assert_eq!(meshes.paint_chunks().len(), paint_chunk_count(plan.track()));
        assert!(
            meshes.draw_chunks().len() > 20 && meshes.paint_chunks().len() > 900,
            "the shipping course is ~24 spans and ~927 paint chunks: {} / {}",
            meshes.draw_chunks().len(),
            meshes.paint_chunks().len()
        );
    }

    /// **The most important test here.** A prepared chunk is byte-identical to
    /// what the inline builder produces, so moving the cut earlier cannot move
    /// a single vertex — and therefore cannot move a golden.
    #[test]
    fn a_prepared_chunk_matches_the_inline_builder() {
        let (meshes, course) = prepared_meshes();
        let plan = course.plan();
        let tuning = Tuning::DEFAULT.course;
        (0..meshes.draw_chunks().len()).for_each(|index| {
            let inline = build_draw_mesh(plan.track(), index, &tuning);
            let prepared = &meshes.draw_chunks()[index];
            assert_eq!(prepared.surface, inline.surface, "span {index} surface");
            assert_eq!(prepared.paint, inline.paint, "span {index} paint");
            assert_eq!(prepared.rail, inline.rail, "span {index} rail");
            assert_eq!(prepared.verge, inline.verge, "span {index} verge");
        });
        (0..meshes.paint_chunks().len()).for_each(|index| {
            assert_eq!(
                meshes.paint_chunks()[index],
                build_paint_chunk(plan.track(), index, &tuning),
                "paint chunk {index}"
            );
        });
    }

    /// Same course in, same geometry out.
    #[test]
    fn two_cuts_of_one_course_are_identical() {
        let (a, course) = prepared_meshes();
        let b = PreparedMeshes::cut(course.plan().track(), &Tuning::DEFAULT.course);
        assert_eq!(a.draw_chunks().len(), b.draw_chunks().len());
        // `ChunkMeshes` carries no `PartialEq`, and adding one to a file this
        // manifest does not own would be scope creep — compare the four
        // `MeshData` parts, which do.
        a.draw_chunks()
            .iter()
            .zip(b.draw_chunks())
            .enumerate()
            .for_each(|(index, (x, y))| {
                assert_eq!(x.surface, y.surface, "span {index} surface");
                assert_eq!(x.paint, y.paint, "span {index} paint");
                assert_eq!(x.rail, y.rail, "span {index} rail");
                assert_eq!(x.verge, y.verge, "span {index} verge");
            });
        assert_eq!(a.paint_chunks(), b.paint_chunks());
    }

    /// The read-before-write hazard, proven rather than assumed: a mesh task
    /// whose course cell is empty fails the phase instead of panicking.
    #[test]
    fn cutting_without_a_course_fails_the_phase() {
        let out = Rc::new(RefCell::new(None));
        let mut task = MeshTask {
            course: Rc::new(RefCell::new(None)),
            tuning: Tuning::DEFAULT.course,
            out: Rc::clone(&out),
        };
        let error = task.prepare().expect_err("an absent course must fail");
        assert_eq!(error.code(), RuntimeErrorCode::PreparationFailed);
        assert!(out.borrow().is_none(), "and writes no product");
    }
}
