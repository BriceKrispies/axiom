//! **Can this frame differ from the one already on the screen?**
//!
//! A frame that is pixel-for-pixel the picture already being displayed costs
//! the full fragment bill to produce nothing. That bill is the dominant one:
//! a main pass runs at roughly a fixed setup cost plus a few milliseconds per
//! megapixel *covered*, so re-submitting an identical frame at 60 Hz on a phone
//! is a continuous, invisible burn. This module is the engine's answer to the
//! question in the title, so that no app — not just the one that noticed —
//! redraws a frame that cannot have changed.
//!
//! ## Where the answer comes from, and why it is complete
//!
//! [`FramePacket`] is not *a* description of the frame; it is *the* description
//! of the frame — the single artifact every render backend consumes. Whatever
//! moved upstream (a transform, the camera, a light, the authored sky, a
//! material, the clock, the render scale) either changes the packet or does not
//! reach the pixels. So the honest test is not a hand-written checklist of
//! "inputs that dirty a frame", which would silently rot the first time somebody
//! adds a field. It is: **is this packet the packet we last presented?**
//!
//! [`FramePacket::presents_identically_to`] answers that, and it is written so
//! it cannot rot. It rebases the two packets onto a common bookkeeping stamp and
//! then defers to the *derived* `PartialEq` — so a field added to `FramePacket`
//! tomorrow is compared tomorrow, with no edit here and no way to forget.
//!
//! ## The two lanes that are bookkeeping, not pixels
//!
//! `frame_index` and `tick` count frames. No backend reads either one to decide
//! a pixel — they reach the submission *report* and nothing else — and both
//! advance every frame by construction, so including them would make every
//! frame "different" and the whole question unanswerable. They are the only two
//! fields rebased away.
//!
//! Note what is **not** rebased away: `time`. The presentation clock is what a
//! time-varying authored surface samples, so a frame whose packet carries a
//! moving time genuinely produces moving pixels and is redrawn, every time. The
//! distinction is exactly right: `tick` is how the engine counts, `time` is what
//! the image reads.
//!
//! ## What the packet cannot see — [`FrameRevision`]
//!
//! A packet names its meshes and materials by id, not by content. Re-upload a
//! texture under an id the packet already carries and the pixels change while
//! the packet does not. The same is true of every fact that lives in the backend
//! rather than in the frame: the GPU device and its swapchain (a context loss
//! and rebuild leaves a blank surface), the render scale in force, a
//! resize/reconfigure the frame's own viewport has not caught up with, joint
//! palettes and any other per-frame stream a backend takes alongside the packet.
//!
//! Those are not guessed at. They are named, by the caller, as a
//! [`FrameRevision`] — a monotonic counter of "everything outside the packet".
//! A caller that changes one of them bumps the revision, and the next frame is
//! redrawn. Making the blind spot an explicit parameter is the point: it cannot
//! be forgotten quietly, because a caller has to pass *something*.
//!
//! ## The bias, stated once
//!
//! Skipping a frame that should have drawn is visual corruption. Drawing a frame
//! that need not have is merely the status quo. Every judgement call here leans
//! the second way:
//!
//! - a ledger with nothing recorded redraws ([`RedrawVerdict::FirstFrame`]);
//! - a `NaN` anywhere in the packet compares unequal to itself, so a frame
//!   carrying one always redraws;
//! - `-0.0` and `0.0` have different bit patterns but compare *equal*, and they
//!   shade identically, so that one is not even a false positive;
//! - any revision change at all redraws, whether or not the packet moved.
//!
//! ## Ticks, and why replay is untouched
//!
//! **The ledger never advances, holds back, or observes a tick.** It is asked
//! about a packet that has already been built and answers whether to hand it to
//! a backend. Simulation stepping is exactly as it was: tick *N* is stepped when
//! it was always stepped, and `time_at(tick)` still derives the same seconds
//! from the same counter, so tick *N* looks like tick *N* has always looked.
//! What a skipped frame skips is the *presentation* of a tick, never the tick.
//!
//! That is not merely a safe rule, it is a free one, because of a small theorem:
//! *a frame can only be skipped when its clock is not deciding its pixels.* If
//! the packet's `time` moves, the packet differs and the frame is drawn. So no
//! animation can ever be frozen by the ledger, and there is no reason to couple
//! it to the tick counter to protect one.
//!
//! A replay that steps and presents every tick is byte-identical to one that
//! ever ran, because the ledger changes *whether* a packet is submitted and
//! never *what* is in it.

