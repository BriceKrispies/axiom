//! The deterministic penalty-kick scene composition: the object model plus the
//! fixed constants and ordered builders that lay out every diorama object.
//!
//! Everything here is a pure function of compile-time constants. There is no
//! wall-clock time, no randomness, and no unordered iteration: objects are
//! emitted into an explicit `Vec` in a fixed order and given sequential,
//! stable [`ObjectId`]s. Rebuilding always yields byte-identical output.
//!
//! Each object carries its *semantic* [`DioramaRole`], its geometry, and a
//! [`PenaltyMaterialId`] — but **not** a draw layer or a raw color. The render
//! plan (`penalty_render_plan`) maps role → draw layer and resolves +
//! flat-shades the material. Keeping "what a thing is" separate from "when it
//! draws" and "how it is colored" is what lets each pass own one concern.
//!
//! ## Coordinate convention (app-local)
//! - `+X` is right, `+Y` is up, `+Z` runs from the goal toward the kicker and
//!   the camera.
//! - The goal line sits at `z = 0`; the ball/penalty spot at `z = PENALTY_SPOT_Z`.
//! - The camera looks down `-Z` toward the goal (see `static_diorama.rs`).

use axiom_math::{Quat, Vec3};

use crate::soccer_penalty::low_poly_assets::PrimitiveShape;
use crate::soccer_penalty::penalty_blob_shadow::BLOB_SHADOWS;
use crate::soccer_penalty::penalty_goalie_pose::PenaltyGoaliePose;
use crate::soccer_penalty::penalty_kicker;
use crate::soccer_penalty::penalty_materials::PenaltyMaterialId;

/// A stable identifier for one diorama object, assigned in deterministic build
/// order. It is the *stable object ordinal* the render plan uses as the final
/// tie-breaker in its sort key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectId(pub u32);

/// The semantic group an object belongs to. The render plan maps each role to a
/// [`crate::soccer_penalty::penalty_render_plan::PenaltyDrawLayer`]; roles never drive runtime
/// control flow directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DioramaRole {
    Field,
    FieldLine,
    PenaltySpot,
    GoalFrame,
    RearNet,
    FrontNet,
    Kicker,
    Ball,
    Goalie,
    StadiumWall,
    CrowdCard,
    AdBoard,
    BlobShadow,
    /// A ball-trail sample during flight (Pass 5). Rendered in ForegroundEffects.
    BallTrail,
    /// A goalie save-volume debug marker (Pass 6), only emitted when goalie
    /// debug visualization is enabled. Rendered in ForegroundEffects.
    GoalieDebugVolume,
    /// A Pass 10 impact-polish flash / mark. Rendered in ForegroundEffects.
    ImpactEffect,
}

/// One fully-described diorama object: a single flat-shaded primitive placed in
/// the world. This is app-local scene data, not an engine scene node.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DioramaObject {
    pub id: ObjectId,
    pub role: DioramaRole,
    pub shape: PrimitiveShape,
    /// World-space center of the primitive.
    pub position: Vec3,
    /// Full extents for a `Box`/`Quad`; `size.x` is the radius for a `FacetedBall`.
    pub size: Vec3,
    /// Local orientation of the primitive about its center. `Quat::IDENTITY` for
    /// every axis-aligned object (field/goal/net/ball/backdrop); the humanoid kit
    /// authors real rotations here so posed angular limbs (a bent knee, an angled
    /// arm, a leaning torso) render at their pose orientation.
    pub rotation: Quat,
    /// The named material this object is drawn with (color lives in the palette).
    pub material: PenaltyMaterialId,
    /// A stable, greppable name for this object (e.g. `"kicker.torso"`).
    pub label: &'static str,
}

// ---------------------------------------------------------------------------
// Fixed geometry constants (meters). These are the "clear constants for field
// dimensions, goal dimensions, and object placement" the app is built from.
// ---------------------------------------------------------------------------

// Field.
pub const FIELD_HALF_WIDTH: f32 = 34.0;
pub const FIELD_FAR_Z: f32 = -6.0;
pub const FIELD_NEAR_Z: f32 = 20.0;
pub const GRASS_BAND_COUNT: u32 = 8;
pub const GROUND_Y: f32 = 0.0;

