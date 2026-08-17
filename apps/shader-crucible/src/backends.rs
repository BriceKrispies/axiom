//! **Station 9 — both backends**, and the substitution the software arm makes.
//!
//! `axiom_surface::supported_by(reqs, profile)` is the backend-**neutral** half
//! of "will this render there?": a pure query with no device, no program and no
//! frame. This module asks it of every station's surface against both of the
//! engine's real profiles, and reports the answers as data a label can print and
//! a test can assert.
//!
//! ## What the two arms actually do with a surface
//!
//! * **GPU** — the surface's channel graphs are lowered into one generated WGSL
//!   function per stage and evaluated **per pixel**. That is the path this whole
//!   app exists to exercise.
//! * **Canvas2D** — the software rasterizer executes no shader, but a surface's
//!   channels are *fields* with a reference evaluator, so it evaluates each
//!   channel **once per triangle**, at that triangle's object-space centroid.
//!   `RenderCapability::ProceduralSurface` is therefore ON in its profile too:
//!   the substitution is the **sampling rate**, not the appearance.
//!
//! ## Limitation 3, and why nothing here hides it
//!
//! One sample per triangle is a coarse sampling rate, and a mask finer than a
//! triangle simply is not sampled. Station 1's scratch mask is a family of lines
//! a fraction of a body wide; on a low-poly sphere it can miss every centroid and
//! **vanish entirely** on the software arm. That is the honest cost of
//! "substitute, not drop", and the correct response to it is to say so.
//!
//! **The meshes are deliberately not tessellated to hide it.** Subdividing the
//! bodies until the software arm resolved the scratches would make the picture
//! prettier and the report false: it would be measuring a mesh, not a backend.

use axiom_host::{BackendCapabilityProfile, RenderCapability};
use axiom_surface::{supported_by, Surface};

/// The two profiles a surface is reported against.
///
/// Both attempt procedural surfaces, which is what makes the comparison
/// interesting: the difference between the arms is not *whether* they render an
/// authored surface but *how densely they sample it*.
pub fn profiles() -> [(&'static str, BackendCapabilityProfile); 2] {
    [
        ("gpu", BackendCapabilityProfile::all()),
        (
            "canvas2d",
            BackendCapabilityProfile::none().with(RenderCapability::ProceduralSurface),
        ),
    ]
}

/// One station surface's verdict on both arms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupportReport {
    /// Which station surface, by index into `stations::all_surfaces`.
    pub index: usize,
    /// Whether the GPU profile clears the capability gate.
    pub gpu: bool,
    /// Whether the Canvas2D profile clears it.
    pub canvas2d: bool,
    /// How many operator nodes the surface's bound graphs hold in total — the
    /// number the GPU backend's own 256-node shader budget is checked against.
    pub nodes: u16,
    /// How many parameter slots it declares.
    pub params: u16,
}

/// Every station surface's verdict, in station order.
pub fn support_report(surfaces: &[Surface]) -> Vec<SupportReport> {
    surfaces
        .iter()
        .enumerate()
        .map(|(index, surface)| {
            let reqs = surface.requirements();
            SupportReport {
                index,
                gpu: supported_by(&reqs, BackendCapabilityProfile::all()),
                canvas2d: supported_by(
                    &reqs,
                    BackendCapabilityProfile::none().with(RenderCapability::ProceduralSurface),
                ),
                nodes: reqs.node_count(),
                params: reqs.param_count(),
            }
        })
        .collect()
}

/// The report as the lines the on-screen panel and the README print.
pub fn support_lines(surfaces: &[Surface]) -> Vec<String> {
    support_report(surfaces)
        .iter()
        .map(|r| {
            format!(
                "surface {:>2}  gpu:{:<5}  canvas2d:{:<5}  {:>3} nodes  {:>2} params",
                r.index, r.gpu, r.canvas2d, r.nodes, r.params
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stations::all_surfaces;

    /// **`supported_by` reports the truth for both profiles, before anything is
    /// rendered.** Every station clears the capability gate on both arms — which
    /// is the point: the software arm does not *drop* a procedural surface, it
    /// substitutes a coarser sampling rate for it.
    #[test]
    fn every_station_clears_the_capability_gate_on_both_profiles() {
        let surfaces = all_surfaces();
        let report = support_report(&surfaces);
        report
            .iter()
            .for_each(|r| println!("{r:?}"));
        assert_eq!(report.len(), 11);
        assert!(report.iter().all(|r| r.gpu), "a station failed the GPU gate");
        assert!(
            report.iter().all(|r| r.canvas2d),
            "a station failed the Canvas2D gate"
        );
    }

    /// **A profile that does not attempt procedural surfaces says so**, and that
    /// is what makes the two `true`s above a measurement rather than a tautology.
    #[test]
    fn a_profile_without_the_capability_reports_false_for_a_field_authored_surface() {
        let surfaces = all_surfaces();
        let bare = BackendCapabilityProfile::none();
        let verdicts: Vec<bool> = surfaces
            .iter()
            .map(|s| supported_by(&s.requirements(), bare))
            .collect();
        assert!(
            verdicts.iter().all(|v| !v),
            "a field-authored surface must not claim support from a profile that \
             does not attempt procedural surfaces"
        );
    }

    /// Every station's total node count sits inside the GPU backend's own
    /// 256-node shader budget — the ceiling `supported_by` deliberately does not
    /// check, because it is a property of the backend and not of the surface.
    #[test]
    fn every_station_fits_the_backends_shader_node_budget() {
        support_report(&all_surfaces()).iter().for_each(|r| {
            assert!(
                r.nodes <= 256,
                "surface {} holds {} nodes, past the 256-node shader budget",
                r.index,
                r.nodes
            );
        });
    }

    #[test]
    fn the_report_lines_name_every_surface() {
        let lines = support_lines(&all_surfaces());
        assert_eq!(lines.len(), 11);
        assert!(lines.iter().all(|l| l.contains("gpu:") && l.contains("canvas2d:")));
    }

    #[test]
    fn the_two_profiles_are_named() {
        let names: Vec<&str> = profiles().iter().map(|(name, _)| *name).collect();
        assert_eq!(names, vec!["gpu", "canvas2d"]);
    }
}
