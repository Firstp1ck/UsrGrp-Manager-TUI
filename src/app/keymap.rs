//! Keybinding configuration: parse `keybinds.conf`, provide defaults, and map keys to actions.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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
    // canonical mapping from (modifiers, code) to action
    bindings: std::collections::HashMap<(KeyModifiers, KeyCode), KeyAction>,
}

impl Keymap {
    pub fn new_defaults() -> Self {
        use KeyCode::*;
        use KeyModifiers as M;
        let mut bindings = std::collections::HashMap::new();
        // Core actions matching current hardcoded behavior
        bindings.insert((M::NONE, Char('q')), KeyAction::Quit);
        bindings.insert((M::NONE, Esc), KeyAction::Ignore);
        bindings.insert((M::NONE, Char('f')), KeyAction::OpenFilterMenu);
        bindings.insert((M::NONE, Char('/')), KeyAction::StartSearch);
        bindings.insert((M::NONE, Char('n')), KeyAction::NewUser);
        bindings.insert((M::NONE, Char('?')), KeyAction::OpenHelp);
        bindings.insert((M::NONE, KeyCode::Delete), KeyAction::DeleteSelection);
        bindings.insert((M::NONE, Tab), KeyAction::SwitchTab);
        // Shift+Tab is BackTab in crossterm
        bindings.insert((M::NONE, BackTab), KeyAction::ToggleUsersFocus);
        // Some terminals report BackTab with SHIFT modifier, and some send Tab+SHIFT
        bindings.insert((M::SHIFT, BackTab), KeyAction::ToggleUsersFocus);
        bindings.insert((M::SHIFT, Tab), KeyAction::ToggleUsersFocus);
        // Ctrl+Tab no longer toggles panes in Groups

        bindings.insert((M::NONE, Enter), KeyAction::EnterAction);
        // Navigation
        bindings.insert((M::NONE, Up), KeyAction::MoveUp);
        bindings.insert((M::NONE, Down), KeyAction::MoveDown);
        bindings.insert((M::NONE, Left), KeyAction::MoveLeftPage);
        bindings.insert((M::NONE, Right), KeyAction::MoveRightPage);
        // Vim-like keys
        bindings.insert((M::NONE, Char('k')), KeyAction::MoveUp);
        bindings.insert((M::NONE, Char('j')), KeyAction::MoveDown);
        bindings.insert((M::NONE, Char('h')), KeyAction::MoveLeftPage);
        bindings.insert((M::NONE, Char('l')), KeyAction::MoveRightPage);
        // Toggle keybindings pane (support Shift+K variants across terminals)
        bindings.insert((M::SHIFT, Char('k')), KeyAction::ToggleKeybindsPane);
        bindings.insert((M::SHIFT, Char('K')), KeyAction::ToggleKeybindsPane);
        bindings.insert((M::NONE, Char('K')), KeyAction::ToggleKeybindsPane);

        // Page keys
        bindings.insert((M::NONE, PageUp), KeyAction::PageUp);
        bindings.insert((M::NONE, PageDown), KeyAction::PageDown);

        Self { bindings }
    }

    pub fn load_or_init(path: &str) -> Self {
        let p = std::path::Path::new(path);
        if p.exists() {
            return Self::from_file(path).unwrap_or_default();
        }
        if let Some(existing) = crate::app::config_file_read_path("keybinds.conf") {
            return Self::from_file(&existing).unwrap_or_default();
        }
        let km = Self::default();
        let _ = km.write_file(path);
        km
    }

