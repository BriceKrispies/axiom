//! Ported from Claude-of-Duty `src/sky/celestial.js:1-136`.
//!
//! Where the sun and moon actually are: standard spherical astronomy —
//! declination from the day of year, hour angle from local solar time, then
//! the altitude/azimuth transform for the site latitude.
//!
//! Azimuth convention: `0 = north = -Z`, `90 = east = +X`. `north_angle_deg`
//! rotates the whole celestial sphere for art direction without touching the
//! astronomy.

use super::atmosphere::Vec3;

const DEG: f64 = std::f64::consts::PI / 180.0;

/// `SITE`'s shape, `celestial.js:23-40`.
#[derive(Debug, Clone, Copy)]
pub struct Site {
    pub latitude_deg: f64,
    pub day_of_year: f64,
    /// Rotates north in world space. 0 keeps north at -Z.
    pub north_angle_deg: f64,
    /// Moon hour-angle offset from the sun, degrees.
    pub moon_hour_offset_deg: f64,
    pub moon_declination_deg: f64,
}

/// `SITE`, `celestial.js:23-40`. Lat 45N, summer solstice — the site the
/// source's graded shot list is built against.
pub const SITE: Site = Site {
    latitude_deg: 45.0,
    day_of_year: 172.0, // summer solstice
    north_angle_deg: 0.0,
    moon_hour_offset_deg: 244.0,
    moon_declination_deg: 28.0,
};

/// Solar declination, Cooper's approximation. `solarDeclination`,
/// `celestial.js:45-47`.
pub fn solar_declination(day_of_year: f64) -> f64 {
    23.44 * DEG * (((2.0 * std::f64::consts::PI) / 365.0) * (284.0 + day_of_year)).sin()
}

/// Altitude/azimuth for a body, both in radians.
#[derive(Debug, Clone, Copy, Default)]
pub struct AltAz {
    pub alt: f64,
    pub az: f64,
}

/// Altitude/azimuth for a body at a given hour angle and declination.
/// `hour_angle` in radians, 0 at local meridian, positive in the afternoon.
/// `altAz`, `celestial.js:53-72`.
pub fn alt_az(hour_angle: f64, declination: f64, latitude_deg: f64) -> AltAz {
    let lat = latitude_deg * DEG;
    let sin_lat = lat.sin();
    let cos_lat = lat.cos();
    let sin_d = declination.sin();
    let cos_d = declination.cos();
    let sin_alt = sin_lat * sin_d + cos_lat * cos_d * hour_angle.cos();
    let alt = sin_alt.clamp(-1.0, 1.0).asin();
    let cos_alt = alt.cos();
    let mut cos_az = 0.0;
    if cos_alt > 1e-6 && cos_lat > 1e-6 {
        cos_az = (sin_d - sin_alt * sin_lat) / (cos_alt * cos_lat);
    }
    let mut az = cos_az.clamp(-1.0, 1.0).acos();
    // Hour angle positive = past the meridian = western half of the sky.
    if hour_angle.sin() > 0.0 {
        az = 2.0 * std::f64::consts::PI - az;
    }
    AltAz { alt, az }
}

/// World-space unit vector from altitude/azimuth. Points *toward* the body.
/// `dirFromAltAz`, `celestial.js:75-79`.
pub fn dir_from_alt_az(alt: f64, az: f64, north_angle_rad: f64) -> Vec3 {
    let a = az + north_angle_rad;
    let ca = alt.cos();
    Vec3::new(ca * a.sin(), alt.sin(), -ca * a.cos()).normalize()
}

/// A row-major 3x3 rotation matrix — just enough matrix algebra for
/// [`Celestial::celestial_matrix`] (the equatorial -> world rotation the
/// source's starfield, not ported in this slice, consumes as a `mat3`
/// uniform). `mat[i]` is row `i`; `a.mul(b)` composes as `a * b` (apply `b`
/// first), matching three.js's `.premultiply` semantics used at the call
/// site below.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mat3(pub [[f64; 3]; 3]);

