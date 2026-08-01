mod common;

use usrgrp_manager::{
    error::CoreError,
    sys::{CommandSpec, KnownProgram, PasswordRecord, SecretString, UserName},
};

#[test]
fn redacted_preview_and_classified_error_do_not_expose_password_material() {
    let secret = "do-not-render-this-password";
    let record =
        PasswordRecord::new(UserName::new("alice").unwrap(), SecretString::new(secret)).unwrap();
    let spec = CommandSpec::new(KnownProgram::ChPasswd)
        .password_record(record)
        .unwrap();

    let preview = spec.redacted_preview().render();
    let error = CoreError::ExitStatus {
        program: "chpasswd",
        code: Some(1),
    }
    .to_string();
    assert!(preview.contains("redacted password record"));
    assert!(!preview.contains(secret));
    assert!(!error.contains(secret));
}

#[test]
fn secret_input_rejects_record_delimiters_before_any_runner_can_receive_it() {
    for invalid in ["line\nbreak", "carriage\rreturn", "nul\0byte"] {
        let result =
            PasswordRecord::new(UserName::new("alice").unwrap(), SecretString::new(invalid));
        assert!(matches!(
            result,
            Err(CoreError::Validation {
                field: "password",
                ..
            })
        ));
    }
}

#[test]
fn password_record_requires_a_direct_chpasswd_stdin_contract() {
    let record =
        PasswordRecord::new(UserName::new("alice").unwrap(), SecretString::new("safe")).unwrap();
    let error = match CommandSpec::new(KnownProgram::UserMod).password_record(record) {
        Err(error) => error,
        Ok(_) => panic!("non-chpasswd password record must be rejected"),
    };
    assert!(matches!(
        error,
        CoreError::Validation {
            field: "chpasswd command",
            ..
        }
    ));
}
