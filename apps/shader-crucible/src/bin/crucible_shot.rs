//! **The crucible's own capture harness**, and the reason it has one.
//!
//! `axiom-shot` renders any registered slice, and the crucible is registered
//! there — but neither of its arms can carry an authored surface:
//!
//! * `--backend gpu` calls `GpuBackendApi::render_offscreen_rgba`, whose whole
//!   argument list is meshes / materials / lights / instance batches. **There is
//!   no surface lane on it**, and no other public GPU entry renders off-screen.
//! * `--backend canvas2d` calls `Canvas2dBackendApi::render_offscreen_rgba_skinned`,
//!   which likewise takes no surfaces.
//!
//! So an `axiom-shot` capture of this app shows every station in its **constant
//! fallback** — which is a genuinely useful control image, and is exactly what
//! the frames this app authors look like when the surface lane is dropped.
//!
//! This binary renders the other one. `Canvas2dBackendApi::render_offscreen_rgba_with_surfaces`
//! is the **only public, native, headless path in the engine that renders an
//! authored `Surface` to pixels**, and it is what produces the "with surfaces"
//! capture below. Running both and putting them side by side is the whole
//! demonstration: the difference between the two images *is* the surface system.
//!
//! ```sh
//! cargo run -p axiom-shader-crucible --bin crucible_shot -- \
//!     --out screenshots/crucible-c2d.png --tick 0
//! ```
//!
//! It writes two files: `<out>` (surfaces evaluated) and
//! `<out stem>-fallback.png` (the same frame with the surface set withheld).

use axiom_canvas2d_backend::Canvas2dBackendApi;
use axiom_shader_crucible::frame::packet_of;
use axiom_shader_crucible::preparation::presentation_request;
use axiom_shader_crucible::report::report;
use axiom_shader_crucible::layout::{HEIGHT, WIDTH};
use axiom_shader_crucible::scene::crucible_core;
use axiom_shader_crucible::stations::all_surfaces;

/// The software rasterizer's High quality tier caps its internal framebuffer at
/// **426 px on the longest edge** (`canvas_policy::dimensions`), whatever surface
/// it is sized from. So the capture is 426x213 and no scaling of the request
/// changes that: it is the resolution the software arm genuinely renders at, and
/// reporting a larger one would be an upscale pretending to be a render.

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let out = flag(&args, "--out").unwrap_or_else(|| "screenshots/crucible-c2d.png".to_string());
    let tick: u64 = flag(&args, "--tick")
        .and_then(|t| t.parse().ok())
        .unwrap_or(0);
    let quality: u8 = flag(&args, "--quality")
        .and_then(|q| q.parse().ok())
        .unwrap_or(3);

    let (mut app, prepared) = crucible_core();
    let product = prepared.borrow().clone();
    println!("{}", report(product.as_ref()));

    let outcome = app.render(tick);
    let packet = packet_of(&outcome, WIDTH, HEIGHT);
    let surfaces = all_surfaces();

    let mut backend = Canvas2dBackendApi::new(&presentation_request(WIDTH, HEIGHT));
    backend.load_meshes(&app.mesh_set());
    backend.set_quality_level(quality);

    let (pixels, w, h) = backend.render_offscreen_rgba_with_surfaces(&packet, &surfaces);
    write_png(&out, &pixels, w, h);
    println!("crucible_shot: wrote {out} ({w}x{h}, tick={tick}, surfaces evaluated)");

    // The control: the identical frame with the surface set withheld — which is
    // what every other capture path in the engine produces for this app.
    let fallback = out
        .strip_suffix(".png")
        .map_or_else(|| format!("{out}-fallback"), |stem| format!("{stem}-fallback"));
    let fallback = format!("{fallback}.png");
    let (bare, w, h) = backend.render_offscreen_rgba_with_surfaces(&packet, &[]);
    write_png(&fallback, &bare, w, h);
    println!("crucible_shot: wrote {fallback} ({w}x{h}, tick={tick}, surfaces WITHHELD)");

    let changed = pixels
        .iter()
        .zip(bare.iter())
        .filter(|(a, b)| a != b)
        .count();
    println!(
        "crucible_shot: the surface set changed {:.1}% of the frame's bytes",
        100.0 * changed as f64 / pixels.len() as f64
    );
}

/// Write an RGBA8 buffer to a PNG at `path`, creating parent directories.
fn write_png(path: &str, rgba: &[u8], width: u32, height: u32) {
    std::path::Path::new(path)
        .parent()
        .into_iter()
        .for_each(|parent| {
            std::fs::create_dir_all(parent).expect("create the output directory");
        });
    let file = std::fs::File::create(path).expect("create the PNG file");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("write the PNG header");
    writer.write_image_data(rgba).expect("write the PNG data");
}
