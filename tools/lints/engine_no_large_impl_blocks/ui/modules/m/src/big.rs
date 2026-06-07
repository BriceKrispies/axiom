// This fixture's path contains `modules/`, so it is engine code.
// An impl block with MORE than MAX_ITEMS (30) items MUST be flagged.
// A small impl block in the same file must NOT be flagged.
#![allow(dead_code)]

// ---- FLAGGED: 32 methods, exceeds the limit of 30 ----

struct BigType;

impl BigType {
    fn method_0(&self) {}
    fn method_1(&self) {}
    fn method_2(&self) {}
    fn method_3(&self) {}
    fn method_4(&self) {}
    fn method_5(&self) {}
    fn method_6(&self) {}
    fn method_7(&self) {}
    fn method_8(&self) {}
    fn method_9(&self) {}
    fn method_10(&self) {}
    fn method_11(&self) {}
    fn method_12(&self) {}
    fn method_13(&self) {}
    fn method_14(&self) {}
    fn method_15(&self) {}
    fn method_16(&self) {}
    fn method_17(&self) {}
    fn method_18(&self) {}
    fn method_19(&self) {}
    fn method_20(&self) {}
    fn method_21(&self) {}
    fn method_22(&self) {}
    fn method_23(&self) {}
    fn method_24(&self) {}
    fn method_25(&self) {}
    fn method_26(&self) {}
    fn method_27(&self) {}
    fn method_28(&self) {}
    fn method_29(&self) {}
    fn method_30(&self) {}
    fn method_31(&self) {}
}

// ---- NOT flagged: small impl block well under the limit ----

struct SmallType;

impl SmallType {
    fn alpha(&self) {}
    fn beta(&self) {}
    fn gamma(&self) {}
}

fn main() {}
