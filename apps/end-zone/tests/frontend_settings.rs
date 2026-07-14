//! Settings: the five typed categories, working-vs-committed editing with
//! APPLY / RESET DEFAULTS / BACK, the discard-confirm dialog, live preview,
//! rebind capture, and accessibility settings that provably change behavior.

use axiom_end_zone::frontend::bindings::{BindableAction, ControlBindings};
use axiom_end_zone::frontend::input::FrontendInputFrame;
use axiom_end_zone::frontend::navigation::WidgetId;
use axiom_end_zone::frontend::persistence::FrontendProfile;
use axiom_end_zone::frontend::screens::settings_rows::{fields_for, row_view, SettingField};
use axiom_end_zone::frontend::screens::settings_values::{activate_field, adjust_field};
use axiom_end_zone::frontend::screens::{settings, settings_rows};
use axiom_end_zone::frontend::settings::{EndZoneSettings, SettingsCategory, UiScale, Volume};
use axiom_end_zone::frontend::state::{ModalKind, Screen, CAPTURE_TIMEOUT};
use axiom_end_zone::frontend::theme::Theme;
use axiom_end_zone::frontend::transitions::{ActiveTransition, TransitionKind};
use axiom_end_zone::frontend::FrontendApp;
use axiom_end_zone::launch::Difficulty;

fn app() -> FrontendApp {
    FrontendApp::new(5, FrontendProfile::default())
}

fn step(fe: &mut FrontendApp, held: &[&str]) -> axiom_end_zone::frontend::FrontendFrame {
    let input = FrontendInputFrame {
        keys_down: held.iter().map(|s| s.to_string()).collect(),
        ..Default::default()
    };
    fe.frame(&input, 1280.0, 720.0)
}

fn tap(fe: &mut FrontendApp, token: &str) -> axiom_end_zone::frontend::FrontendFrame {
    let frame = step(fe, &[token]);
    step(fe, &[]);
    frame
}

fn open_settings(fe: &mut FrontendApp) {
    tap(fe, "Enter"); // Title → MainMenu
    tap(fe, "ArrowDown"); // SETTINGS
    tap(fe, "Enter");
    assert_eq!(fe.state().screen, Screen::Settings);
}

#[test]
fn every_category_carries_real_typed_fields() {
    assert_eq!(SettingsCategory::ALL.len(), 5);
    for category in SettingsCategory::ALL {
        assert!(!fields_for(category).is_empty(), "{category:?} has fields");
    }
    // Every field renders a row against defaults + default bindings.
    let settings = EndZoneSettings::default();
    let bindings = ControlBindings::default();
    for category in SettingsCategory::ALL {
        for field in fields_for(category) {
            let row = row_view(field, &settings, &bindings, None);
            assert!(!row.label.is_empty());
        }
    }
}

#[test]
fn adjust_steps_are_bounded_and_activate_cycles() {
    let mut s = EndZoneSettings::default();
    assert!(adjust_field(SettingField::Difficulty, &mut s, 1));
    assert_eq!(s.difficulty, Difficulty::AllStar);
    assert!(
        !adjust_field(SettingField::Difficulty, &mut s, 1),
        "clamped at the end"
    );
    assert!(adjust_field(SettingField::Difficulty, &mut s, -1));
    assert!(adjust_field(SettingField::Difficulty, &mut s, -1));
    assert_eq!(s.difficulty, Difficulty::Rookie);
    // Toggles flip on activate.
    let was = s.reduced_motion;
    assert!(activate_field(SettingField::ReducedMotion, &mut s));
    assert_eq!(s.reduced_motion, !was);
    // Volumes clamp at both ends.
    s.master_volume = Volume(10);
    assert!(!adjust_field(SettingField::MasterVolume, &mut s, 1));
    s.master_volume = Volume(0);
    assert!(!adjust_field(SettingField::MasterVolume, &mut s, -1));
}