// Goal (FIFA-ish: 7.32 m wide, 2.44 m tall).
pub const GOAL_HALF_WIDTH: f32 = 3.66;
pub const GOAL_HEIGHT: f32 = 2.44;
pub const POST_THICKNESS: f32 = 0.12;
pub const GOAL_LINE_Z: f32 = 0.0;
// Shallow net so it reads as a mesh DRAPING just behind the mouth (as in the
// reference), not a deep free-floating wireframe cage. Pulled in from 1.9.
pub const NET_DEPTH: f32 = 0.95;

// Penalty markings.
pub const PENALTY_SPOT_Z: f32 = 11.0;
pub const PENALTY_BOX_FRONT_Z: f32 = 16.5;
pub const PENALTY_BOX_HALF_WIDTH: f32 = 20.15;
pub const GOAL_AREA_FRONT_Z: f32 = 5.5;
pub const GOAL_AREA_HALF_WIDTH: f32 = 9.16;
// Thicker so the visible goal-area / goal-line markings read crisply from the
// elevated camera (they were hairline-thin).
pub const LINE_THICKNESS: f32 = 0.17;

// Actors. The kicker is nearest the camera, the ball sits between kicker and
// goal (on the spot), the goalie stands just in front of the goal line.
pub const KICKER_X: f32 = -0.7;
pub const KICKER_Z: f32 = 12.6;
pub const GOALIE_X: f32 = 0.0;
pub const GOALIE_Z: f32 = 0.5;
pub const BALL_RADIUS: f32 = 0.32; // exaggerated for readability (real ~0.11 m)

// Backdrop.
pub const STADIUM_WALL_Z: f32 = -4.6;
// A low, dark barrier wall: it occludes the pitch behind the goal and gives the
// lowest crowd row a base to rise from. Kept short so the crowd fills down close
// to the goal top (as in the reference) instead of leaving a tall dead band.
pub const STADIUM_WALL_HEIGHT: f32 = 1.2;
pub const CROWD_CARD_COUNT: u32 = 44;
/// Vertical sub-cells each terrace band is diced into, so the crowd reads as a
/// granular mass of individual spectators (as in the reference) rather than a
/// handful of tall monolithic slabs.
pub const CROWD_ROW_CELLS: u32 = 3;
pub const AD_BOARD_COUNT: u32 = 9;
pub const AD_BOARD_Z: f32 = -2.6;
pub const AD_BOARD_AXIOM_INDEX: u32 = 2;

/// Accumulates diorama objects while handing out sequential stable ids.
struct SceneBuilder {
    objects: Vec<DioramaObject>,
    next_id: u32,
}

impl SceneBuilder {
    fn new() -> Self {
        Self { objects: Vec::new(), next_id: 0 }
    }

    fn emit(
        &mut self,
        role: DioramaRole,
        shape: PrimitiveShape,
        position: Vec3,
        size: Vec3,
        material: PenaltyMaterialId,
        label: &'static str,
    ) {
        self.emit_rotated(role, shape, position, Quat::IDENTITY, size, material, label);
    }

    /// Emit an object with an explicit local rotation (the humanoid kit uses this
    /// for posed angular limbs). `emit` forwards here with `Quat::IDENTITY`.
    #[allow(clippy::too_many_arguments)]
    fn emit_rotated(
        &mut self,
        role: DioramaRole,
        shape: PrimitiveShape,
        position: Vec3,
        rotation: Quat,
        size: Vec3,
        material: PenaltyMaterialId,
        label: &'static str,
    ) {
        self.objects.push(DioramaObject {
            id: ObjectId(self.next_id),
            role,
            shape,
            position,
            size,
            rotation,
            material,
            label,
        });
        self.next_id += 1;
    }
}

/// Build the full, ordered list of diorama objects from fixed constants.
///
/// The build order determines the stable [`ObjectId`] assignment. Draw ordering
/// is a separate concern owned by the render plan.
pub fn build_penalty_objects() -> Vec<DioramaObject> {
    let mut b = SceneBuilder::new();
    field(&mut b);
    field_markings(&mut b);
    blob_shadows(&mut b);
    goal_frame(&mut b);
    net(&mut b);
    kicker(&mut b);
    ball(&mut b);
    goalie(&mut b);
    backdrop(&mut b);
    b.objects
}

