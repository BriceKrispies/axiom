//! Axiom Animation Lab CLI — scrub, play, and inspect the humanoid kick.
//!
//! Usage:
//!   axiom-animation-lab                      # inspection table (5 key frames)
//!   axiom-animation-lab --full               # one line per frame (full scrub)
//!   axiom-animation-lab --frame N [--out F]  # render frame N as SVG (stdout or file F)
//!   axiom-animation-lab --all --out DIR      # render every frame to DIR/frame_NNN.svg

use std::path::Path;

use axiom_animation_lab::inspect::{full_table, inspection_table};
use axiom_animation_lab::scene::LabScene;
use axiom_animation_lab::svg::render_frame;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let scene = LabScene::new();

    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_usage();
        return;
    }

    if args.iter().any(|a| a == "--all") {
        let out = flag_value(&args, "--out").unwrap_or_else(|| ".".to_string());
        render_all(&scene, &out);
        return;
    }

    if let Some(frame) = flag_value(&args, "--frame").and_then(|v| v.parse::<u32>().ok()) {
        let frame = frame.min(scene.frame_count() - 1);
        let svg = render_frame(&scene, frame);
        match flag_value(&args, "--out") {
            Some(path) => write_file(&path, &svg),
            None => print!("{svg}"),
        }
        return;
    }

    if args.iter().any(|a| a == "--full") {
        print!("{}", full_table(&scene));
        return;
    }

    print!("{}", inspection_table(&scene));
}

/// The value following `flag` in `args`, if present.
fn flag_value(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn render_all(scene: &LabScene, dir: &str) {
    std::fs::create_dir_all(dir).ok();
    for frame in 0..scene.frame_count() {
        let svg = render_frame(scene, frame);
        let path = Path::new(dir).join(format!("frame_{frame:03}.svg"));
        write_file(&path.to_string_lossy(), &svg);
    }
    println!(
        "wrote {} SVG frames to {dir}/ (frame_000..frame_{:03})",
        scene.frame_count(),
        scene.frame_count() - 1
    );
}

fn write_file(path: &str, contents: &str) {
    match std::fs::write(path, contents) {
        Ok(()) => println!("wrote {path}"),
        Err(e) => eprintln!("error writing {path}: {e}"),
    }
}

fn print_usage() {
    println!("Axiom Animation Lab — deterministic humanoid kick scrubber");
    println!();
    println!("  (default)                 inspection table for the 5 key frames");
    println!("  --full                    one line per frame (full scrub)");
    println!("  --frame N [--out FILE]    render frame N as SVG (stdout or FILE)");
    println!("  --all --out DIR           render every frame to DIR/frame_NNN.svg");
    println!("  --help                    this message");
}