#[test]
fn editing_touches_the_working_copy_and_apply_commits_it() {
    let mut fe = app();
    open_settings(&mut fe);
    let committed = fe.state().profile.settings;
    tap(&mut fe, "ArrowDown"); // first row (DIFFICULTY)
    tap(&mut fe, "ArrowRight"); // Pro → All-Star in the WORKING copy
    let edit = fe.state().settings_edit.clone().expect("editor open");
    assert_ne!(edit.working.difficulty, committed.difficulty);
    assert_eq!(
        fe.state().profile.settings,
        committed,
        "committed untouched"
    );

    // Walk to the footer and APPLY.
    for _ in 0..3 {
        tap(&mut fe, "ArrowDown");
    }
    let frame = tap(&mut fe, "Enter");
    assert_eq!(fe.state().profile.settings.difficulty, Difficulty::AllStar);
    assert!(frame.persist, "apply requests persistence");
}

#[test]
fn backing_out_with_changes_raises_the_discard_dialog() {
    let mut fe = app();
    open_settings(&mut fe);
    tap(&mut fe, "ArrowDown");
    tap(&mut fe, "ArrowRight");
    tap(&mut fe, "Escape");
    let modal = fe.state().modal.expect("discard dialog");
    assert_eq!(modal.kind, ModalKind::DiscardSettings);
    assert_eq!(modal.focused, 0, "the SAFE option is focused first");
    assert_eq!(fe.state().screen, Screen::Settings);
    // Confirm the discard: back at the origin with nothing committed.
    tap(&mut fe, "ArrowRight");
    tap(&mut fe, "Enter");
    assert_eq!(fe.state().screen, Screen::MainMenu);
    assert_eq!(fe.state().profile.settings, EndZoneSettings::default());
}

#[test]
fn backing_out_clean_never_asks() {
    let mut fe = app();
    open_settings(&mut fe);
    tap(&mut fe, "Escape");
    assert!(fe.state().modal.is_none());
    assert_eq!(fe.state().screen, Screen::MainMenu);
}

#[test]
fn reset_defaults_resets_the_working_copy_only() {
    let mut fe = app();
    open_settings(&mut fe);
    tap(&mut fe, "ArrowDown");
    tap(&mut fe, "ArrowRight"); // dirty
    for _ in 0..3 {
        tap(&mut fe, "ArrowDown"); // footer, APPLY focused
    }
    tap(&mut fe, "ArrowRight"); // RESET DEFAULTS
    tap(&mut fe, "Enter");
    let edit = fe.state().settings_edit.clone().expect("editor open");
    assert_eq!(edit.working, EndZoneSettings::default());
}

#[test]
fn the_working_copy_previews_live_through_effective_settings() {
    let mut fe = app();
    open_settings(&mut fe);
    let before = Theme::from_settings(fe.state().effective_settings()).fingerprint();
    fe.state_mut()
        .settings_edit
        .as_mut()
        .expect("editor open")
        .working
        .high_contrast = true;
    let after = Theme::from_settings(fe.state().effective_settings()).fingerprint();
    assert_ne!(before, after, "high contrast changes the computed palette");
    assert_eq!(
        Theme::from_settings(&fe.state().profile.settings).fingerprint(),
        before,
        "committed theme unchanged until APPLY"
    );
}

#[test]
fn ui_scale_and_text_size_are_real_multipliers() {
    let mut s = EndZoneSettings::default();
    s.ui_scale = UiScale::Large;
    let theme = Theme::from_settings(&s);
    assert!(theme.ui_scale > 1.0);
    s.text_size = axiom_end_zone::frontend::settings::TextSize::Large;
    assert!(Theme::from_settings(&s).text_scale > 1.0);
}

#[test]
fn reduced_motion_replaces_sweeps_with_short_fades() {
    let full =
        ActiveTransition::start(TransitionKind::Wipe, Screen::Title, Screen::MainMenu, false);
    let reduced =
        ActiveTransition::start(TransitionKind::Wipe, Screen::Title, Screen::MainMenu, true);
    assert_eq!(full.kind, TransitionKind::Wipe);
    assert_eq!(reduced.kind, TransitionKind::Fade);
    assert!(reduced.duration < full.duration);
    // Progress completes at exactly 1.0 either way.
    let mut t = reduced;
    while t.advance() {}
    assert_eq!(t.progress(), 1.0);
}

