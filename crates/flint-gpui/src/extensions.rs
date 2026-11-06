use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const MANIFEST: &str = "extension.json";
const CALL_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_RESPONSE: u64 = 4 << 20;

#[derive(Clone, Debug, PartialEq)]
pub struct Extension {
    pub name: String,
    pub description: String,
    pub directory: PathBuf,
    pub command: PathBuf,
    pub hooks: Vec<String>,
}

impl Extension {
    fn supports(&self, hook: &str) -> bool {
        self.hooks.iter().any(|entry| entry == hook)
    }
}

pub fn extensions_dir() -> PathBuf {
    flint_core::config::config_dir().join("extensions")
}

pub fn discover(directory: &Path) -> Vec<Extension> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut extensions = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        match load(&path) {
            Ok(extension) => extensions.push(extension),
            Err(error) => log::warn!("跳过扩展 {}：{error}", path.display()),
        }
    }
    extensions.sort_by(|left, right| left.name.cmp(&right.name));
    extensions
}

pub fn load(directory: &Path) -> Result<Extension, String> {
    let manifest_path = directory.join(MANIFEST);
    let text = std::fs::read_to_string(&manifest_path)
        .map_err(|error| format!("无法读取 {}：{error}", manifest_path.display()))?;
    let manifest: Value =
        serde_json::from_str(&text).map_err(|error| format!("清单不是合法 JSON：{error}"))?;
    let name = manifest
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .ok_or("缺少 name")?
        .to_string();
    let description = manifest
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let command = manifest
        .get("command")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|command| !command.is_empty())
        .ok_or("缺少 command")?;
    let hooks = manifest
        .get("hooks")
        .and_then(Value::as_array)
        .ok_or("缺少 hooks 数组")?
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect::<Vec<_>>();
    if hooks.is_empty() {
        return Err("hooks 不能为空".to_string());
    }
    let command = resolve_command(directory, command)?;
    Ok(Extension {
        name,
        description,
        directory: directory.to_path_buf(),
        command,
        hooks,
    })
}

fn resolve_command(directory: &Path, command: &str) -> Result<PathBuf, String> {
    let relative = Path::new(command);
    if relative.is_absolute() || relative.components().any(|part| part.as_os_str() == "..") {
        return Err("command 必须是扩展目录内的相对路径".to_string());
    }
    let path = directory.join(relative);
    let Ok(canonical) = path.canonicalize() else {
        return Err(format!("找不到可执行文件 {}", path.display()));
    };
    let root = directory
        .canonicalize()
        .map_err(|error| format!("无法解析扩展目录：{error}"))?;
    if !canonical.starts_with(&root) {
        return Err("command 越出扩展目录".to_string());
    }
    let meta = std::fs::metadata(&canonical).map_err(|error| error.to_string())?;
    if !meta.is_file() {
        return Err("command 不是文件".to_string());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o111 == 0 {
            return Err("command 没有执行权限".to_string());
        }
    }
    Ok(canonical)
}

pub fn run_hook(hook: &str, request: Value, extensions: &[Extension]) -> Result<Value, String> {
    let mut last_error = format!("没有扩展处理 {hook}");
    for extension in extensions.iter().filter(|item| item.supports(hook)) {
        let mut payload = request.clone();
        if let Some(object) = payload.as_object_mut() {
            object.insert("hook".to_string(), json!(hook));
        }
        match call(extension, &payload) {
            Ok(response) => return Ok(response),
            Err(error) => {
                log::warn!("扩展 {} 执行失败：{error}", extension.name);
                last_error = format!("{}：{error}", extension.name);
            }
        }
    }
    Err(last_error)
}

