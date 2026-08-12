//! On-screen controls for touch devices: a virtual joystick and a button pad.
//!
//! This is the **model**, not the glue. It takes pointer down/move/up events in
//! viewport coordinates and produces exactly two things: an analogue steering
//! value, and a list of synthetic key tokens. Those tokens go through the same
//! [`crate::controls::Controls`] action table the keyboard and the gamepad use,
//! so there is one binding table and one command path for all three devices —
//! a touch player and a keyboard player are indistinguishable to the simulation.
//!
//! Keeping it here rather than in `web.rs` is the point: layout, hit testing,
//! the joystick's deadzone and its clamping are all decisions worth testing, and
//! none of them need a browser. `web.rs` is left with "attach three listeners
//! and draw some circles".
//!
//! ## The joystick is dynamic
//!
//! The stick has no fixed position. It appears centred on wherever the first
//! finger lands inside the steering zone, and steering is measured from *that*
//! point. A fixed on-screen stick requires the player to find it without
//! looking; a dynamic one is always exactly where their thumb already is, which
//! on a phone is the difference between a playable racing game and a novelty.

use axiom_math::Vec2;

use crate::profile::PlayProfile;

/// The actions the on-screen pad can fire, and the key token each one presents
/// itself to the action table as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadButton {
    /// The accelerator.
    Accelerate,
    /// Brake and reverse.
    Brake,
    /// Handbrake, for deliberate drifts.
    Handbrake,
    /// Boost.
    Boost,
    /// Reset to the last safe point.
    Reset,
    /// Hop one lane left.
    ///
    /// No layout places this any more — it is the name the **swipe** presents
    /// itself under, and nothing else. Keeping it in this vocabulary rather than
    /// hard-coding `"KeyA"` at the gesture is what keeps [`PadButton::key`] the
    /// single table mapping a touch intent to the action bindings.
    LaneLeft,
    /// Hop one lane right. Gesture-only, exactly as [`PadButton::LaneLeft`].
    LaneRight,
}

impl PadButton {
    /// Every button, in a stable order.
    pub const ALL: [PadButton; 7] = [
        PadButton::Accelerate,
        PadButton::Brake,
        PadButton::Handbrake,
        PadButton::Boost,
        PadButton::Reset,
        PadButton::LaneLeft,
        PadButton::LaneRight,
    ];

    /// The key token this button presents itself as, so it flows through the
    /// same action bindings as the physical key.
    pub const fn key(self) -> &'static str {
        match self {
            PadButton::Accelerate => "KeyW",
            PadButton::Brake => "KeyS",
            PadButton::Handbrake => "Space",
            PadButton::Boost => "ShiftLeft",
            PadButton::Reset => "KeyR",
            // The same tokens the keyboard steers with. `controls` reads their
            // press edge as a lane hop, so one binding serves both games and the
            // rails solver never learns what a touchscreen is.
            PadButton::LaneLeft => "KeyA",
            PadButton::LaneRight => "KeyD",
        }
    }

    /// The label drawn on the button.
    pub const fn label(self) -> &'static str {
        match self {
            PadButton::Accelerate => "GAS",
            PadButton::Brake => "BRAKE",
            PadButton::Handbrake => "DRIFT",
            PadButton::Boost => "BOOST",
            PadButton::Reset => "R",
            PadButton::LaneLeft => "◀",
            PadButton::LaneRight => "▶",
        }
    }

    /// The accent the button is ringed and lettered in.
    ///
    /// The control cluster is the bottom third of a phone frame, sitting over a
    /// road that is the brightest thing on screen. Drawn in one grey it reads as
    /// a wash of identical discs and the eye has to *read* five words to find
    /// the one it wants; drawn in its own colour, GAS is found by its green and
    /// BOOST by its cyan before either word resolves. That is a composition
    /// decision — it is what gives the bottom of the frame a shape — not a
    /// styling flourish.
    ///
    /// It belongs on the button rather than in the painter because the accent is
    /// part of what the button *means*: the boost meter above the cluster is
    /// drawn in [`BOOST_ACCENT`] too, and the meter and the button agreeing is
    /// the whole reason the colour is a constant and not a literal in one
    /// `format!`.
    pub const fn accent(self) -> &'static str {
        match self {
            PadButton::Accelerate => GAS_ACCENT,
            PadButton::Boost => BOOST_ACCENT,
            PadButton::Handbrake => DRIFT_ACCENT,
            // Braking, resetting and hopping lanes are not modes you hold the
            // car in, so they stay neutral and let the two coloured buttons be
            // the ones the eye lands on.
            PadButton::Brake | PadButton::Reset | PadButton::LaneLeft | PadButton::LaneRight => {
                NEUTRAL_ACCENT
            }
        }
    }
}

/// The accelerator's green.
pub const GAS_ACCENT: &str = "#5ade7c";
/// The boost's cyan — shared with the boost meter, which is the same idea.
pub const BOOST_ACCENT: &str = "#5ac2ee";
/// The handbrake's amber.
pub const DRIFT_ACCENT: &str = "#ffd166";
/// Everything that is a one-shot action rather than a mode.
pub const NEUTRAL_ACCENT: &str = "#e9f2ff";

/// One button's position and size, in viewport pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PadSlot {
    pub button: PadButton,
    pub centre: Vec2,
    pub radius: f32,
}

impl PadSlot {
    /// Whether `point` is inside this button.
    pub fn contains(&self, point: Vec2) -> bool {
        let dx = point.x - self.centre.x;
        let dy = point.y - self.centre.y;
        dx * dx + dy * dy <= self.radius * self.radius
    }
}

/// The on-screen layout for a viewport, computed from its size.
///
/// Everything scales off one unit derived from the smaller viewport dimension,
/// so the pad is the same size relative to a thumb on a phone, a tablet and a
/// desktop window.
#[derive(Debug, Clone, PartialEq)]
pub struct PadLayout {
    pub viewport: Vec2,
    pub unit: f32,
    pub slots: Vec<PadSlot>,
    /// Height (px) of the clear strip the pad leaves along the bottom edge of
    /// the frame. See [`BOTTOM_STRIP_UNITS`]: the boost meter and the controls
    /// legend are laid out inside it, so it is part of the pad's contract with
    /// the HUD rather than an incidental margin.
    pub bottom_strip: f32,
    /// Radius of the joystick ring once it appears.
    pub stick_radius: f32,
    /// Whether this layout has a joystick at all. False on rails, where lateral
    /// intent is two buttons and a stick would be a second, contradictory way to
    /// ask for the same thing.
    pub stick_enabled: bool,
}

impl PadLayout {
    /// Lay the controls out for a viewport of `width` × `height` pixels, for
    /// whichever game [`PlayProfile`] says this is.
    pub fn for_profile(width: f32, height: f32, profile: PlayProfile) -> PadLayout {
        [
            PadLayout::for_viewport as fn(f32, f32) -> PadLayout,
            PadLayout::rails_for_viewport,
        ][usize::from(profile.is_rails())](width, height)
    }