impl Mat3 {
    pub fn identity() -> Self {
        Mat3([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]])
    }

    /// `Matrix4.makeRotationY`.
    pub fn rotation_y(theta: f64) -> Self {
        let (s, c) = (theta.sin(), theta.cos());
        Mat3([[c, 0.0, s], [0.0, 1.0, 0.0], [-s, 0.0, c]])
    }

    /// `Matrix4.makeRotationX`.
    pub fn rotation_x(theta: f64) -> Self {
        let (s, c) = (theta.sin(), theta.cos());
        Mat3([[1.0, 0.0, 0.0], [0.0, c, -s], [0.0, s, c]])
    }

    pub fn mul(self, other: Mat3) -> Mat3 {
        let mut out = [[0.0; 3]; 3];
        for (i, row) in out.iter_mut().enumerate() {
            for (j, cell) in row.iter_mut().enumerate() {
                *cell = self.0[i][0] * other.0[0][j]
                    + self.0[i][1] * other.0[1][j]
                    + self.0[i][2] * other.0[2][j];
            }
        }
        Mat3(out)
    }
}

/// Full celestial state for an hour of the day. `sun`/`moon` are unit world
/// directions pointing at the body. `Celestial`, `celestial.js:85-135`.
pub struct Celestial {
    pub site: Site,
    pub sun: Vec3,
    pub moon: Vec3,
    pub sun_alt: f64,
    pub sun_az: f64,
    pub moon_alt: f64,
    pub moon_az: f64,
    /// Illuminated fraction of the lunar disc, 0..1.
    pub moon_phase: f64,
    /// Angular separation sun-moon; drives the terminator on the disc.
    pub moon_elongation: f64,
    celestial_rotation: Mat3,
}

impl Celestial {
    /// `new Celestial(site = SITE)`, `celestial.js:86-101`.
    pub fn new(site: Site) -> Self {
        Celestial {
            site,
            sun: Vec3::new(0.0, 1.0, 0.0),
            moon: Vec3::new(0.0, -1.0, 0.0),
            sun_alt: 0.0,
            sun_az: 0.0,
            moon_alt: 0.0,
            moon_az: 0.0,
            moon_phase: 1.0,
            moon_elongation: std::f64::consts::PI,
            celestial_rotation: Mat3::identity(),
        }
    }

    /// `setHour`, `celestial.js:103-129`.
    pub fn set_hour(&mut self, hour: f64) -> &mut Self {
        let north = self.site.north_angle_deg * DEG;
        let decl = solar_declination(self.site.day_of_year);
        let h = (hour - 12.0) * 15.0 * DEG;

        let aa = alt_az(h, decl, self.site.latitude_deg);
        self.sun_alt = aa.alt;
        self.sun_az = aa.az;
        self.sun = dir_from_alt_az(self.sun_alt, self.sun_az, north);

        let hm = h + self.site.moon_hour_offset_deg * DEG;
        let aam = alt_az(hm, self.site.moon_declination_deg * DEG, self.site.latitude_deg);
        self.moon_alt = aam.alt;
        self.moon_az = aam.az;
        self.moon = dir_from_alt_az(self.moon_alt, self.moon_az, north);

        self.moon_elongation = self.sun.dot(self.moon).clamp(-1.0, 1.0).acos();
        self.moon_phase = 0.5 * (1.0 - self.moon_elongation.cos());

        // Equatorial -> world rotation for the starfield: the sky turns
        // 15 deg/hour about the polar axis, which is tilted from vertical by
        // (90 - latitude).
        let polar_tilt = (90.0 - self.site.latitude_deg) * DEG;
        self.celestial_rotation = Mat3::rotation_x(polar_tilt).mul(Mat3::rotation_y(-h + north));
        self
    }

    /// `celestialMatrix`, `celestial.js:131-134`.
    pub fn celestial_matrix(&self) -> Mat3 {
        self.celestial_rotation
    }
}
