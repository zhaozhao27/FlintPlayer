#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyAction {
    ToggleOsd,
    BriefInfo,
    PlaybackInfo,
    ToggleFreeze,
    Snapshot,
    ToggleRecording,
    ToggleAudioRecording,
    ToggleAudioProcessing,
    ToggleAudioNormalize,
    ToggleAudioCrystallize,
    ToggleAudioDenoise,
    ToggleAudioReverb,
    ToggleAudioVocalRemove,
    ToggleAudioVocalEnhance,
    ToggleEqualizer,
    StepBackward,
    StepForward,
    ToggleFullscreen,
    ExitFullscreen,
    ClosePanel,
    OpenSettings,
    OpenFile,
    OpenClipboard,
    OpenUrl,
    MenuSearch,
    Reopen,
    OpenSubtitle,
    OpenSubtitleBrowser,
    ManualSubtitle,
    ToggleSubtitles,
    OpenFolder,
    OpenAbout,
    ToggleSidebar,
    OpenBurst,
    OpenThumbnail,
    PreviousPanel,
    NextPanel,
    SeekBackward,
    SeekForward,
    VolumeUp,
    VolumeDown,
    ToggleMute,
    SpeedUp,
    SpeedDown,
    SpeedReset,
    ToggleAbLoop,
    AddBookmark,
    PreviousChapter,
    NextChapter,
    FavoriteAddFile,
    FavoriteAddFolder,
    CycleStereo,
    CaptureSlot1,
    CaptureSlot2,
    CaptureStart1,
    CaptureStart2,
    BrightnessDown,
    BrightnessUp,
    SaturationDown,
    SaturationUp,
    AdjustReset,
    AudioSyncLag,
    AudioSyncLead,
    AudioSyncReset,
    AudioSwap,
    FlipVertical,
    FlipHorizontal,
    CycleMargin,
    RotateCycle,
    CyclePostAction,
    TopmostToggle,
    AspectCycle,
    AspectRatioCycle,
    FrameLarger,
    FrameSmaller,
    FrameWider,
    FrameNarrower,
    FrameTaller,
    FrameShorter,
    FrameShiftLeft,
    FrameShiftRight,
    FrameShiftUp,
    FrameShiftDown,
    FrameCenter,
    None,
}