    /// The rails pad: **one button**. BOOST, under the left thumb.
    ///
    /// Everything else went, and each one for its own reason rather than for
    /// tidiness:
    ///
    /// * **LEFT / RIGHT** — replaced outright by the swipe. Two buttons and a
    ///   gesture doing the same job is two ways to ask for one thing, and the
    ///   buttons were the worse one: they sat in a fixed corner while the swipe
    ///   works wherever the thumb already is.
    /// * **GAS** — a button whose correct state is "held" for the entire race is
    ///   not a control, it is a tax. The lane game holds the throttle itself
    ///   (`RaceSim::for_profile`), so there is nothing left for the button to say.
    /// * **BRAKE** — with the throttle held, braking is the one input that makes
    ///   the game strictly harder to win at: the race is a time, and the boost
    ///   economy is earned by threading traffic at speed. Slowing down was never
    ///   an answer to anything the lane game asks.
    /// * **DRIFT** — already absent; a railed car has no slide to provoke.
    ///
    /// What is left is the shape the game actually has: **the road decides
    /// where you can go, the swipe decides which lane, and BOOST decides how
    /// fast.** One button, and it is the one the whole reward loop feeds.
    ///
    /// It sits bottom-**left** because the swipe is now the busy hand's job and
    /// the two should not be the same thumb. It is drawn a full unit — the size
    /// GAS used to be — because a lone control on a phone screen should be the
    /// thing you cannot miss.
    pub fn rails_for_viewport(width: f32, height: f32) -> PadLayout {
        let viewport = Vec2::new(width.max(1.0), height.max(1.0));
        let unit = (viewport.x.min(viewport.y) * UNIT_FRACTION).clamp(UNIT_MIN, UNIT_MAX);
        let margin = unit * 0.55;
        let bottom_strip = (unit * BOTTOM_STRIP_UNITS).max(BOTTOM_STRIP_MIN);
        let right = viewport.x - margin;
        let bottom = viewport.y - bottom_strip;
        let left = margin;
        let slots = vec![
            PadSlot {
                button: PadButton::Boost,
                centre: Vec2::new(left + unit, bottom - unit),
                radius: unit,
            },
            // RESET stays, on the far side from BOOST. It is not a driving
            // control — it is the way out of a car wedged against a barrier —
            // and a run with no way to un-stick itself is a run that ends there.
            PadSlot {
                button: PadButton::Reset,
                centre: Vec2::new(right - unit * 0.6, margin + unit * 0.6),
                radius: (unit * 0.55).max(MIN_TOUCH_RADIUS),
            },
        ];
        PadLayout {
            viewport,
            unit,
            slots,
            bottom_strip,
            stick_radius: unit * 1.3,
            stick_enabled: false,
        }
    }

    /// Lay the wheel game's controls out for a viewport of `width` × `height`
    /// pixels: the dynamic joystick plus GAS/BRAKE/DRIFT/BOOST.
    pub fn for_viewport(width: f32, height: f32) -> PadLayout {
        let viewport = Vec2::new(width.max(1.0), height.max(1.0));
        let unit = (viewport.x.min(viewport.y) * UNIT_FRACTION).clamp(UNIT_MIN, UNIT_MAX);
        let margin = unit * 0.55;
        let bottom_strip = (unit * BOTTOM_STRIP_UNITS).max(BOTTOM_STRIP_MIN);
        let right = viewport.x - margin;
        let bottom = viewport.y - bottom_strip;
        // The accelerator is the largest and sits under the thumb; everything
        // else is arranged around it, further from the resting position in
        // rough order of how often it is used.
        let slots = vec![
            PadSlot {
                button: PadButton::Accelerate,
                centre: Vec2::new(right - unit, bottom - unit),
                radius: unit,
            },
            PadSlot {
                button: PadButton::Brake,
                centre: Vec2::new(right - unit * 3.05, bottom - unit * 0.80),
                radius: unit * 0.72,
            },
            PadSlot {
                button: PadButton::Boost,
                centre: Vec2::new(right - unit * 1.05, bottom - unit * 2.90),
                radius: unit * 0.74,
            },
            PadSlot {
                button: PadButton::Handbrake,
                centre: Vec2::new(right - unit * 2.95, bottom - unit * 2.60),
                radius: unit * 0.66,
            },
            PadSlot {
                button: PadButton::Reset,
                centre: Vec2::new(right - unit * 0.6, margin + unit * 0.6),
                radius: (unit * 0.55).max(MIN_TOUCH_RADIUS),
            },
        ];
        PadLayout {
            viewport,
            unit,
            slots,
            bottom_strip,
            stick_radius: unit * 1.3,
            stick_enabled: true,
        }
    }

    /// The button under `point`, if any.
    pub fn hit(&self, point: Vec2) -> Option<PadButton> {
        self.slots
            .iter()
            .find(|slot| slot.contains(point))
            .map(|slot| slot.button)
    }

    /// The slot for `button`.
    pub fn slot(&self, button: PadButton) -> Option<&PadSlot> {
        self.slots.iter().find(|slot| slot.button == button)
    }

    /// Whether `point` is inside the steering zone — the left of the screen,
    /// below the HUD, and not on a button.
    pub fn in_steering_zone(&self, point: Vec2) -> bool {
        let left = point.x < self.viewport.x * STEER_ZONE_WIDTH;
        let below_hud = point.y > self.viewport.y * STEER_ZONE_TOP;
        self.stick_enabled && left && below_hud && self.hit(point).is_none()
    }
}

/// Fraction of the smaller viewport dimension one layout unit takes.
const UNIT_FRACTION: f32 = 0.115;
/// How far, in layout units, the pad stops short of the bottom edge.
///
/// This is deliberately deeper than the side/top `margin` of `0.55` units, and
/// it is a **composition** number rather than a comfort one. The bottom of the
/// frame is a stack read from the edge upward — controls legend, boost meter,
/// then the pad — and until this constant existed there was nothing reserving
/// room for the first two. The pad simply sat `margin` off the bottom edge and
/// the HUD placed its meter at a fixed `bottom: 92px`, a number true of no
/// frame in particular: on the 470x836 phone frame the campaign captures, the
/// pad's bottom row spans 30..138 px off the edge and the meter landed at 92,
/// i.e. printed straight through the lane buttons and into the accelerator.
///
/// Reserving the strip here rather than nudging that `92` is what makes the fix
/// hold at other frame sizes: the strip scales with the pad, so the meter and
/// the legend are positioned against the space the pad actually left rather
/// than against a pixel count someone measured once.
///
/// `1.15` is the smallest strip that clears a two-line legend and the meter
/// above it. It also moves the pad *toward* the reference rather than away: on
/// the capture frame the bottom row's centres go to 719.8 px against the
/// reference's ~727.5, where `0.55` put them at 752.2 — a 25 px error becomes
/// an 8 px one, and in the direction that opens the bottom of the frame up
/// instead of crowding it.
const BOTTOM_STRIP_UNITS: f32 = 1.15;
/// The strip never gets shorter than this (px), whatever the unit clamps do.
///
/// The strip holds *type*, and type does not scale below legibility: two legend
/// lines and the meter above them need about this much height however small the
/// frame is. Without the floor a 320x240 window drives the unit to `UNIT_MIN`
/// and the strip to 46 px, at which point the meter sits back on top of the
/// legend — the same class of collision one layer down.
const BOTTOM_STRIP_MIN: f32 = 52.0;
/// Smallest a layout unit may get (px). Set so that even the smallest button on
/// the smallest viewport clears [`MIN_TOUCH_RADIUS`].
const UNIT_MIN: f32 = 40.0;
/// The smallest a button may ever be (px radius) — a 44 px target is the floor
/// below which a thumb starts missing, on every platform's guidance.
const MIN_TOUCH_RADIUS: f32 = 22.0;
/// Largest a layout unit may get (px).
const UNIT_MAX: f32 = 78.0;
/// Fraction of the viewport width the steering zone occupies.
const STEER_ZONE_WIDTH: f32 = 0.5;
/// Fraction of the viewport height above which the steering zone does not go —
/// keeps the HUD readable and stops a stray tap near the speedo from steering.
const STEER_ZONE_TOP: f32 = 0.28;
/// Fraction of the stick radius inside which steering reads as centred.
const STICK_DEADZONE: f32 = 0.14;

