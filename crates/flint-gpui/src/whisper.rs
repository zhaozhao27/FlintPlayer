use std::path::{Path, PathBuf};
use std::process::Command;

const ENGINES: [&str; 4] = ["whisper-cli", "whisper-cpp", "whisper", "main"];

pub fn engine_candidates() -> Vec<PathBuf> {
    let mut directories: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        directories.push(home.join(".local/bin"));
    }
    directories.push(PathBuf::from("/usr/local/bin"));
    let mut found = Vec::new();
    for name in ENGINES {
        for directory in &directories {
            let candidate = directory.join(name);
            if is_executable(&candidate) {
                found.push(candidate);
            }
        }
    }
    found
}

pub fn find_engine() -> Option<PathBuf> {
    engine_candidates().into_iter().next()
}

pub fn models_dir() -> PathBuf {
    flint_core::config::config_dir().join("whisper")
}

pub fn find_model(directory: &Path) -> Option<PathBuf> {
    find_model_with(directory, std::env::var_os("FLINTPLAYER_WHISPER_MODEL").map(PathBuf::from))
}

fn find_model_with(directory: &Path, override_path: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(path) = override_path.filter(|path| path.is_file()) {
        return Some(path);
    }
    let mut models: Vec<PathBuf> = std::fs::read_dir(directory)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|value| value == "bin"))
        .collect();
    models.sort();
    models.into_iter().next()
}

pub fn available() -> bool {
    find_engine().is_some() && find_model(&models_dir()).is_some()
}

pub fn transcribe(media: &Path, output: &Path) -> Result<(), String> {
    let engine = find_engine().ok_or("未找到 whisper 可执行文件（whisper-cli/whisper-cpp）")?;
    let model = find_model(&models_dir())
        .ok_or_else(|| format!("未找到模型：将 *.bin 放入 {} 或用 FLINTPLAYER_WHISPER_MODEL 指定", models_dir().display()))?;
    if !media.is_file() {
        return Err(format!("媒体文件不存在：{}", media.display()));
    }
    let stem = output
        .file_stem()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| "subtitles".to_string());
    let directory = output.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let wav = directory.join(format!("{stem}.whisper.wav"));
    let extracted = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(media)
        .args(["-vn", "-ac", "1", "-ar", "16000", "-f", "wav"])
        .arg(&wav)
        .status()
        .map_err(|error| format!("无法运行 ffmpeg：{error}"))?;
    if !extracted.success() {
        let _ = std::fs::remove_file(&wav);
        return Err("ffmpeg 提取音频失败".to_string());
    }
    let prefix = directory.join(&stem);
    let result = Command::new(&engine)
        .arg("-m")
        .arg(&model)
        .arg("-f")
        .arg(&wav)
        .args(["-l", "auto", "-osrt", "-of"])
        .arg(&prefix)
        .status();
    let _ = std::fs::remove_file(&wav);
    let status = result.map_err(|error| format!("无法运行 whisper：{error}"))?;
    if !status.success() {
        return Err(format!("whisper 退出码 {:?}", status.code()));
    }
    let produced = PathBuf::from(format!("{}.srt", prefix.display()));
    if produced != output && produced.is_file() {
        std::fs::rename(&produced, output).map_err(|error| error.to_string())?;
    }
    if output.is_file() {
        Ok(())
    } else {
        Err("whisper 没有生成字幕文件".to_string())
    }
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        return std::fs::metadata(path)
            .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
            .unwrap_or(false);
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "flintplayer-whisper-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("sandbox");
        path
    }

    #[cfg(unix)]
    #[test]
    fn finds_models_in_the_configured_directory() {
        use std::os::unix::fs::PermissionsExt;
        let directory = sandbox("models");
        assert_eq!(find_model_with(&directory, None), None);
        let b = directory.join("ggml-small.bin");
        let a = directory.join("ggml-base.bin");
        std::fs::write(&b, b"model").expect("model");
        std::fs::write(&a, b"model").expect("model");
        std::fs::write(directory.join("notes.txt"), b"x").expect("note");
        assert_eq!(find_model_with(&directory, None), Some(a.clone()));
        assert_eq!(find_model_with(&directory, Some(b.clone())), Some(b.clone()));
        std::fs::set_permissions(&b, std::fs::Permissions::from_mode(0o644)).expect("chmod");
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[cfg(unix)]
    #[test]
    fn reports_missing_engine() {
        assert!(transcribe(Path::new("/nonexistent/video.mp4"), &sandbox("missing").join("out.srt")).is_err());
    }
}
