use std::fs;

use usrgrp_manager::{
    app::{GroupsFilter, Theme, UsersFilter, filterconf::FiltersConfig, keymap::Keymap},
    config::atomic_write,
};

#[test]
fn canonical_configs_round_trip_and_keep_all_values() {
    let directory = tempfile::tempdir().unwrap();
    let theme_path = directory.path().join("theme.conf");
    let filters_path = directory.path().join("filter.conf");
    let keymap_path = directory.path().join("keybinds.conf");

    let mut theme = Theme::mocha();
    theme.highlight_bg = ratatui::style::Color::Indexed(9);
    theme.write_file(theme_path.to_str().unwrap()).unwrap();
    assert_eq!(
        Theme::from_file(theme_path.to_str().unwrap())
            .unwrap()
            .highlight_bg,
        theme.highlight_bg
    );
    let dark_path = directory.path().join("dark-theme.conf");
    Theme::dark()
        .write_file(dark_path.to_str().unwrap())
        .unwrap();
    assert_eq!(
        Theme::from_file(dark_path.to_str().unwrap()).unwrap(),
        Theme::dark()
    );

    let filters = FiltersConfig {
        users_filter: Some(UsersFilter::OnlyUserIds),
        groups_filter: Some(GroupsFilter::OnlySystemGids),
        human_only: true,
        system_only: false,
        inactive: true,
        no_home: true,
        locked: true,
        no_password: true,
        expired: true,
    };
    filters.write_file(filters_path.to_str().unwrap()).unwrap();
    assert_eq!(
        FiltersConfig::from_file(filters_path.to_str().unwrap()).unwrap(),
        filters
    );

    let keymap = Keymap::default();
    keymap.write_file(keymap_path.to_str().unwrap()).unwrap();
    let mut parsed_bindings = Keymap::from_file(keymap_path.to_str().unwrap())
        .unwrap()
        .all_bindings();
    let mut expected_bindings = keymap.all_bindings();
    parsed_bindings.sort_by_key(|((modifiers, code), action)| {
        (format!("{action:?}"), Keymap::format_key(*modifiers, *code))
    });
    expected_bindings.sort_by_key(|((modifiers, code), action)| {
        (format!("{action:?}"), Keymap::format_key(*modifiers, *code))
    });
    assert_eq!(parsed_bindings, expected_bindings);
}

#[test]
fn successful_atomic_write_replaces_complete_contents() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config.conf");
    fs::write(&path, "old\n").unwrap();
    atomic_write(&path, b"new\n").unwrap();
    assert_eq!(fs::read_to_string(path).unwrap(), "new\n");
}

#[test]
fn unknown_duplicate_and_invalid_config_entries_report_bounded_line_diagnostics() {
    let directory = tempfile::tempdir().unwrap();
    let theme = directory.path().join("theme.conf");
    fs::write(&theme, "text = #FFFFFF\nunknown = #000000\n").unwrap();
    assert!(
        Theme::from_file(theme.to_str().unwrap())
            .unwrap_err()
            .to_string()
            .contains("line 2")
    );

    let filters = directory.path().join("filter.conf");
    fs::write(&filters, "human_only = true\nhuman_only = false\n").unwrap();
    assert!(
        FiltersConfig::from_file(filters.to_str().unwrap())
            .unwrap_err()
            .to_string()
            .contains("line 2")
    );

    let keymap = directory.path().join("keybinds.conf");
    fs::write(&keymap, "UnknownAction = q\n").unwrap();
    assert!(
        Keymap::from_file(keymap.to_str().unwrap())
            .unwrap_err()
            .to_string()
            .contains("line 1")
    );
}

#[cfg(unix)]
#[test]
fn atomic_write_refuses_existing_symlink() {
    use std::os::unix::fs::symlink;
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("target");
    let link = directory.path().join("link");
    fs::write(&target, "safe\n").unwrap();
    symlink(&target, &link).unwrap();
    assert!(atomic_write(&link, b"unsafe\n").is_err());
    assert_eq!(fs::read_to_string(target).unwrap(), "safe\n");
}
