//! **The instrument**: one `wgpu` timestamp query set, threaded through every
//! render pass of a frame, resolved asynchronously.
//!
//! The arithmetic and the vocabulary live in [`crate::gpu_pass_timing`], which is
//! pure and fully covered on native. This file is the thinnest possible binding
//! to a real device: it exists only where a GPU does (`wasm32`, or the native
//! `offscreen` feature), and it holds the query set, the two buffers the resolve
//! travels through, and the small amount of bookkeeping that makes an
//! *asynchronous* readback safe to drive from a `&self` present.
//!
//! ## Gated on the adapter, and free when absent
//!
//! `wgpu::Features::TIMESTAMP_QUERY` is optional and the browser's WebGL2
//! fallback cannot do it at all. [`GpuPassClock::try_new`] therefore returns
//! `None` unless the *device* was actually created with the feature, and the
//! device request only ever asks for it when the adapter already advertises it
//! (`adapter.features() & TIMESTAMP_QUERY`, which is empty on an adapter without
//! it — so the request is bit-identical to the one this engine has always made).
//! With no clock every `timestamp_writes` argument below is `None`, no query set
//! is allocated, no buffer is copied and no frame is mapped: the off path is the
//! path this backend ran before timing existed.
//!
//! ## Why the numbers are always a frame or two old
//!
//! `resolve_query_set` writes into a GPU buffer; getting at it means mapping that
//! buffer, which completes on a later task — on the browser, a later *frame*.
//! Blocking for it would cost more than everything being measured. So one
//! resolve is in flight at a time: a frame records, resolves, copies and requests
//! the map, and whichever later frame finds the map complete publishes that
//! reading together with the [`axiom_kernel::FrameIndex`] it was taken on. A
//! frame that arrives while a map is outstanding records nothing and costs
//! nothing — which is also why the read buffer can never be a copy destination
//! while it is mapped.

use std::sync::{Arc, Mutex};

use axiom_kernel::FrameIndex;

use crate::gpu_pass_timing::{GpuFrameTiming, PASS_COUNT};

/// Slot of the directional shadow-map depth pre-pass.
pub(crate) const PASS_SHADOW: usize = 0;
/// Slot of the lit/textured/shadowed scene pass (surface programs included).
pub(crate) const PASS_MAIN: usize = 1;
/// Slot of the SDF raymarch composite — recorded only on frames carrying one.
pub(crate) const PASS_SDF: usize = 2;
/// Slot of the present-side fullscreen work: the bloom + grade chain when the
/// app authored one, the plain upscale blit when it did not.
pub(crate) const PASS_POST: usize = 3;
/// Slot of the alpha-blended 2D quad pass.
pub(crate) const PASS_DRAW2D: usize = 4;

/// Two timestamps — beginning and end — per named pass.
const TIMESTAMP_SLOTS: u32 = 2 * PASS_COUNT as u32;

/// Bytes each pass's resolved pair occupies in the destination buffer.
///
/// A pass's two ticks are 16 bytes, but `resolve_query_set` demands a
/// **256-byte-aligned** destination offset, so each pass owns a 256-byte lane
/// and only the first 16 bytes of it are read. The alignment is what makes the
/// lanes independent — and they must be independent, because *only the passes a
/// frame actually ran may be resolved at all*: a query that was never written is
/// uninitialised, and copying it is a driver error, not a zero. (Vulkan says so
/// out loud: `VUID-vkCmdCopyQueryPoolResults-None-09402`.)
const RESOLVE_STRIDE: u64 = 256;

/// Bytes the whole resolve destination occupies: one lane per pass.
const RESOLVE_BYTES: u64 = (PASS_COUNT as u64) * RESOLVE_STRIDE;

/// Why an adapter reports no GPU pass timings.
pub(crate) const ADAPTER_HAS_NO_TIMESTAMP_QUERY: &str =
    "this adapter does not expose wgpu TIMESTAMP_QUERY (the browser's WebGL2 fallback never can)";

/// Why a timing-capable backend reports no numbers yet.
pub(crate) const NOTHING_RESOLVED_YET: &str =
    "timestamps are being recorded, but no frame has finished resolving one yet";