#[test]
fn rebind_capture_rebinds_and_times_out() {
    let mut fe = app();
    open_settings(&mut fe);
    fe.state_mut()
        .settings_edit
        .as_mut()
        .expect("editor open")
        .category = SettingsCategory::Controls;
    // Arm capture on the CONFIRM row (row ids are ROW_BASE + index).
    let confirm_index = fields_for(SettingsCategory::Controls)
        .iter()
        .position(|f| *f == SettingField::Bind(BindableAction::Confirm))
        .expect("confirm row exists");
    settings::confirm(fe.state_mut(), WidgetId(200 + confirm_index as u32));
    assert!(fe
        .state()
        .settings_edit
        .as_ref()
        .expect("editor")
        .capture
        .is_some());

    // While capturing, a pressed key becomes the primary binding.
    step(&mut fe, &["KeyZ"]);
    let edit = fe.state().settings_edit.clone().expect("editor");
    assert!(edit.capture.is_none());
    assert_eq!(
        edit.working_bindings.tokens(BindableAction::Confirm)[0],
        "KeyZ"
    );
    assert_eq!(
        fe.state().profile.bindings.tokens(BindableAction::Confirm)[0],
        "Enter",
        "committed bindings untouched until APPLY"
    );

    // A fresh capture expires on its own.
    settings::confirm(fe.state_mut(), WidgetId(200 + confirm_index as u32));
    for _ in 0..=CAPTURE_TIMEOUT as usize {
        step(&mut fe, &[]);
    }
    assert!(fe
        .state()
        .settings_edit
        .as_ref()
        .expect("editor")
        .capture
        .is_none());
}

#[test]
fn bindings_default_conflict_free_within_each_group_and_rebind_reorders() {
    let mut bindings = ControlBindings::default();
    for action in BindableAction::ALL {
        let Some(primary) = bindings.tokens(action).first().cloned() else {
            continue;
        };
        let game = |a: BindableAction| {
            matches!(
                a,
                BindableAction::GamePrimary
                    | BindableAction::GameSecondary
                    | BindableAction::GameSwitchPlayer
            )
        };
        let same_group: Vec<_> = bindings
            .conflicts(action, &primary)
            .into_iter()
            .filter(|other| game(*other) == game(action))
            .collect();
        assert!(same_group.is_empty(), "{action:?} defaults conflict-free");
    }
    bindings.rebind(BindableAction::Pause, "KeyO");
    assert_eq!(bindings.tokens(BindableAction::Pause)[0], "KeyO");
    assert!(bindings.matches(BindableAction::Pause, "KeyO"));
    bindings.restore(BindableAction::Pause);
    assert_eq!(bindings.tokens(BindableAction::Pause)[0], "KeyP");
    // The emergency keyboard path always works, whatever the bindings say.
    bindings.rebind(BindableAction::Confirm, "KeyZ");
    assert!(bindings.matches(BindableAction::Confirm, "Enter"));
}

#[test]
fn settings_rows_render_capture_and_conflict_states() {
    let settings_model = EndZoneSettings::default();
    let mut bindings = ControlBindings::default();
    let capturing = row_view(
        settings_rows::SettingField::Bind(BindableAction::Pause),
        &settings_model,
        &bindings,
        Some((BindableAction::Pause, 100)),
    );
    assert!(capturing.value.contains("PRESS"));
    // Force a same-group conflict and see it reported.
    bindings.rebind(BindableAction::Cancel, "KeyP");
    let row = row_view(
        settings_rows::SettingField::Bind(BindableAction::Cancel),
        &settings_model,
        &bindings,
        None,
    );
    match row.control {
        axiom_end_zone::frontend::widgets::RowControl::Binding { conflict, .. } => {
            assert!(conflict, "KeyP now drives both CANCEL and PAUSE");
        }
        other => panic!("expected a binding control, got {other:?}"),
    }
}