/// How far across the screen a finger must travel to hop one lane, in layout
/// units. See [`TouchControls::drag_swipe`].
///
/// Sized in units rather than pixels for the same reason every other distance
/// here is: a swipe has to feel the same on a 4-inch phone and a tablet, and a
/// unit is already "one thumb's worth" on both. At the unit's clamps this is a
/// 16..31 px flick — far enough that the jitter of a tap (a few px) cannot
/// reach it, short enough that the lane changes near the *start* of the flick
/// rather than at the end of it.
///
/// It used to be twice this, and lowering it is not a taste change — it is what
/// one-lane-per-gesture bought. While a held finger could keep hopping, a
/// too-eager threshold was dangerous: it did not cost you a lane, it cost you
/// however many lanes the rest of the drag crossed, which at racing speed is a
/// barrier. Now the worst a false positive can do is one lane, and the gesture
/// is spent. A cheaper mistake is allowed to be a more sensitive one.
const SWIPE_UNITS: f32 = 0.4;

/// How much more horizontal than vertical a drag must be to read as a lane
/// swipe. Anything flatter than this is someone dragging up or down the screen
/// and is not asking for a lane.
const SWIPE_HORIZONTAL_BIAS: f32 = 1.2;

/// A finger that is not on a button and not on the stick, tracked in case it
/// turns into a lane swipe.
///
/// It is **consumed** by the hop it fires: one finger down is one lane, however
/// far it goes afterwards, and the next lane costs a lift and a new flick.
///
/// That is the whole gesture, and it is deliberately not the obvious one. The
/// obvious one re-measures from wherever the last hop fired, so a long drag
/// pays a lane per threshold — which reads well on paper and badly in the hand:
/// a flick does not stop at the moment the player stopped meaning it, so a
/// single decisive swipe slides the car across two or three lanes and into
/// whatever is in the third. "One flick, one lane" is a gesture the player can
/// aim; "one lane per 16 px of follow-through" is one they can only approximate.
#[derive(Debug, Clone, Copy, PartialEq)]
struct SwipeTrack {
    pointer: i32,
    /// Where the finger landed. Fixed for the life of the gesture.
    origin: Vec2,
}

/// The live joystick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VirtualStick {
    /// Where the finger first landed — the steering origin.
    pub origin: Vec2,
    /// Where the finger is now, clamped to the ring.
    pub knob: Vec2,
    /// The pointer holding it.
    pub pointer: i32,
}

/// The on-screen control state.
#[derive(Debug, Clone)]
pub struct TouchControls {
    profile: PlayProfile,
    layout: PadLayout,
    stick: Option<VirtualStick>,
    /// Held buttons, each with the pointer holding it.
    held: Vec<(PadButton, i32)>,
    /// The finger that might be swiping, if one is down off the buttons.
    swipe: Option<SwipeTrack>,
    /// Lane hops a swipe has asked for and no frame has taken yet.
    ///
    /// A swipe fires from a pointer event, which happens whenever the browser
    /// feels like it; a command is built once a frame. Queueing the hops here
    /// rather than steering directly is what stops a flick between two frames
    /// from being dropped, and what stops a flick that spans four frames from
    /// being counted four times.
    pending_swipe: Vec<PadButton>,
    /// Whether the previous frame delivered a swipe token, so this one must not.
    /// See [`TouchControls::frame_keys`] — a token on two consecutive frames is
    /// one held key, not two presses.
    swipe_emitted: bool,
    /// Set by the first touch, and never cleared: once a device has been touched
    /// it is a touch device, and the controls stay up.
    engaged: bool,
}

impl TouchControls {
    /// Controls laid out for a viewport, for the wheel game.
    pub fn new(width: f32, height: f32) -> TouchControls {
        TouchControls::for_profile(width, height, PlayProfile::Wheel)
    }

    /// Controls laid out for a viewport, for whichever game `profile` names.
    pub fn for_profile(width: f32, height: f32, profile: PlayProfile) -> TouchControls {
        TouchControls {
            profile,
            layout: PadLayout::for_profile(width, height, profile),
            stick: None,
            held: Vec::new(),
            swipe: None,
            pending_swipe: Vec::new(),
            swipe_emitted: false,
            engaged: false,
        }
    }

    /// Re-lay the controls for a resized viewport, releasing anything held —
    /// a rotated phone has moved every button, so keeping a press would leave a
    /// finger holding a button that is no longer under it.
    pub fn resize(&mut self, width: f32, height: f32) {
        let layout = PadLayout::for_profile(width, height, self.profile);
        if layout.viewport != self.layout.viewport {
            self.layout = layout;
            self.stick = None;
            self.held.clear();
            // A swipe measured against the old viewport means nothing against
            // the new one; the hops it already earned are still owed.
            self.swipe = None;
        }
    }

    /// The current layout.
    pub const fn layout(&self) -> &PadLayout {
        &self.layout
    }

    /// The live joystick, if one is held.
    pub const fn stick(&self) -> Option<VirtualStick> {
        self.stick
    }

    /// Whether the on-screen controls should be drawn.
    pub const fn engaged(&self) -> bool {
        self.engaged
    }

    /// Force the controls visible — used when the device reports touch support,
    /// so the pad is up before the player's first tap rather than after it.
    pub fn engage(&mut self) {
        self.engaged = true;
    }

    /// Whether `button` is currently held.
    pub fn is_held(&self, button: PadButton) -> bool {
        self.held.iter().any(|(b, _)| *b == button)
    }

