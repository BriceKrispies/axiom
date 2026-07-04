//! `axiom-proc-player` — expand the demo room's recipes and print the size
//! report. A fixed seed keeps the run reproducible.

fn main() {
    let seed = 0xA11CE;
    let (_app, report) = axiom_proc_player::expand_room(seed);
    println!("{report}");
}
