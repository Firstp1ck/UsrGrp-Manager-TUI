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
fn injected_mouse_setup_failure_unwinds_every_prior_capability() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let error = match TerminalResources::acquire_with(RecordingTerminal {
        calls: calls.clone(),
        fail: Some("mouse+"),
    }) {
        Ok(_) => panic!("injected mouse setup must fail"),
        Err(error) => error,
    };

    assert!(error.to_string().contains("injected mouse+ failure"));
    assert_eq!(
        *calls.lock().unwrap(),
        ["raw+", "alt+", "mouse+", "alt-", "raw-"]
    );
}

#[test]
fn cleanup_attempts_all_resources_when_an_earlier_cleanup_fails() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut resources = TerminalResources::acquire_with(RecordingTerminal {
        calls: calls.clone(),
        fail: Some("mouse-"),
    })
    .unwrap();
    assert!(resources.restore().is_err());

    assert_eq!(
        *calls.lock().unwrap(),
        ["raw+", "alt+", "mouse+", "mouse-", "alt-", "raw-"]
    );
}
