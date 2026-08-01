//! Panic-safe terminal session ownership with independently tracked resources.

use std::io;

use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

/// Minimal terminal side-effect seam. Tests inject a recording implementation;
/// production uses [`SystemTerminalControl`].
pub trait TerminalControl {
    fn enable_raw_mode(&mut self) -> io::Result<()>;
    fn disable_raw_mode(&mut self) -> io::Result<()>;
    fn enter_alternate_screen(&mut self) -> io::Result<()>;
    fn leave_alternate_screen(&mut self) -> io::Result<()>;
    fn enable_mouse_capture(&mut self) -> io::Result<()>;
    fn disable_mouse_capture(&mut self) -> io::Result<()>;
}

/// Production control implementation. Each acquisition is emitted separately
/// so a later failure can unwind every earlier successful acquisition.
pub struct SystemTerminalControl;
impl TerminalControl for SystemTerminalControl {
    fn enable_raw_mode(&mut self) -> io::Result<()> {
        enable_raw_mode()
    }
    fn disable_raw_mode(&mut self) -> io::Result<()> {
        disable_raw_mode()
    }
    fn enter_alternate_screen(&mut self) -> io::Result<()> {
        execute!(std::io::stdout(), EnterAlternateScreen)
    }
    fn leave_alternate_screen(&mut self) -> io::Result<()> {
        execute!(std::io::stdout(), LeaveAlternateScreen)
    }
    fn enable_mouse_capture(&mut self) -> io::Result<()> {
        execute!(std::io::stdout(), EnableMouseCapture)
    }
    fn disable_mouse_capture(&mut self) -> io::Result<()> {
        execute!(std::io::stdout(), DisableMouseCapture)
    }
}

/// Tracks only capabilities which have actually been acquired. It is public so
/// injected failure tests do not need a real terminal or PTY.
pub struct TerminalResources<C: TerminalControl> {
    control: C,
    raw_mode: bool,
    alternate_screen: bool,
    mouse_capture: bool,
}

impl<C: TerminalControl> TerminalResources<C> {
    pub fn acquire_with(control: C) -> io::Result<Self> {
        let mut resources = Self {
            control,
            raw_mode: false,
            alternate_screen: false,
            mouse_capture: false,
        };
        resources.control.enable_raw_mode()?;
        resources.raw_mode = true;
        if let Err(error) = resources.control.enter_alternate_screen() {
            return Err(resources.with_cleanup(error));
        }
        resources.alternate_screen = true;
        if let Err(error) = resources.control.enable_mouse_capture() {
            return Err(resources.with_cleanup(error));
        }
        resources.mouse_capture = true;
        Ok(resources)
    }

    /// Best-effort reverse cleanup. Every acquired capability is attempted even
    /// when an earlier cleanup fails; the first failure remains observable.
    pub fn restore(&mut self) -> io::Result<()> {
        let mut first_error = None;
        if self.mouse_capture {
            match self.control.disable_mouse_capture() {
                Ok(()) => self.mouse_capture = false,
                Err(error) => first_error = Some(error),
            }
        }
        if self.alternate_screen {
            match self.control.leave_alternate_screen() {
                Ok(()) => self.alternate_screen = false,
                Err(error) if first_error.is_none() => first_error = Some(error),
                Err(_) => {}
            }
        }
        if self.raw_mode {
            match self.control.disable_raw_mode() {
                Ok(()) => self.raw_mode = false,
                Err(error) if first_error.is_none() => first_error = Some(error),
                Err(_) => {}
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    fn with_cleanup(&mut self, primary: io::Error) -> io::Error {
        match self.restore() {
            Ok(()) => primary,
            Err(cleanup) => io::Error::new(
                primary.kind(),
                format!("terminal setup failed: {primary}; cleanup also failed: {cleanup}"),
            ),
        }
    }
}

impl<C: TerminalControl> Drop for TerminalResources<C> {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

/// Owns terminal setup and restores every acquired resource on drop, including
/// partial initialization failures and panic unwinding.
pub struct TerminalSession {
    terminal: Terminal<CrosstermBackend<std::io::Stdout>>,
    resources: TerminalResources<SystemTerminalControl>,
}

impl TerminalSession {
    pub fn enter() -> io::Result<Self> {
        let mut resources = TerminalResources::acquire_with(SystemTerminalControl)?;
        match Terminal::new(CrosstermBackend::new(std::io::stdout())) {
            Ok(terminal) => Ok(Self {
                terminal,
                resources,
            }),
            Err(error) => Err(resources.with_cleanup(error)),
        }
    }

    pub fn terminal_mut(&mut self) -> &mut Terminal<CrosstermBackend<std::io::Stdout>> {
        &mut self.terminal
    }

    /// Explicit cleanup reports failure to callers while `Drop` remains the
    /// final panic-safe backstop. Cursor restoration is attempted regardless
    /// of resource cleanup failures.
    pub fn restore(&mut self) -> io::Result<()> {
        let resource_error = self.resources.restore().err();
        let cursor_error = self.terminal.show_cursor().err();
        match (resource_error, cursor_error) {
            (None, None) => Ok(()),
            (Some(error), None) | (None, Some(error)) => Err(error),
            (Some(resource), Some(cursor)) => Err(io::Error::new(
                resource.kind(),
                format!(
                    "terminal cleanup failed: {resource}; cursor cleanup also failed: {cursor}"
                ),
            )),
        }
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}