use super::FramePacket;

impl FramePacket {
    /// **Would presenting this packet put the same pixels on the screen as
    /// presenting `other`?**
    ///
    /// Every field that decides a pixel is compared; `frame_index` and `tick`,
    /// which decide none and advance every frame, are not (see the module docs).
    ///
    /// Deliberately implemented by *rebasing and deferring to derived
    /// equality*, rather than by listing the fields that matter. A field added
    /// to [`FramePacket`] later is compared automatically, so this cannot fall
    /// out of date — the one failure mode that would turn a saved frame into a
    /// stale image on a user's screen.
    ///
    /// Float comparison is exact, and exactness is the safe direction here: a
    /// packet carrying a `NaN` is unequal to itself and so is always redrawn.
    pub fn presents_identically_to(&self, other: &FramePacket) -> bool {
        let rebased = FramePacket {
            frame_index: other.frame_index,
            tick: other.tick,
            ..self.clone()
        };
        &rebased == other
    }
}

/// **The version of everything a [`FramePacket`] cannot carry.**
///
/// A packet names a mesh and a material by id; it does not carry their bytes.
/// It describes a frame; it does not describe the device that will draw it. So
/// a re-uploaded texture, a rebuilt GPU context, a changed render scale, a
/// reconfigured surface, or a per-frame side-channel a backend consumes
/// alongside the packet all change the image with the packet standing still.
///
/// This is the caller's handle on exactly that set: a counter it bumps whenever
/// one of those moves. [`PresentationLedger::admit`] redraws on any change to
/// it, so a caller that bumps too eagerly loses nothing but the saving, while
/// one that never bumps at all gets today's behaviour.
///
/// It is deliberately a plain, opaque counter rather than an enumeration of
/// causes. The set of things that live outside a frame packet is open — it grows
/// every time a backend gains resident state — and a closed enum here would be a
/// promise this layer cannot keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct FrameRevision(u64);

impl FrameRevision {
    /// The revision nothing has been changed under yet.
    pub const ORIGIN: FrameRevision = FrameRevision(0);

    /// A revision with an explicit count — for a caller that already keeps a
    /// generation number for its uploaded resources and would rather mix its own
    /// than track a second one.
    pub const fn new(revision: u64) -> FrameRevision {
        FrameRevision(revision)
    }

    /// The raw count.
    pub const fn get(self) -> u64 {
        self.0
    }

    /// The next revision: *something outside the packet changed.*
    ///
    /// Wrapping, because the only thing ever asked of the count is whether two
    /// readings differ, and a wrap that lands back on the immediately preceding
    /// value would take 2^64 bumps to arrange.
    pub const fn bumped(self) -> FrameRevision {
        FrameRevision(self.0.wrapping_add(1))
    }
}

/// **Why this frame is being drawn — or that it is not.**
///
/// The variants are ordered so that only the last one skips, which is what makes
/// [`Self::draws`] a comparison rather than a branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RedrawVerdict {
    /// Nothing has been presented through this ledger yet, so there is no image
    /// on the screen to be identical to. The first frame always draws.
    FirstFrame,
    /// The packet differs from the presented one in a field that decides a
    /// pixel.
    ContentChanged,
    /// The packet is identical, but the [`FrameRevision`] moved: something the
    /// packet cannot see — a re-uploaded texture, a rebuilt device, a changed
    /// render scale — did change the image.
    ExternalChange,
    /// The packet would present exactly the pixels already on the screen, and
    /// nothing outside it moved. **This is the frame worth not drawing.**
    Unchanged,
}

/// The sentence each verdict prints, indexed by the verdict itself. A table
/// rather than a `match`, so the mapping is data.
const REASONS: [&str; 4] = [
    "drawing — nothing has been presented yet",
    "drawing — the frame's content changed",
    "drawing — something outside the frame packet changed",
    "idle — this frame's pixels are already on the screen",
];

impl RedrawVerdict {
    /// **Whether the caller should submit this frame.** True for everything but
    /// [`RedrawVerdict::Unchanged`].
    pub const fn draws(self) -> bool {
        (self as u8) != (RedrawVerdict::Unchanged as u8)
    }

