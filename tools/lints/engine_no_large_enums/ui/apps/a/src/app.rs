// This fixture's path contains `apps/`, not `crates/` or `modules/`, so it is a
// composition leaf outside the engine spine: the lint must NOT fire even for an
// over-limit enum. (Expected output: empty.)
#![allow(dead_code)]

enum BigAppEnum {
    V0, V1, V2, V3, V4, V5, V6, V7, V8, V9,
    V10, V11, V12, V13, V14, V15, V16, V17, V18, V19,
    V20, V21, V22, V23, V24, V25,
}

fn main() {}
