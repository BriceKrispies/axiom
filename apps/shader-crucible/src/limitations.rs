//! **The four things this system does not do**, stated as data so the same
//! sentences reach the page, the console and the README, and cannot drift apart.
//!
//! A demonstration that quietly avoids the broken cases is worse than no
//! demonstration. Each of these is next to the station it affects — on the
//! page's legend, in `crate::report`, and in this app's `README.md` — and none
//! of them is fixed here. Fixing them is engine work; **naming them is the job
//! this app was built for**.

/// One limitation: what breaks, which station shows it, and why it is not
/// worked around.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limitation {
    /// `1..=4`.
    pub number: u8,
    /// The one-line statement.
    pub headline: &'static str,
    /// Which station a viewer sees it at.
    pub station: u8,
    /// Why it is not hidden, and what fixing it would take.
    pub detail: &'static str,
}

/// The four, in the order the manifest states them.
pub const LIMITATIONS: [Limitation; 4] = [
    Limitation {
        number: 1,
        headline: "A displaced vertex casts an UNDISPLACED shadow.",
        station: 5,
        detail: "The shadow depth pre-pass is a separate WGSL module and runs no \
                 displacement program, so the depth it writes is the depth of the \
                 undeformed mesh. Station 5 is lit at a low angle onto a bright \
                 ground and its amplitude is deliberately large, so the gap between \
                 a leaning body and its upright shadow is visible in the frame. \
                 Fixing it means teaching the shadow pass to run the vertex program \
                 — an engine change, not an app one.",
    },
    Limitation {
        number: 2,
        headline: "Skinned geometry always gets the default program.",
        station: 5,
        detail: "SkinnedGpuDraw carries no surface_program lane at all, and the \
                 skinned vertex stage binds all 16 vertex attributes a WebGL2 \
                 downlevel target guarantees — the ceiling that already costs a \
                 skinned material its emissive and its specular — so it runs no \
                 displacement program either. The crucible therefore shows NO \
                 skinned body: there is no such thing as a surfaced one, and \
                 rendering an unsurfaced figure beside nine surfaced ones would \
                 read as a bug rather than as a limitation. What the app does show \
                 is the backend's own answer: the barrier records \
                 skinned_surface_degradations for its displacing stations, and it \
                 is non-empty.",
    },
    Limitation {
        number: 3,
        headline: "Canvas2D shades ONE sample per triangle.",
        station: 9,
        detail: "The software rasterizer executes no shader; it evaluates each \
                 channel once per triangle, at that triangle's object-space \
                 centroid. That is a substitute, not a drop — but a mask finer \
                 than a triangle is not sampled at all, so station 1's scratch \
                 lines can vanish there entirely. The meshes are deliberately NOT \
                 tessellated to hide it: subdividing until the software arm \
                 resolved the scratches would be measuring a mesh instead of a \
                 backend. Two further gaps the capture makes visible, neither \
                 of them about surfaces: the software 3D path samples NO albedo \
                 texture (load_textures feeds the 2D sprite path only, and the \
                 frame reports FrameFeature::AlbedoSampling), so station 3's \
                 baked tile renders untextured there; and its lighting is a \
                 hemisphere ambient plus one directional term applied linearly, \
                 with no point light and no tone mapping, so the same authored \
                 rig is markedly darker than on the GPU.",
    },
    Limitation {
        number: 4,
        headline: "metallic changes no pixel.",
        station: 1,
        detail: "SurfaceChannel::Metallic is a channel, not a BRDF: it is carried, \
                 digested and reported, and no lighting model reads it. Station 1's \
                 base binds it to 1.0 and its paint and dirt bind it to 0.0, and \
                 moving any of them moves nothing on screen. It is labelled rather \
                 than omitted, because a demo that shows a channel without saying \
                 it is inert is a demo that lies.",
    },
];

/// The limitations as the lines the page legend and the console report print.
pub fn limitation_lines() -> Vec<String> {
    LIMITATIONS
        .iter()
        .map(|l| {
            format!(
                "limitation {} (station {}): {} {}",
                l.number, l.station, l.headline, l.detail
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn there_are_four_numbered_limitations_each_bound_to_a_station() {
        assert_eq!(LIMITATIONS.len(), 4);
        LIMITATIONS.iter().enumerate().for_each(|(index, l)| {
            assert_eq!(l.number as usize, index + 1);
            assert!((1..=10).contains(&l.station));
            assert!(!l.headline.is_empty());
            assert!(l.detail.len() > 80, "a one-word excuse is not a statement");
        });
    }

    /// **Nothing here promises a fix.** These are statements of what does not
    /// work, and an app that quietly implied it had solved one would be worse
    /// than one that said nothing.
    #[test]
    fn every_limitation_says_what_it_would_take_rather_than_claiming_a_fix() {
        assert!(LIMITATIONS[0].detail.contains("engine change"));
        assert!(LIMITATIONS[1].detail.contains("no surface_program lane"));
        assert!(LIMITATIONS[2].detail.contains("deliberately NOT"));
        assert!(LIMITATIONS[3].detail.contains("labelled rather than omitted"));
    }

    #[test]
    fn the_lines_carry_every_headline() {
        let lines = limitation_lines();
        assert_eq!(lines.len(), 4);
        LIMITATIONS.iter().zip(lines.iter()).for_each(|(l, line)| {
            assert!(line.contains(l.headline));
        });
    }
}
