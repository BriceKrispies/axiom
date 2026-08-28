//! Ported from Claude-of-Duty `src/world/layout.js:1-453`.
//!
//! WORLD — the map. A Middle-Eastern market street in the spirit of Crash / Backlot:
//! one long main street running -Z, buildings tight to both kerbs, two flanking alleys,
//! a plaza, and an arched gate closing the far vista. All coordinates are in LEVEL space;
//! the WorldSystem rotates the whole thing so the street runs down the canonical
//! hero-shot camera axis.
//!
//! Sides: 0 = -Z, 1 = +X, 2 = +Z, 3 = -X.

use std::f64::consts::PI;

pub struct Street {
    pub half_width: f64,
    pub kerb: f64,
    pub walk_h: f64,
    pub z_min: f64,
    pub z_max: f64,
}

pub const STREET: Street = Street {
    half_width: 4.5, // asphalt
    kerb: 6.5,       // building line
    walk_h: 0.145,
    z_min: -58.0,
    z_max: 46.0,
};

/// Alleys and open ground, as rects [x0, z0, x1, z1].
pub struct Alley {
    pub x0: f64,
    pub z0: f64,
    pub x1: f64,
    pub z1: f64,
    pub surface: &'static str,
}

pub const ALLEYS: &[Alley] = &[
    Alley {
        x0: -27.0,
        z0: -12.2,
        x1: -6.5,
        z1: -8.2,
        surface: "dirt",
    }, // west alley (mid street)
    Alley {
        x0: -26.0,
        z0: 20.2,
        x1: -6.5,
        z1: 24.2,
        surface: "dirt",
    }, // west alley (near)
    Alley {
        x0: -25.0,
        z0: 5.6,
        x1: -6.5,
        z1: 9.6,
        surface: "dirt",
    }, // west courtyard — lets the late sun through onto the market, and flanks the main street
    Alley {
        x0: 6.5,
        z0: 1.8,
        x1: 29.0,
        z1: 7.8,
        surface: "dirt",
    }, // east alley — main flank
    Alley {
        x0: 6.5,
        z0: -14.2,
        x1: 29.0,
        z1: -30.2,
        surface: "gravel",
    }, // yard behind the ruin
    Alley {
        x0: -30.0,
        z0: -50.0,
        x1: 30.0,
        z1: -44.0,
        surface: "dirt",
    }, // far cross street
];

/// Per-floor setback (`spec.setback = { from, depth, side? }`,
/// `layout.js` building entries, consumed by `buildings.js`'s `floorSpec`/
/// `terrace`). Pulls every floor at or above `from` back from one face by
/// `depth`, leaving a roof terrace over the floor below.
#[derive(Debug, Clone, Copy)]
pub struct Setback {
    pub from: u32,
    pub depth: f64,
    /// `side ?? spec.streetSide` (`buildings.js:91`) — `None` means "resolve
    /// to `street_side` at the call site", matching that fallback exactly.
    pub side: Option<u32>,
}

/// One hand-authored `doorBays[side] = bay` entry (`buildings.js:299`):
/// force bay `bay` of `side` to be a ground-floor door.
#[derive(Debug, Clone, Copy)]
pub struct DoorBay {
    pub side: u32,
    pub bay: i32,
}

/// One hand-authored `bayKinds[side][floor][bay] = kind | {kind, drop}`
/// override (`buildings.js:310-318`) — a bay carrying a sightline the map
/// depends on, forced by hand rather than by the dice. `drop` is read only
/// when `kind == "shop"` (`buildings.js:340`, `forced?.drop`).
#[derive(Debug, Clone, Copy)]
pub struct BayOverride {
    pub side: u32,
    pub floor: u32,
    pub bay: u32,
    pub kind: &'static str,
    pub drop: Option<f64>,
}

/// One `stairFlights[]` entry (`buildings.js`'s `buildInterior`).
#[derive(Debug, Clone, Copy)]
pub struct StairFlight {
    pub floor: u32,
    pub x: f64,
    pub z: f64,
    pub ry: f64,
    pub w: f64,
    /// `fl.railing ?? 'right'` (`buildings.js:712`) — every flight in
    /// `BUILDINGS` sets this explicitly, so the fallback never actually
    /// fires; kept as a plain string (not `kit::StairRailing`) to keep this
    /// data module free of a dependency on the kit's element vocabulary.
    pub railing: &'static str,
}

/// One `stairHoles[level] = {x0,x1,z0,z1}` entry: the stairwell void punched
/// through the floor slab at `level` (`buildings.js`'s `interiorSlab`).
#[derive(Debug, Clone, Copy)]
pub struct StairHole {
    pub level: u32,
    pub x0: f64,
    pub x1: f64,
    pub z0: f64,
    pub z1: f64,
}

/// One interior partition wall (`rooms[f].walls[]`, normalised 0..1 across
/// the interior footprint, `buildings.js`'s `buildInterior`). `door_at` is
/// the normalised position along the wall of a door opening, or `None` for
/// a solid partition (the source's `wall[4]` being `undefined`).
#[derive(Debug, Clone, Copy)]
pub struct RoomWall {
    pub ax: f64,
    pub az: f64,
    pub bx: f64,
    pub bz: f64,
    pub door_at: Option<f64>,
}