    /// A pointer went down at `point`.
    pub fn press(&mut self, pointer: i32, point: Vec2) {
        self.engaged = true;
        if let Some(button) = self.layout.hit(point) {
            // A button already held by another pointer stays held by that one;
            // holding the same button with two fingers must not release it when
            // only one lifts.
            self.held.retain(|(_, p)| *p != pointer);
            self.held.push((button, pointer));
            return;
        }
        if self.layout.in_steering_zone(point) && self.stick.is_none() {
            self.stick = Some(VirtualStick {
                origin: point,
                knob: point,
                pointer,
            });
            return;
        }
        // Anywhere else on a rails screen, a finger is a potential lane swipe.
        //
        // Deliberately the *whole* screen rather than a gesture zone. The lane
        // buttons are still there and still the precise control; a swipe is the
        // one you reach for without looking, mid-corner, with the hand that is
        // not on the accelerator — and a swipe that only works in a rectangle
        // the player cannot see is a swipe that reads as broken half the time.
        // Buttons win where they overlap, because `press` has already returned
        // by the time this runs.
        if self.profile.is_rails() {
            self.swipe = Some(SwipeTrack {
                pointer,
                origin: point,
            });
        }
    }

    /// A pointer moved to `point`.
    ///
    /// Drives whichever of the two lateral gestures this pointer owns: the
    /// joystick on the wheel game, the lane swipe on rails. They are mutually
    /// exclusive by construction — [`Self::press`] starts exactly one — so
    /// there is no precedence rule to get wrong.
    pub fn drag(&mut self, pointer: i32, point: Vec2) {
        self.drag_swipe(pointer, point);
        let Some(stick) = self.stick.as_mut() else {
            return;
        };
        if stick.pointer != pointer {
            return;
        }
        // Clamp the knob to the ring, so a finger dragged across the whole
        // screen is still full lock rather than an ever-growing number.
        let dx = point.x - stick.origin.x;
        let dy = point.y - stick.origin.y;
        let distance = (dx * dx + dy * dy).sqrt();
        let scale = if distance > self.layout.stick_radius {
            self.layout.stick_radius / distance
        } else {
            1.0
        };
        stick.knob = Vec2::new(
            stick.origin.x + dx * scale,
            stick.origin.y + dy * scale,
        );
    }

    /// Advance the lane swipe: the moment the finger has travelled far enough
    /// sideways, queue **one** hop and end the gesture.
    ///
    /// The two conditions are the whole recogniser. **Distance** keeps a tap
    /// from hopping a lane — a thumb moves a few pixels just being lifted, and a
    /// game where a mis-registered tap changes lane at 600 km/h is a game that
    /// feels possessed. **Horizontal dominance** keeps a drag up or down the
    /// screen from being read as a lateral intent it plainly is not.
    ///
    /// Firing on the *first* move that clears the threshold, rather than on the
    /// finger lifting, is what makes the lane change land while the flick is
    /// still happening instead of after it. The rest of the gesture — the
    /// follow-through, the finger coming to rest, the lift — is already
    /// irrelevant by then, which is exactly the point: the player has committed
    /// and the car has already gone.
    fn drag_swipe(&mut self, pointer: i32, point: Vec2) {
        let threshold = self.layout.unit * SWIPE_UNITS;
        let Some(swipe) = self.swipe else {
            return;
        };
        if swipe.pointer != pointer {
            return;
        }
        let dx = point.x - swipe.origin.x;
        let dy = point.y - swipe.origin.y;
        if dx.abs() < threshold || dx.abs() < dy.abs() * SWIPE_HORIZONTAL_BIAS {
            return;
        }
        self.pending_swipe.push(if dx > 0.0 {
            PadButton::LaneRight
        } else {
            PadButton::LaneLeft
        });
        // Spent. Everything this finger does from here is follow-through, and
        // the next lane costs a lift and a new flick.
        self.swipe = None;
    }

    /// A pointer lifted.
    pub fn release(&mut self, pointer: i32) {
        self.held.retain(|(_, p)| *p != pointer);
        if self.stick.map(|s| s.pointer) == Some(pointer) {
            self.stick = None;
        }
        // The hops it already earned survive: they are owed to the player, not
        // to the finger. Only the tracking stops.
        if self.swipe.map(|s| s.pointer) == Some(pointer) {
            self.swipe = None;
        }
    }

    /// Release everything — a lost pointer capture, a backgrounded tab.
    pub fn release_all(&mut self) {
        self.held.clear();
        self.stick = None;
        self.swipe = None;
    }

    /// The steering value, `-1..1`.
    ///
    /// Horizontal only: on a touch screen the vertical axis is the throttle's
    /// job and mixing the two into one stick makes both worse.
    pub fn steer(&self) -> f32 {
        let Some(stick) = self.stick else {
            return 0.0;
        };
        let radius = self.layout.stick_radius.max(1.0);
        let raw = (stick.knob.x - stick.origin.x) / radius;
        // Rescale past the deadzone so the full travel is still reachable.
        let magnitude = raw.abs();
        if magnitude <= STICK_DEADZONE {
            return 0.0;
        }
        let scaled = (magnitude - STICK_DEADZONE) / (1.0 - STICK_DEADZONE);
        raw.signum() * scaled.clamp(0.0, 1.0)
    }