fn call(extension: &Extension, request: &Value) -> Result<Value, String> {
    let mut child = Command::new(&extension.command)
        .current_dir(&extension.directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| error.to_string())?;
    if let Some(mut stdin) = child.stdin.take() {
        let line = request.to_string();
        stdin
            .write_all(line.as_bytes())
            .and_then(|()| stdin.write_all(b"\n"))
            .map_err(|error| error.to_string())?;
    }
    let reader = child.stdout.take().map(|stdout| {
        std::thread::spawn(move || {
            let mut text = String::new();
            let _ = stdout.take(MAX_RESPONSE).read_to_string(&mut text);
            text
        })
    });
    let deadline = Instant::now() + CALL_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => return Err(error.to_string()),
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("执行超时".to_string());
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let output = reader
        .map(|handle| handle.join().unwrap_or_default())
        .unwrap_or_default();
    if !status.success() {
        return Err(format!("退出码 {:?}", status.code()));
    }
    let line = output
        .lines()
        .find(|line| !line.trim().is_empty())
        .ok_or("没有输出")?;
    serde_json::from_str(line).map_err(|error| format!("输出不是 JSON：{error}"))
}

pub fn parse_playlist(response: &Value) -> Result<Vec<(String, String)>, String> {
    let items = response
        .get("playlist")
        .and_then(Value::as_array)
        .ok_or("响应缺少 playlist 数组")?;
    let mut result = Vec::new();
    for item in items {
        let url = item
            .get("url")
            .and_then(Value::as_str)
            .filter(|url| !url.is_empty())
            .ok_or("playlist 项缺少 url")?;
        let title = item.get("title").and_then(Value::as_str).unwrap_or_default();
        result.push((title.to_string(), url.to_string()));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "flintplayer-ext-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("sandbox");
        path
    }

    fn write_manifest(directory: &Path, body: &str) {
        std::fs::write(directory.join(MANIFEST), body).expect("manifest");
    }

    #[cfg(unix)]
    fn write_script(directory: &Path, name: &str, body: &str) {
        use std::os::unix::fs::PermissionsExt;
        let path = directory.join(name);
        std::fs::write(&path, body).expect("script");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }

    #[test]
    fn rejects_paths_outside_the_extension() {
        let directory = sandbox("escape");
        write_manifest(
            &directory,
            r#"{"name":"x","command":"../evil","hooks":["playlist-parse"]}"#,
        );
        assert!(load(&directory).unwrap_err().contains("相对路径"));

        write_manifest(
            &directory,
            r#"{"name":"x","command":"/bin/sh","hooks":["playlist-parse"]}"#,
        );
        assert!(load(&directory).unwrap_err().contains("相对路径"));
    }

    #[test]
    fn rejects_missing_fields() {
        let directory = sandbox("fields");
        write_manifest(&directory, r#"{"name":"x","hooks":[]}"#);
        assert!(load(&directory).unwrap_err().contains("command"));
        write_manifest(&directory, r#"{"name":"x","command":"run.sh"}"#);
        assert!(load(&directory).unwrap_err().contains("hooks"));
    }

    #[cfg(unix)]
    #[test]
    fn runs_a_hook_and_parses_playlist() {
        let directory = sandbox("run");
        write_script(
            &directory,
            "run.sh",
            "#!/bin/sh\nread line\nprintf '%s\\n' '{\"playlist\":[{\"title\":\"T\",\"url\":\"https://example.invalid/a.m3u8\"}]}'\n",
        );
        write_manifest(
            &directory,
            r#"{"name":"demo","command":"run.sh","hooks":["playlist-parse"]}"#,
        );
        let extension = load(&directory).expect("load");
        let response = run_hook(
            "playlist-parse",
            json!({ "url": "https://example.invalid/page" }),
            std::slice::from_ref(&extension),
        )
        .expect("hook");
        let items = parse_playlist(&response).expect("playlist");
        assert_eq!(items, vec![("T".to_string(), "https://example.invalid/a.m3u8".to_string())]);
        assert!(run_hook("subtitle-search", json!({}), &[extension]).is_err());
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[cfg(unix)]
    #[test]
    fn times_out_hung_extensions() {
        let directory = sandbox("hang");
        write_script(&directory, "run.sh", "#!/bin/sh\nsleep 60\n");
        write_manifest(
            &directory,
            r#"{"name":"slow","command":"run.sh","hooks":["playlist-parse"]}"#,
        );
        let extension = load(&directory).expect("load");
        let started = Instant::now();
        let result = run_hook("playlist-parse", json!({}), &[extension]);
        assert!(result.is_err());
        assert!(started.elapsed() < CALL_TIMEOUT + Duration::from_secs(5));
        let _ = std::fs::remove_dir_all(&directory);
    }
}
