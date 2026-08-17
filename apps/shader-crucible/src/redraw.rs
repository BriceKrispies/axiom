//! **When the crucible redraws — and why, until now, it never stopped.**
//!
//! The app drew a frame on every `requestAnimationFrame` callback, unconditionally,
//! from the moment the device bound until the tab closed. On a still camera looking
//! at a static station that is **the same image, sixty times a second**: the full
//! scene walk, the full packet translation, the full command submission, and — the
//! part that actually costs — the full fragment bill for every covered pixel, paid
//! again for a picture that is already on the screen.
//!
//! That is not a micro-inefficiency. This app's own panel measures the main pass at
//! roughly 3 ms per megapixel of covered body, so a solo'd station filling a
//! 1280x640 backbuffer burns a couple of milliseconds of GPU per frame producing
//! nothing at all. On a phone, where this investigation actually lives, it is the
//! difference between a warm device and a cool one.
//!
//! ## The rule
//!
//! > **A frame is submitted only when its pixels would differ from the pixels
//! > already on the screen.**
//!
//! The crucible's pixels are a pure function of exactly three things, and this
//! module is the proof of that claim rather than an assertion of it:
//!
//! 1. **The camera transform.** `crate::scene`'s scene has no per-tick motion —
//!    `scene::tests::a_replayed_tick_is_identical_and_a_later_one_differs` pins
//!    `app.render(n)` as a pure function of the tick, and every body's world matrix
//!    is authored once and never animated. The only thing that moves the image is
//!    the eye: an orbit gesture, or the fixed framing a `SOLO` selects.
//! 2. **The lever state.** [`crate::levers::Levers`] is the complete set of knobs
//!    the page can turn, and every one of them either cuts the draw list
//!    ([`crate::levers::PacketPlan`]), re-derives the backend's capability word, or
//!    reallocates the render target. Nothing else configures a frame.
//! 3. **The frame clock — but only when something on screen reads it.** The packet
//!    carries `crate::frame::time_at(tick)`, and a surface that binds no
//!    `FieldOp::Time` is written an exact zero whatever that says. So the tick
//!    matters to the image *if and only if* a body wearing a clock-reading surface
//!    survives the frame's plan.
//!
//! Point 3 is derived, not listed. [`clock_readers`] asks each authored `Surface`
//! for its own [`SurfaceRequirements`](axiom_surface::SurfaceRequirements) and reads
//! whether it names [`SurfaceInput::TIME`] — so a station that starts reading the
//! clock starts animating, and one that stops reading it stops, with nobody
//! remembering to edit a list. Today that set is exactly station 5's two bodies (the
//! wind and the ripple); tomorrow it is whatever the graphs say it is.
//!
//! ## The tick counter counts frames the app has drawn
//!
//! `crate::frame::time_at` is unchanged: engine time is still `tick / 60`, still a
//! count and never a wall clock, and tick *N* still produces exactly the pixels it
//! always did. What changes is *which* ticks the page reaches — the counter advances
//! on a frame the loop draws and stands still on one it skips.
//!
//! That is the deliberate choice, and the alternative — advancing the tick on every
//! callback and simply not drawing some of them — is worse in a way that matters
//! here. Under this rule the set of frames the crucible has ever presented is always
//! a **contiguous prefix** `0..=N`, so "replay this run" means "step ticks 0 to N",
//! full stop. Under the alternative it would be a sparse subset chosen by when the
//! user happened to touch the screen, which is a wall clock wearing a fake
//! moustache. The visible consequence is that a station's animation *pauses* while
//! the loop idles and resumes from where it stopped — which is honest, because the
//! app's clock is its frame counter and while it draws nothing there is nothing for
//! the counter to count.
//!
//! ## Two levers hold the loop open
//!
//! `FORCE` and `ADAPTIVE` both keep it drawing every frame, for different reasons —
//! see [`Redraw::Held`]. Everything else is decided by the rule above.
//!
//! ## What is deliberately absent, and what would have to be added with it
//!
//! **A resize is not in the identity, because this app has no resize path.** The
//! backbuffer is sized once, in `crate::web::backbuffer`, and fixed when the
//! surface is configured — which is why `DEVICE PX` is a *reload* lever. A CSS
//! resize (a rotated phone, a dragged window) therefore stretches an unchanged
//! backing store and changes no rendered pixel, so idling through one is correct
//! today. It stops being correct the moment anything reconfigures the surface at
//! runtime: whoever adds that must add the backbuffer size to [`FrameIdentity`] in
//! the same change, or the first frame after a resize will never be drawn.
//!
//! The same reasoning is the test for anything else added to a frame later. If a
//! new input can change a pixel, it belongs in [`FrameIdentity`] — and
//! `tests::every_lever_wakes_the_loop` is the shape of the assertion that proves
//! it does.