/// One `rooms[f].furnish[]` entry: a normalised rect handed to `furnishRoom`
/// (`src/world/interiors.js`) — **not yet ported** (a concurrent slice), so
/// `crate::world::buildings::build_interior` records this data but does not
/// yet act on it. See that module's doc for the deferral.
#[derive(Debug, Clone, Copy)]
pub struct RoomFurnish {
    pub kind: &'static str,
    pub x0: f64,
    pub z0: f64,
    pub x1: f64,
    pub z1: f64,
}

/// One `rooms[]` entry: one floor's partition plan.
#[derive(Debug, Clone, Copy)]
pub struct RoomPlan {
    pub walls: &'static [RoomWall],
    pub furnish: &'static [RoomFurnish],
}

/// Buildings. `w` is the X extent, `d` the Z extent.
/// Interiors are described in normalised room coordinates (0..1 across the interior),
/// so a plan survives a change of footprint.
///
/// Every field below `damage` is optional in the source (`buildings.js`
/// reads each through `spec.field ?? default` or `spec.field?.`) and is
/// carried here even where a given building never sets it, so every literal
/// below states its defaults explicitly (`None`/`false`/`&[]`) rather than
/// leaving them to a `Default` impl a `const` array cannot use.
#[derive(Debug, Clone, Copy)]
pub struct Building {
    pub id: &'static str,
    pub x: f64,
    pub z: f64,
    pub w: f64,
    pub d: f64,
    pub floors: u32,
    pub wall_key: &'static str,
    pub street_side: u32,
    pub damage: f64,
    pub setback: Option<Setback>,
    /// `spec.trimKey` (`buildings.js:506,515`) — string-course/cornice
    /// material, defaulting to `"concrete"` when absent.
    pub trim_key: Option<&'static str>,
    /// `spec.secondarySide` (`buildings.js:280`) — a second "open" facade
    /// besides `street_side` (shops/arches/balconies roll there too).
    pub secondary_side: Option<u32>,
    /// `spec.balconies` (`buildings.js:305`) — per-bay balcony-door
    /// probability on an open facade, floor >= 1. `None` resolves to the
    /// source's default `0.35`.
    pub balconies: Option<f64>,
    /// `spec.arches` (`buildings.js:304`) — floor 1 rolls arched windows
    /// instead of plain ones.
    pub arches: bool,
    pub door_bays: &'static [DoorBay],
    pub bay_kinds: &'static [BayOverride],
    /// `spec.enterable` (`buildings.js:216,370`) — builds a real partitioned
    /// interior instead of a dark core, and lets ground-floor windows show a
    /// lit room behind them.
    pub enterable: bool,
    /// `spec.roofAccess` (`buildings.js:743`) — a stair penthouse box on the
    /// roof.
    pub roof_access: bool,
    pub stair_flights: &'static [StairFlight],
    pub stair_holes: &'static [StairHole],
    pub rooms: &'static [RoomPlan],
    /// `spec.ruin` (`buildings.js:288,443`) — the top floor's facade top
    /// edge is jagged rather than flat, on `ruin_side` (or `street_side`).
    pub ruin: bool,
    pub ruin_side: Option<u32>,
    /// `spec.collapse` — flags this building for `collapseRoof` (a hole in
    /// the roof slab + a rubble heap below), applied by a caller outside
    /// `buildings.js` that decides the hole's position.
    pub collapse: bool,
    /// `spec.skipSides` (`buildings.js:188`) — sides never built at all
    /// (background buildings whose far face is never seen).
    pub skip_sides: &'static [u32],
    /// `spec.roofProps` (`dressing.js:1554`, `roofProps ?? 2`) — drives the
    /// roof-clutter count in the dressing pass (`(roofProps * 2.4).round() +
    /// 2` items). Every real `BUILDINGS` entry sets this explicitly.
    pub roof_props: u32,
}