/// The bookkeeping shared between the present path and the buffer-map callback.
#[derive(Debug, Default)]
struct ClockState {
    /// Frames this clock has begun since the binding was created.
    frame: FrameIndex,
    /// Which passes the frame currently being recorded attached timestamps to.
    recording: u32,
    /// The frame whose resolve has been copied into the read buffer, and the
    /// mask of passes it recorded. `None` when nothing is in flight.
    pending: Option<(FrameIndex, u32)>,
    /// Whether the map has been requested for `pending`.
    mapping: bool,
    /// Set by the map callback: the request finished, successfully or not.
    settled: bool,
    /// Whether that finish was a success (the read buffer is mapped).
    settled_ok: bool,
    /// The most recent fully resolved reading.
    latest: Option<GpuFrameTiming>,
}

/// A `wgpu` timestamp query set plus the buffers and bookkeeping that turn it
/// into per-pass durations. Built only when the device really has
/// `TIMESTAMP_QUERY`; absent otherwise, and absence costs nothing.
#[derive(Debug)]
pub(crate) struct GpuPassClock {
    set: wgpu::QuerySet,
    /// `resolve_query_set`'s destination — GPU-only.
    resolve: wgpu::Buffer,
    /// The mappable copy of it the CPU reads.
    read: wgpu::Buffer,
    /// Nanoseconds one tick of this queue's timestamps represents.
    period_ns: f32,
    shared: Arc<Mutex<ClockState>>,
}

