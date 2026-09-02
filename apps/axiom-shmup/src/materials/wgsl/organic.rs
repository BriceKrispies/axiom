//! WGSL transcription of Claude-of-Duty `src/materials/glsl/surfaces-organic.js`.
//!
//! Source header (`surfaces-organic.js:1-6`):
//!
//! > Wood, fabric, sandbag/burlap, foliage, rubber, glass.
//! > Foliage writes its cutout mask into the height channel's companion — see
//! > generator.js, which routes `h` to albedo.a for parallax on most surfaces but
//! > to the alpha-test mask for `foliage`.
//!
//! Each constant holds the WGSL body of that block's `owSurface`. GLSL `out`
//! parameters become `ptr<function, …>`; a prologue copies each one into a local
//! `var` of the same name and an epilogue writes them back, so every statement
//! between them is the source line-for-line (this also reproduces `metal` being
//! both a local and an out-param, exactly as GLSL has it).

/// `WOOD` (`surfaces-organic.js:8-120`). Planked, weathered timber: staggered
/// butt joints, warped grain rings with knots, splits, saw marks, nails with a
/// rust weep, and ground-in soil.
pub const WOOD: &str = include_str!("wood.wgsl");

/// `FABRIC` (`surfaces-organic.js:122-199`). Plain-weave cloth tinted by
/// `uTintA`/`uTintB`: warp-over-weft cells, fuzz and slubs, a drape-fold field,
/// threadbare wear, pulled threads, stains and dust.
pub const FABRIC: &str = include_str!("fabric.wgsl");

/// `BURLAP` (`surfaces-organic.js:201-257`). Coarse hessian sacking: per-thread
/// irregular thickness, jute/pale/soil colouring, sun rot, loose standing
/// fibres, and spilled sand caught in the weave.
pub const BURLAP: &str = include_str!("burlap.wgsl");

/// `FOLIAGE` (`surfaces-organic.js:259-328`). One serrated elliptical leaf per
/// cell, sampled over the 3x3 neighbourhood so leaves overlap; the nearest
/// (highest-`depth`) leaf wins. As the file header notes, `h` here doubles as
/// the alpha-test cutout mask rather than a height (see `generator.js`).
pub const FOLIAGE: &str = include_str!("foliage.wgsl");

/// `RUBBER` (`surfaces-organic.js:330-380`). Moulded pebble-grain rubber: a
/// Worley pebble field, a mould seam, chalky abrasion scuffs, ozone cracking,
/// and settled dust.
pub const RUBBER: &str = include_str!("rubber.wgsl");

/// `GLASS` (`surfaces-organic.js:382-415`). Near-black albedo whose look comes
/// from the roughness channel: wiped smear, dust film, water spots, and fine
/// scratches.
pub const GLASS: &str = include_str!("glass.wgsl");