pub const BUILDINGS: &[Building] = &[
    // ------------------------------------------------------------- west row --
    Building {
        id: "W5",
        x: -12.5,
        z: 31.0,
        w: 12.0,
        d: 14.0,
        floors: 2,
        wall_key: "plaster_cream",
        street_side: 1,
        damage: 0.15,
        setback: Some(Setback { from: 1, depth: 2.2, side: Some(1) }),
        trim_key: None,
        secondary_side: Some(0),
        balconies: Some(0.3),
        arches: false,
        door_bays: &[DoorBay { side: 1, bay: 1 }],
        bay_kinds: &[],
        enterable: false,
        roof_access: false,
        stair_flights: &[],
        stair_holes: &[],
        rooms: &[],
        ruin: false,
        ruin_side: None,
        collapse: false,
        skip_sides: &[],
        roof_props: 3,
    },
    Building {
        id: "W1",
        x: -13.5,
        z: 15.0,
        w: 14.0,
        d: 10.0,
        floors: 2,
        wall_key: "plaster_cream",
        street_side: 1,
        damage: 0.25,
        setback: Some(Setback { from: 1, depth: 2.6, side: Some(1) }),
        trim_key: Some("concrete"),
        secondary_side: Some(2),
        balconies: Some(0.55),
        arches: true,
        door_bays: &[DoorBay { side: 1, bay: 1 }],
        bay_kinds: &[],
        enterable: false,
        roof_access: false,
        stair_flights: &[],
        stair_holes: &[],
        rooms: &[],
        ruin: false,
        ruin_side: None,
        collapse: false,
        skip_sides: &[],
        roof_props: 4,
    },
    Building {
        id: "W2",
        x: -14.0,
        z: -1.5,
        w: 15.0,
        d: 13.0,
        floors: 2,
        wall_key: "plaster_sand",
        street_side: 1,
        damage: 0.3,
        setback: Some(Setback { from: 1, depth: 2.4, side: Some(1) }),
        trim_key: None,
        secondary_side: Some(0),
        balconies: Some(0.6),
        arches: false,
        door_bays: &[DoorBay { side: 1, bay: 2 }],
        // The interior camera stands in the shop and looks out through bay 1
        // of the street facade, so that bay is an open shopfront by hand,
        // not by dice (`buildings.js:89`).
        bay_kinds: &[BayOverride { side: 1, floor: 0, bay: 1, kind: "shop", drop: Some(0.0) }],
        enterable: true,
        roof_access: false,
        stair_flights: &[StairFlight { floor: 0, x: 0.14, z: 0.28, ry: 0.0, w: 1.2, railing: "right" }],
        stair_holes: &[StairHole { level: 1, x0: -20.4, x1: -18.0, z0: -4.8, z1: 1.5 }],
        rooms: &[
            RoomPlan {
                // ground floor: a shop opening onto the street, storage and a back room
                walls: &[
                    RoomWall { ax: 0.55, az: 0.0, bx: 0.55, bz: 1.0, door_at: Some(0.34) },
                    RoomWall { ax: 0.0, az: 0.52, bx: 0.55, bz: 0.52, door_at: Some(0.7) },
                ],
                furnish: &[
                    RoomFurnish { kind: "shop", x0: 0.55, z0: 0.0, x1: 1.0, z1: 1.0 },
                    RoomFurnish { kind: "storage", x0: 0.0, z0: 0.0, x1: 0.55, z1: 0.52 },
                    RoomFurnish { kind: "living", x0: 0.0, z0: 0.52, x1: 0.55, z1: 1.0 },
                ],
            },
            RoomPlan {
                // first floor: apartment
                walls: &[
                    RoomWall { ax: 0.48, az: 0.0, bx: 0.48, bz: 1.0, door_at: Some(0.62) },
                    RoomWall { ax: 0.48, az: 0.45, bx: 1.0, bz: 0.45, door_at: Some(0.25) },
                ],
                furnish: &[
                    RoomFurnish { kind: "living", x0: 0.48, z0: 0.45, x1: 1.0, z1: 1.0 },
                    RoomFurnish { kind: "storage", x0: 0.48, z0: 0.0, x1: 1.0, z1: 0.45 },
                    RoomFurnish { kind: "living", x0: 0.0, z0: 0.0, x1: 0.48, z1: 1.0 },
                ],
            },
            RoomPlan {
                walls: &[RoomWall { ax: 0.5, az: 0.0, bx: 0.5, bz: 1.0, door_at: Some(0.5) }],
                furnish: &[
                    RoomFurnish { kind: "ruin", x0: 0.5, z0: 0.0, x1: 1.0, z1: 1.0 },
                    RoomFurnish { kind: "storage", x0: 0.0, z0: 0.0, x1: 0.5, z1: 1.0 },
                ],
            },
        ],
        ruin: false,
        ruin_side: None,
        collapse: false,
        skip_sides: &[],
        roof_props: 5,
    },
    Building {
        id: "W3",
        x: -13.0,
        z: -19.0,
        w: 13.0,
        d: 14.0,
        floors: 2,
        wall_key: "plaster_blue",
        street_side: 1,
        damage: 0.55,
        setback: None,
        trim_key: None,
        secondary_side: Some(2),
        balconies: Some(0.3),
        arches: false,
        door_bays: &[DoorBay { side: 1, bay: 0 }],
        bay_kinds: &[],
        enterable: false,
        roof_access: false,
        stair_flights: &[],
        stair_holes: &[],
        rooms: &[],
        ruin: true,
        ruin_side: Some(1),
        collapse: false,
        skip_sides: &[],
        roof_props: 2,
    },
    Building {
        id: "W4",
        x: -14.5,
        z: -34.5,
        w: 16.0,
        d: 15.0,
        floors: 2,
        wall_key: "plaster_pink",
        street_side: 1,
        damage: 0.3,
        setback: Some(Setback { from: 1, depth: 2.8, side: Some(1) }),
        trim_key: None,
        secondary_side: Some(0),
        balconies: Some(0.5),
        arches: true,
        door_bays: &[DoorBay { side: 1, bay: 2 }],
        bay_kinds: &[],
        enterable: false,
        roof_access: false,
        stair_flights: &[],
        stair_holes: &[],
        rooms: &[],
        ruin: false,
        ruin_side: None,
        collapse: false,
        skip_sides: &[],
        roof_props: 4,
    },
    // ------------------------------------------------------------- east row --
    Building {
        id: "E5",
        x: 12.5,
        z: 33.0,
        w: 12.0,
        d: 14.0,
        floors: 2,
        wall_key: "plaster_blue",
        street_side: 3,
        damage: 0.2,
        setback: None,
        trim_key: None,
        secondary_side: None,
        balconies: None,
        arches: false,
        door_bays: &[DoorBay { side: 3, bay: 2 }],
        bay_kinds: &[],
        enterable: false,
        roof_access: false,
        stair_flights: &[],
        stair_holes: &[],
        rooms: &[],
        ruin: false,
        ruin_side: None,
        collapse: false,
        skip_sides: &[],
        roof_props: 3,
    },
    Building {
        id: "E1",
        x: 14.0,
        z: 16.0,
        w: 15.0,
        d: 16.0,
        floors: 3,
        wall_key: "plaster_cream",
        street_side: 3,
        damage: 0.3,
        setback: None,
        trim_key: None,
        secondary_side: Some(0),
        balconies: Some(0.45),
        arches: false,
        door_bays: &[DoorBay { side: 3, bay: 2 }, DoorBay { side: 0, bay: 1 }],
        bay_kinds: &[],
        enterable: true,
        roof_access: true,
        stair_flights: &[
            StairFlight { floor: 0, x: 0.72, z: 0.12, ry: 0.0, w: 1.2, railing: "right" },
            StairFlight { floor: 1, x: 0.72, z: 0.12, ry: 0.0, w: 1.2, railing: "right" },
        ],
        stair_holes: &[
            StairHole { level: 1, x0: 16.3, x1: 18.1, z0: 9.4, z1: 16.2 },
            StairHole { level: 2, x0: 16.3, x1: 18.1, z0: 9.4, z1: 16.2 },
        ],
        rooms: &[
            RoomPlan {
                walls: &[
                    RoomWall { ax: 0.0, az: 0.42, bx: 0.62, bz: 0.42, door_at: Some(0.3) },
                    RoomWall { ax: 0.62, az: 0.0, bx: 0.62, bz: 0.42, door_at: Some(0.5) },
                ],
                furnish: &[
                    RoomFurnish { kind: "shop", x0: 0.0, z0: 0.42, x1: 0.62, z1: 1.0 },
                    RoomFurnish { kind: "storage", x0: 0.0, z0: 0.0, x1: 0.62, z1: 0.42 },
                ],
            },
            RoomPlan {
                walls: &[RoomWall { ax: 0.0, az: 0.45, bx: 0.62, bz: 0.45, door_at: Some(0.72) }],
                furnish: &[
                    RoomFurnish { kind: "living", x0: 0.0, z0: 0.45, x1: 0.62, z1: 1.0 },
                    RoomFurnish { kind: "ruin", x0: 0.0, z0: 0.0, x1: 0.62, z1: 0.45 },
                ],
            },
        ],
        ruin: false,
        ruin_side: None,
        collapse: false,
        skip_sides: &[],
        roof_props: 6,
    },
    Building {
        id: "E2",
        x: 13.5,
        z: -5.0,
        w: 14.0,
        d: 14.0,
        floors: 3,
        wall_key: "plaster_blue",
        street_side: 3,
        damage: 0.3,
        setback: None,
        trim_key: None,
        secondary_side: Some(2),
        balconies: Some(0.7),
        arches: false,
        door_bays: &[DoorBay { side: 3, bay: 1 }],
        bay_kinds: &[],
        enterable: false,
        roof_access: false,
        stair_flights: &[],
        stair_holes: &[],
        rooms: &[],
        ruin: false,
        ruin_side: None,
        collapse: false,
        skip_sides: &[],
        roof_props: 5,
    },
    Building {
        id: "E3",
        x: 14.0,
        z: -22.0,
        w: 15.0,
        d: 16.0,
        floors: 2,
        wall_key: "plaster_sand",
        street_side: 3,
        damage: 0.75,
        setback: None,
        trim_key: None,
        secondary_side: Some(0),
        balconies: None,
        arches: false,
        door_bays: &[DoorBay { side: 3, bay: 1 }],
        bay_kinds: &[],
        enterable: true,
        roof_access: false,
        stair_flights: &[],
        stair_holes: &[],
        rooms: &[
            RoomPlan {
                walls: &[RoomWall { ax: 0.45, az: 0.0, bx: 0.45, bz: 0.7, door_at: Some(0.4) }],
                furnish: &[
                    RoomFurnish { kind: "ruin", x0: 0.45, z0: 0.0, x1: 1.0, z1: 1.0 },
                    RoomFurnish { kind: "storage", x0: 0.0, z0: 0.0, x1: 0.45, z1: 0.7 },
                    RoomFurnish { kind: "ruin", x0: 0.0, z0: 0.7, x1: 0.45, z1: 1.0 },
                ],
            },
            RoomPlan {
                walls: &[],
                furnish: &[RoomFurnish { kind: "ruin", x0: 0.0, z0: 0.0, x1: 1.0, z1: 1.0 }],
            },
        ],
        ruin: true,
        ruin_side: Some(3),
        collapse: true,
        skip_sides: &[],
        roof_props: 2,
    },
    Building {
        id: "E4",
        x: 13.5,
        z: -39.0,
        w: 14.0,
        d: 14.0,
        floors: 3,
        wall_key: "plaster_pink",
        street_side: 3,
        damage: 0.35,
        setback: None,
        trim_key: None,
        secondary_side: Some(2),
        balconies: Some(0.4),
        arches: true,
        door_bays: &[DoorBay { side: 3, bay: 2 }],
        bay_kinds: &[],
        enterable: false,
        roof_access: false,
        stair_flights: &[],
        stair_holes: &[],
        rooms: &[],
        ruin: false,
        ruin_side: None,
        collapse: false,
        skip_sides: &[],
        roof_props: 4,
    },
    // ------------------------------------------------- background / infill --
    // The mass BEHIND the gate. Only its top four metres and its roofline are
    // visible — through the sliver of sky over the arch spandrel — but that is the
    // whole point: it is the third plane of depth that stops the terminator
    // reading as a flat cut-out, and it is offset west so a slice of real sky
    // survives on the east side of the gap.
    Building {
        id: "BS3",
        x: -4.0,
        z: -53.0,
        w: 9.0,
        d: 8.0,
        floors: 4,
        wall_key: "plaster_sand",
        street_side: 2,
        damage: 0.3,
        setback: None,
        trim_key: None,
        secondary_side: None,
        balconies: Some(0.2),
        arches: false,
        door_bays: &[],
        bay_kinds: &[],
        enterable: false,
        roof_access: false,
        stair_flights: &[],
        stair_holes: &[],
        rooms: &[],
        ruin: false,
        ruin_side: None,
        collapse: false,
        skip_sides: &[],
        roof_props: 4,
    },
    Building {
        id: "BW1",
        x: -30.0,
        z: 8.0,
        w: 16.0,
        d: 22.0,
        floors: 3,
        wall_key: "plaster_sand",
        street_side: 1,
        damage: 0.15,
        setback: None,
        trim_key: None,
        secondary_side: None,
        balconies: None,
        arches: false,
        door_bays: &[],
        bay_kinds: &[],
        enterable: false,
        roof_access: false,
        stair_flights: &[],
        stair_holes: &[],
        rooms: &[],
        ruin: false,
        ruin_side: None,
        collapse: false,
        skip_sides: &[1],
        roof_props: 3,
    },
    Building {
        id: "BW2",
        x: -31.0,
        z: -18.0,
        w: 18.0,
        d: 24.0,
        floors: 2,
        wall_key: "plaster_cream",
        street_side: 1,
        damage: 0.2,
        setback: None,
        trim_key: None,
        secondary_side: None,
        balconies: None,
        arches: false,
        door_bays: &[],
        bay_kinds: &[],
        enterable: false,
        roof_access: false,
        stair_flights: &[],
        stair_holes: &[],
        rooms: &[],
        ruin: false,
        ruin_side: None,
        collapse: false,
        skip_sides: &[1],
        roof_props: 2,
    },
    Building {
        id: "BE1",
        x: 31.0,
        z: 18.0,
        w: 18.0,
        d: 20.0,
        floors: 3,
        wall_key: "plaster_pink",
        street_side: 3,
        damage: 0.15,
        setback: None,
        trim_key: None,
        secondary_side: None,
        balconies: None,
        arches: false,
        door_bays: &[],
        bay_kinds: &[],
        enterable: false,
        roof_access: false,
        stair_flights: &[],
        stair_holes: &[],
        rooms: &[],
        ruin: false,
        ruin_side: None,
        collapse: false,
        skip_sides: &[3],
        roof_props: 3,
    },
    Building {
        id: "BE2",
        x: 32.0,
        z: -8.0,
        w: 18.0,
        d: 20.0,
        floors: 2,
        wall_key: "plaster_blue",
        street_side: 3,
        damage: 0.2,
        setback: None,
        trim_key: None,
        secondary_side: None,
        balconies: None,
        arches: false,
        door_bays: &[],
        bay_kinds: &[],
        enterable: false,
        roof_access: false,
        stair_flights: &[],
        stair_holes: &[],
        rooms: &[],
        ruin: false,
        ruin_side: None,
        collapse: false,
        skip_sides: &[3],
        roof_props: 2,
    },
    Building {
        id: "BE3",
        x: 30.0,
        z: -34.0,
        w: 16.0,
        d: 18.0,
        floors: 3,
        wall_key: "plaster_cream",
        street_side: 3,
        damage: 0.25,
        setback: None,
        trim_key: None,
        secondary_side: None,
        balconies: None,
        arches: false,
        door_bays: &[],
        bay_kinds: &[],
        enterable: false,
        roof_access: false,
        stair_flights: &[],
        stair_holes: &[],
        rooms: &[],
        ruin: false,
        ruin_side: None,
        collapse: false,
        skip_sides: &[3],
        roof_props: 2,
    },
    // BS1/BS2 pulled apart to make room for BS3 in the middle of the far skyline.
    Building {
        id: "BS1",
        x: -19.0,
        z: -58.0,
        w: 20.0,
        d: 14.0,
        floors: 3,
        wall_key: "plaster_sand",
        street_side: 2,
        damage: 0.2,
        setback: None,
        trim_key: None,
        secondary_side: None,
        balconies: None,
        arches: false,
        door_bays: &[],
        bay_kinds: &[],
        enterable: false,
        roof_access: false,
        stair_flights: &[],
        stair_holes: &[],
        rooms: &[],
        ruin: false,
        ruin_side: None,
        collapse: false,
        skip_sides: &[],
        roof_props: 2,
    },
    Building {
        id: "BS2",
        x: 14.0,
        z: -60.0,
        w: 24.0,
        d: 16.0,
        floors: 2,
        wall_key: "plaster_blue",
        street_side: 2,
        damage: 0.2,
        setback: None,
        trim_key: None,
        secondary_side: None,
        balconies: None,
        arches: false,
        door_bays: &[],
        bay_kinds: &[],
        enterable: false,
        roof_access: false,
        stair_flights: &[],
        stair_holes: &[],
        rooms: &[],
        ruin: false,
        ruin_side: None,
        collapse: false,
        skip_sides: &[],
        roof_props: 2,
    },
    Building {
        id: "BN1",
        x: -16.0,
        z: 50.0,
        w: 20.0,
        d: 14.0,
        floors: 2,
        wall_key: "plaster_cream",
        street_side: 0,
        damage: 0.15,
        setback: None,
        trim_key: None,
        secondary_side: None,
        balconies: None,
        arches: false,
        door_bays: &[],
        bay_kinds: &[],
        enterable: false,
        roof_access: false,
        stair_flights: &[],
        stair_holes: &[],
        rooms: &[],
        ruin: false,
        ruin_side: None,
        collapse: false,
        skip_sides: &[],
        roof_props: 2,
    },
    Building {
        id: "BN2",
        x: 14.0,
        z: 52.0,
        w: 22.0,
        d: 16.0,
        floors: 3,
        wall_key: "plaster_pink",
        street_side: 0,
        damage: 0.15,
        setback: None,
        trim_key: None,
        secondary_side: None,
        balconies: None,
        arches: false,
        door_bays: &[],
        bay_kinds: &[],
        enterable: false,
        roof_access: false,
        stair_flights: &[],
        stair_holes: &[],
        rooms: &[],
        ruin: false,
        ruin_side: None,
        collapse: false,
        skip_sides: &[],
        roof_props: 2,
    },
];