fn field(b: &mut SceneBuilder) {
    let width = FIELD_HALF_WIDTH * 2.0;
    let depth = FIELD_NEAR_Z - FIELD_FAR_Z;
    let center_z = (FIELD_NEAR_Z + FIELD_FAR_Z) * 0.5;
    b.emit(
        DioramaRole::Field,
        PrimitiveShape::Quad,
        Vec3::new(0.0, GROUND_Y, center_z),
        Vec3::new(width, 0.0, depth),
        PenaltyMaterialId::DarkerGrassBand,
        "field.plane",
    );
    // Alternating light/dark grass bands running across the pitch (constant
    // count, deterministic material by parity). Each band lies within the base
    // plane's extent, so the plane always sorts behind them (see the render
    // plan's depth-bucket rule).
    let band_depth = depth / GRASS_BAND_COUNT as f32;
    (0..GRASS_BAND_COUNT).for_each(|i| {
        let z = FIELD_FAR_Z + band_depth * (i as f32 + 0.5);
        let material = [PenaltyMaterialId::FieldGrass, PenaltyMaterialId::DarkerGrassBand][(i % 2) as usize];
        b.emit(
            DioramaRole::Field,
            PrimitiveShape::Quad,
            Vec3::new(0.0, GROUND_Y + 0.005, z),
            Vec3::new(width, 0.0, band_depth * 0.94),
            material,
            "field.band",
        );
    });
}

fn field_markings(b: &mut SceneBuilder) {
    let mut line = |x: f32, z: f32, len_x: f32, len_z: f32, label: &'static str| {
        b.emit(
            DioramaRole::FieldLine,
            PrimitiveShape::Quad,
            Vec3::new(x, GROUND_Y + 0.02, z),
            Vec3::new(len_x, 0.0, len_z),
            PenaltyMaterialId::WhiteFieldLines,
            label,
        );
    };
    // Goal line and the two nested boxes (penalty box + goal area), each three
    // segments: front edge and two sides.
    line(0.0, GOAL_LINE_Z, PENALTY_BOX_HALF_WIDTH * 2.0, LINE_THICKNESS, "line.goal");
    line(0.0, PENALTY_BOX_FRONT_Z, PENALTY_BOX_HALF_WIDTH * 2.0, LINE_THICKNESS, "line.box.front");
    line(-PENALTY_BOX_HALF_WIDTH, PENALTY_BOX_FRONT_Z * 0.5, LINE_THICKNESS, PENALTY_BOX_FRONT_Z, "line.box.left");
    line(PENALTY_BOX_HALF_WIDTH, PENALTY_BOX_FRONT_Z * 0.5, LINE_THICKNESS, PENALTY_BOX_FRONT_Z, "line.box.right");
    line(0.0, GOAL_AREA_FRONT_Z, GOAL_AREA_HALF_WIDTH * 2.0, LINE_THICKNESS, "line.area.front");
    line(-GOAL_AREA_HALF_WIDTH, GOAL_AREA_FRONT_Z * 0.5, LINE_THICKNESS, GOAL_AREA_FRONT_Z, "line.area.left");
    line(GOAL_AREA_HALF_WIDTH, GOAL_AREA_FRONT_Z * 0.5, LINE_THICKNESS, GOAL_AREA_FRONT_Z, "line.area.right");
    // The penalty spot itself.
    b.emit(
        DioramaRole::PenaltySpot,
        PrimitiveShape::Quad,
        Vec3::new(0.0, GROUND_Y + 0.021, PENALTY_SPOT_Z),
        Vec3::new(0.34, 0.0, 0.34),
        PenaltyMaterialId::WhiteFieldLines,
        "spot.penalty",
    );
}

fn blob_shadows(b: &mut SceneBuilder) {
    // Fake flat blob shadows from fixed descriptors (see penalty_blob_shadow).
    // Emitted as ground quads in the ActorShadow layer, before the actors.
    BLOB_SHADOWS.iter().for_each(|s| {
        b.emit(
            DioramaRole::BlobShadow,
            PrimitiveShape::Quad,
            s.center,
            Vec3::new(s.radius_x * 2.0, 0.0, s.radius_z * 2.0),
            PenaltyMaterialId::BlobShadow,
            s.label,
        );
    });
}

