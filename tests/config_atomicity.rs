use std::{fs, io};

use usrgrp_manager::config::{AtomicWriteStage, atomic_write_with_fault};

fn injected(stage: AtomicWriteStage) -> io::Error {
    io::Error::other(format!("injected {stage:?} failure"))
}

#[test]
fn every_pre_rename_fault_preserves_complete_old_contents_and_removes_temp() {
    for stage in [
        AtomicWriteStage::BeforeWrite,
        AtomicWriteStage::BeforeFlush,
        AtomicWriteStage::BeforeFileSync,
        AtomicWriteStage::BeforeRename,
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.conf");
        fs::write(&path, "old-complete\n").unwrap();
        let result = atomic_write_with_fault(&path, b"new-complete\n", |point| {
            if point == stage {
                Err(injected(point))
            } else {
                Ok(())
            }
        });

        assert!(result.is_err(), "{stage:?} must fail");
        assert_eq!(fs::read_to_string(&path).unwrap(), "old-complete\n");
        assert!(fs::read_dir(directory.path()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".tmp")
        }));
    }
}

#[test]
fn post_rename_directory_sync_fault_leaves_a_complete_old_or_new_file() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config.conf");
    fs::write(&path, "old-complete\n").unwrap();
    let result = atomic_write_with_fault(&path, b"new-complete\n", |point| {
        if point == AtomicWriteStage::BeforeDirectorySync {
            Err(injected(point))
        } else {
            Ok(())
        }
    });

    assert!(result.is_err());
    let complete = fs::read_to_string(&path).unwrap();
    assert!(matches!(
        complete.as_str(),
        "old-complete\n" | "new-complete\n"
    ));
    assert_eq!(complete, "new-complete\n");
}