/// The street terminator at the south end of the vista.
///
/// This is the surface the eye lands on in eight of the eleven canonical shots,
/// so it is not one flat crenellated slab: it is a mass of four blocks at four
/// different heights, stepped in Z as well as Y, with a pointed archway through
/// the middle and a genuine sliver of sky over the arch that shows the receding
/// roofline of `BS3` behind it. Three planes of depth (bastion / gatehouse /
/// background block) is what makes the alley read as continuing rather than as
/// ending at a wall.
pub struct Gate {
    pub z: f64,
    pub depth: f64,
    pub span: f64,
    pub height: f64,
    pub outer_w: f64,
    /// Height over the arch spandrel — deliberately the LOWEST part of the mass.
    pub body_h: f64,
    /// West block.
    pub x_l0: f64,
    pub x_l1: f64,
    pub h_l: f64,
    /// East block, standing half a metre proud of the arch.
    pub x_r0: f64,
    pub x_r1: f64,
    pub h_r: f64,
    pub east_proud: f64,
    /// Tower. Standing 1.5 m proud toward the camera matters for a specific
    /// reason: at 16:30 the sun is in the level's -X/-Z quadrant, so the whole
    /// north elevation of the terminator is in shade and its WEST returns are in
    /// full sun. Pushing the tower forward turns that return into a wide sunlit
    /// flank facing the camera — two stops brighter than the shaded face beside it,
    /// which is the value break the elevation needs.
    pub x_t0: f64,
    pub x_t1: f64,
    pub h_t: f64,
    pub tower_proud: f64,
}