use axiom_math::Transform;
use axiom_surface::{Surface, SurfaceInput};

use crate::levers::{Levers, PacketPlan, BODY_COUNT, BODY_SURFACE};

/// **Why this frame is being drawn — or that it is not.**
///
/// The panel prints this verbatim. An instrument whose needle has stopped must say
/// whether it stopped because nothing is happening or because it broke, and this is
/// that sentence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Redraw {
    /// Nothing that decides a pixel has moved. The image on the screen is already
    /// the image this frame would produce, so the frame is not produced.
    Idle,
    /// The camera or a lever moved: this frame's image differs from the last one's.
    Changed,
    /// A body wearing a clock-reading surface survives the frame's plan, so the
    /// image genuinely differs every tick.
    Animated,
    /// A lever is holding the loop open regardless of whether the image changes.
    ///
    /// Two levers do this. `FORCE` exists so the panel can measure a steady-state
    /// frame cost — the whole reason the panel exists — on a still camera and a
    /// static station, which is precisely the configuration the rule would
    /// otherwise idle. `ADAPTIVE` does it because
    /// `axiom_host::RenderScaleController` is a closed loop over the *measured
    /// frame interval*: feed it the gap across a five-second idle and it drops
    /// straight to the ladder's floor and reports a resolution nobody asked for.
    Held,
}

impl Redraw {
    /// Whether the loop should submit this frame.
    pub fn draws(self) -> bool {
        self != Redraw::Idle
    }

    /// The sentence the panel prints.
    pub fn reason(self) -> &'static str {
        match self {
            Redraw::Idle => "IDLE — nothing that decides a pixel has moved",
            Redraw::Changed => "drawing — the camera or a lever moved",
            Redraw::Animated => "drawing — a clock-reading station is on screen",
            Redraw::Held => "drawing — held open for measurement (FORCE / ADAPTIVE)",
        }
    }
}

/// Everything a submitted frame's pixels are a function of, except the clock.
///
/// Deliberately **not** the `FrameOutcome` or the packet: those are what the frame
/// costs to produce, and comparing them would mean producing them, which is the
/// work being avoided. These are the frame's *inputs*, all of them available before
/// `RunningApp::render` is called.
#[derive(Clone, Copy, Debug, PartialEq)]
struct FrameIdentity {
    /// The transform the frame's camera will be authored with — the orbit
    /// gestures' accumulated state, or the fixed framing a `SOLO` selects.
    camera: Transform,
    /// Every knob the page can turn. A superset of [`PacketPlan`]: it also carries
    /// the two that reconfigure the backend rather than the draw list.
    levers: Levers,
}

/// **Which authored surfaces read the frame clock**, in `all_surfaces()` order.
///
/// Read off each surface's own derived requirements rather than from a list
/// somebody has to maintain: `SurfaceRequirements` is scanned out of the bound
/// graphs by `axiom_surface`, so this is the same fact the WGSL emitter acts on.
pub fn clock_readers(surfaces: &[Surface]) -> Vec<bool> {
    surfaces
        .iter()
        .map(|surface| surface.requirements().inputs().contains(SurfaceInput::TIME))
        .collect()
}

/// **Whether `plan` puts a clock-reading body on the screen.**
///
/// A body animates only if all three hold: it survives the plan's cut, it wears an
/// authored surface, and that surface still *keeps* its generated program — a body
/// past the `SURFACES` lever is handed the constant fallback pipeline, which reads
/// no clock and is therefore as still as the ground plate.
pub fn animates(plan: &PacketPlan, readers: &[bool]) -> bool {
    let animated = |body: usize| {
        BODY_SURFACE
            .get(body)
            .copied()
            .flatten()
            .is_some_and(|surface| {
                (surface < plan.surfaces) & readers.get(surface).copied().unwrap_or(false)
            })
    };
    plan.solo
        .map(animated)
        .unwrap_or_else(|| (0..BODY_COUNT).any(animated))
}

/// **The decision, and the one frame of memory it needs.**
///
/// Held by the frame loop and asked once per callback. It remembers the identity of
/// the frame that is currently on the screen; everything else it is told.
pub struct RedrawGate {
    /// Which authored surfaces read the clock — computed once, at startup, from
    /// the same surface set the barrier is handed.
    readers: Vec<bool>,
    /// The identity of the last frame the loop was told to draw. `None` before the
    /// first, which is why the first frame always draws.
    last: Option<FrameIdentity>,
}

