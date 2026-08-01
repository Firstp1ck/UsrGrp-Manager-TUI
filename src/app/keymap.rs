//! Lossless, atomically persisted keyboard configuration.

use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum KeyAction {
    Quit,
    OpenFilterMenu,
    OpenHelp,
    StartSearch,
    NewUser,
    DeleteSelection,
    SwitchTab,
    ToggleUsersFocus,
    ToggleGroupsFocus,
    ToggleKeybindsPane,
    EnterAction,
    MoveUp,
    MoveDown,
    PageUp,
    PageDown,
    MoveLeftPage,
    MoveRightPage,
    Ignore,
}

#[derive(Clone, Debug)]
pub struct Keymap {
    bindings: HashMap<(KeyModifiers, KeyCode), KeyAction>,
}

impl Keymap {
    pub fn new_defaults() -> Self {
        use KeyAction::*;
        use KeyCode::*;
        use KeyModifiers as Modifiers;
        let entries = [
            ((Modifiers::NONE, Char('q')), Quit),
            ((Modifiers::NONE, Esc), Ignore),
            ((Modifiers::NONE, Char('f')), OpenFilterMenu),
            ((Modifiers::NONE, Char('/')), StartSearch),
            ((Modifiers::NONE, Char('n')), NewUser),
            ((Modifiers::NONE, Char('?')), OpenHelp),
            ((Modifiers::NONE, Delete), DeleteSelection),
            ((Modifiers::NONE, Tab), SwitchTab),
            ((Modifiers::NONE, BackTab), ToggleUsersFocus),
            ((Modifiers::SHIFT, BackTab), ToggleUsersFocus),
            ((Modifiers::SHIFT, Tab), ToggleUsersFocus),
            ((Modifiers::NONE, Enter), EnterAction),
            ((Modifiers::NONE, Up), MoveUp),
            ((Modifiers::NONE, Down), MoveDown),
            ((Modifiers::NONE, Left), MoveLeftPage),
            ((Modifiers::NONE, Right), MoveRightPage),
            ((Modifiers::NONE, Char('k')), MoveUp),
            ((Modifiers::NONE, Char('j')), MoveDown),
            ((Modifiers::NONE, Char('h')), MoveLeftPage),
            ((Modifiers::NONE, Char('l')), MoveRightPage),
            ((Modifiers::SHIFT, Char('k')), ToggleKeybindsPane),
            ((Modifiers::SHIFT, Char('K')), ToggleKeybindsPane),
            ((Modifiers::NONE, Char('K')), ToggleKeybindsPane),
            ((Modifiers::NONE, KeyCode::PageUp), KeyAction::PageUp),
            ((Modifiers::NONE, KeyCode::PageDown), KeyAction::PageDown),
        ];
        Self {
            bindings: entries.into_iter().collect(),
        }
    }

    pub fn load_or_init(path: &str) -> std::io::Result<Self> {
        match Self::from_file(path) {
            Ok(keymap) => Ok(keymap),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let keymap = Self::default();
                keymap.write_file(path)?;
                Ok(keymap)
            }
            Err(error) => Err(error),
        }
    }

    /// Read either canonical `Action = Key` lines or the old reversed form.
    pub fn from_file(path: &str) -> std::io::Result<Self> {
        let contents = crate::config::read_bounded(path)?;
        let mut keymap = Self::default();
        let mut seen = std::collections::HashSet::new();
        for assignment in crate::config::parse_assignments(&contents)? {
            let left = assignment.key.as_str();
            let right = assignment.value.as_str();
            let Some(action) = parse_action(left).or_else(|| parse_action(right)) else {
                return invalid_line(assignment.line, "unknown key action");
            };
            let key = if parse_action(left).is_some() {
                parse_key(right)
            } else {
                parse_key(left)
            }
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "configuration line {}: invalid key binding",
                        assignment.line
                    ),
                )
            })?;
            if !seen.insert(key) {
                return invalid_line(assignment.line, "duplicate key binding");
            }
            keymap.bindings.insert(key, action);
        }
        Ok(keymap)
    }

    /// Persist exactly every current binding in a stable order.
    pub fn write_file(&self, path: &str) -> std::io::Result<()> {
        let mut bindings = self.all_bindings();
        bindings.sort_by_key(|((modifiers, code), action)| {
            (format_action(*action), Self::format_key(*modifiers, *code))
        });
        let mut contents = String::from(
            "# usrgrp-manager keybindings\n# Format: Action = KeySpec\n# This file is atomically saved; every active binding is retained.\n\n",
        );
        for ((modifiers, code), action) in bindings {
            contents.push_str(format_action(action));
            contents.push_str(" = ");
            contents.push_str(&Self::format_key(modifiers, code));
            contents.push('\n');
        }
        crate::config::atomic_write(path, contents.as_bytes())
    }

    pub fn resolve(&self, key: &KeyEvent) -> Option<KeyAction> {
        self.bindings.get(&(key.modifiers, key.code)).copied()
    }

    pub fn all_bindings(&self) -> Vec<((KeyModifiers, KeyCode), KeyAction)> {
        self.bindings
            .iter()
            .map(|(key, action)| (*key, *action))
            .collect()
    }

    pub fn format_key(modifiers: KeyModifiers, code: KeyCode) -> String {
        let mut prefixes = Vec::new();
        if modifiers.contains(KeyModifiers::CONTROL) {
            prefixes.push("Ctrl");
        }
        if modifiers.contains(KeyModifiers::ALT) {
            prefixes.push("Alt");
        }
        if modifiers.contains(KeyModifiers::SHIFT) {
            prefixes.push("Shift");
        }
        let code = match code {
            KeyCode::Enter => "Enter".to_owned(),
            KeyCode::Delete => "Delete".to_owned(),
            KeyCode::Esc => "Esc".to_owned(),
            KeyCode::Tab => "Tab".to_owned(),
            KeyCode::BackTab => "BackTab".to_owned(),
            KeyCode::Up => "Up".to_owned(),
            KeyCode::Down => "Down".to_owned(),
            KeyCode::Left => "Left".to_owned(),
            KeyCode::Right => "Right".to_owned(),
            KeyCode::PageUp => "PageUp".to_owned(),
            KeyCode::PageDown => "PageDown".to_owned(),
            KeyCode::Char(character) => character.to_string(),
            other => format!("{other:?}"),
        };
        if prefixes.is_empty() {
            code
        } else {
            format!("{}+{code}", prefixes.join("+"))
        }
    }
}

