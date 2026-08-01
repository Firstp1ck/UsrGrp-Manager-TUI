use std::{fs, path::Path};

#[test]
fn integration_tests_cannot_construct_production_runners_or_processes() {
    scan_rust_sources(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .as_path(),
    );
}

#[test]
fn source_unit_tests_cannot_construct_production_runners_or_account_tools() {
    scan_source_test_modules(Path::new(env!("CARGO_MANIFEST_DIR")).join("src").as_path());
}

#[test]
fn public_adapter_surface_has_no_legacy_mutation_or_password_facade() {
    let adapter = include_str!("../src/sys/mod.rs");
    for forbidden in [
        "pub fn with_sudo_password",
        "pub fn add_user_to_group",
        "pub fn remove_user_from_group",
        "pub fn create_user",
        "pub fn set_user_password",
    ] {
        assert!(
            !adapter.contains(forbidden),
            "legacy public trusted-boundary facade remains: {forbidden}"
        );
    }
}

fn scan_source_test_modules(path: &Path) {
    for entry in fs::read_dir(path).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            scan_source_test_modules(&path);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            let source = fs::read_to_string(&path).unwrap();
            let Some((_, test_source)) = source.split_once("#[cfg(test)]") else {
                continue;
            };
            let production_runner = ["Local", "CommandRunner"].concat();
            assert!(
                !test_source.contains(&production_runner),
                "unit-test source {} constructs the production runner",
                path.display()
            );
            for program in [
                "sudo", "useradd", "usermod", "userdel", "groupadd", "groupmod", "groupdel",
                "gpasswd", "chpasswd", "chage",
            ] {
                let direct_spawn = ["Command::", "new(\"", program, "\")"].concat();
                assert!(
                    !test_source.contains(&direct_spawn),
                    "unit-test source {} directly spawns privileged tool {program}",
                    path.display()
                );
            }
        }
    }
}

fn scan_rust_sources(path: &Path) {
    for entry in fs::read_dir(path).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            scan_rust_sources(&path);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            let source = fs::read_to_string(&path).unwrap();
            let forbidden = [
                ["Local", "CommandRunner"].concat(),
                ["std::process", "::Command"].concat(),
                ["Command::", "new("].concat(),
                ["Command::", "spawn("].concat(),
            ];
            for forbidden in forbidden {
                assert!(
                    !source.contains(&forbidden),
                    "normal test source {} contains forbidden process boundary {forbidden}",
                    path.display()
                );
            }
        }
    }
}
