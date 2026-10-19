use std::{
    io,
    sync::{Arc, Mutex},
};

use usrgrp_manager::terminal::{TerminalControl, TerminalResources};

#[derive(Clone)]
struct RecordingTerminal {
    calls: Arc<Mutex<Vec<&'static str>>>,
    fail: Option<&'static str>,
}

impl RecordingTerminal {
    fn record(&self, stage: &'static str) -> io::Result<()> {
        self.calls.lock().unwrap().push(stage);
        if self.fail == Some(stage) {
            Err(io::Error::other(format!("injected {stage} failure")))
        } else {
            Ok(())
        }
    }
}

impl TerminalControl for RecordingTerminal {
    fn enable_raw_mode(&mut self) -> io::Result<()> {
        self.record("raw+")
    }
    fn disable_raw_mode(&mut self) -> io::Result<()> {
        self.record("raw-")
    }
    fn enter_alternate_screen(&mut self) -> io::Result<()> {
        self.record("alt+")
    }
    fn leave_alternate_screen(&mut self) -> io::Result<()> {
        self.record("alt-")
    }
    fn enable_mouse_capture(&mut self) -> io::Result<()> {
        self.record("mouse+")
    }
    fn disable_mouse_capture(&mut self) -> io::Result<()> {
        self.record("mouse-")
    }
}

#[test]
fn acquisition_never_captures_the_mouse_so_terminal_selection_keeps_working() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut resources = TerminalResources::acquire_with(RecordingTerminal {
        calls: calls.clone(),
        fail: Some("mouse+"),
    })
    .expect("mouse capture is never requested, so its failure cannot abort setup");
    resources.restore().unwrap();

    assert_eq!(*calls.lock().unwrap(), ["raw+", "alt+", "alt-", "raw-"]);
}

#[test]
fn injected_alternate_screen_setup_failure_unwinds_raw_mode() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let error = match TerminalResources::acquire_with(RecordingTerminal {
        calls: calls.clone(),
        fail: Some("alt+"),
    }) {
        Ok(_) => panic!("injected alternate screen setup must fail"),
        Err(error) => error,
    };

    assert!(error.to_string().contains("injected alt+ failure"));
    assert_eq!(*calls.lock().unwrap(), ["raw+", "alt+", "raw-"]);
}

#[test]
fn cleanup_attempts_all_resources_when_an_earlier_cleanup_fails() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut resources = TerminalResources::acquire_with(RecordingTerminal {
        calls: calls.clone(),
        fail: Some("alt-"),
    })
    .unwrap();
    assert!(resources.restore().is_err());

    assert_eq!(*calls.lock().unwrap(), ["raw+", "alt+", "alt-", "raw-"]);
}

#[test]
fn drop_restores_without_touching_mouse_capture() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    drop(
        TerminalResources::acquire_with(RecordingTerminal {
            calls: calls.clone(),
            fail: None,
        })
        .unwrap(),
    );

    let calls = calls.lock().unwrap();
    assert_eq!(*calls, ["raw+", "alt+", "alt-", "raw-"]);
    assert!(!calls.iter().any(|call| call.starts_with("mouse")));
}