impl Default for Keymap {
    fn default() -> Self {
        Self::new_defaults()
    }
}

fn parse_key(specification: &str) -> Option<(KeyModifiers, KeyCode)> {
    let mut modifiers = KeyModifiers::NONE;
    let mut remaining = specification.trim();
    while let Some((prefix, rest)) = remaining.split_once('+') {
        match prefix {
            "Ctrl" => modifiers |= KeyModifiers::CONTROL,
            "Alt" => modifiers |= KeyModifiers::ALT,
            "Shift" => modifiers |= KeyModifiers::SHIFT,
            _ => break,
        }
        remaining = rest;
    }
    let code = match remaining {
        "Enter" => KeyCode::Enter,
        "Delete" => KeyCode::Delete,
        "Esc" | "Escape" => KeyCode::Esc,
        "Tab" => KeyCode::Tab,
        "BackTab" => KeyCode::BackTab,
        "Up" => KeyCode::Up,
        "Down" => KeyCode::Down,
        "Left" => KeyCode::Left,
        "Right" => KeyCode::Right,
        "PageUp" => KeyCode::PageUp,
        "PageDown" => KeyCode::PageDown,
        value if value.chars().count() == 1 => KeyCode::Char(value.chars().next()?),
        _ => return None,
    };
    Some((modifiers, code))
}

fn invalid_line<T>(line: usize, reason: &str) -> std::io::Result<T> {
    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!("configuration line {line}: {reason}"),
    ))
}

fn parse_action(value: &str) -> Option<KeyAction> {
    use KeyAction::*;
    Some(match value.trim() {
        "Quit" => Quit,
        "OpenFilterMenu" => OpenFilterMenu,
        "OpenHelp" => OpenHelp,
        "StartSearch" => StartSearch,
        "NewUser" => NewUser,
        "DeleteSelection" => DeleteSelection,
        "SwitchTab" => SwitchTab,
        "ToggleUsersFocus" => ToggleUsersFocus,
        "ToggleGroupsFocus" => ToggleGroupsFocus,
        "ToggleKeybindsPane" => ToggleKeybindsPane,
        "EnterAction" => EnterAction,
        "MoveUp" => MoveUp,
        "MoveDown" => MoveDown,
        "MoveLeftPage" => MoveLeftPage,
        "MoveRightPage" => MoveRightPage,
        "PageUp" => PageUp,
        "PageDown" => PageDown,
        "Ignore" => Ignore,
        _ => return None,
    })
}

pub fn format_action(action: KeyAction) -> &'static str {
    use KeyAction::*;
    match action {
        Quit => "Quit",
        OpenFilterMenu => "OpenFilterMenu",
        OpenHelp => "OpenHelp",
        StartSearch => "StartSearch",
        NewUser => "NewUser",
        DeleteSelection => "DeleteSelection",
        SwitchTab => "SwitchTab",
        ToggleUsersFocus => "ToggleUsersFocus",
        ToggleGroupsFocus => "ToggleGroupsFocus",
        ToggleKeybindsPane => "ToggleKeybindsPane",
        EnterAction => "EnterAction",
        MoveUp => "MoveUp",
        MoveDown => "MoveDown",
        PageUp => "PageUp",
        PageDown => "PageDown",
        MoveLeftPage => "MoveLeftPage",
        MoveRightPage => "MoveRightPage",
        Ignore => "Ignore",
    }
}
