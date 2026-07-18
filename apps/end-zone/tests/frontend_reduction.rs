//! Frontend-reduction guards: the removed screens, widgets, and launch types do
//! not return. These are precise source checks over the app's production source
//! (comments and strings stripped) so documentation history elsewhere is never
//! rejected.

use std::fs;
use std::path::{Path, PathBuf};

fn src() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("readable dir") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

/// Strip `//` line comments and string/char literals, so a name only survives
/// when it appears as real code.
fn strip(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let (mut in_string, mut in_char) = (false, false);
    while let Some(c) = chars.next() {
        if in_string {
            if c == '\\' {
                chars.next();
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        if in_char {
            if c == '\\' {
                chars.next();
            } else if c == '\'' {
                in_char = false;
            }
            continue;
        }
        if c == '/' && chars.peek() == Some(&'/') {
            for next in chars.by_ref() {
                if next == '\n' {
                    out.push('\n');
                    break;
                }
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '\'' => in_char = true,
            _ => out.push(c),
        }
    }
    out
}

#[test]
fn removed_frontend_concepts_do_not_return() {
    let forbidden = [
        "MainMenu",
        "TeamSelect",
        "OpponentSelect",
        "MatchSetup",
        "Credits",
        "TeamCard",
        "MatchLaunchConfig",
        "DifficultySetting",
        "CameraStyleSetting",
        "GameSpeedSetting",
        "ControlRebind",
        "Attract",
    ];
    let mut files = Vec::new();
    collect_rs(&src(), &mut files);
    let mut hits = Vec::new();
    for path in files {
        let code = strip(&fs::read_to_string(&path).expect("utf-8"));
        for needle in forbidden {
            if code.contains(needle) {
                hits.push(format!("{}: {needle}", path.display()));
            }
        }
    }
    assert!(
        hits.is_empty(),
        "removed concept re-appeared:\n{}",
        hits.join("\n")
    );
}

#[test]
fn the_deleted_screen_files_are_gone() {
    let screens = src().join("frontend").join("screens");
    for removed in [
        "attract.rs",
        "credits.rs",
        "main_menu.rs",
        "match_setup.rs",
        "modal.rs",
        "team_select.rs",
        "settings_rows.rs",
        "settings_values.rs",
    ] {
        assert!(
            !screens.join(removed).exists(),
            "{removed} must be deleted, not hidden"
        );
    }
}

#[test]
fn exactly_the_six_screen_states_exist() {
    let code =
        strip(&fs::read_to_string(src().join("frontend").join("screen.rs")).expect("screen.rs"));
    for state in [
        "Title", "InGame", "Paused", "Settings", "Controls", "GameOver",
    ] {
        assert!(code.contains(state), "keeps {state}");
    }
    // The screen count constant is fixed at six.
    assert!(code.contains("SCREEN_COUNT: usize = 6"));
}
