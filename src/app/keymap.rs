//! Lossless, atomically persisted keyboard configuration.

use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum KeyAction {
    Quit,
    OpenFilterMenu,
    OpenHelp,
    StartSearch,
    ClearSearch,
    Refresh,
    CycleSort,
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
            // The event loop quits on Ctrl+C in every mode anyway; binding it
            // here lists it in the keybindings panel.
            ((Modifiers::CONTROL, Char('c')), Quit),
            // Esc in the main view clears the active tab's search. `Ignore`
            // stays valid in keybinds.conf but is no longer bound by default.
            ((Modifiers::NONE, Esc), ClearSearch),
            ((Modifiers::NONE, Char('f')), OpenFilterMenu),
            ((Modifiers::NONE, Char('/')), StartSearch),
            ((Modifiers::NONE, Char('r')), Refresh),
            ((Modifiers::NONE, Char('s')), CycleSort),
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
            // Left/Right and h/l switch the focused pane. Paging stays on
            // PageUp/PageDown; MoveLeftPage/MoveRightPage remain valid in
            // keybinds.conf but are no longer bound by default.
            ((Modifiers::NONE, Left), ToggleUsersFocus),
            ((Modifiers::NONE, Right), ToggleUsersFocus),
            ((Modifiers::NONE, Char('k')), MoveUp),
            ((Modifiers::NONE, Char('j')), MoveDown),
            ((Modifiers::NONE, Char('h')), ToggleUsersFocus),
            ((Modifiers::NONE, Char('l')), ToggleUsersFocus),
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
        "ClearSearch" => ClearSearch,
        "Refresh" => Refresh,
        "CycleSort" => CycleSort,
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
        ClearSearch => "ClearSearch",
        Refresh => "Refresh",
        CycleSort => "CycleSort",
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

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn left_right_and_h_l_switch_panes_by_default() {
        let keymap = Keymap::default();
        for code in [
            KeyCode::Left,
            KeyCode::Right,
            KeyCode::Char('h'),
            KeyCode::Char('l'),
        ] {
            assert_eq!(
                keymap.resolve(&key(code)),
                Some(KeyAction::ToggleUsersFocus)
            );
        }
        assert_eq!(
            keymap.resolve(&key(KeyCode::PageDown)),
            Some(KeyAction::PageDown)
        );
        assert!(keymap.all_bindings().iter().all(|(_, action)| !matches!(
            action,
            KeyAction::MoveLeftPage | KeyAction::MoveRightPage
        )));
    }

    #[test]
    fn example_keybinds_parse_and_switch_panes_with_h_l() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/example-configs/keybinds.conf");
        let keymap = Keymap::from_file(path).unwrap();
        assert_eq!(
            keymap.resolve(&key(KeyCode::Char('l'))),
            Some(KeyAction::ToggleUsersFocus)
        );
        assert_eq!(
            keymap.resolve(&key(KeyCode::Left)),
            Some(KeyAction::ToggleUsersFocus)
        );
    }

    #[test]
    fn esc_clears_search_by_default_and_ignore_stays_parseable() {
        assert_eq!(
            Keymap::default().resolve(&key(KeyCode::Esc)),
            Some(KeyAction::ClearSearch)
        );
        assert_eq!(parse_action("ClearSearch"), Some(KeyAction::ClearSearch));
        assert_eq!(format_action(KeyAction::ClearSearch), "ClearSearch");
        assert_eq!(parse_action("Ignore"), Some(KeyAction::Ignore));
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/example-configs/keybinds.conf");
        assert_eq!(
            Keymap::from_file(path).unwrap().resolve(&key(KeyCode::Esc)),
            Some(KeyAction::ClearSearch)
        );
    }

    #[test]
    fn r_refreshes_by_default_and_in_the_example_config() {
        assert_eq!(
            Keymap::default().resolve(&key(KeyCode::Char('r'))),
            Some(KeyAction::Refresh)
        );
        assert_eq!(parse_action("Refresh"), Some(KeyAction::Refresh));
        assert_eq!(format_action(KeyAction::Refresh), "Refresh");
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/example-configs/keybinds.conf");
        assert_eq!(
            Keymap::from_file(path)
                .unwrap()
                .resolve(&key(KeyCode::Char('r'))),
            Some(KeyAction::Refresh)
        );
    }

    #[test]
    fn s_cycles_sort_by_default_and_in_the_example_config() {
        assert_eq!(
            Keymap::default().resolve(&key(KeyCode::Char('s'))),
            Some(KeyAction::CycleSort)
        );
        assert_eq!(parse_action("CycleSort"), Some(KeyAction::CycleSort));
        assert_eq!(format_action(KeyAction::CycleSort), "CycleSort");
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/example-configs/keybinds.conf");
        assert_eq!(
            Keymap::from_file(path)
                .unwrap()
                .resolve(&key(KeyCode::Char('s'))),
            Some(KeyAction::CycleSort)
        );
    }

    #[test]
    fn ctrl_c_quits_by_default_and_in_the_example_config() {
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(Keymap::default().resolve(&ctrl_c), Some(KeyAction::Quit));
        assert_eq!(
            Keymap::format_key(KeyModifiers::CONTROL, KeyCode::Char('c')),
            "Ctrl+c"
        );
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/example-configs/keybinds.conf");
        assert_eq!(
            Keymap::from_file(path).unwrap().resolve(&ctrl_c),
            Some(KeyAction::Quit)
        );
    }

    #[test]
    fn page_actions_stay_parseable_for_existing_configs() {
        assert_eq!(parse_action("MoveLeftPage"), Some(KeyAction::MoveLeftPage));
        assert_eq!(
            parse_action("MoveRightPage"),
            Some(KeyAction::MoveRightPage)
        );
    }
}
