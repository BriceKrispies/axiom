// This lint only acts on crates that have a `layer.toml` (read from
// CARGO_MANIFEST_DIR). A compiletest fixture has none, so the lint stays silent
// here. This fixture just pins that the lint compiles and does not false-fire on
// ordinary code. (Expected output: empty — no `.stderr`.)
fn main() {
    let _ = std::cmp::max(1, 2);
}