    /// A one-line explanation, for a diagnostic overlay or a log. An instrument
    /// whose needle has stopped must be able to say whether it stopped because
    /// nothing is happening or because it broke.
    pub const fn reason(self) -> &'static str {
        REASONS[self as usize]
    }
}

/// The verdict for each `(nothing presented, revision moved, packet identical)`
/// combination, indexed `first * 4 + external * 2 + identical`.
///
/// The two entries marked unreachable cannot occur — "nothing presented" and
/// "identical to what was presented" are contradictory — and are filled with
/// [`RedrawVerdict::FirstFrame`] so that even a future mistake in the index
/// arithmetic errs toward drawing.
const VERDICTS: [RedrawVerdict; 8] = [
    RedrawVerdict::ContentChanged,
    RedrawVerdict::Unchanged,
    RedrawVerdict::ExternalChange,
    RedrawVerdict::ExternalChange,
    RedrawVerdict::FirstFrame,
    RedrawVerdict::FirstFrame, // unreachable
    RedrawVerdict::FirstFrame,
    RedrawVerdict::FirstFrame, // unreachable
];

/// **What the ledger decided, and the ledger that decided it.**
///
/// Returned by value with the *next* ledger inside it, rather than mutating one
/// in place: the ledger is state the caller owns and hands back, so the same
/// ledger asked the same question always gives the same answer, and a frame loop
/// is replayable by replaying its packets.
#[derive(Debug, Clone, PartialEq)]
pub struct RedrawDecision {
    verdict: RedrawVerdict,
    ledger: PresentationLedger,
}

impl RedrawDecision {
    /// Why this frame is, or is not, being drawn.
    pub const fn verdict(&self) -> RedrawVerdict {
        self.verdict
    }

    /// Whether the caller should submit this frame.
    pub const fn draws(&self) -> bool {
        self.verdict.draws()
    }

    /// The ledger to carry into the next frame.
    pub fn into_ledger(self) -> PresentationLedger {
        self.ledger
    }
}

/// **The record of what is currently on the screen.**
///
/// One frame of memory, owned by whoever owns the frame loop: the packet last
/// admitted for presentation, and the [`FrameRevision`] it was admitted under.
/// It is a plain value — state in, state out — so a loop that holds one is as
/// replayable as one that does not, and a test can drive a hundred frames
/// through it without a browser, a device, or a clock.
///
/// **Opting in is a deliberate act.** A loop that never builds a ledger keeps
/// presenting every frame, exactly as it always did; there is no ambient
/// default that could start skipping frames behind an app's back.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PresentationLedger {
    /// The packet last admitted for presentation, or `None` before the first —
    /// which is why the first frame always draws.
    presented: Option<FramePacket>,
    /// The revision that packet was admitted under.
    revision: FrameRevision,
}

impl PresentationLedger {
    /// A ledger that has presented nothing. Its first [`Self::admit`] draws.
    pub const fn new() -> PresentationLedger {
        PresentationLedger {
            presented: None,
            revision: FrameRevision::ORIGIN,
        }
    }

    /// The revision the last admitted frame was presented under.
    pub const fn revision(&self) -> FrameRevision {
        self.revision
    }

