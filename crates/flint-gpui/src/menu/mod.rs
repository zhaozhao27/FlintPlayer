mod catalog;
mod dynamic;

pub use catalog::main_menu;
pub use dynamic::{apply_dynamic, MenuData};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EntryKind {
    #[default]
    Action,
    Submenu,
    Separator,
}

#[derive(Clone, Debug, Default)]
pub struct MenuEntry {
    pub kind: EntryKind,
    pub title: String,
    pub shortcut: Option<String>,
    pub action: Option<String>,
    pub icon: Option<String>,
    pub enabled: bool,
    pub checked: Option<bool>,
    pub children: Vec<MenuEntry>,
}

impl MenuEntry {
    pub fn action(title: impl Into<String>, action: impl Into<String>) -> Self {
        Self {
            kind: EntryKind::Action,
            title: title.into(),
            action: Some(action.into()),
            enabled: true,
            ..Self::default()
        }
    }

    pub fn submenu(title: impl Into<String>, children: Vec<MenuEntry>) -> Self {
        Self {
            kind: EntryKind::Submenu,
            title: title.into(),
            children,
            enabled: true,
            ..Self::default()
        }
    }

    pub fn separator() -> Self {
        Self {
            kind: EntryKind::Separator,
            ..Self::default()
        }
    }

    pub fn with_shortcut(mut self, shortcut: impl Into<String>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    pub fn with_icon(mut self, icon: impl Into<String>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = Some(checked);
        self
    }

    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    pub fn disabled_if(mut self, disabled: bool) -> Self {
        self.enabled = !disabled;
        self
    }

    pub fn is_separator(&self) -> bool {
        self.kind == EntryKind::Separator
    }

    pub fn is_submenu(&self) -> bool {
        self.kind == EntryKind::Submenu
    }

    pub fn set_enabled(&mut self, action: &str, enabled: bool) {
        for child in &mut self.children {
            child.set_enabled(action, enabled);
        }
        if self.action.as_deref() == Some(action) {
            self.enabled = enabled;
        }
    }

    pub fn set_checked(&mut self, action: &str, checked: bool) {
        for child in &mut self.children {
            child.set_checked(action, checked);
        }
        if self.action.as_deref() == Some(action) {
            self.checked = Some(checked);
        }
    }

    pub fn find(&self, action: &str) -> Option<&MenuEntry> {
        if self.action.as_deref() == Some(action) {
            return Some(self);
        }
        self.children.iter().find_map(|child| child.find(action))
    }
}

pub fn collect_shortcuts(entries: &[MenuEntry]) -> Vec<(String, String)> {
    fn walk(entries: &[MenuEntry], out: &mut Vec<(String, String)>) {
        for entry in entries {
            if let (Some(shortcut), Some(action)) = (&entry.shortcut, &entry.action) {
                if !shortcut.is_empty() && !action.is_empty() {
                    out.push((shortcut.clone(), action.clone()));
                }
            }
            walk(&entry.children, out);
        }
    }
    let mut out = Vec::new();
    walk(entries, &mut out);
    out
}

pub fn translate_titles(entries: &mut [MenuEntry]) {
    for entry in entries {
        entry.title = crate::i18n::tr(&entry.title);
        translate_titles(&mut entry.children);
    }
}

pub fn search_actions(entries: &[MenuEntry], query: &str) -> Vec<(String, String)> {
    fn walk(entries: &[MenuEntry], query: &str, out: &mut Vec<(String, String)>) {
        for entry in entries {
            if let Some(action) = &entry.action {
                if !action.is_empty()
                    && entry.enabled
                    && !entry.is_submenu()
                    && entry.title.to_lowercase().contains(query)
                {
                    out.push((entry.title.clone(), action.clone()));
                }
            }
            walk(&entry.children, query, out);
        }
    }
    let mut out = Vec::new();
    walk(entries, query, &mut out);
    out
}

pub fn find_action<'a>(entries: &'a [MenuEntry], action: &str) -> Option<&'a MenuEntry> {
    entries.iter().find_map(|entry| entry.find(action))
}

pub fn set_checked_all(entries: &mut [MenuEntry], action: &str, checked: bool) {
    for entry in entries {
        entry.set_checked(action, checked);
    }
}

pub fn set_enabled_all(entries: &mut [MenuEntry], action: &str, enabled: bool) {
    for entry in entries {
        entry.set_enabled(action, enabled);
    }
}
