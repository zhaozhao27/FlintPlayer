#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyAction {
    ToggleOsd,
    ToggleFreeze,
    Snapshot,
    ToggleFullscreen,
    ExitFullscreen,
    ToggleDeviceMenu,
    ClosePanel,
    None,
}

pub fn action_for(text: &str) -> KeyAction {
    match text {
        "\t" => KeyAction::ToggleOsd,
        " " => KeyAction::ToggleFreeze,
        "s" | "S" => KeyAction::Snapshot,
        "\n" | "\r" | "f" | "F" => KeyAction::ToggleFullscreen,
        "\u{1b}" => KeyAction::ExitFullscreen,
        "alt+d" | "alt+D" => KeyAction::ToggleDeviceMenu,
        "ctrl+w" | "ctrl+W" | "\u{17}" => KeyAction::ClosePanel,
        _ => KeyAction::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_potplayer_shortcuts() {
        assert_eq!(action_for("\t"), KeyAction::ToggleOsd);
        assert_eq!(action_for(" "), KeyAction::ToggleFreeze);
        assert_eq!(action_for("s"), KeyAction::Snapshot);
        assert_eq!(action_for("S"), KeyAction::Snapshot);
        assert_eq!(action_for("\n"), KeyAction::ToggleFullscreen);
        assert_eq!(action_for("f"), KeyAction::ToggleFullscreen);
        assert_eq!(action_for("\u{1b}"), KeyAction::ExitFullscreen);
        assert_eq!(action_for("alt+d"), KeyAction::ToggleDeviceMenu);
        assert_eq!(action_for("alt+D"), KeyAction::ToggleDeviceMenu);
        assert_eq!(action_for("ctrl+w"), KeyAction::ClosePanel);
        assert_eq!(action_for("ctrl+W"), KeyAction::ClosePanel);
        assert_eq!(action_for("\u{17}"), KeyAction::ClosePanel);
    }

    #[test]
    fn ignores_other_keys() {
        assert_eq!(action_for("a"), KeyAction::None);
        assert_eq!(action_for(""), KeyAction::None);
        assert_eq!(action_for("F5"), KeyAction::None);
    }
}