impl GpuPassClock {
    /// A clock for `device`, or `None` when the device was not created with
    /// `TIMESTAMP_QUERY` — which is every WebGL2 device and every adapter that
    /// does not advertise the feature.
    pub(crate) fn try_new(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<GpuPassClock> {
        device
            .features()
            .contains(wgpu::Features::TIMESTAMP_QUERY)
            .then(|| GpuPassClock {
                set: device.create_query_set(&wgpu::QuerySetDescriptor {
                    label: Some("axiom-pass-timestamps"),
                    ty: wgpu::QueryType::Timestamp,
                    count: TIMESTAMP_SLOTS,
                }),
                resolve: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("axiom-pass-timestamp-resolve"),
                    size: RESOLVE_BYTES,
                    usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                    mapped_at_creation: false,
                }),
                read: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("axiom-pass-timestamp-read"),
                    size: RESOLVE_BYTES,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                }),
                period_ns: queue.get_timestamp_period(),
                shared: Arc::new(Mutex::new(ClockState::default())),
            })
    }

    /// Start a new frame: advance the index these timestamps will be reported
    /// under and forget which passes the previous frame recorded.
    pub(crate) fn begin_frame(&self) {
        self.shared.lock().iter_mut().for_each(|state| {
            state.frame = state.frame.next();
            state.recording = 0;
        });
    }

    /// Attach **both** ends of `pass` to one render pass — the shape every pass
    /// that is a single `begin_render_pass` uses.
    pub(crate) fn writes(&self, pass: usize) -> wgpu::RenderPassTimestampWrites<'_> {
        self.mark(pass);
        wgpu::RenderPassTimestampWrites {
            query_set: &self.set,
            beginning_of_pass_write_index: Some(2 * pass as u32),
            end_of_pass_write_index: Some(2 * pass as u32 + 1),
        }
    }

    /// Attach only the **beginning** of `pass`, for a multi-pass stage whose
    /// span starts here (the post chain's bright pass).
    pub(crate) fn opens(&self, pass: usize) -> wgpu::RenderPassTimestampWrites<'_> {
        self.mark(pass);
        wgpu::RenderPassTimestampWrites {
            query_set: &self.set,
            beginning_of_pass_write_index: Some(2 * pass as u32),
            end_of_pass_write_index: None,
        }
    }

    /// Attach only the **end** of `pass`, closing a span [`Self::opens`] started
    /// (the post chain's composite).
    pub(crate) fn closes(&self, pass: usize) -> wgpu::RenderPassTimestampWrites<'_> {
        wgpu::RenderPassTimestampWrites {
            query_set: &self.set,
            beginning_of_pass_write_index: None,
            end_of_pass_write_index: Some(2 * pass as u32 + 1),
        }
    }

    /// Record that `pass` really did attach its timestamps this frame. A pass
    /// without its bit is reported as *absent*, never as a zero duration.
    fn mark(&self, pass: usize) {
        self.shared
            .lock()
            .iter_mut()
            .for_each(|state| state.recording |= 1 << pass);
    }

    /// Resolve this frame's query set into the read buffer — **unless a previous
    /// frame's map is still outstanding**, in which case this frame is simply not
    /// sampled. Copying into a mapped buffer is illegal, and waiting for the map
    /// would put the readback on the frame's critical path.
    ///
    /// Call this after every pass of the frame has been encoded, on an encoder
    /// that is submitted after them.
    pub(crate) fn resolve(&self, encoder: &mut wgpu::CommandEncoder) {
        let sample = self.shared.lock().iter_mut().fold(None, |_, state| {
            let free = state.pending.is_none();
            free.then(|| state.pending = Some((state.frame, state.recording)));
            free.then_some(state.recording)
        });
        sample.into_iter().for_each(|recorded| {
            // **Only the passes this frame ran.** Each lands in its own aligned
            // lane, so an absent pass leaves a lane untouched rather than making
            // the whole resolve illegal.
            (0..PASS_COUNT)
                .filter(|pass| recorded & (1 << pass) != 0)
                .for_each(|pass| {
                    let first = 2 * pass as u32;
                    encoder.resolve_query_set(
                        &self.set,
                        first..first + 2,
                        &self.resolve,
                        (pass as u64) * RESOLVE_STRIDE,
                    );
                });
            encoder.copy_buffer_to_buffer(&self.resolve, 0, &self.read, 0, RESOLVE_BYTES);
        });
    }

    /// Move the asynchronous readback along, without ever blocking: publish a
    /// map that has completed, then request one for a resolve that is waiting.
    /// Call once per frame, after the frame's submission.
    pub(crate) fn pump(&self, device: &wgpu::Device) {
        self.publish();
        self.request();
        // Native devices only make progress on map callbacks when polled; on the
        // browser the callback rides the real WebGPU promise and this is inert.
        let _ = device.poll(wgpu::PollType::Poll);
    }

    /// Read a completed map into [`ClockState::latest`] and free the buffer for
    /// the next frame. A map that failed frees the slot without publishing
    /// anything — the next frame simply tries again.
    fn publish(&self) {
        let settled = self
            .shared
            .lock()
            .iter()
            .fold((false, false), |_, state| (state.settled, state.settled_ok));
        // Reading the map is only legal when it succeeded, and `settled_ok` is
        // never set without `settled`.
        let reading = settled.1.then(|| self.read_ticks());
        settled.0.then(|| {
            self.shared.lock().iter_mut().for_each(|state| {
                let taken = state.pending.take();
                let previous = state.latest;
                // A failed map publishes nothing and, crucially, erases nothing:
                // the last real reading stays the last real reading.
                state.latest = reading
                    .as_ref()
                    .and_then(|ticks| {
                        taken.map(|(frame, recorded)| {
                            GpuFrameTiming::resolved(ticks, recorded, self.period_ns, frame)
                        })
                    })
                    .or(previous);
                state.mapping = false;
                state.settled = false;
                state.settled_ok = false;
            });
            settled.1.then(|| self.read.unmap());
        });
    }

    /// The mapped read buffer's ticks, flattened out of their aligned lanes into
    /// the interleaved `begin, end` per pass that
    /// [`crate::gpu_pass_timing::GpuFrameTiming::resolved`] reads.
    fn read_ticks(&self) -> Vec<u64> {
        let view = self.read.slice(..).get_mapped_range();
        let ticks = (0..TIMESTAMP_SLOTS as usize)
            .map(|slot| {
                let lane = (slot / 2) * (RESOLVE_STRIDE as usize);
                let at = lane + (slot % 2) * 8;
                u64::from_le_bytes(std::array::from_fn(|byte| {
                    view.get(at + byte).copied().unwrap_or_default()
                }))
            })
            .collect();
        drop(view);
        ticks
    }

    /// Ask for the map of an outstanding resolve, once.
    fn request(&self) {
        let wanted = self.shared.lock().iter_mut().fold(false, |_, state| {
            let due = state.pending.is_some() & !state.mapping;
            due.then(|| state.mapping = true);
            due
        });
        wanted.then(|| {
            let shared = Arc::clone(&self.shared);
            self.read
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |result| {
                    shared.lock().iter_mut().for_each(|state| {
                        state.settled = true;
                        state.settled_ok = result.is_ok();
                    });
                });
        });
    }

    /// The most recent resolved reading, or the reason there is not one yet.
    pub(crate) fn timing(&self) -> GpuFrameTiming {
        self.shared
            .lock()
            .iter()
            .fold(None, |_, state| state.latest)
            .unwrap_or_else(|| GpuFrameTiming::unavailable(NOTHING_RESOLVED_YET))
    }
}