    /// The key tokens the held buttons present themselves as, in a stable order.
    pub fn keys(&self) -> Vec<&'static str> {
        PadButton::ALL
            .iter()
            .filter(|button| self.is_held(**button))
            .map(|button| button.key())
            .collect()
    }

    /// One frame's key tokens: the held buttons, plus **one** queued swipe hop.
    ///
    /// Call exactly once per frame — it consumes what it reports.
    ///
    /// A swipe arrives as the same `KeyA`/`KeyD` token the lane buttons and the
    /// keyboard present, which is what keeps the promise this module opens with:
    /// there is one binding table and one command path, and the simulation
    /// cannot tell a thumb from a keyboard. [`crate::controls::Controls`] reads
    /// the token's *press edge* as a lane hop, so the token has to appear for a
    /// frame and then be gone — which is precisely what "consume one queued hop
    /// per frame" produces.
    ///
    /// Draining one at a time rather than all of them is the same requirement
    /// read from the other end: two hops emitted in one frame would be one press
    /// edge and the second lane change would be silently eaten.
    ///
    /// **And one frame of silence between them, for exactly the same reason.**
    /// A token present on two consecutive frames is a key being *held*, not a
    /// key pressed twice — so a queue drained on every frame would deliver two
    /// flicks as one lane change and look like a dropped input. The gap frame is
    /// what makes the second press a press.
    ///
    /// A gesture only ever queues one hop ([`Self::drag_swipe`]), so in practice
    /// the queue holds one and this costs nothing: the flick's lane change goes
    /// out on the very next frame. The queue earns its keep when two flicks land
    /// inside 16 ms — the second waits a frame rather than being swallowed.
    ///
    /// Nothing can contend for the token: the lane buttons that used to share
    /// it are gone from the pad, so a swipe is the only thing that presses
    /// `KeyA`/`KeyD` on a touchscreen.
    pub fn frame_keys(&mut self) -> Vec<&'static str> {
        let mut keys = self.keys();
        let hop = (!self.swipe_emitted)
            .then(|| self.pending_swipe.first().copied())
            .flatten();
        hop.iter().for_each(|_| {
            self.pending_swipe.remove(0);
        });
        self.swipe_emitted = hop.is_some();
        keys.extend(hop.map(PadButton::key));
        keys
    }

    /// How many swipe hops are waiting to be taken. Diagnostics and tests; the
    /// game reads them through [`Self::frame_keys`].
    pub fn pending_swipes(&self) -> usize {
        self.pending_swipe.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A typical portrait phone.
    fn phone() -> TouchControls {
        TouchControls::new(390.0, 844.0)
    }

    /// A typical landscape phone — the orientation the game is actually played
    /// in, and the one the layout has to work in.
    fn landscape() -> TouchControls {
        TouchControls::new(844.0, 390.0)
    }

    /// A landscape phone playing the shipping game.
    fn rails_phone() -> TouchControls {
        TouchControls::for_profile(844.0, 390.0, PlayProfile::Rails)
    }

    /// A point in open road — no button under it, and clear of the pad.
    fn open_road(touch: &TouchControls) -> Vec2 {
        let point = Vec2::new(touch.layout().viewport.x * 0.5, touch.layout().viewport.y * 0.35);
        assert!(touch.layout().hit(point).is_none(), "the fixture must be clear of the pad");
        point
    }

    /// One flick of `dx` pixels from `from`, delivered as a press, a move and a
    /// lift — the three events a browser actually sends.
    fn flick(touch: &mut TouchControls, from: Vec2, dx: f32, dy: f32) {
        touch.press(1, from);
        touch.drag(1, Vec2::new(from.x + dx, from.y + dy));
        touch.release(1);
    }

    #[test]
    fn a_swipe_right_hops_a_lane_right_and_a_swipe_left_hops_left() {
        let mut touch = rails_phone();
        let from = open_road(&touch);
        let far = touch.layout().unit * SWIPE_UNITS + 4.0;

        flick(&mut touch, from, far, 0.0);
        assert_eq!(touch.frame_keys(), vec![PadButton::LaneRight.key()]);
        assert_eq!(touch.pending_swipes(), 0, "and it is spent");

        flick(&mut touch, from, -far, 0.0);
        // The gap frame that lets the last token go up — see `frame_keys`.
        assert!(touch.frame_keys().is_empty());
        assert_eq!(touch.frame_keys(), vec![PadButton::LaneLeft.key()]);
    }

    /// The token has to be there for exactly one frame, or the press edge the
    /// action table reads never happens — see [`TouchControls::frame_keys`].
    #[test]
    fn a_swipes_key_lasts_exactly_one_frame() {
        let mut touch = rails_phone();
        let from = open_road(&touch);
        let far = touch.layout().unit * SWIPE_UNITS + 4.0;
        flick(&mut touch, from, far, 0.0);
        assert_eq!(touch.frame_keys().len(), 1, "down on this frame");
        assert!(touch.frame_keys().is_empty(), "and up on the next");
    }

    /// A tap is not a swipe. A thumb moves a few pixels just being lifted, and a
    /// game that changes lane at 600 km/h because of that is a game that feels
    /// possessed.
    #[test]
    fn a_tap_and_a_short_drag_ask_for_nothing() {
        let mut touch = rails_phone();
        let from = open_road(&touch);
        let just_short = touch.layout().unit * SWIPE_UNITS - 2.0;
        flick(&mut touch, from, 0.0, 0.0);
        flick(&mut touch, from, just_short, 0.0);
        assert_eq!(touch.pending_swipes(), 0);
        assert!(touch.frame_keys().is_empty());
    }

    /// Dragging up or down the screen is not a lateral intent, however far it
    /// happens to wander sideways on the way.
    #[test]
    fn a_mostly_vertical_drag_is_not_a_lane_swipe() {
        let mut touch = rails_phone();
        let from = open_road(&touch);
        let far = touch.layout().unit * SWIPE_UNITS + 4.0;
        flick(&mut touch, from, far, far * SWIPE_HORIZONTAL_BIAS + 8.0);
        assert_eq!(touch.pending_swipes(), 0, "that was a drag, not a flick");
    }

    /// **One flick, one lane.** A finger dragged clean across the screen is
    /// still a single lane change: the gesture is spent the moment it fires, and
    /// the follow-through — which is most of a real flick — asks for nothing.
    #[test]
    fn one_swipe_is_one_lane_however_far_the_finger_goes() {
        let mut touch = rails_phone();
        let from = open_road(&touch);
        let step = touch.layout().unit * SWIPE_UNITS + 2.0;
        touch.press(1, from);
        // Six thresholds' worth of travel, in six separate moves, without ever
        // lifting. Under the gesture this replaced that was six lanes.
        (1..=6).for_each(|n| {
            touch.drag(1, Vec2::new(from.x + step * n as f32, from.y));
        });
        assert_eq!(touch.pending_swipes(), 1, "a held swipe is still one lane");

        assert_eq!(touch.frame_keys(), vec![PadButton::LaneRight.key()]);
        assert!(touch.frame_keys().is_empty());
        assert!(touch.frame_keys().is_empty(), "and nothing more is owed");
    }

    /// Dragging back the other way without lifting does not get a second lane
    /// either — in either direction, the finger is done.
    #[test]
    fn a_swipe_back_the_other_way_needs_a_new_finger() {
        let mut touch = rails_phone();
        let from = open_road(&touch);
        let step = touch.layout().unit * SWIPE_UNITS + 2.0;
        touch.press(1, from);
        touch.drag(1, Vec2::new(from.x + step, from.y));
        touch.drag(1, Vec2::new(from.x - step * 3.0, from.y));
        assert_eq!(touch.pending_swipes(), 1);

        // Lift, flick again: now it counts.
        touch.release(1);
        touch.press(1, from);
        touch.drag(1, Vec2::new(from.x - step, from.y));
        assert_eq!(touch.pending_swipes(), 2);
    }

    /// The lane has to change while the flick is still happening. The recogniser
    /// fires on the first move that clears the threshold, not on the lift, so
    /// the hop is already queued before the finger comes off the glass.
    #[test]
    fn the_lane_is_asked_for_mid_flick_rather_than_on_release() {
        let mut touch = rails_phone();
        let from = open_road(&touch);
        touch.press(1, from);
        touch.drag(1, Vec2::new(from.x + touch.layout().unit * SWIPE_UNITS + 1.0, from.y));
        assert_eq!(touch.pending_swipes(), 1, "queued before the lift");
        // And it is out on the very next frame — no queue to wait behind.
        assert_eq!(touch.frame_keys(), vec![PadButton::LaneRight.key()]);
    }

    /// The gap frame is the press-edge contract, so it is asserted as a property
    /// rather than as one hand-counted sequence: however many flicks land, no
    /// two consecutive frames may both carry the token, and none is lost.
    #[test]
    fn no_two_consecutive_frames_carry_a_swipe_token() {
        let mut touch = rails_phone();
        let from = open_road(&touch);
        let far = touch.layout().unit * SWIPE_UNITS + 2.0;
        // Five separate flicks, all landing before a single frame is drawn.
        (0..5).for_each(|_| flick(&mut touch, from, far, 0.0));
        assert_eq!(touch.pending_swipes(), 5);

        let frames: Vec<bool> = (0..12).map(|_| !touch.frame_keys().is_empty()).collect();
        assert!(
            frames.windows(2).all(|w| !(w[0] && w[1])),
            "a token was held across two frames: {frames:?}"
        );
        assert_eq!(
            frames.iter().filter(|carried| **carried).count(),
            5,
            "every queued hop is delivered, and none twice: {frames:?}"
        );
    }

    /// Lifting the finger does not cancel what it already earned: the hop is
    /// owed to the player, not to the pointer.
    #[test]
    fn a_hop_survives_the_finger_lifting_before_the_next_frame() {
        let mut touch = rails_phone();
        let from = open_road(&touch);
        let far = touch.layout().unit * SWIPE_UNITS + 4.0;
        touch.press(1, from);
        touch.drag(1, Vec2::new(from.x + far, from.y));
        touch.release(1);
        assert_eq!(touch.pending_swipes(), 1);
        assert_eq!(touch.frame_keys(), vec![PadButton::LaneRight.key()]);
    }

    /// A finger that starts on a button is operating that button. BOOST is a
    /// *held* control and the swipe covers the whole screen, so a thumb that
    /// slides while holding boost must not also be changing lane.
    #[test]
    fn a_drag_that_starts_on_a_button_holds_the_button_rather_than_swiping() {
        let mut touch = rails_phone();
        let boost = touch
            .layout()
            .slot(PadButton::Boost)
            .expect("the rails pad has a boost button")
            .centre;
        touch.press(1, boost);
        touch.drag(1, Vec2::new(boost.x + 400.0, boost.y));
        assert_eq!(touch.pending_swipes(), 0);
        assert!(touch.is_held(PadButton::Boost), "and it is still boosting");
    }

    /// The wheel game's lateral intent is the stick. A swipe there would be a
    /// second, contradictory way to ask for the same thing — and `lane_step` is
    /// not even read, so it would be a control that silently does nothing.
    #[test]
    fn the_wheel_game_has_no_swipe() {
        let mut touch = landscape();
        // The right half of the screen, so the steering zone does not claim it.
        let from = Vec2::new(touch.layout().viewport.x * 0.8, touch.layout().viewport.y * 0.35);
        assert!(touch.layout().hit(from).is_none());
        let far = touch.layout().unit * SWIPE_UNITS + 40.0;
        flick(&mut touch, from, far, 0.0);
        assert_eq!(touch.pending_swipes(), 0);
    }

    /// The stick and the swipe never both run: a rails screen has no stick, and
    /// a wheel screen never starts a swipe.
    #[test]
    fn the_two_lateral_gestures_are_mutually_exclusive() {
        let mut rails = rails_phone();
        let from = open_road(&rails);
        rails.press(1, from);
        assert!(rails.stick().is_none(), "no stick on rails");
        rails.drag(1, Vec2::new(from.x + 200.0, from.y));
        assert!(rails.stick().is_none());
        assert!(rails.pending_swipes() > 0);
    }

    /// A rotated phone has moved every button; a swipe measured against the old
    /// frame means nothing against the new one.
    #[test]
    fn resizing_drops_a_swipe_in_progress() {
        let mut touch = rails_phone();
        let from = open_road(&touch);
        touch.press(1, from);
        touch.resize(390.0, 844.0);
        touch.drag(1, Vec2::new(from.x + 400.0, from.y));
        assert_eq!(touch.pending_swipes(), 0);
    }

    #[test]
    fn the_layout_scales_with_the_viewport_and_stays_on_screen() {
        for (w, h) in [(390.0, 844.0), (844.0, 390.0), (1280.0, 720.0), (320.0, 240.0)] {
            let layout = PadLayout::for_viewport(w, h);
            assert!((UNIT_MIN..=UNIT_MAX).contains(&layout.unit));
            for slot in &layout.slots {
                assert!(
                    slot.centre.x - slot.radius >= 0.0 && slot.centre.x + slot.radius <= w,
                    "{:?} runs off the {w}x{h} viewport horizontally",
                    slot.button
                );
                assert!(
                    slot.centre.y - slot.radius >= 0.0 && slot.centre.y + slot.radius <= h,
                    "{:?} runs off the {w}x{h} viewport vertically",
                    slot.button
                );
                assert!(
                    slot.radius >= MIN_TOUCH_RADIUS,
                    "{:?} is {} px, below the {MIN_TOUCH_RADIUS} px thumb target",
                    slot.button,
                    slot.radius
                );
            }
        }
    }

    /// The bottom band's contract: whatever the frame, the pad leaves a strip
    /// along the bottom edge and no button reaches into it. The HUD's boost
    /// meter and controls legend are laid out inside that strip, so a pad that
    /// stopped honouring it would put the meter back through the buttons —
    /// which is exactly the defect `BOTTOM_STRIP_UNITS` was introduced to end.
    #[test]
    fn the_pad_leaves_the_bottom_strip_clear_on_every_frame() {
        for (w, h) in [(470.0, 836.0), (390.0, 844.0), (844.0, 390.0), (320.0, 240.0)] {
            for profile in [PlayProfile::Wheel, PlayProfile::Rails] {
                let layout = PadLayout::for_profile(w, h, profile);
                assert!(
                    layout.bottom_strip > layout.unit * 0.55,
                    "the strip must be deeper than the side margin, or it reserves nothing"
                );
                let floor = h - layout.bottom_strip;
                for slot in &layout.slots {
                    assert!(
                        slot.centre.y + slot.radius <= floor + 1.0e-3,
                        "{:?} reaches {} px into the strip the meter lives in on {w}x{h}",
                        slot.button,
                        slot.centre.y + slot.radius - floor
                    );
                }
            }
        }
    }

    /// The strip has to be tall enough to actually hold the two things it was
    /// reserved for: a two-line legend and the boost meter above it. `web.rs`
    /// places them at 0.06 and 0.66 of the strip; at the smallest unit the
    /// clamps allow, that must still leave the meter under the pad.
    #[test]
    fn the_bottom_strip_has_room_for_the_meter_and_the_legend() {
        let layout = PadLayout::for_viewport(320.0, 240.0);
        let strip = layout.bottom_strip;
        let legend_top = strip * 0.06 + (strip * 0.19).clamp(10.0, 13.0) * 1.45 * 2.0;
        let meter_top = strip * 0.66 + (strip * 0.24).clamp(12.0, 16.0) * 1.2;
        assert!(strip * 0.66 >= legend_top, "the meter would print on the legend");
        assert!(meter_top <= strip, "the meter would print on the pad");
    }

    #[test]
    fn every_button_is_present_distinct_and_does_not_overlap_its_neighbours() {
        // The wheel pad carries every button EXCEPT the lane hops, which belong
        // to the rails pad — see `the_rails_pad_swaps_the_stick_for_lane_buttons`.
        let layout = PadLayout::for_viewport(844.0, 390.0);
        let expected: Vec<PadButton> = PadButton::ALL
            .into_iter()
            .filter(|b| !matches!(b, PadButton::LaneLeft | PadButton::LaneRight))
            .collect();
        assert_eq!(layout.slots.len(), expected.len());
        for button in PadButton::ALL {
            assert!(!button.key().is_empty());
            assert!(!button.label().is_empty());
        }
        for button in expected {
            assert!(layout.slot(button).is_some(), "{button:?} is missing");
        }
        for (i, a) in layout.slots.iter().enumerate() {
            for b in layout.slots.iter().skip(i + 1) {
                let gap = (a.centre.x - b.centre.x).hypot(a.centre.y - b.centre.y);
                assert!(
                    gap > a.radius + b.radius,
                    "{:?} and {:?} overlap",
                    a.button,
                    b.button
                );
            }
        }
    }

    /// The cluster's colour-coding is the thing that gives the bottom of the
    /// frame a shape, so it is asserted rather than left to a stylesheet nobody
    /// tests: every button has an accent, and the two *modes* — the accelerator
    /// and the boost — do not share one with each other or with the neutrals.
    #[test]
    fn the_two_modal_buttons_are_colour_coded_apart_from_everything_else() {
        for button in PadButton::ALL {
            let accent = button.accent();
            assert!(accent.starts_with('#'), "{button:?} has no accent");
            assert_eq!(accent.len(), 7, "{button:?} accent is not a hex triplet");
        }
        let gas = PadButton::Accelerate.accent();
        let boost = PadButton::Boost.accent();
        assert_ne!(gas, boost, "GAS and BOOST must not read as the same button");
        for neutral in [PadButton::Brake, PadButton::Reset, PadButton::LaneLeft] {
            assert_eq!(neutral.accent(), NEUTRAL_ACCENT);
            assert_ne!(neutral.accent(), gas);
            assert_ne!(neutral.accent(), boost);
        }
        // The boost meter is painted from the same constant the button is, so a
        // future edit cannot drift the two apart.
        assert_eq!(boost, BOOST_ACCENT);
    }

    #[test]
    fn the_accelerator_is_the_biggest_button() {
        let layout = PadLayout::for_viewport(844.0, 390.0);
        let gas = layout.slot(PadButton::Accelerate).unwrap().radius;
        for slot in &layout.slots {
            if slot.button != PadButton::Accelerate {
                assert!(gas >= slot.radius, "{:?} is bigger than the gas", slot.button);
            }
        }
    }

    #[test]
    fn pressing_a_button_holds_it_and_lifting_releases_it() {
        let mut touch = landscape();
        let gas = touch.layout().slot(PadButton::Accelerate).unwrap().centre;
        assert!(!touch.is_held(PadButton::Accelerate));

        touch.press(1, gas);
        assert!(touch.is_held(PadButton::Accelerate));
        assert_eq!(touch.keys(), vec!["KeyW"]);
        assert!(touch.engaged(), "the first touch brings the pad up");

        touch.release(1);
        assert!(!touch.is_held(PadButton::Accelerate));
        assert!(touch.keys().is_empty());
        assert!(touch.engaged(), "and it stays up");
    }

    #[test]
    fn several_buttons_can_be_held_at_once() {
        let mut touch = landscape();
        let at = |b| touch.layout().slot(b).unwrap().centre;
        let (gas, boost, drift) = (
            at(PadButton::Accelerate),
            at(PadButton::Boost),
            at(PadButton::Handbrake),
        );
        touch.press(1, gas);
        touch.press(2, boost);
        touch.press(3, drift);
        let keys = touch.keys();
        assert!(keys.contains(&"KeyW"));
        assert!(keys.contains(&"ShiftLeft"));
        assert!(keys.contains(&"Space"));
        assert_eq!(keys.len(), 3);

        // Lifting one leaves the others held.
        touch.release(2);
        assert!(touch.is_held(PadButton::Accelerate));
        assert!(!touch.is_held(PadButton::Boost));
        assert!(touch.is_held(PadButton::Handbrake));
    }

    #[test]
    fn a_second_finger_on_a_held_button_does_not_release_it_when_it_lifts() {
        let mut touch = landscape();
        let gas = touch.layout().slot(PadButton::Accelerate).unwrap().centre;
        touch.press(1, gas);
        touch.press(2, gas);
        touch.release(2);
        assert!(
            touch.is_held(PadButton::Accelerate),
            "the first finger is still on it"
        );
        touch.release(1);
        assert!(!touch.is_held(PadButton::Accelerate));
    }

    #[test]
    fn the_joystick_appears_where_the_finger_lands() {
        let mut touch = landscape();
        let point = Vec2::new(120.0, 300.0);
        assert!(touch.layout().in_steering_zone(point));
        touch.press(7, point);
        let stick = touch.stick().expect("the stick appeared");
        assert_eq!(stick.origin, point);
        assert_eq!(stick.knob, point);
        assert_eq!(stick.pointer, 7);
        assert_eq!(touch.steer(), 0.0, "and it starts centred");
    }

    #[test]
    fn dragging_the_joystick_steers_both_ways() {
        let mut touch = landscape();
        let origin = Vec2::new(150.0, 300.0);
        touch.press(1, origin);
        let radius = touch.layout().stick_radius;

        touch.drag(1, Vec2::new(origin.x + radius, origin.y));
        assert!((touch.steer() - 1.0).abs() < 1.0e-5, "full right lock");

        touch.drag(1, Vec2::new(origin.x - radius, origin.y));
        assert!((touch.steer() + 1.0).abs() < 1.0e-5, "full left lock");

        touch.drag(1, Vec2::new(origin.x, origin.y));
        assert_eq!(touch.steer(), 0.0, "back to centre");
    }

    #[test]
    fn the_joystick_clamps_rather_than_running_away() {
        let mut touch = landscape();
        let origin = Vec2::new(150.0, 300.0);
        touch.press(1, origin);
        // Dragged clear off the screen.
        touch.drag(1, Vec2::new(origin.x + 5_000.0, origin.y + 5_000.0));
        assert!(touch.steer() <= 1.0 + 1.0e-6);
        let stick = touch.stick().unwrap();
        let travel = (stick.knob.x - origin.x).hypot(stick.knob.y - origin.y);
        assert!(
            (travel - touch.layout().stick_radius).abs() < 1.0e-3,
            "the knob is clamped to the ring: {travel}"
        );
    }

    #[test]
    fn the_deadzone_stops_a_resting_thumb_from_steering_but_full_lock_still_reaches_one() {
        let mut touch = landscape();
        let origin = Vec2::new(150.0, 300.0);
        touch.press(1, origin);
        let radius = touch.layout().stick_radius;

        touch.drag(1, Vec2::new(origin.x + radius * STICK_DEADZONE * 0.5, origin.y));
        assert_eq!(touch.steer(), 0.0, "inside the deadzone");

        touch.drag(1, Vec2::new(origin.x + radius * (STICK_DEADZONE + 0.01), origin.y));
        assert!(touch.steer() > 0.0, "just outside it");
        assert!(touch.steer() < 0.1, "and only just");

        touch.drag(1, Vec2::new(origin.x + radius, origin.y));
        assert!(
            (touch.steer() - 1.0).abs() < 1.0e-5,
            "the deadzone does not cost full lock"
        );
    }

    #[test]
    fn lifting_the_joystick_recentres_the_steering() {
        let mut touch = landscape();
        let origin = Vec2::new(150.0, 300.0);
        touch.press(1, origin);
        touch.drag(1, Vec2::new(origin.x + 200.0, origin.y));
        assert!(touch.steer() > 0.5);
        touch.release(1);
        assert!(touch.stick().is_none());
        assert_eq!(touch.steer(), 0.0);
    }

    #[test]
    fn a_press_on_a_button_never_starts_the_joystick() {
        let mut touch = phone();
        // The reset button sits top-right; the gas bottom-right. Neither is in
        // the steering zone, but the test is explicit about the rule.
        for button in PadButton::ALL {
            // Only the buttons this pad actually lays out.
            let Some(slot) = touch.layout().slot(button) else {
                continue;
            };
            let centre = slot.centre;
            let mut touch = touch.clone();
            touch.press(1, centre);
            assert!(touch.is_held(button));
            assert!(touch.stick().is_none(), "{button:?} started a joystick");
        }
        touch.release_all();
    }

    /// **The rails pad is one driving control.** Every absence below is a
    /// decision the game makes for the player rather than a control it forgot.
    #[test]
    fn the_rails_pad_is_boost_and_nothing_else() {
        let layout = PadLayout::for_profile(390.0, 844.0, PlayProfile::Rails);
        assert!(!layout.stick_enabled, "rails has no joystick");

        let boost = layout.slot(PadButton::Boost).expect("BOOST is the control");
        assert!(
            boost.centre.x < layout.viewport.x * 0.5,
            "BOOST lives on the left, clear of the hand that swipes: {}",
            boost.centre.x
        );

        // Lane hops are the swipe's job, the throttle is held for the player,
        // braking is not an answer to anything the lane game asks, and a railed
        // car has no slide to provoke.
        [
            PadButton::LaneLeft,
            PadButton::LaneRight,
            PadButton::Accelerate,
            PadButton::Brake,
            PadButton::Handbrake,
        ]
        .iter()
        .for_each(|gone| {
            assert!(
                layout.slot(*gone).is_none(),
                "{gone:?} is absent from the rails pad, not merely inert"
            );
        });

        // No stick means no steering zone, so a stray finger on the left of the
        // screen cannot start one.
        assert!(!layout.in_steering_zone(Vec2::new(10.0, 800.0)));
        // Nothing overlaps anything.
        layout.slots.iter().enumerate().for_each(|(i, a)| {
            layout.slots.iter().skip(i + 1).for_each(|b| {
                let gap = (a.centre.x - b.centre.x).hypot(a.centre.y - b.centre.y);
                assert!(
                    gap > a.radius + b.radius,
                    "{:?} overlaps {:?}",
                    a.button,
                    b.button
                );
            });
        });
    }

    #[test]
    fn the_wheel_pad_keeps_its_joystick() {
        let layout = PadLayout::for_profile(1440.0, 900.0, PlayProfile::Wheel);
        assert!(layout.stick_enabled);
        assert!(layout.slot(PadButton::Handbrake).is_some());
        assert!(layout.slot(PadButton::LaneLeft).is_none());
    }

    #[test]
    fn the_steering_zone_excludes_the_hud_and_the_button_side() {
        let touch = landscape();
        let layout = touch.layout();
        assert!(layout.in_steering_zone(Vec2::new(100.0, 320.0)), "lower left steers");
        assert!(
            !layout.in_steering_zone(Vec2::new(100.0, 20.0)),
            "the HUD strip does not"
        );
        assert!(
            !layout.in_steering_zone(Vec2::new(800.0, 320.0)),
            "the button side does not"
        );
        let gas = layout.slot(PadButton::Accelerate).unwrap().centre;
        assert!(!layout.in_steering_zone(gas), "and nor does a button");
    }

    #[test]
    fn only_one_joystick_exists_at_a_time() {
        let mut touch = landscape();
        touch.press(1, Vec2::new(120.0, 300.0));
        touch.press(2, Vec2::new(220.0, 340.0));
        let stick = touch.stick().expect("still one stick");
        assert_eq!(stick.pointer, 1, "the second finger does not steal it");
        // And the second pointer cannot drag it.
        touch.drag(2, Vec2::new(400.0, 300.0));
        assert_eq!(touch.steer(), 0.0);
    }

    #[test]
    fn dragging_without_a_stick_is_harmless() {
        let mut touch = landscape();
        touch.drag(9, Vec2::new(400.0, 200.0));
        assert!(touch.stick().is_none());
        assert_eq!(touch.steer(), 0.0);
        touch.release(9);
        touch.release_all();
    }

    #[test]
    fn rotating_the_device_relays_out_and_drops_everything_held() {
        let mut touch = landscape();
        let gas = touch.layout().slot(PadButton::Accelerate).unwrap().centre;
        touch.press(1, gas);
        touch.press(2, Vec2::new(120.0, 300.0));
        assert!(touch.is_held(PadButton::Accelerate) && touch.stick().is_some());

        touch.resize(390.0, 844.0);
        assert!(
            !touch.is_held(PadButton::Accelerate),
            "a finger cannot hold a button that moved out from under it"
        );
        assert!(touch.stick().is_none());
        assert_eq!(touch.layout().viewport, Vec2::new(390.0, 844.0));

        // Re-laying out at the same size changes nothing.
        touch.press(3, touch.layout().slot(PadButton::Boost).unwrap().centre);
        touch.resize(390.0, 844.0);
        assert!(touch.is_held(PadButton::Boost), "an unchanged viewport is a no-op");
    }

    #[test]
    fn engaging_shows_the_pad_before_the_first_tap() {
        let mut touch = landscape();
        assert!(!touch.engaged());
        touch.engage();
        assert!(touch.engaged());
    }

    #[test]
    fn a_degenerate_viewport_still_produces_a_usable_layout() {
        let touch = TouchControls::new(0.0, 0.0);
        assert!(touch.layout().unit >= UNIT_MIN);
        assert!(touch.layout().stick_radius > 0.0);
        assert_eq!(touch.steer(), 0.0);
        assert!(touch.keys().is_empty());
    }

    #[test]
    fn the_keys_are_in_a_stable_order() {
        let mut touch = landscape();
        let at = |b| touch.layout().slot(b).unwrap().centre;
        let (gas, brake) = (at(PadButton::Accelerate), at(PadButton::Brake));
        touch.press(1, brake);
        touch.press(2, gas);
        // Declared order, not press order — so a frame's key list is stable.
        assert_eq!(touch.keys(), vec!["KeyW", "KeyS"]);
    }
}