    pub fn from_file(path: &str) -> Option<Self> {
        let contents = std::fs::read_to_string(path).ok()?;
        let mut map = Self::default();
        // Start from defaults, then override with user-specified bindings
        for raw in contents.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.splitn(2, '=');
            let lhs = parts.next().map(|s| s.trim()).unwrap_or("");
            let rhs = parts.next().map(|s| s.trim()).unwrap_or("");
            if lhs.is_empty() || rhs.is_empty() {
                continue;
            }
            // Preferred format: Action = KeySpec
            if let (Some(action), Some(key)) = (parse_action(lhs), parse_key(rhs)) {
                map.bindings.insert(key, action);
                continue;
            }
            // Backward-compatible format: KeySpec = Action
            if let (Some(key), Some(action)) = (parse_key(lhs), parse_action(rhs)) {
                map.bindings.insert(key, action);
                continue;
            }
        }
        Some(map)
    }

    pub fn write_file(&self, path: &str) -> std::io::Result<()> {
        use std::fmt::Write as _;
        let mut buf = String::new();
        buf.push_str("# usrgrp-manager keybindings\n");
        buf.push_str("# Format: <Action> = <KeySpec>\n");
        buf.push_str("# KeySpec examples: q, Ctrl+q, Enter, Esc, Tab, BackTab, Up, Down, Left, Right, PageUp, PageDown, Delete, /, n, f, j, k, h, l\n");
        buf.push_str("# Actions: Quit, OpenFilterMenu, StartSearch, NewUser, DeleteSelection, SwitchTab, ToggleUsersFocus, ToggleGroupsFocus, ToggleKeybindsPane, EnterAction, MoveUp, MoveDown, MoveLeftPage, MoveRightPage, PageUp, PageDown, Ignore\n\n");
        buf.push_str("# Additional: OpenHelp (mapped to '?')\n\n");

        // Emit a stable, readable subset of current bindings
        let dump = [
            ("q", KeyAction::Quit),
            ("Esc", KeyAction::Ignore),
            ("f", KeyAction::OpenFilterMenu),
            ("/", KeyAction::StartSearch),
            ("n", KeyAction::NewUser),
            ("Tab", KeyAction::SwitchTab),
            ("BackTab", KeyAction::ToggleUsersFocus),
            ("?", KeyAction::OpenHelp),
            ("Enter", KeyAction::EnterAction),
            ("Up", KeyAction::MoveUp),
            ("Down", KeyAction::MoveDown),
            ("Left", KeyAction::MoveLeftPage),
            ("Right", KeyAction::MoveRightPage),
            ("j", KeyAction::MoveDown),
            ("k", KeyAction::MoveUp),
            ("h", KeyAction::MoveLeftPage),
            ("l", KeyAction::MoveRightPage),
            ("PageUp", KeyAction::PageUp),
            ("PageDown", KeyAction::PageDown),
            ("Delete", KeyAction::DeleteSelection),
        ];
        for (k, a) in dump {
            let _ = writeln!(&mut buf, "{} = {}", format_action(a), k);
        }

        std::fs::write(path, buf)
    }

    pub fn resolve(&self, key: &KeyEvent) -> Option<KeyAction> {
        let mm = key.modifiers;
        let code = key.code;
        self.bindings.get(&(mm, code)).copied()
    }

    /// Return a snapshot of all bindings as ((modifiers, code), action) pairs.
    pub fn all_bindings(&self) -> Vec<((KeyModifiers, KeyCode), KeyAction)> {
        self.bindings.iter().map(|(k, v)| (*k, *v)).collect()
    }

    /// Format a key (modifiers + code) into a human-readable spec like "Ctrl+q", "BackTab".
    pub fn format_key(mods: KeyModifiers, code: KeyCode) -> String {
        use KeyCode::*;
        let base = match code {
            Enter => "Enter".to_string(),
            Delete => "Delete".to_string(),
            Esc => "Esc".to_string(),
            Tab => "Tab".to_string(),
            BackTab => "BackTab".to_string(),
            Up => "Up".to_string(),
            Down => "Down".to_string(),
            Left => "Left".to_string(),
            Right => "Right".to_string(),
            PageUp => "PageUp".to_string(),
            PageDown => "PageDown".to_string(),
            Char('/') => "/".to_string(),
            Char(c) => c.to_string(),
            _ => format!("{:?}", code),
        };
        if mods.contains(KeyModifiers::CONTROL) {
            format!("Ctrl+{}", base)
        } else if mods.is_empty() {
            base
        } else {
            // Future: format other modifiers when supported
            base
        }
    }
}