    /// **Decide whether `packet` must be presented**, given `revision` — the
    /// current version of everything the packet cannot carry (see
    /// [`FrameRevision`]).
    ///
    /// The returned [`RedrawDecision`] carries the next ledger. It records this
    /// packet when the frame draws, and keeps the one already recorded when it
    /// does not — which is free rather than merely equivalent, because a
    /// declined packet is by definition identical to the recorded one, so
    /// storing it would buy nothing and cost a clone. The ledger always names
    /// the pixels that are actually on the screen.
    pub fn admit(self, packet: &FramePacket, revision: FrameRevision) -> RedrawDecision {
        let first = self.presented.is_none();
        let outside_moved = self.revision != revision;
        let identical = self
            .presented
            .as_ref()
            .is_some_and(|presented| presented.presents_identically_to(packet));
        let verdict = VERDICTS[usize::from(first) * 4
            + usize::from(outside_moved) * 2
            + usize::from(identical)];
        let redraws = verdict.draws();
        RedrawDecision {
            verdict,
            ledger: PresentationLedger {
                // Cloned only on a frame that actually draws: when the verdict
                // is `Unchanged` the packet already recorded is, by definition,
                // the one that would have been stored.
                presented: self
                    .presented
                    .filter(|_| !redraws)
                    .or_else(|| Some(packet.clone())),
                revision,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame_camera::FrameCamera;
    use crate::frame_packet::{FrameDrawItem, FrameFeatureSet, FrameLight, FrameViewport};
    use axiom_kernel::{Ratio, Seconds};

    fn mat(seed: f32) -> [f32; 16] {
        [seed; 16]
    }

    /// A packet with every mandatory lane populated, at frame 4 / tick 240.
    fn packet() -> FramePacket {
        FramePacket::new(
            4,
            240,
            FrameViewport::new(800, 600),
            [0.1, 0.2, 0.3, 1.0],
            Some(FrameCamera::new(mat(1.0), mat(2.0), mat(3.0))),
            vec![FrameDrawItem::new(
                7,
                11,
                13,
                mat(9.0),
                mat(5.0),
                [0.4, 0.5, 0.6, 1.0],
                false,
            )],
            vec![FrameLight::new(0, [0.0, -1.0, 0.0], [1.0, 1.0, 1.0, 1.0])],
            mat(7.0),
            FrameFeatureSet::new(false, true, 1, 0),
        )
    }

    /// The same packet, one frame later: the two bookkeeping lanes advanced and
    /// nothing else. This is what an idling loop actually hands the ledger.
    fn next_frame(of: &FramePacket) -> FramePacket {
        FramePacket::new(
            of.frame_index() + 1,
            of.tick() + 1,
            of.viewport(),
            of.clear_color(),
            of.camera(),
            of.draws().to_vec(),
            of.lights().to_vec(),
            of.light_view_proj(),
            of.features(),
        )
    }

    // -- the bookkeeping lanes ------------------------------------------------

    /// **The frame counter is not the frame.** Two packets that differ only in
    /// `frame_index` and `tick` present the same pixels — which is the whole
    /// premise, because those two advance on every frame ever submitted.
    #[test]
    fn the_two_bookkeeping_lanes_do_not_decide_a_pixel() {
        let base = packet();
        let later = next_frame(&base);
        assert_ne!(later, base, "the packets are genuinely different values");
        assert!(later.presents_identically_to(&base));
        assert!(base.presents_identically_to(&later));
        // ...and a packet is identical to itself, in either direction.
        assert!(base.presents_identically_to(&base.clone()));
    }

    // -- every input that must force a redraw ---------------------------------

    /// **Every field of the packet that decides a pixel forces a redraw.**
    ///
    /// One case per lane, and the lanes are the engine's whole list of frame
    /// inputs: viewport (resize / render scale), clear colour, camera, the draw
    /// list (a transform, a spawn, a despawn, a material, an authored surface
    /// program, a contact-shadow flag), the lights, the shadow projection, the
    /// feature word, the SDF scene, and every authored-look attachment
    /// (volumetrics, ambient, depth fog, grade, sky, bloom, retro profile), plus
    /// the presentation clock.
    #[test]
    fn every_pixel_deciding_field_of_the_packet_forces_a_redraw() {
        let base = packet();
        let draw = |item: FrameDrawItem| {
            FramePacket::new(
                base.frame_index(),
                base.tick(),
                base.viewport(),
                base.clear_color(),
                base.camera(),
                vec![item],
                base.lights().to_vec(),
                base.light_view_proj(),
                base.features(),
            )
        };
        let moved = FrameDrawItem::new(7, 11, 13, mat(9.5), mat(5.0), [0.4, 0.5, 0.6, 1.0], false);
        let sdf = crate::sdf_scene::SdfScene::new(
            vec![crate::sdf_scene::SdfPrimitive::new(
                crate::sdf_scene::SdfPrimitive::SPHERE,
                mat(1.0),
                [0.5, 0.0, 0.0, 1.0],
                [1.0, 0.0, 0.0, 1.0],
            )],
            mat(2.0),
            mat(3.0),
            [0.0, 0.0, 5.0],
            [100.0, 0.001, 0.0, 0.0],
        );
        let sky = crate::frame_sky::FrameSky::gradient([0.2, 0.4, 0.9], [0.7, 0.8, 1.0]);
        let bloom = crate::frame_bloom::FrameBloom::new(0.8, 0.2, 0.5, 1.0);
        // The look attachments read back off the packet as attached — the
        // accessors a backend consults to decide what it can honour, and the
        // reason attaching one is a change the ledger must see.
        assert_eq!(base.clone().with_sky(sky).sky(), Some(&sky));
        assert_eq!(base.clone().with_bloom(bloom).bloom(), Some(&bloom));
        let variants: Vec<(&str, FramePacket)> = vec![
            // -- resize / render scale --
            (
                "viewport",
                FramePacket::new(
                    base.frame_index(),
                    base.tick(),
                    FrameViewport::new(640, 480),
                    base.clear_color(),
                    base.camera(),
                    base.draws().to_vec(),
                    base.lights().to_vec(),
                    base.light_view_proj(),
                    base.features(),
                ),
            ),
            // -- the authored background --
            (
                "clear colour",
                FramePacket::new(
                    base.frame_index(),
                    base.tick(),
                    base.viewport(),
                    [0.9, 0.2, 0.3, 1.0],
                    base.camera(),
                    base.draws().to_vec(),
                    base.lights().to_vec(),
                    base.light_view_proj(),
                    base.features(),
                ),
            ),
            // -- the camera --
            (
                "camera",
                FramePacket::new(
                    base.frame_index(),
                    base.tick(),
                    base.viewport(),
                    base.clear_color(),
                    Some(FrameCamera::new(mat(1.5), mat(2.0), mat(3.0))),
                    base.draws().to_vec(),
                    base.lights().to_vec(),
                    base.light_view_proj(),
                    base.features(),
                ),
            ),
            (
                "camera removed",
                FramePacket::new(
                    base.frame_index(),
                    base.tick(),
                    base.viewport(),
                    base.clear_color(),
                    None,
                    base.draws().to_vec(),
                    base.lights().to_vec(),
                    base.light_view_proj(),
                    base.features(),
                ),
            ),
            // -- the scene: a moved transform, a despawn, a spawn --
            ("a transform moved", draw(moved)),
            (
                "a node despawned",
                FramePacket::new(
                    base.frame_index(),
                    base.tick(),
                    base.viewport(),
                    base.clear_color(),
                    base.camera(),
                    Vec::new(),
                    base.lights().to_vec(),
                    base.light_view_proj(),
                    base.features(),
                ),
            ),
            (
                "a node spawned",
                FramePacket::new(
                    base.frame_index(),
                    base.tick(),
                    base.viewport(),
                    base.clear_color(),
                    base.camera(),
                    [base.draws(), base.draws()].concat(),
                    base.lights().to_vec(),
                    base.light_view_proj(),
                    base.features(),
                ),
            ),
            // -- materials, on the draw --
            (
                "a material id",
                draw(FrameDrawItem::new(
                    7,
                    11,
                    14,
                    mat(9.0),
                    mat(5.0),
                    [0.4, 0.5, 0.6, 1.0],
                    false,
                )),
            ),
            (
                "a material colour",
                draw(FrameDrawItem::new(
                    7,
                    11,
                    13,
                    mat(9.0),
                    mat(5.0),
                    [0.9, 0.5, 0.6, 1.0],
                    false,
                )),
            ),
            (
                "a material emissive",
                draw(base.draws()[0].with_emissive([1.0, 0.0, 0.0])),
            ),
            (
                "a material specular",
                draw(base.draws()[0].with_specular(Ratio::finite_or_zero(0.5))),
            ),
            (
                "an authored surface program",
                draw(base.draws()[0].with_surface_program(9)),
            ),
            (
                "a mesh id",
                draw(FrameDrawItem::new(
                    7,
                    12,
                    13,
                    mat(9.0),
                    mat(5.0),
                    [0.4, 0.5, 0.6, 1.0],
                    false,
                )),
            ),
            (
                "a contact-shadow flag",
                draw(FrameDrawItem::new(
                    7,
                    11,
                    13,
                    mat(9.0),
                    mat(5.0),
                    [0.4, 0.5, 0.6, 1.0],
                    true,
                )),
            ),
            // -- the lights and the shadow projection --
            (
                "a light",
                FramePacket::new(
                    base.frame_index(),
                    base.tick(),
                    base.viewport(),
                    base.clear_color(),
                    base.camera(),
                    base.draws().to_vec(),
                    vec![FrameLight::new(0, [1.0, -1.0, 0.0], [1.0, 1.0, 1.0, 1.0])],
                    base.light_view_proj(),
                    base.features(),
                ),
            ),
            (
                "the shadow projection",
                FramePacket::new(
                    base.frame_index(),
                    base.tick(),
                    base.viewport(),
                    base.clear_color(),
                    base.camera(),
                    base.draws().to_vec(),
                    base.lights().to_vec(),
                    mat(8.0),
                    base.features(),
                ),
            ),
            (
                "the feature word",
                FramePacket::new(
                    base.frame_index(),
                    base.tick(),
                    base.viewport(),
                    base.clear_color(),
                    base.camera(),
                    base.draws().to_vec(),
                    base.lights().to_vec(),
                    base.light_view_proj(),
                    FrameFeatureSet::new(true, true, 1, 0),
                ),
            ),
            // -- the raymarched peer of the triangles --
            ("an SDF scene", base.clone().with_sdf(sdf)),
            // -- the authored look, attachment by attachment --
            (
                "volumetrics",
                base.clone()
                    .with_volumetrics(crate::frame_volumetrics::FrameVolumetrics::new(
                        8,
                        0.5,
                        0.9,
                        0.4,
                        1.0,
                        0.6,
                        [1.0, 0.9, 0.7],
                    )),
            ),
            (
                "ambient",
                base.clone()
                    .with_ambient(crate::frame_ambient::FrameAmbient::new(
                        [0.6, 0.7, 0.8],
                        [0.2, 0.15, 0.1],
                    )),
            ),
            (
                "depth fog",
                base.clone()
                    .with_depth_fog(crate::frame_depth_fog::FrameDepthFog::new(
                        Ratio::finite_or_zero(0.97),
                        Ratio::finite_or_zero(1.0),
                        Ratio::finite_or_zero(0.9),
                        [0.02, 0.03, 0.07],
                    )),
            ),
            (
                "the grade",
                base.clone()
                    .with_postprocess(crate::frame_postprocess::FramePostProcess::cinematic()),
            ),
            ("the sky", base.clone().with_sky(sky)),
            ("bloom", base.clone().with_bloom(bloom)),
            (
                "the retro profile",
                base.clone().with_retro_32bit_profile(
                    crate::frame_retro_32bit::FrameRetro32BitProfile::retro_32bit(),
                ),
            ),
            // -- the clock a time-varying authored surface reads --
            (
                "the presentation clock",
                base.clone().with_time(Seconds::finite_or_zero(1.5)),
            ),
        ];

        variants.iter().for_each(|(what, variant)| {
            assert!(
                !variant.presents_identically_to(&base),
                "{what} changed the frame and the packet comparison missed it"
            );
            // ...and it wakes a settled ledger on the very next frame, with no
            // frame of latency.
            let settled = PresentationLedger::new()
                .admit(&base, FrameRevision::ORIGIN)
                .into_ledger();
            assert!(!settled
                .clone()
                .admit(&next_frame(&base), FrameRevision::ORIGIN)
                .draws());
            let decision = settled.admit(variant, FrameRevision::ORIGIN);
            assert!(decision.draws(), "{what} left the loop asleep");
            assert_eq!(decision.verdict(), RedrawVerdict::ContentChanged, "{what}");
        });
    }

    /// **The one input the packet cannot see forces a redraw too.** A re-uploaded
    /// texture, a rebuilt GPU device, a changed render scale, a reconfigured
    /// surface, a per-frame stream a backend takes alongside the packet: all of
    /// them arrive as a bumped revision, and all of them draw.
    #[test]
    fn a_bumped_revision_forces_a_redraw_with_an_identical_packet() {
        let base = packet();
        let ledger = PresentationLedger::new()
            .admit(&base, FrameRevision::ORIGIN)
            .into_ledger();
        assert_eq!(ledger.revision(), FrameRevision::ORIGIN);

        let decision = ledger.admit(&next_frame(&base), FrameRevision::ORIGIN.bumped());
        assert!(decision.draws());
        assert_eq!(decision.verdict(), RedrawVerdict::ExternalChange);
        let ledger = decision.into_ledger();
        assert_eq!(ledger.revision(), FrameRevision::new(1));

        // Settled again at the new revision.
        assert!(!ledger
            .clone()
            .admit(&next_frame(&base), FrameRevision::new(1))
            .draws());
        // A revision that moves while the content moves too is still a redraw —
        // reported as the external change, which is the stronger fact.
        let both = ledger.admit(
            &base.clone().with_time(Seconds::finite_or_zero(3.0)),
            FrameRevision::new(2),
        );
        assert_eq!(both.verdict(), RedrawVerdict::ExternalChange);
        assert!(both.draws());
    }

    // -- the decision itself --------------------------------------------------

    /// **The first frame always draws**, and a genuinely unchanged frame idles
    /// from the second onward — for as long as it stays unchanged. This is the
    /// defect the whole module exists to fix.
    #[test]
    fn an_unchanged_frame_idles_forever_after_the_first() {
        let base = packet();
        let first = PresentationLedger::new().admit(&base, FrameRevision::ORIGIN);
        assert!(first.draws());
        assert_eq!(first.verdict(), RedrawVerdict::FirstFrame);

        let idled = (0..600).fold(first.into_ledger(), |ledger, frame| {
            let decision = ledger.admit(&next_frame(&base), FrameRevision::ORIGIN);
            assert!(!decision.draws(), "frame {frame} redrew an identical image");
            assert_eq!(decision.verdict(), RedrawVerdict::Unchanged);
            decision.into_ledger()
        });
        // Six hundred idle frames later the ledger still knows what is on the
        // screen — an idle frame must not quietly forget it.
        assert!(!idled.admit(&base, FrameRevision::ORIGIN).draws());
    }

    /// **A ledger whose first frame lands on a non-origin revision still draws
    /// it** — the "nothing presented yet" fact outranks everything, which is the
    /// bias toward drawing made concrete (the `first * 4` lane of the table).
    #[test]
    fn the_first_frame_draws_whatever_the_revision_says() {
        let base = packet();
        let decision = PresentationLedger::new().admit(&base, FrameRevision::new(42));
        assert_eq!(decision.verdict(), RedrawVerdict::FirstFrame);
        assert!(decision.draws());
        assert_eq!(decision.into_ledger().revision(), FrameRevision::new(42));
    }

    /// **A frame carrying a `NaN` is never equal to itself, so it always draws.**
    /// The safe direction: a packet the engine cannot reason about is one it
    /// redraws.
    #[test]
    fn a_packet_carrying_a_nan_always_redraws() {
        let mut nan = mat(0.0);
        nan[5] = f32::NAN;
        let base = FramePacket::new(
            0,
            0,
            FrameViewport::new(4, 4),
            [0.0; 4],
            Some(FrameCamera::new(nan, mat(1.0), mat(1.0))),
            Vec::new(),
            Vec::new(),
            mat(0.0),
            FrameFeatureSet::new(false, false, 0, 0),
        );
        assert!(!base.presents_identically_to(&base));
        let ledger = PresentationLedger::new()
            .admit(&base, FrameRevision::ORIGIN)
            .into_ledger();
        let decision = ledger.admit(&base, FrameRevision::ORIGIN);
        assert!(decision.draws());
        assert_eq!(decision.verdict(), RedrawVerdict::ContentChanged);
    }

    /// **A negative zero shades exactly like a zero**, and compares equal to one,
    /// so it is not even a false positive — worth pinning, because a bitwise
    /// fingerprint would have got this wrong.
    #[test]
    fn a_negative_zero_is_not_a_change() {
        let base = packet();
        let signed = FramePacket::new(
            base.frame_index(),
            base.tick(),
            base.viewport(),
            [-0.0, 0.2, 0.3, 1.0],
            base.camera(),
            base.draws().to_vec(),
            base.lights().to_vec(),
            base.light_view_proj(),
            base.features(),
        );
        let zeroed = FramePacket::new(
            base.frame_index(),
            base.tick(),
            base.viewport(),
            [0.0, 0.2, 0.3, 1.0],
            base.camera(),
            base.draws().to_vec(),
            base.lights().to_vec(),
            base.light_view_proj(),
            base.features(),
        );
        assert!(signed.presents_identically_to(&zeroed));
    }

    // -- the value vocabulary -------------------------------------------------

    /// Every verdict says whether it draws and why, and exactly one of the four
    /// skips.
    #[test]
    fn only_the_unchanged_verdict_skips_the_frame() {
        let all = [
            RedrawVerdict::FirstFrame,
            RedrawVerdict::ContentChanged,
            RedrawVerdict::ExternalChange,
            RedrawVerdict::Unchanged,
        ];
        let drawing: Vec<RedrawVerdict> = all.iter().copied().filter(|v| v.draws()).collect();
        assert_eq!(
            drawing,
            vec![
                RedrawVerdict::FirstFrame,
                RedrawVerdict::ContentChanged,
                RedrawVerdict::ExternalChange
            ]
        );
        assert!(!RedrawVerdict::Unchanged.draws());
        all.iter().for_each(|verdict| {
            assert!(!verdict.reason().is_empty());
            assert!(format!("{verdict:?}").contains(match_free_name(*verdict)));
        });
        assert!(RedrawVerdict::Unchanged.reason().starts_with("idle"));
        // Ordered, hashable, comparable — it is a value, and a diagnostic panel
        // may want to sort or key on it.
        assert!(RedrawVerdict::FirstFrame < RedrawVerdict::Unchanged);
        assert_eq!(RedrawVerdict::Unchanged, RedrawVerdict::Unchanged);
        assert_ne!(RedrawVerdict::FirstFrame, RedrawVerdict::Unchanged);
        let mut sorted = all;
        sorted.sort();
        assert_eq!(sorted, all);
        use std::collections::BTreeSet;
        assert_eq!(all.iter().copied().collect::<BTreeSet<_>>().len(), 4);
    }

    /// The `Debug` name of a verdict — a test helper, so it may branch.
    fn match_free_name(verdict: RedrawVerdict) -> &'static str {
        match verdict {
            RedrawVerdict::FirstFrame => "FirstFrame",
            RedrawVerdict::ContentChanged => "ContentChanged",
            RedrawVerdict::ExternalChange => "ExternalChange",
            RedrawVerdict::Unchanged => "Unchanged",
        }
    }

    /// The revision is a plain, ordered, hashable counter — and its default is
    /// the origin, so a caller with nothing outside the packet to track can
    /// simply not think about it.
    #[test]
    fn the_revision_is_an_ordered_counter_starting_at_the_origin() {
        assert_eq!(FrameRevision::default(), FrameRevision::ORIGIN);
        assert_eq!(FrameRevision::ORIGIN.get(), 0);
        assert_eq!(FrameRevision::ORIGIN.bumped(), FrameRevision::new(1));
        assert!(FrameRevision::ORIGIN < FrameRevision::new(1));
        assert_ne!(FrameRevision::ORIGIN, FrameRevision::new(1));
        assert_eq!(FrameRevision::new(7).get(), 7);
        assert!(format!("{:?}", FrameRevision::ORIGIN).contains("FrameRevision"));
        // Wrapping, so the counter can never trap or panic in a long session.
        assert_eq!(FrameRevision::new(u64::MAX).bumped(), FrameRevision::ORIGIN);
        use std::collections::BTreeSet;
        let seen: BTreeSet<FrameRevision> =
            [FrameRevision::ORIGIN, FrameRevision::new(1)].into_iter().collect();
        assert_eq!(seen.len(), 2);
    }

    /// The ledger and the decision are ordinary values: constructible, cloneable,
    /// comparable, printable. `Default` is the empty ledger, so a loop can hold
    /// one in a struct without a constructor call.
    #[test]
    fn the_ledger_and_the_decision_are_plain_values() {
        assert_eq!(PresentationLedger::default(), PresentationLedger::new());
        let base = packet();
        let decision = PresentationLedger::new().admit(&base, FrameRevision::ORIGIN);
        assert_eq!(decision.clone(), decision);
        assert!(format!("{decision:?}").contains("RedrawDecision"));
        let ledger = decision.into_ledger();
        assert_eq!(ledger.clone(), ledger);
        assert_ne!(ledger, PresentationLedger::new());
        assert!(format!("{ledger:?}").contains("PresentationLedger"));
    }
}
