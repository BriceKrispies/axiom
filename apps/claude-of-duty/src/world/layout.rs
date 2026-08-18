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

/// Buildings. `w` is the X extent, `d` the Z extent.
/// Interiors are described in normalised room coordinates (0..1 across the interior),
/// so a plan survives a change of footprint.
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