impl Default for Keymap {
    fn default() -> Self {
        Self::new_defaults()
    }
}

fn parse_key(spec: &str) -> Option<(KeyModifiers, KeyCode)> {
    use KeyCode::*;
    let s = spec.trim();
    let mut rest = s;
    let mut mods = KeyModifiers::NONE;
    if let Some(after) = s.strip_prefix("Ctrl+") {
        mods |= KeyModifiers::CONTROL;
        rest = after;
    }
    // Future: Alt+ / Shift+
    let code = match rest {
        "Enter" => Enter,
        "Delete" => Delete,
        "/" => Char('/'),
        "Esc" | "Escape" => Esc,
        "Tab" => Tab,
        "BackTab" => BackTab,
        "Up" => Up,
        "Down" => Down,
        "Left" => Left,
        "Right" => Right,
        "PageUp" => PageUp,
        "PageDown" => PageDown,
        _ => {
            let chars: Vec<char> = rest.chars().collect();
            if chars.len() == 1 {
                KeyCode::Char(chars[0])
            } else {
                return None;
            }
        }
    };
    Some((mods, code))
}

fn parse_action(s: &str) -> Option<KeyAction> {
    match s.trim() {
        "Quit" => Some(KeyAction::Quit),
        "OpenFilterMenu" => Some(KeyAction::OpenFilterMenu),
        "OpenHelp" => Some(KeyAction::OpenHelp),
        "StartSearch" => Some(KeyAction::StartSearch),
        "NewUser" => Some(KeyAction::NewUser),
        "DeleteSelection" => Some(KeyAction::DeleteSelection),
        "SwitchTab" => Some(KeyAction::SwitchTab),
        "ToggleUsersFocus" => Some(KeyAction::ToggleUsersFocus),
        "ToggleGroupsFocus" => Some(KeyAction::ToggleGroupsFocus),
        "ToggleKeybindsPane" => Some(KeyAction::ToggleKeybindsPane),
        "EnterAction" => Some(KeyAction::EnterAction),
        "MoveUp" => Some(KeyAction::MoveUp),
        "MoveDown" => Some(KeyAction::MoveDown),
        "MoveLeftPage" => Some(KeyAction::MoveLeftPage),
        "MoveRightPage" => Some(KeyAction::MoveRightPage),
        "PageUp" => Some(KeyAction::PageUp),
        "PageDown" => Some(KeyAction::PageDown),
        "Ignore" => Some(KeyAction::Ignore),
        _ => None,
    }
}

pub fn format_action(a: KeyAction) -> &'static str {
    match a {
        KeyAction::Quit => "Quit",
        KeyAction::OpenFilterMenu => "OpenFilterMenu",
        KeyAction::OpenHelp => "OpenHelp",
        KeyAction::StartSearch => "StartSearch",
        KeyAction::NewUser => "NewUser",
        KeyAction::DeleteSelection => "DeleteSelection",
        KeyAction::SwitchTab => "SwitchTab",
        KeyAction::ToggleUsersFocus => "ToggleUsersFocus",
        KeyAction::ToggleGroupsFocus => "ToggleGroupsFocus",
        KeyAction::ToggleKeybindsPane => "ToggleKeybindsPane",
        KeyAction::EnterAction => "EnterAction",
        KeyAction::MoveUp => "MoveUp",
        KeyAction::MoveDown => "MoveDown",
        KeyAction::MoveLeftPage => "MoveLeftPage",
        KeyAction::MoveRightPage => "MoveRightPage",
        KeyAction::PageUp => "PageUp",
        KeyAction::PageDown => "PageDown",
        KeyAction::Ignore => "Ignore",
    }
}
