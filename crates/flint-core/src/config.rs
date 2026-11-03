use std::fs;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Config {
    pub source: String,
    pub sources: Vec<String>,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub osd_visible: bool,
    pub always_on_top: bool,
    pub remember_window: bool,
    pub snapshot_dir: PathBuf,
    pub window_w: f32,
    pub window_h: f32,
    pub window_x: Option<f32>,
    pub window_y: Option<f32>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            source: String::new(),
            sources: Vec::new(),
            width: 1280,
            height: 720,
            fps: 30,
            osd_visible: false,
            always_on_top: false,
            remember_window: true,
            snapshot_dir: dirs::picture_dir()
                .or_else(dirs::home_dir)
                .unwrap_or_else(|| PathBuf::from("."))
                .join("FlintPlayer"),
            window_w: 1280.0,
            window_h: 800.0,
            window_x: None,
            window_y: None,
        }
    }
}

impl Config {
    pub fn load() -> Self {
        match fs::read_to_string(config_path()) {
            Ok(text) => Self::parse(&text),
            Err(_) => Self::default(),
        }
    }

    pub fn parse(text: &str) -> Self {
        let mut cfg = Self::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim().trim_matches('"');
            match key.trim() {
                "source" => cfg.source = value.to_string(),
                "sources" => cfg.sources = decode_sources(value),
                "width" => cfg.width = value.parse().unwrap_or(cfg.width),
                "height" => cfg.height = value.parse().unwrap_or(cfg.height),
                "fps" => cfg.fps = value.parse().unwrap_or(cfg.fps),
                "osd_visible" => cfg.osd_visible = value == "true" || value == "1",
                "always_on_top" => cfg.always_on_top = value == "true" || value == "1",
                "remember_window" => cfg.remember_window = value == "true" || value == "1",
                "snapshot_dir" => cfg.snapshot_dir = PathBuf::from(value),
                "window_w" => cfg.window_w = value.parse().unwrap_or(cfg.window_w).max(720.0),
                "window_h" => cfg.window_h = value.parse().unwrap_or(cfg.window_h).max(480.0),
                "window_x" => cfg.window_x = value.parse().ok(),
                "window_y" => cfg.window_y = value.parse().ok(),
                _ => {}
            }
        }
        cfg
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let sanitize = |value: &str| value.replace(['\n', '\r'], " ");
        let text = format!(
            "# FlintPlayer\n\
             source={}\n\
             sources={}\n\
             width={}\n\
             height={}\n\
             fps={}\n\
             osd_visible={}\n\
             always_on_top={}\n\
             remember_window={}\n\
             snapshot_dir={}\n\
             window_w={}\n\
             window_h={}\n\
             window_x={}\n\
             window_y={}\n",
            sanitize(&self.source),
            sanitize(&encode_sources(&self.sources)),
            self.width,
            self.height,
            self.fps,
            self.osd_visible,
            self.always_on_top,
            self.remember_window,
            sanitize(&self.snapshot_dir.display().to_string()),
            self.window_w,
            self.window_h,
            self.window_x.map(|v| v.to_string()).unwrap_or_default(),
            self.window_y.map(|v| v.to_string()).unwrap_or_default(),
        );
        fs::write(path, text)
    }
}

fn encode_sources(sources: &[String]) -> String {
    sources
        .iter()
        .map(|source| source.replace('\\', "\\\\").replace('|', "\\|"))
        .collect::<Vec<_>>()
        .join("|")
}

fn decode_sources(value: &str) -> Vec<String> {
    let mut entries = Vec::new();
    let mut current = String::new();
    let mut escaped = false;
    for character in value.chars() {
        if escaped {
            current.push(character);
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == '|' {
            entries.push(std::mem::take(&mut current));
        } else {
            current.push(character);
        }
    }
    if escaped {
        current.push('\\');
    }
    entries.push(current);
    entries
        .into_iter()
        .map(|entry| entry.trim().to_string())
        .filter(|entry| !entry.is_empty())
        .collect()
}

pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("flintplayer")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.ini")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_capture_values() {
        let text = "source=/dev/video0\nsources=/dev/video0|/dev/video2\nwidth=1920\nheight=1080\nfps=60\n\
                    osd_visible=true\nremember_window=false\nsnapshot_dir=/tmp/shots\n";
        let parsed = Config::parse(text);
        assert_eq!(parsed.source, "/dev/video0");
        assert_eq!(parsed.sources, vec!["/dev/video0", "/dev/video2"]);
        assert_eq!((parsed.width, parsed.height, parsed.fps), (1920, 1080, 60));
        assert!(parsed.osd_visible);
        assert!(!parsed.remember_window);
        assert_eq!(parsed.snapshot_dir, PathBuf::from("/tmp/shots"));
    }

    #[test]
    fn keeps_source_url_intact() {
        let parsed = Config::parse("source=rtsp://cam.local/live?channel=1&x=2\n");
        assert_eq!(parsed.source, "rtsp://cam.local/live?channel=1&x=2");
    }

    #[test]
    fn round_trips_sources_with_separator_characters() {
        let text = "sources=rtsp://cam/live\\|main|/dev/video2|C:\\\\share\n";
        let parsed = Config::parse(text);
        assert_eq!(
            parsed.sources,
            vec!["rtsp://cam/live|main", "/dev/video2", "C:\\share"]
        );
        assert_eq!(
            encode_sources(&parsed.sources),
            "rtsp://cam/live\\|main|/dev/video2|C:\\\\share"
        );
    }

    #[test]
    fn clamps_window_size() {
        let parsed = Config::parse("window_w=100\nwindow_h=100\n");
        assert!(parsed.window_w >= 720.0);
        assert!(parsed.window_h >= 480.0);
    }
}