pub const GATE: Gate = Gate {
    z: -42.5,
    depth: 3.2,
    span: 5.6,
    height: 4.9,
    outer_w: 17.0,
    body_h: 6.7,
    x_l0: -8.6,
    x_l1: -2.8,
    h_l: 7.9,
    x_r0: 2.8,
    x_r1: 6.1,
    h_r: 9.5,
    east_proud: 0.55,
    x_t0: 6.1,
    x_t1: 9.4,
    h_t: 12.4,
    tower_proud: 1.5,
};

/// Hand-placed set pieces. Dressing adds the hundreds of small props around these.
pub struct SetPieces {
    pub stalls: &'static [[f64; 4]],
    pub jerseys: &'static [[f64; 3]],
    pub sandbag_walls: &'static [[f64; 4]],
    pub wrecks: &'static [[f64; 4]],
    pub palms: &'static [[f64; 3]],
    pub lamps: &'static [[f64; 3]],
    pub cables: &'static [[f64; 7]],
    pub laundry: &'static [[f64; 6]],
    pub hangings: &'static [[f64; 6]],
    pub rubble: &'static [[f64; 4]],
    pub tyres: &'static [[f64; 3]],
}

pub const SET_PIECES: SetPieces = SetPieces {
    // Market stalls: [x, z, ry, width]
    stalls: &[
        [-3.2, 6.4, 0.08, 2.4],
        [-3.0, 2.2, -0.05, 2.2],
        [3.1, 9.5, 3.2, 2.4],
        [3.4, 4.0, 3.05, 2.6],
        [-0.4, 2.6, 1.62, 2.3],
        [3.0, -9.0, 3.25, 2.2],
        [-3.3, -14.5, 0.12, 2.4],
        [2.9, -20.0, 3.0, 2.3],
    ],
    // Jersey barriers: [x, z, ry]
    jerseys: &[
        [-2.6, 17.5, 0.12],
        [-0.4, 16.2, 1.5],
        [2.9, 12.0, -0.1],
        [1.6, -2.5, 1.62],
        [-2.4, -6.0, 0.05],
        [3.2, -16.0, 0.1],
        [-1.0, -24.0, 1.55],
        [1.2, -30.0, 0.2],
        [-3.0, -34.0, 0.0],
    ],
    // Sandbag emplacements: [x, z, ry, length]
    sandbag_walls: &[
        [-3.6, 11.0, 0.0, 3.0],
        [3.6, -2.0, 0.0, 2.6],
        [-1.6, -18.5, 1.57, 2.4],
        [3.4, -27.0, 0.0, 3.2],
    ],
    // Burnt-out vehicles: [x, z, ry, rollDeg]
    wrecks: &[
        [2.5, 0.5, 0.42, 0.0],
        [-2.8, -28.5, -2.6, 4.0],
        [4.9, 24.0, 1.5, 0.0],
    ],
    // Palm trees: [x, z, scale]
    palms: &[
        [-5.4, 20.0, 1.0],
        [5.5, 6.5, 1.1],
        [-5.5, -4.5, 0.92],
        [5.6, -20.5, 1.05],
        [-5.5, -32.0, 1.0],
        [8.5, 5.0, 0.85],
        [-9.0, -10.2, 0.9],
    ],
    // Street lamps: [x, z, ry] — ry points the arm across the street.
    lamps: &[
        [-5.9, 15.0, -PI / 2.0],
        [5.9, 3.0, PI / 2.0],
        [-5.9, -11.0, -PI / 2.0],
        [5.9, -24.0, PI / 2.0],
        [-5.9, -36.0, -PI / 2.0],
    ],
    // Overhead cable spans: [x0, y0, z0, x1, y1, z1, sag]
    cables: &[
        [-6.4, 7.2, 10.0, 6.4, 6.6, 12.5, 1.1],
        [-6.4, 8.4, -2.0, 6.4, 7.9, -0.5, 1.4],
        [-6.4, 6.2, -16.0, 6.4, 6.6, -14.5, 1.0],
        [-6.4, 7.6, -30.0, 6.4, 7.2, -28.0, 1.2],
        [-6.4, 5.4, 19.0, -6.4, 5.6, 24.5, 0.6],
        [6.4, 5.6, 2.0, 6.4, 5.4, 8.0, 0.7],
    ],
    // Laundry lines with hanging cloth: [x0, y0, z0, x1, y1, z1]
    // Kept off the main sightline and up at balcony height: lines that cross the
    // street at eye level clutter the vista and read as floating cards.
    laundry: &[
        [6.35, 3.6, 9.0, 6.35, 3.75, 14.2],
        [-6.35, 3.7, 1.0, -6.35, 3.6, 5.4],
        [-6.35, 6.6, -20.5, -6.35, 6.4, -15.5],
        [6.35, 6.5, -6.0, 6.35, 6.7, -1.0],
        [-6.35, 3.65, -27.0, -6.35, 3.8, -22.0],
        [6.4, 3.7, 21.0, 6.4, 3.6, 25.5],
    ],
    // Hanging rugs / cloth on facades: [x, y, z, ry, w, h]
    hangings: &[
        [-6.45, 2.6, 8.5, PI / 2.0, 1.5, 2.1],
        [-6.45, 2.4, 4.5, PI / 2.0, 1.2, 1.7],
        [6.45, 2.7, 6.0, -PI / 2.0, 1.6, 2.2],
        [6.45, 2.5, -8.5, -PI / 2.0, 1.3, 1.9],
        [-6.45, 2.5, -16.0, PI / 2.0, 1.4, 2.0],
    ],
    // Rubble piles: [x, z, radius, count]
    rubble: &[
        [-4.2, -20.5, 2.4, 34.0],
        [5.0, -14.5, 2.8, 40.0],
        [-1.5, -40.0, 2.0, 26.0],
        [7.6, -30.5, 2.2, 28.0],
        [-5.0, 26.0, 1.6, 18.0],
    ],
    // Tyre stacks: [x, z, n]
    tyres: &[
        [-5.2, 12.5, 4.0],
        [5.3, -6.0, 3.0],
        [6.2, 3.0, 5.0],
        [-5.4, -28.0, 3.0],
    ],
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_building_count() {
        assert_eq!(BUILDINGS.len(), 20, "Expected 20 buildings (5 west + 5 east + 10 background)");
    }

    #[test]
    fn test_set_piece_counts() {
        assert_eq!(SET_PIECES.stalls.len(), 8, "Expected 8 stalls");
        assert_eq!(SET_PIECES.jerseys.len(), 9, "Expected 9 jersey barriers");
        assert_eq!(SET_PIECES.sandbag_walls.len(), 4, "Expected 4 sandbag walls");
        assert_eq!(SET_PIECES.wrecks.len(), 3, "Expected 3 wrecks");
        assert_eq!(SET_PIECES.palms.len(), 7, "Expected 7 palms");
        assert_eq!(SET_PIECES.lamps.len(), 5, "Expected 5 lamps");
        assert_eq!(SET_PIECES.cables.len(), 6, "Expected 6 cable spans");
        assert_eq!(SET_PIECES.laundry.len(), 6, "Expected 6 laundry lines");
        assert_eq!(SET_PIECES.hangings.len(), 5, "Expected 5 hangings");
        assert_eq!(SET_PIECES.rubble.len(), 5, "Expected 5 rubble piles");
        assert_eq!(SET_PIECES.tyres.len(), 4, "Expected 4 tyre stacks");
    }

    #[test]
    fn test_street_bounds() {
        assert_eq!(STREET.half_width, 4.5);
        assert_eq!(STREET.kerb, 6.5);
        assert_eq!(STREET.z_min, -58.0);
        assert_eq!(STREET.z_max, 46.0);
    }

    #[test]
    fn test_building_coordinates() {
        // West buildings
        assert_eq!(BUILDINGS[0].id, "W5");
        assert_eq!(BUILDINGS[0].x, -12.5);
        assert_eq!(BUILDINGS[0].z, 31.0);

        // East buildings
        assert_eq!(BUILDINGS[5].id, "E5");
        assert_eq!(BUILDINGS[5].x, 12.5);
        assert_eq!(BUILDINGS[5].z, 33.0);

        // Background infill
        assert_eq!(BUILDINGS[17].id, "BS2");
        assert_eq!(BUILDINGS[17].x, 14.0);
        assert_eq!(BUILDINGS[17].z, -60.0);
    }

    #[test]
    fn test_roof_props() {
        // `spec.roofProps` (`layout.js`) drives the dressing pass's roof
        // clutter density; every real BUILDINGS entry sets it explicitly.
        assert_eq!(BUILDINGS[0].id, "W5");
        assert_eq!(BUILDINGS[0].roof_props, 3);
        assert_eq!(BUILDINGS[6].id, "E1");
        assert_eq!(BUILDINGS[6].roof_props, 6);
        assert_eq!(BUILDINGS[19].id, "BN2");
        assert_eq!(BUILDINGS[19].roof_props, 2);
    }

    #[test]
    fn test_alley_coordinates() {
        // West alley (mid street)
        assert_eq!(ALLEYS[0].x0, -27.0);
        assert_eq!(ALLEYS[0].z0, -12.2);
        assert_eq!(ALLEYS[0].surface, "dirt");

        // East alley
        assert_eq!(ALLEYS[3].x0, 6.5);
        assert_eq!(ALLEYS[3].z0, 1.8);
        assert_eq!(ALLEYS[3].surface, "dirt");
    }

    #[test]
    fn test_gate_dimensions() {
        assert_eq!(GATE.z, -42.5);
        assert_eq!(GATE.span, 5.6);
        assert_eq!(GATE.height, 4.9);
        assert_eq!(GATE.x_t0, 6.1);
        assert_eq!(GATE.h_t, 12.4);
    }

    #[test]
    fn test_lamp_angles() {
        // First lamp should be at -PI/2
        let lamp_ry = SET_PIECES.lamps[0][2];
        assert!((lamp_ry - (-PI / 2.0)).abs() < 1e-10, "Lamp angle mismatch");

        // Second lamp should be at PI/2
        let lamp_ry_2 = SET_PIECES.lamps[1][2];
        assert!((lamp_ry_2 - (PI / 2.0)).abs() < 1e-10, "Lamp angle mismatch");
    }

    #[test]
    fn test_stall_coordinates() {
        // First stall
        assert_eq!(SET_PIECES.stalls[0][0], -3.2);
        assert_eq!(SET_PIECES.stalls[0][1], 6.4);
        assert_eq!(SET_PIECES.stalls[0][3], 2.4);
    }
}
