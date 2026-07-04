//! Axiom Animation Lab CLI — scrub the kick, and emit the shared game assets.
//!
//! Usage:
//!   axiom-animation-lab                       # inspection table (key frames)
//!   axiom-animation-lab --full                # one line per frame
//!   axiom-animation-lab --frame N [--out F]   # render frame N as SVG (stdout or F)
//!   axiom-animation-lab --all --out DIR       # render every frame to DIR/frame_NNN.svg
//!   axiom-animation-lab --emit-assets DIR     # write kicker.figure + kick_right.clip

use std::path::Path;

use axiom_animation_lab::authoring::{self, phase_name};
use axiom_animation_lab::scene::LabScene;
use axiom_animation_lab::svg::render_frame;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_usage();
        return;
    }

    if let Some(dir) = flag_value(&args, "--emit-assets") {
        emit_assets(&dir);
        return;
    }

    let scene = LabScene::new();

    if args.iter().any(|a| a == "--all") {
        let out = flag_value(&args, "--out").unwrap_or_else(|| ".".to_string());
        render_all(&scene, &out);
        return;
    }

    if let Some(frame) = flag_value(&args, "--frame").and_then(|v| v.parse::<u32>().ok()) {
        let frame = frame.min(scene.frame_count() - 1);
        let svg = render_frame(&scene, frame);
        match flag_value(&args, "--out") {
            Some(path) => write_bytes(&path, svg.as_bytes()),
            None => print!("{svg}"),
        }
        return;
    }

    let full = args.iter().any(|a| a == "--full");
    print!("{}", table(&scene, full));
}

/// The value following `flag` in `args`, if present.
fn flag_value(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

/// Write the shared binary assets the game embeds.
fn emit_assets(dir: &str) {
    std::fs::create_dir_all(dir).ok();
    write_bytes(&Path::new(dir).join("kicker.figure").to_string_lossy(), &authoring::figure_bytes());
    write_bytes(&Path::new(dir).join("kick_right.clip").to_string_lossy(), &authoring::clip_bytes());
}

/// A text table of frames: every frame with `full`, else a handful of key ones.
fn table(scene: &LabScene, full: bool) -> String {
    let key = [0_u32, 12, 18, 24, 33, 42, 47];
    let frames: Vec<u32> = if full { (0..scene.frame_count()).collect() } else { key.to_vec() };
    let mut out = String::from("frame  phase           contact  Rfoot(z,y)       plant(z,y)\n");
    for f in frames {
        let view = scene.view(f);
        let phase = view.phase.map_or("-", phase_name);
        let contact = if view.is_contact_frame { "  *  " } else { "     " };
        out.push_str(&format!(
            "{f:>3}    {phase:<14}  {contact}   ({:+.2}, {:+.2})   ({:+.2}, {:+.2})\n",
            view.right_foot.z, view.right_foot.y, view.plant_foot.z, view.plant_foot.y
        ));
    }
    out
}

fn render_all(scene: &LabScene, dir: &str) {
    std::fs::create_dir_all(dir).ok();
    for frame in 0..scene.frame_count() {
        let svg = render_frame(scene, frame);
        let path = Path::new(dir).join(format!("frame_{frame:03}.svg"));
        write_bytes(&path.to_string_lossy(), svg.as_bytes());
    }
    println!("wrote {} SVG frames to {dir}/", scene.frame_count());
}

fn write_bytes(path: &str, contents: &[u8]) {
    match std::fs::write(path, contents) {
        Ok(()) => println!("wrote {path} ({} bytes)", contents.len()),
        Err(e) => eprintln!("error writing {path}: {e}"),
    }
}

fn print_usage() {
    println!("Axiom Animation Lab — deterministic kicker scrubber + asset emitter");
    println!();
    println!("  (default)                 inspection table for key frames");
    println!("  --full                    one line per frame");
    println!("  --frame N [--out FILE]    render frame N as SVG (stdout or FILE)");
    println!("  --all --out DIR           render every frame to DIR/frame_NNN.svg");
    println!("  --emit-assets DIR         write kicker.figure + kick_right.clip");
    println!("  --help                    this message");
}