fn goal_frame(b: &mut SceneBuilder) {
    let mut post = |x: f32, y: f32, sx: f32, sy: f32, sz: f32, label: &'static str| {
        b.emit(
            DioramaRole::GoalFrame,
            PrimitiveShape::Box,
            Vec3::new(x, y, GOAL_LINE_Z),
            Vec3::new(sx, sy, sz),
            PenaltyMaterialId::GoalFrameWhite,
            label,
        );
    };
    post(-GOAL_HALF_WIDTH, GOAL_HEIGHT * 0.5, POST_THICKNESS, GOAL_HEIGHT, POST_THICKNESS, "goal.post.left");
    post(GOAL_HALF_WIDTH, GOAL_HEIGHT * 0.5, POST_THICKNESS, GOAL_HEIGHT, POST_THICKNESS, "goal.post.right");
    post(0.0, GOAL_HEIGHT, GOAL_HALF_WIDTH * 2.0 + POST_THICKNESS, POST_THICKNESS, POST_THICKNESS, "goal.crossbar");
}

fn net(b: &mut SceneBuilder) {
    // A real net pocket behind the goal mouth, built from a FINE grid of thin
    // white bars (not a textured plane): a back wall, a top roof, and two side
    // walls sloping back from the goal line to `back_z`. Bar geometry reads as a
    // net on EVERY backend — WebGPU, WebGL2, and the flat Canvas2D fallback (which
    // ignores textures) — so the three stay in sync, and the see-through gaps let
    // the crowd show through. All rear-layer: the keeper (z = 0.5) stands in front.
    let hw = GOAL_HALF_WIDTH;
    let h = GOAL_HEIGHT;
    let back_z = GOAL_LINE_Z - NET_DEPTH;
    let mid_z = GOAL_LINE_Z - NET_DEPTH * 0.5;
    // Strand gauge: 0.038 m. Thin enough to still read as a hanging MESH (not a
    // stack of chunky bars), but thick enough to SURVIVE rasterization and the
    // canvas2d sub-pixel cull at this camera distance — the earlier 0.02 strands
    // fell below a pixel and the net all but vanished, leaving the goal reading as
    // an open frame instead of the bright dense curtain the reference shows.
    let t = 0.038;
    let mut bar = |pos: Vec3, size: Vec3, tag: &'static str| {
        b.emit(DioramaRole::RearNet, PrimitiveShape::Line, pos, size, PenaltyMaterialId::NetOffWhite, tag);
    };
    // Back wall: 30 verticals × 15 horizontals — a fine, dense mesh (was 24 × 12),
    // the main visible net face draped just behind the mouth.
    (0..=29).for_each(|i| {
        let x = -hw + (hw * 2.0) * (i as f32 / 29.0);
        bar(Vec3::new(x, h * 0.5, back_z), Vec3::new(t, h, t), "net.back.v");
    });
    (0..=14).for_each(|j| {
        let y = h * (j as f32 / 14.0);
        bar(Vec3::new(0.0, y, back_z), Vec3::new(hw * 2.0, t, t), "net.back.h");
    });
    // Top roof: depth strands from the crossbar back to the top of the back wall.
    (0..=11).for_each(|i| {
        let x = -hw + (hw * 2.0) * (i as f32 / 11.0);
        bar(Vec3::new(x, h, mid_z), Vec3::new(t, t, NET_DEPTH), "net.top");
    });
    // Side walls: depth strands down each edge.
    (0..=6).for_each(|k| {
        let y = h * (k as f32 / 6.0);
        bar(Vec3::new(-hw, y, mid_z), Vec3::new(t, t, NET_DEPTH), "net.left");
        bar(Vec3::new(hw, y, mid_z), Vec3::new(t, t, NET_DEPTH), "net.right");
    });
}

