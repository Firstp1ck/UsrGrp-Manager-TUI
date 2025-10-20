//! Keybinding configuration: parse `keybinds.conf`, provide defaults, and map keys to actions.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KeyAction {
    Quit,
    OpenFilterMenu,
    StartSearch,
    NewUser,
    SwitchTab,
    ToggleUsersFocus,
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
    pub fn default() -> Self {
        use KeyCode::*;
        use KeyModifiers as M;
        let mut bindings = std::collections::HashMap::new();
        // Core actions matching current hardcoded behavior
        bindings.insert((M::NONE, Char('q')), KeyAction::Quit);
        bindings.insert((M::NONE, Esc), KeyAction::Ignore);
        bindings.insert((M::NONE, Char('f')), KeyAction::OpenFilterMenu);
        bindings.insert((M::NONE, Char('/')), KeyAction::StartSearch);
        bindings.insert((M::NONE, Char('n')), KeyAction::NewUser);
        bindings.insert((M::NONE, Tab), KeyAction::SwitchTab);
        // Shift+Tab is BackTab in crossterm
        bindings.insert((M::NONE, BackTab), KeyAction::ToggleUsersFocus);

        bindings.insert((M::NONE, Enter), KeyAction::EnterAction);
        // Navigation
        bindings.insert((M::NONE, Up), KeyAction::MoveUp);
        bindings.insert((M::NONE, Down), KeyAction::MoveDown);
        bindings.insert((M::NONE, Left), KeyAction::MoveLeftPage);
        bindings.insert((M::NONE, Right), KeyAction::MoveRightPage);
        // Vim keys
        bindings.insert((M::NONE, Char('k')), KeyAction::MoveUp);
        bindings.insert((M::NONE, Char('j')), KeyAction::MoveDown);
        bindings.insert((M::NONE, Char('h')), KeyAction::MoveLeftPage);
        bindings.insert((M::NONE, Char('l')), KeyAction::MoveRightPage);
        // Page keys
        bindings.insert((M::NONE, PageUp), KeyAction::PageUp);
        bindings.insert((M::NONE, PageDown), KeyAction::PageDown);

        Self { bindings }
    }

    pub fn load_or_init(path: &str) -> Self {
        let p = std::path::Path::new(path);
        if p.exists() {
            return Self::from_file(path).unwrap_or_else(Self::default);
        }
        if let Some(existing) = crate::app::config_file_read_path("keybinds.conf") {
            return Self::from_file(&existing).unwrap_or_else(Self::default);
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
            if line.is_empty() || line.starts_with('#') { continue; }
            let mut parts = line.splitn(2, '=');
            let lhs = parts.next().map(|s| s.trim()).unwrap_or("");
            let rhs = parts.next().map(|s| s.trim()).unwrap_or("");
            if lhs.is_empty() || rhs.is_empty() { continue; }
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
        buf.push_str("# KeySpec examples: q, Ctrl+q, Enter, Esc, Tab, BackTab, Up, Down, Left, Right, PageUp, PageDown, /, n, f, j, k, h, l\n");
        buf.push_str("# Actions: Quit, OpenFilterMenu, StartSearch, NewUser, SwitchTab, ToggleUsersFocus, EnterAction, MoveUp, MoveDown, MoveLeftPage, MoveRightPage, PageUp, PageDown, Ignore\n\n");

        // Emit a stable, readable subset of current bindings
        let dump = [
            ("q", KeyAction::Quit),
            ("Esc", KeyAction::Ignore),
            ("f", KeyAction::OpenFilterMenu),
            ("/", KeyAction::StartSearch),
            ("n", KeyAction::NewUser),
            ("Tab", KeyAction::SwitchTab),
            ("BackTab", KeyAction::ToggleUsersFocus),
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
}

// Intentionally no Default trait to avoid test/builds pulling in file IO; use Keymap::default()

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
        "StartSearch" => Some(KeyAction::StartSearch),
        "NewUser" => Some(KeyAction::NewUser),
        "SwitchTab" => Some(KeyAction::SwitchTab),
        "ToggleUsersFocus" => Some(KeyAction::ToggleUsersFocus),
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

fn format_action(a: KeyAction) -> &'static str {
    match a {
        KeyAction::Quit => "Quit",
        KeyAction::OpenFilterMenu => "OpenFilterMenu",
        KeyAction::StartSearch => "StartSearch",
        KeyAction::NewUser => "NewUser",
        KeyAction::SwitchTab => "SwitchTab",
        KeyAction::ToggleUsersFocus => "ToggleUsersFocus",
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


