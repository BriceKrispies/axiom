//! Built-in level templates for the authoring workspace's library.
//!
//! Each entry is a `(name, toml)` pair embedded at build time (like
//! [`crate::zanzoban::LEVEL_001_TOML`]), so the editor library needs no
//! filesystem. The showcase levels double as documentation of each add-on.

/// A blank, valid 10×10 room — the "new level" starting point.
pub const BLANK_10: &str = "title = \"Untitled\"\n\
width = 10\n\
height = 10\n\
[player]\n\
start = [1, 1]\n\
[exit]\n\
position = [8, 8]\n";

/// Every built-in template, in library order.
pub const TEMPLATES: &[(&str, &str)] = &[
    ("Blank 10×10", BLANK_10),
    ("Button Door", super::LEVEL_001_TOML),
    ("Fading Steps · decay + wells", include_str!("levels/002-fading-steps.toml")),
    ("Latches · switches", include_str!("levels/003-latches.toml")),
    ("Heavy Cargo · crates", include_str!("levels/004-heavy-cargo.toml")),
    ("Three Echoes · budget", include_str!("levels/005-three-echoes.toml")),
    ("Minefield · hazards", include_str!("levels/006-minefield.toml")),
];

/// The TOML of the template named `name`, if it exists.
pub fn template(name: &str) -> Option<&'static str> {
    TEMPLATES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, toml)| *toml)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zanzoban::{level_codec, level_validation};

    #[test]
    fn every_template_parses_and_validates() {
        for (name, toml) in TEMPLATES {
            let level = level_codec::from_toml(toml)
                .unwrap_or_else(|e| panic!("template {name:?} must parse: {e}"));
            let report = level_validation::validate_level(&level);
            assert!(
                report.is_valid(),
                "template {name:?} must validate: {:?}",
                report.messages()
            );
        }
    }

    #[test]
    fn template_lookup_round_trips() {
        assert!(template("Blank 10×10").is_some());
        assert!(template("nope").is_none());
    }
}