/// Emit the articulated kicker: the shared `axiom-figure` kicker posed at its
/// run-up display frame (a planted/cocked stride, not the limp T-pose rest).
/// Unlike the old frozen box puppet, this is the same data the lab
/// authors and scrubs; the per-frame kick pose is overlaid in
/// `soccer_penalty_app` (like the goalie's dive), driven by the shot so the
/// strike lands as the ball is struck.
fn kicker(b: &mut SceneBuilder) {
    let boxes = penalty_kicker::KickerRig::new().boxes_at(penalty_kicker::DISPLAY_FRAME);
    boxes.iter().for_each(|kb| {
        // Emit each part at its bone ROTATION (like the goalie), so limbs render as
        // oriented capsules along their bones instead of axis-aligned sticks.
        b.emit_rotated(DioramaRole::Kicker, PrimitiveShape::Box, kb.center, kb.rotation, kb.size, kb.material, kb.label);
    });
    // A dark hair cap over the kicker's head. The reference #10 — the largest,
    // nearest subject — has prominent black hair, but the shared figure's head is
    // bare skin. Emitted as a plain (non-rig) box sized/placed from the idle head
    // box, mirroring `goalie.hair`; the per-frame kicker pose overlay passes this
    // label straight through, so it holds this rest position (the head barely
    // moves across the kick). Reuses the existing dark hair material — no palette
    // change.
    if let Some(head) = boxes.iter().find(|kb| kb.label == "kicker.head") {
        let cap_h = head.size.y * 0.42;
        b.emit_rotated(
            DioramaRole::Kicker,
            PrimitiveShape::Box,
            head.center.add(Vec3::new(0.0, head.size.y * 0.5 - cap_h * 0.4, 0.0)),
            head.rotation,
            Vec3::new(head.size.x * 1.06, cap_h, head.size.z * 1.06),
            PenaltyMaterialId::GoalieHair,
            "kicker.hair",
        );
    }
}

fn goalie(b: &mut SceneBuilder) {
    // Pass 7: the goalie is an articulated 16-part puppet rig, emitted here at
    // its idle rest pose. The app overlays the sampled dive pose per frame (see
    // `soccer_penalty_app`), so at rest this is the goalie you see.
    let idle = PenaltyGoaliePose::idle_display().resolve();
    idle.parts().iter().for_each(|part| {
        // Emit each bone with its resolved WORLD rotation, not identity. The
        // smooth-figure renderer draws every limb as a capsule along its local Y
        // and orients it by this rotation, so discarding it (via `emit`) collapsed
        // the authored wide-arm braced stance into vertical capsules stacked beside
        // the head — the keeper read as arms-up "goalpost" instead of the
        // reference's arms-spread, knees-bent set position. Passing the bone
        // rotation lays each capsule along its bone so the pose finally reads.
        b.emit_rotated(
            DioramaRole::Goalie,
            PrimitiveShape::Box,
            part.world.translation,
            part.world.rotation,
            part.size,
            part.material,
            part.kind.label(),
        );
    });
    // A hair cap over the head. Emitted as a plain (non-rig) object — the goalie
    // pose overlay passes labels it doesn't own straight through, so it sits at
    // this rest position (the head barely moves at the idle stance).
    b.emit(
        DioramaRole::Goalie,
        PrimitiveShape::Box,
        Vec3::new(GOALIE_X, 1.97, GOALIE_Z),
        Vec3::new(0.28, 0.14, 0.28),
        PenaltyMaterialId::GoalieHair,
        "goalie.hair",
    );
}

fn ball(b: &mut SceneBuilder) {
    let center = Vec3::new(0.0, BALL_RADIUS, PENALTY_SPOT_Z);
    b.emit(
        DioramaRole::Ball,
        PrimitiveShape::FacetedBall,
        center,
        Vec3::new(BALL_RADIUS, BALL_RADIUS, BALL_RADIUS),
        PenaltyMaterialId::BallWhite,
        "ball",
    );
    // The classic dark pentagon panels are now baked into the ball's surface
    // texture (`recipe_textures::ball` → `TextureOp::Spots`, mapped through the
    // sphere's UVs), not stamped here as separate world-space quads. That is the
    // fix for the panels "floating" at the penalty spot: as part of the sphere
    // surface they carry the ball's per-frame pose automatically (and will roll
    // with it once the ball is given spin). The single `"ball"` renderable above
    // is the whole ball.
}