impl RedrawGate {
    /// A gate over the authored surface set, having drawn nothing yet.
    pub fn new(surfaces: &[Surface]) -> RedrawGate {
        RedrawGate {
            readers: clock_readers(surfaces),
            last: None,
        }
    }

    /// Decide whether the frame the loop is about to build must be built.
    ///
    /// The identity is recorded unconditionally, which is correct in both arms:
    /// when the answer is [`Redraw::Idle`] the identity is by definition the one
    /// already recorded, and when it is anything else the frame is drawn.
    pub fn decide(&mut self, camera: Transform, levers: Levers) -> Redraw {
        let identity = FrameIdentity { camera, levers };
        let changed = self.last != Some(identity);
        self.last = Some(identity);
        [
            [
                [Redraw::Idle, Redraw::Changed][usize::from(changed)],
                Redraw::Held,
            ][usize::from(levers.force | levers.adaptive)],
            Redraw::Animated,
        ][usize::from(animates(&levers.plan(), &self.readers))]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stations::all_surfaces;
    use axiom_math::Vec3;

    /// A camera that is not the seeded one, for "the eye moved".
    fn moved() -> Transform {
        Transform::from_translation(Vec3::new(1.0, 2.0, 3.0))
    }

    fn gate() -> RedrawGate {
        RedrawGate::new(&all_surfaces())
    }

    /// **Station 5's two bodies are the app's only animation, and the app works
    /// that out for itself.**
    ///
    /// Not from a list of body numbers: from each surface's own derived
    /// requirements. A station that starts binding `FieldOp::Time` starts animating
    /// here without anyone editing this file — which is the difference between a
    /// derived fact and a duplicated one.
    #[test]
    fn the_clock_reading_surfaces_are_derived_from_the_graphs_themselves() {
        let readers = clock_readers(&all_surfaces());
        let reading: Vec<usize> = readers
            .iter()
            .enumerate()
            .filter(|(_, reads)| **reads)
            .map(|(index, _)| index)
            .collect();
        // Surfaces 3 and 4 are `displacement::wind_surface` and
        // `displacement::ripple_surface` — station 5's pair, and the only two of
        // the eleven that name `SurfaceInput::TIME`.
        assert_eq!(reading, vec![3, 4]);
        assert_eq!(readers.len(), crate::levers::SURFACE_COUNT);
    }

    /// **Solo-ing a static station is the case that can idle completely**, and
    /// solo-ing an animated one is the case that must not.
    #[test]
    fn a_solod_station_animates_only_when_its_own_surface_reads_the_clock() {
        let readers = clock_readers(&all_surfaces());
        let solo = |body: usize| {
            animates(
                &Levers {
                    solo: Some(body),
                    ..Levers::SHIPPING
                }
                .plan(),
                &readers,
            )
        };
        // Bodies 5 and 6 (indices 4 and 5) wear surfaces 3 and 4.
        assert!(solo(4), "the wind body must animate");
        assert!(solo(5), "the ripple body must animate");
        (0..BODY_COUNT)
            .filter(|body| (*body != 4) & (*body != 5))
            .for_each(|body| assert!(!solo(body), "body {} is not animated", body + 1));
    }

    /// With every body on the stand, station 5 is on screen, so the whole frame
    /// animates — the shipping configuration never idles.
    #[test]
    fn the_shipping_frame_animates_because_station_five_is_on_screen() {
        assert!(animates(
            &Levers::SHIPPING.plan(),
            &clock_readers(&all_surfaces())
        ));
    }

    /// **A body that has lost its generated program has lost its clock.** The
    /// `SURFACES` lever hands a body the constant fallback pipeline, which reads no
    /// time at all, so a frame cut below station 5's surfaces is genuinely static.
    #[test]
    fn a_body_cut_below_the_surfaces_lever_stops_animating() {
        let readers = clock_readers(&all_surfaces());
        // `SURFACES 3` keeps only surfaces 0..3 — station 5's are 3 and 4.
        assert!(!animates(
            &Levers {
                surfaces: Some(3),
                ..Levers::SHIPPING
            }
            .plan(),
            &readers
        ));
        // `SURFACES 6` reaches past them, so the wind and the ripple are back.
        assert!(animates(
            &Levers {
                surfaces: Some(6),
                ..Levers::SHIPPING
            }
            .plan(),
            &readers
        ));
        assert!(!animates(
            &Levers {
                surfaces: Some(0),
                ..Levers::SHIPPING
            }
            .plan(),
            &readers
        ));
    }

    /// **The first frame always draws**, and a still camera on a static station
    /// idles from the second onward — the defect this module exists to fix.
    #[test]
    fn a_still_camera_on_a_static_station_idles_after_the_first_frame() {
        let mut gate = gate();
        // Body 1 is the layered material: heavy, and completely static.
        let still = Levers {
            solo: Some(0),
            ..Levers::SHIPPING
        };
        let camera = crate::levers::solo_camera(0);
        assert_eq!(gate.decide(camera, still), Redraw::Changed);
        (0..600).for_each(|_| assert_eq!(gate.decide(camera, still), Redraw::Idle));
        assert!(!Redraw::Idle.draws());
    }

    /// **A moved camera draws immediately** — the very next decision, with nothing
    /// held back and no frame of latency.
    #[test]
    fn a_moved_camera_draws_on_the_next_frame() {
        let mut gate = gate();
        let still = Levers {
            solo: Some(0),
            ..Levers::SHIPPING
        };
        let camera = crate::levers::solo_camera(0);
        gate.decide(camera, still);
        assert_eq!(gate.decide(camera, still), Redraw::Idle);
        assert_eq!(gate.decide(moved(), still), Redraw::Changed);
        // ...and settles again the moment the gesture stops.
        assert_eq!(gate.decide(moved(), still), Redraw::Idle);
    }

    /// **A moved lever draws immediately**, for every lever there is. A knob that
    /// changed the frame but not the identity would leave a stale image on the
    /// screen, which is the one failure mode this gate can have.
    #[test]
    fn every_lever_wakes_the_loop() {
        let base = Levers {
            solo: Some(0),
            ..Levers::SHIPPING
        };
        let camera = crate::levers::solo_camera(0);
        let alternatives = [
            Levers { captions: false, ..base },
            Levers { shadows: false, ..base },
            Levers { surfaces: Some(3), ..base },
            Levers { solo: Some(1), ..base },
            Levers { half_res: true, ..base },
            Levers { adaptive: true, ..base },
            Levers { force: true, ..base },
            Levers { device_pixels: false, ..base },
            Levers { back: Some((640, 320)), ..base },
        ];
        alternatives.iter().for_each(|moved| {
            let mut gate = gate();
            gate.decide(camera, base);
            assert_eq!(gate.decide(camera, base), Redraw::Idle);
            assert!(
                gate.decide(camera, *moved).draws(),
                "a lever moved and the loop stayed asleep: {moved:?}"
            );
        });
    }

    /// **An animated station keeps the loop running forever**, still camera or not
    /// — the frame really is different every tick, and skipping it would freeze
    /// station 5 rather than save anything.
    #[test]
    fn an_animated_station_never_idles() {
        let mut gate = gate();
        let animated = Levers {
            solo: Some(4),
            ..Levers::SHIPPING
        };
        let camera = crate::levers::solo_camera(4);
        (0..600).for_each(|_| assert_eq!(gate.decide(camera, animated), Redraw::Animated));
    }

    /// **FORCE and ADAPTIVE hold the loop open**, which is what makes the panel's
    /// steady-state numbers obtainable on a configuration that would otherwise
    /// idle, and what keeps the adaptive controller from reading an idle as a
    /// stall.
    #[test]
    fn force_and_adaptive_hold_the_loop_open() {
        [
            Levers {
                solo: Some(0),
                force: true,
                ..Levers::SHIPPING
            },
            Levers {
                solo: Some(0),
                adaptive: true,
                ..Levers::SHIPPING
            },
        ]
        .iter()
        .for_each(|held| {
            let mut gate = gate();
            let camera = crate::levers::solo_camera(0);
            gate.decide(camera, *held);
            (0..120).for_each(|_| assert_eq!(gate.decide(camera, *held), Redraw::Held));
        });
    }

    /// Every reason says whether it draws, and only one of the four does not.
    #[test]
    fn only_idle_skips_the_frame() {
        assert!(!Redraw::Idle.draws());
        [Redraw::Changed, Redraw::Animated, Redraw::Held]
            .iter()
            .for_each(|reason| assert!(reason.draws()));
        [
            Redraw::Idle,
            Redraw::Changed,
            Redraw::Animated,
            Redraw::Held,
        ]
        .iter()
        .for_each(|reason| assert!(!reason.reason().is_empty()));
        assert!(Redraw::Idle.reason().contains("IDLE"));
    }
}
