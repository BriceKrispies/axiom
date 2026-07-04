//! `generated-micro-fps` — drive the recipe project from the command line.
//!
//! Commands:
//! - `report`   (default) — expand the level and print the size / perf report.
//! - `pack [out]`          — write the packed recipe blob (default `micro_fps.pack`).
//! - `expand`              — expand the menu + level and print scene counts.
//! - `validate`           — run the nine validation checks and print pass/fail.

use generated_micro_fps::pack::pack;
use generated_micro_fps::scenes::{expand_level, expand_menu};
use generated_micro_fps::{validation, SizeReport, Style};

fn main() {
    let style = Style::facility();
    let command = std::env::args().nth(1).unwrap_or_else(|| "report".to_string());
    match command.as_str() {
        "report" => println!("{}", SizeReport::generate(&style)),
        "pack" => {
            let packed = pack(&style);
            let out = std::env::args().nth(2).unwrap_or_else(|| "micro_fps.pack".to_string());
            std::fs::write(&out, &packed.bytes).expect("write packed recipe");
            println!(
                "packed {} recipes → {out} ({} bytes, determinism hash {:#018x})",
                packed.recipe_count,
                packed.bytes.len(),
                packed.determinism_hash
            );
        }
        "expand" => {
            let level = expand_level(&style);
            let _menu = expand_menu(&style);
            println!(
                "expanded: menu tableau + level ({} renderables, {} enemies, {} verts, {} entities)",
                level.renderable_count, level.layout.enemies.len(), level.mesh_vertices, level.entity_count
            );
        }
        "validate" => {
            let results = validation::run(&style);
            let passed = results.iter().filter(|(_, ok)| *ok).count();
            for (name, ok) in &results {
                println!("  [{}] {name}", if *ok { "PASS" } else { "FAIL" });
            }
            println!("{passed}/{} checks passed", results.len());
            if passed != results.len() {
                std::process::exit(1);
            }
        }
        other => {
            eprintln!("unknown command '{other}' — use: report | pack [out] | expand | validate");
            std::process::exit(2);
        }
    }
}