pub fn action_for(text: &str) -> KeyAction {
    match text {
        "\t" => KeyAction::ToggleOsd,
        "shift+tab" => KeyAction::BriefInfo,
        "scrolllock" => KeyAction::PlaybackInfo,
        " " => KeyAction::ToggleFreeze,
        "s" | "S" | "k" | "K" => KeyAction::Snapshot,
        "r" | "R" => KeyAction::ToggleRecording,
        "G" | "shift+g" | "shift+G" => KeyAction::ToggleAudioRecording,
        "shift+x" | "shift+X" => KeyAction::ToggleAudioProcessing,
        "N" | "shift+n" | "shift+N" => KeyAction::ToggleAudioNormalize,
        "shift+c" | "shift+C" => KeyAction::ToggleAudioCrystallize,
        "D" | "shift+d" | "shift+D" => KeyAction::ToggleAudioDenoise,
        "shift+r" | "shift+R" => KeyAction::ToggleAudioReverb,
        "V" | "shift+v" | "shift+V" => KeyAction::ToggleAudioVocalRemove,
        "A" | "shift+a" | "shift+A" => KeyAction::ToggleAudioVocalEnhance,
        "shift+e" | "shift+E" => KeyAction::ToggleEqualizer,
        "," => KeyAction::StepBackward,
        "." => KeyAction::StepForward,
        "\n" | "\r" | "f" | "F" | "alt+enter" => KeyAction::ToggleFullscreen,
        "\u{1b}" => KeyAction::ExitFullscreen,
        "alt+d" | "alt+D" => KeyAction::OpenSettings,
        "alt+o" | "alt+O" => KeyAction::OpenSubtitle,
        "alt+e" | "alt+E" => KeyAction::OpenSubtitleBrowser,
        "alt+p" | "alt+P" => KeyAction::ManualSubtitle,
        "alt+h" | "alt+H" => KeyAction::ToggleSubtitles,
        "l" | "L" => KeyAction::OpenSubtitle,
        "ctrl+o" | "ctrl+O" => KeyAction::OpenFile,
        "ctrl+u" | "ctrl+U" | "\u{15}" => KeyAction::OpenUrl,
        "ctrl+f12" | "\u{f70f}" => KeyAction::MenuSearch,
        "ctrl+v" | "ctrl+V" | "\u{16}" => KeyAction::OpenClipboard,
        "ctrl+g" | "ctrl+G" | "\u{07}" => KeyAction::OpenBurst,
        "alt+n" | "alt+N" => KeyAction::OpenThumbnail,
        "ctrl+y" | "ctrl+Y" | "\u{19}" => KeyAction::Reopen,
        "ctrl+t" | "ctrl+T" | "\u{14}" => KeyAction::TopmostToggle,
        "ctrl+w" | "ctrl+W" | "\u{17}" => KeyAction::ClosePanel,
        "\u{f704}" => KeyAction::OpenAbout,
        "\u{f705}" => KeyAction::OpenFolder,
        "\u{f706}" => KeyAction::OpenFile,
        "\u{f707}" => KeyAction::ClosePanel,
        "\u{f708}" => KeyAction::OpenSettings,
        "\u{f709}" => KeyAction::ToggleSidebar,
        "\u{f72c}" => KeyAction::PreviousPanel,
        "\u{f72d}" => KeyAction::NextPanel,
        "\u{f700}" => KeyAction::VolumeUp,
        "\u{f701}" => KeyAction::VolumeDown,
        "\u{f702}" => KeyAction::SeekBackward,
        "\u{f703}" => KeyAction::SeekForward,
        "m" | "M" => KeyAction::ToggleMute,
        "c" | "C" => KeyAction::SpeedUp,
        "x" | "X" => KeyAction::SpeedDown,
        "z" | "Z" => KeyAction::SpeedReset,
        "b" | "B" => KeyAction::ToggleAbLoop,
        "h" | "H" => KeyAction::AddBookmark,
        "[" | "{" => KeyAction::PreviousChapter,
        "]" | "}" => KeyAction::NextChapter,
        "alt+insert" => KeyAction::FavoriteAddFile,
        "ctrl+insert" => KeyAction::FavoriteAddFolder,
        "j" | "J" => KeyAction::CycleStereo,
        "w" | "W" => KeyAction::BrightnessDown,
        "e" | "E" => KeyAction::BrightnessUp,
        "y" | "Y" => KeyAction::SaturationDown,
        "u" | "U" => KeyAction::SaturationUp,
        "q" | "Q" => KeyAction::AdjustReset,
        "<" | "shift+," => KeyAction::AudioSyncLag,
        ">" | "shift+." => KeyAction::AudioSyncLead,
        "?" | "shift+/" => KeyAction::AudioSyncReset,
        "t" | "T" => KeyAction::AudioSwap,
        "ctrl+p" | "ctrl+P" | "\u{10}" => KeyAction::FlipVertical,
        "ctrl+z" | "ctrl+Z" | "\u{1a}" => KeyAction::FlipHorizontal,
        "ctrl+alt+b" | "ctrl+alt+B" => KeyAction::CycleMargin,
        "ctrl+alt+f1" | "ctrl+alt+F1" => KeyAction::CaptureSlot1,
        "ctrl+alt+f2" | "ctrl+alt+F2" => KeyAction::CaptureSlot2,
        "ctrl+alt+f3" | "ctrl+alt+F3" => KeyAction::CaptureStart1,
        "ctrl+alt+f4" | "ctrl+alt+F4" => KeyAction::CaptureStart2,
        "alt+k" | "alt+K" => KeyAction::RotateCycle,
        "alt+v" | "alt+V" => KeyAction::NextPanel,
        "ctrl+alt+a" | "ctrl+alt+A" => KeyAction::CyclePostAction,
        "ctrl+f5" | "ctrl+F5" => KeyAction::AspectCycle,
        "ctrl+f6" | "ctrl+F6" => KeyAction::AspectRatioCycle,
        "9" => KeyAction::FrameLarger,
        "1" => KeyAction::FrameSmaller,
        "6" => KeyAction::FrameWider,
        "4" => KeyAction::FrameNarrower,
        "8" => KeyAction::FrameTaller,
        "2" => KeyAction::FrameShorter,
        "5" => KeyAction::FrameCenter,
        "ctrl+6" => KeyAction::FrameShiftRight,
        "ctrl+4" => KeyAction::FrameShiftLeft,
        "ctrl+8" => KeyAction::FrameShiftUp,
        "ctrl+2" => KeyAction::FrameShiftDown,
        "ctrl+5" => KeyAction::FrameCenter,
        _ => KeyAction::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_potplayer_shortcuts() {
        assert_eq!(action_for("\t"), KeyAction::ToggleOsd);
        assert_eq!(action_for("shift+tab"), KeyAction::BriefInfo);
        assert_eq!(action_for("scrolllock"), KeyAction::PlaybackInfo);
        assert_eq!(action_for(" "), KeyAction::ToggleFreeze);
        assert_eq!(action_for("s"), KeyAction::Snapshot);
        assert_eq!(action_for("S"), KeyAction::Snapshot);
        assert_eq!(action_for("r"), KeyAction::ToggleRecording);
        assert_eq!(action_for("R"), KeyAction::ToggleRecording);
        assert_eq!(action_for("G"), KeyAction::ToggleAudioRecording);
        assert_eq!(action_for("shift+g"), KeyAction::ToggleAudioRecording);
        assert_eq!(action_for("shift+G"), KeyAction::ToggleAudioRecording);
        assert_eq!(action_for("shift+x"), KeyAction::ToggleAudioProcessing);
        assert_eq!(action_for("N"), KeyAction::ToggleAudioNormalize);
        assert_eq!(action_for("shift+n"), KeyAction::ToggleAudioNormalize);
        assert_eq!(action_for("shift+c"), KeyAction::ToggleAudioCrystallize);
        assert_eq!(action_for("D"), KeyAction::ToggleAudioDenoise);
        assert_eq!(action_for("shift+r"), KeyAction::ToggleAudioReverb);
        assert_eq!(action_for("V"), KeyAction::ToggleAudioVocalRemove);
        assert_eq!(action_for("A"), KeyAction::ToggleAudioVocalEnhance);
        assert_eq!(action_for("shift+e"), KeyAction::ToggleEqualizer);
        assert_eq!(action_for(","), KeyAction::StepBackward);
        assert_eq!(action_for("."), KeyAction::StepForward);
        assert_eq!(action_for("\n"), KeyAction::ToggleFullscreen);
        assert_eq!(action_for("f"), KeyAction::ToggleFullscreen);
        assert_eq!(action_for("\u{1b}"), KeyAction::ExitFullscreen);
        assert_eq!(action_for("alt+d"), KeyAction::OpenSettings);
        assert_eq!(action_for("alt+D"), KeyAction::OpenSettings);
        assert_eq!(action_for("alt+o"), KeyAction::OpenSubtitle);
        assert_eq!(action_for("alt+e"), KeyAction::OpenSubtitleBrowser);
        assert_eq!(action_for("alt+p"), KeyAction::ManualSubtitle);
        assert_eq!(action_for("alt+h"), KeyAction::ToggleSubtitles);
        assert_eq!(action_for("l"), KeyAction::OpenSubtitle);
        assert_eq!(action_for("L"), KeyAction::OpenSubtitle);
        assert_eq!(action_for("alt+enter"), KeyAction::ToggleFullscreen);
        assert_eq!(action_for("ctrl+o"), KeyAction::OpenFile);
        assert_eq!(action_for("ctrl+u"), KeyAction::OpenUrl);
        assert_eq!(action_for("ctrl+v"), KeyAction::OpenClipboard);
        assert_eq!(action_for("ctrl+g"), KeyAction::OpenBurst);
        assert_eq!(action_for("\u{07}"), KeyAction::OpenBurst);
        assert_eq!(action_for("alt+n"), KeyAction::OpenThumbnail);
        assert_eq!(action_for("ctrl+y"), KeyAction::Reopen);
        assert_eq!(action_for("\u{19}"), KeyAction::Reopen);
        assert_eq!(action_for("ctrl+t"), KeyAction::TopmostToggle);
        assert_eq!(action_for("\u{14}"), KeyAction::TopmostToggle);
        assert_eq!(action_for("ctrl+w"), KeyAction::ClosePanel);
        assert_eq!(action_for("ctrl+W"), KeyAction::ClosePanel);
        assert_eq!(action_for("\u{17}"), KeyAction::ClosePanel);
        assert_eq!(action_for("\u{f708}"), KeyAction::OpenSettings);
        assert_eq!(action_for("\u{f704}"), KeyAction::OpenAbout);
        assert_eq!(action_for("\u{f705}"), KeyAction::OpenFolder);
        assert_eq!(action_for("\u{f706}"), KeyAction::OpenFile);
        assert_eq!(action_for("\u{f707}"), KeyAction::ClosePanel);
        assert_eq!(action_for("\u{f709}"), KeyAction::ToggleSidebar);
        assert_eq!(action_for("\u{f72c}"), KeyAction::PreviousPanel);
        assert_eq!(action_for("\u{f72d}"), KeyAction::NextPanel);
        assert_eq!(action_for("\u{f700}"), KeyAction::VolumeUp);
        assert_eq!(action_for("\u{f701}"), KeyAction::VolumeDown);
        assert_eq!(action_for("\u{f702}"), KeyAction::SeekBackward);
        assert_eq!(action_for("\u{f703}"), KeyAction::SeekForward);
        assert_eq!(action_for("m"), KeyAction::ToggleMute);
        assert_eq!(action_for("M"), KeyAction::ToggleMute);
        assert_eq!(action_for("c"), KeyAction::SpeedUp);
        assert_eq!(action_for("x"), KeyAction::SpeedDown);
        assert_eq!(action_for("z"), KeyAction::SpeedReset);
        assert_eq!(action_for("b"), KeyAction::ToggleAbLoop);
        assert_eq!(action_for("B"), KeyAction::ToggleAbLoop);
        assert_eq!(action_for("h"), KeyAction::AddBookmark);
        assert_eq!(action_for("H"), KeyAction::AddBookmark);
        assert_eq!(action_for("["), KeyAction::PreviousChapter);
        assert_eq!(action_for("]"), KeyAction::NextChapter);
        assert_eq!(action_for("alt+insert"), KeyAction::FavoriteAddFile);
        assert_eq!(action_for("ctrl+insert"), KeyAction::FavoriteAddFolder);
        assert_eq!(action_for("j"), KeyAction::CycleStereo);
        assert_eq!(action_for("J"), KeyAction::CycleStereo);
        assert_eq!(action_for("k"), KeyAction::Snapshot);
        assert_eq!(action_for("K"), KeyAction::Snapshot);
        assert_eq!(action_for("w"), KeyAction::BrightnessDown);
        assert_eq!(action_for("E"), KeyAction::BrightnessUp);
        assert_eq!(action_for("y"), KeyAction::SaturationDown);
        assert_eq!(action_for("U"), KeyAction::SaturationUp);
        assert_eq!(action_for("q"), KeyAction::AdjustReset);
        assert_eq!(action_for("<"), KeyAction::AudioSyncLag);
        assert_eq!(action_for("shift+,"), KeyAction::AudioSyncLag);
        assert_eq!(action_for(">"), KeyAction::AudioSyncLead);
        assert_eq!(action_for("?"), KeyAction::AudioSyncReset);
        assert_eq!(action_for("T"), KeyAction::AudioSwap);
        assert_eq!(action_for("ctrl+p"), KeyAction::FlipVertical);
        assert_eq!(action_for("ctrl+z"), KeyAction::FlipHorizontal);
        assert_eq!(action_for("ctrl+alt+b"), KeyAction::CycleMargin);
        assert_eq!(action_for("alt+k"), KeyAction::RotateCycle);
        assert_eq!(action_for("alt+v"), KeyAction::NextPanel);
        assert_eq!(action_for("ctrl+alt+a"), KeyAction::CyclePostAction);
        assert_eq!(action_for("ctrl+f5"), KeyAction::AspectCycle);
        assert_eq!(action_for("ctrl+f6"), KeyAction::AspectRatioCycle);
        assert_eq!(action_for("9"), KeyAction::FrameLarger);
        assert_eq!(action_for("1"), KeyAction::FrameSmaller);
        assert_eq!(action_for("6"), KeyAction::FrameWider);
        assert_eq!(action_for("4"), KeyAction::FrameNarrower);
        assert_eq!(action_for("8"), KeyAction::FrameTaller);
        assert_eq!(action_for("2"), KeyAction::FrameShorter);
        assert_eq!(action_for("5"), KeyAction::FrameCenter);
        assert_eq!(action_for("ctrl+6"), KeyAction::FrameShiftRight);
        assert_eq!(action_for("ctrl+4"), KeyAction::FrameShiftLeft);
        assert_eq!(action_for("ctrl+8"), KeyAction::FrameShiftUp);
        assert_eq!(action_for("ctrl+2"), KeyAction::FrameShiftDown);
        assert_eq!(action_for("ctrl+5"), KeyAction::FrameCenter);
    }

    #[test]
    fn ignores_other_keys() {
        assert_eq!(action_for("a"), KeyAction::None);
        assert_eq!(action_for(""), KeyAction::None);
        assert_eq!(action_for("F5"), KeyAction::None);
    }
}
