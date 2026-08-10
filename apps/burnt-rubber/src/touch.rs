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
    /// Hop one lane left. Rails only — the wheel game steers with the stick.
    LaneLeft,
    /// Hop one lane right. Rails only.
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

    /// The rails pad: lane buttons under the left thumb, GAS/BOOST/BRAKE under
    /// the right, and no joystick.
    ///
    /// DRIFT is absent on purpose rather than disabled — a railed car has no
    /// slide to provoke, so a handbrake button would be a control that visibly
    /// does nothing. BRAKE stays: longitudinal force is shared between the two
    /// games, so it still slows the car exactly as it always did.
    pub fn rails_for_viewport(width: f32, height: f32) -> PadLayout {
        let viewport = Vec2::new(width.max(1.0), height.max(1.0));
        let unit = (viewport.x.min(viewport.y) * UNIT_FRACTION).clamp(UNIT_MIN, UNIT_MAX);
        let margin = unit * 0.55;
        let bottom_strip = (unit * BOTTOM_STRIP_UNITS).max(BOTTOM_STRIP_MIN);
        let right = viewport.x - margin;
        let bottom = viewport.y - bottom_strip;
        let left = margin;
        // The lane buttons are the primary control and are sized like the
        // accelerator, side by side so a thumb can roll between them without
        // looking. They sit at the bottom-left, mirroring GAS at bottom-right.
        let slots = vec![
            PadSlot {
                button: PadButton::LaneLeft,
                centre: Vec2::new(left + unit, bottom - unit),
                radius: unit,
            },
            PadSlot {
                button: PadButton::LaneRight,
                centre: Vec2::new(left + unit * 3.15, bottom - unit),
                radius: unit,
            },
            PadSlot {
                button: PadButton::Accelerate,
                centre: Vec2::new(right - unit, bottom - unit),
                radius: unit,
            },
            PadSlot {
                button: PadButton::Boost,
                centre: Vec2::new(right - unit * 1.05, bottom - unit * 2.90),
                radius: unit * 0.74,
            },
            PadSlot {
                button: PadButton::Brake,
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
        }
    }

    /// A pointer moved to `point`.
    pub fn drag(&mut self, pointer: i32, point: Vec2) {
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

    /// A pointer lifted.
    pub fn release(&mut self, pointer: i32) {
        self.held.retain(|(_, p)| *p != pointer);
        if self.stick.map(|s| s.pointer) == Some(pointer) {
            self.stick = None;
        }
    }

    /// Release everything — a lost pointer capture, a backgrounded tab.
    pub fn release_all(&mut self) {
        self.held.clear();
        self.stick = None;
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

    #[test]
    fn the_rails_pad_swaps_the_stick_for_lane_buttons() {
        let layout = PadLayout::for_profile(390.0, 844.0, PlayProfile::Rails);
        assert!(
            !layout.stick_enabled,
            "rails has no joystick to contradict the lane buttons"
        );
        assert!(layout.slot(PadButton::LaneLeft).is_some());
        assert!(layout.slot(PadButton::LaneRight).is_some());
        // The two controls the brief keeps.
        assert!(layout.slot(PadButton::Accelerate).is_some());
        assert!(layout.slot(PadButton::Boost).is_some());
        // A railed car cannot slide, so a drift button would do nothing.
        assert!(
            layout.slot(PadButton::Handbrake).is_none(),
            "DRIFT is absent on rails, not merely inert"
        );
        // No stick means no steering zone, so a stray finger on the left of the
        // screen cannot start one.
        assert!(!layout.in_steering_zone(Vec2::new(10.0, 800.0)));
        // The lane buttons must not overlap each other or anything else.
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
