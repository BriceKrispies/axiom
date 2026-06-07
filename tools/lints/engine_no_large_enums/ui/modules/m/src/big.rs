// compile-flags: --test
// This fixture's path contains `modules/`, so it is treated as engine code.
// The over-limit enum MUST be flagged; the small enum MUST NOT be.
#![allow(dead_code)]

// ---- engine code: FLAGGED (26 variants > MAX_VARIANTS of 24) ----

enum BigEnum {
    V0, V1, V2, V3, V4, V5, V6, V7, V8, V9,
    V10, V11, V12, V13, V14, V15, V16, V17, V18, V19,
    V20, V21, V22, V23, V24, V25,
}

// ---- engine code: NOT flagged (4 variants <= MAX_VARIANTS of 24) ----

enum SmallEnum {
    Alpha,
    Beta,
    Gamma,
    Delta,
}