fn backdrop(b: &mut SceneBuilder) {
    // Stadium wall behind the goal.
    b.emit(
        DioramaRole::StadiumWall,
        PrimitiveShape::Box,
        Vec3::new(0.0, STADIUM_WALL_HEIGHT * 0.5, STADIUM_WALL_Z),
        Vec3::new(FIELD_HALF_WIDTH * 2.0, STADIUM_WALL_HEIGHT, 0.4),
        PenaltyMaterialId::StadiumWallDarkGray,
        "stadium.wall",
    );
    // Fake crowd: a row of billboard cards above the wall, cycling three muted
    // crowd materials deterministically by index.
    let crowd_materials = [
        PenaltyMaterialId::CrowdMutedColors,
        PenaltyMaterialId::CrowdMutedColorsAltA,
        PenaltyMaterialId::CrowdMutedColorsAltB,
    ];
    // The crowd is a dense, tall, multi-coloured band packed against the dark
    // stand: two stacked rows of many cards, rising from the top of the wall so
    // the whole upper backdrop reads as a stadium of people (as in the
    // reference) rather than a flat grey slab with a few floating cards.
    let span = FIELD_HALF_WIDTH * 1.9;
    let card_w = span / CROWD_CARD_COUNT as f32;
    // Three stacked, interleaved rows filling from just above the low wall up
    // into the stand, so the whole upper backdrop reads as a packed terrace.
    // Four dense rows starting right at the low wall's top so the crowd fills the
    // whole backdrop down to just above the goal (no tall dead band as before).
    let rows = [
        (2.0_f32, 2.4_f32, 0.0_f32),
        (3.7, 2.4, 0.5),
        (5.4, 2.4, 0.0),
        (7.1, 2.4, 0.5),
    ];
    // Each terrace band is also diced VERTICALLY into `CROWD_ROW_CELLS` short
    // cells, so the crowd reads as a fine granular mass of individual spectators
    // (as in the reference) rather than a handful of tall monolithic slabs. The
    // material cycles on a 2D (column, cell) index so vertically-adjacent cells
    // differ too — a speckle in both axes, not vertical stripes. Cell height
    // stays well above a pixel at this backdrop distance, so the denser field
    // survives rasterization and the canvas2d sub-pixel cull on every backend.
    rows.iter().enumerate().for_each(|(row, &(y, height, phase))| {
        let cell_h = height / CROWD_ROW_CELLS as f32;
        (0..CROWD_CARD_COUNT).for_each(|i| {
            // Half-card horizontal offset on the upper row so the two rows
            // interleave like real terrace seating rather than lining up.
            let x = -span * 0.5 + card_w * (i as f32 + 0.5 + phase);
            (0..CROWD_ROW_CELLS).for_each(|s| {
                let cy = y - height * 0.5 + cell_h * (s as f32 + 0.5);
                let material = crowd_materials[(i as usize + row * 2 + s as usize) % 3];
                b.emit(
                    DioramaRole::CrowdCard,
                    PrimitiveShape::Box,
                    Vec3::new(x, cy, STADIUM_WALL_Z - 0.3),
                    Vec3::new(card_w * 0.9, cell_h * 0.86, 0.2),
                    material,
                    "crowd.card",
                );
            });
        });
    });
    // Bright ad hoardings ringing the goal, alternating red "AXIOM" and blue
    // "SPORTS" boards (as in the reference) — taller and wider than before so they
    // read as a prominent band in front of the crowd.
    let ad_span = GOAL_HALF_WIDTH * 3.4;
    (0..AD_BOARD_COUNT).for_each(|i| {
        let t = i as f32 / (AD_BOARD_COUNT - 1) as f32;
        let x = -ad_span * 0.5 + ad_span * t;
        let is_axiom = i % 2 == 0;
        let material = [PenaltyMaterialId::AdBoardDark, PenaltyMaterialId::AdBoardRed][is_axiom as usize];
        let label = ["ad.board", "ad.board.axiom"][is_axiom as usize];
        b.emit(
            DioramaRole::AdBoard,
            PrimitiveShape::Box,
            Vec3::new(x, 0.62, AD_BOARD_Z),
            Vec3::new(ad_span / AD_BOARD_COUNT as f32 * 0.94, 1.25, 0.12),
            material,
            label,
        );
    });
}
