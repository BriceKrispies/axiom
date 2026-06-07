// This fixture's path contains `apps/`, not `crates/` or `modules/`, so it is
// a composition leaf outside the engine spine: the lint must NOT fire even when
// the impl block has more than MAX_ITEMS items. (Expected output: empty.)
#![allow(dead_code)]

struct AppGodType;

impl AppGodType {
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

fn main() {}
