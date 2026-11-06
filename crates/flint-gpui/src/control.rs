use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use serde_json::{json, Value};

const MAX_LINE: u64 = 1 << 20;
const REPLY_TIMEOUT: Duration = Duration::from_secs(5);
const CLIENT_IDLE: Duration = Duration::from_secs(300);
const MAX_SOCKET_PATH: usize = 107;

#[derive(Debug, PartialEq)]
pub enum ControlRequest {
    Ping,
    Version,
    Open(String),
    Add(String),
    Seek {
        seconds: Option<f64>,
        delta: Option<f64>,
    },
    Volume {
        value: Option<f64>,
        delta: Option<f64>,
    },
    Command(String),
    Get(String),
    Set {
        key: String,
        value: Value,
    },
    Resize {
        width: u32,
        height: u32,
    },
    Show,
    Hide,
}

pub struct ControlJob {
    pub request: ControlRequest,
    pub reply: Sender<Result<Value, String>>,
}

pub struct ControlLink {
    pub jobs: Receiver<ControlJob>,
}

enum Prepare {
    Ready,
    InUse,
    Failed(std::io::Error),
}

pub fn start() -> Option<ControlLink> {
    let uid = unsafe { libc::getuid() };
    start_at(socket_path(uid))
}

pub fn socket_path(uid: u32) -> PathBuf {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|path| path.is_dir());
    socket_path_from(runtime.as_deref(), Path::new("/tmp"), uid)
}

pub fn socket_path_from(runtime: Option<&Path>, tmp: &Path, uid: u32) -> PathBuf {
    let name = format!("flintplayer-{uid}.sock");
    match runtime.map(|dir| dir.join(&name)) {
        Some(path) if path.as_os_str().as_bytes().len() <= MAX_SOCKET_PATH => path,
        _ => tmp.join(name),
    }
}

fn start_at(path: PathBuf) -> Option<ControlLink> {
    match prepare(&path) {
        Prepare::Ready => {}
        Prepare::InUse => {
            log::info!("控制套接字已被其他实例占用：{}", path.display());
            return None;
        }
        Prepare::Failed(error) => {
            log::warn!("无法初始化控制套接字 {}：{error}", path.display());
            return None;
        }
    }
    let (sender, jobs) = mpsc::channel();
    let thread_path = path.clone();
    let spawned = std::thread::Builder::new()
        .name("flint-control".to_string())
        .spawn(move || serve(thread_path, sender));
    match spawned {
        Ok(_) => Some(ControlLink { jobs }),
        Err(error) => {
            log::warn!("无法启动控制服务：{error}");
            None
        }
    }
}

fn prepare(path: &Path) -> Prepare {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Prepare::Ready,
        Err(error) => Prepare::Failed(error),
        Ok(meta) => {
            if !meta.file_type().is_socket() {
                return Prepare::Failed(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    "已存在同名非套接字文件",
                ));
            }
            if UnixStream::connect(path).is_ok() {
                return Prepare::InUse;
            }
            match std::fs::remove_file(path) {
                Ok(()) => Prepare::Ready,
                Err(error) => Prepare::Failed(error),
            }
        }
    }
}

fn serve(path: PathBuf, jobs: Sender<ControlJob>) {
    let listener = match UnixListener::bind(&path) {
        Ok(listener) => listener,
        Err(error) => {
            log::warn!("绑定控制套接字 {} 失败：{error}", path.display());
            return;
        }
    };
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    log::info!("控制套接字：{}", path.display());
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let jobs = jobs.clone();
        std::thread::spawn(move || handle_client(stream, jobs));
    }
}

fn handle_client(mut stream: UnixStream, jobs: Sender<ControlJob>) {
    let self_uid = unsafe { libc::getuid() };
    if !peer_allowed(peer_uid(&stream), self_uid) {
        let _ = write_response(&mut stream, Value::Null, Err("仅允许同一用户连接".to_string()));
        return;
    }
    let _ = stream.set_read_timeout(Some(CLIENT_IDLE));
    let Ok(cloned) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(cloned);
    let mut line = String::new();
    loop {
        line.clear();
        match (&mut reader).take(MAX_LINE).read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(_) => break,
        }
        if !line.ends_with('\n') && line.len() as u64 >= MAX_LINE {
            let _ = write_response(&mut stream, Value::Null, Err("请求过长".to_string()));
            break;
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            continue;
        }
        let (id, request) = match parse_request(trimmed) {
            Ok(parsed) => parsed,
            Err(error) => {
                if write_response(&mut stream, Value::Null, Err(error)).is_err() {
                    break;
                }
                continue;
            }
        };
        let (reply, answer) = mpsc::channel();
        if jobs.send(ControlJob { request, reply }).is_err() {
            let _ = write_response(&mut stream, id, Err("播放器已关闭".to_string()));
            break;
        }
        let result = answer
            .recv_timeout(REPLY_TIMEOUT)
            .unwrap_or_else(|_| Err("控制请求超时".to_string()));
        if write_response(&mut stream, id, result).is_err() {
            break;
        }
    }
}

fn write_response(stream: &mut UnixStream, id: Value, result: Result<Value, String>) -> std::io::Result<()> {
    let response = match result {
        Ok(result) => json!({ "id": id, "ok": true, "result": result }),
        Err(error) => json!({ "id": id, "ok": false, "error": error }),
    };
    stream.write_all(response.to_string().as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()
}

fn peer_uid(stream: &UnixStream) -> Option<u32> {
    let mut credentials = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut credentials as *mut libc::ucred).cast(),
            &mut length,
        )
    };
    (result == 0).then_some(credentials.uid)
}

fn peer_allowed(peer: Option<u32>, self_uid: u32) -> bool {
    peer == Some(self_uid)
}

pub fn parse_request(line: &str) -> Result<(Value, ControlRequest), String> {
    let value: Value =
        serde_json::from_str(line).map_err(|error| format!("JSON 解析失败：{error}"))?;
    let object = value.as_object().ok_or("请求必须是 JSON 对象")?;
    let id = object.get("id").cloned().unwrap_or(Value::Null);
    let method = object
        .get("method")
        .and_then(Value::as_str)
        .ok_or("缺少 method 字段")?;
    let params = object.get("params").unwrap_or(&Value::Null);
    let request = match method {
        "ping" => ControlRequest::Ping,
        "version" => ControlRequest::Version,
        "open" => ControlRequest::Open(text_param(params, "path")?),
        "add" => ControlRequest::Add(text_param(params, "path")?),
        "seek" => {
            let seconds = optional_number(params, "seconds")?;
            let delta = optional_number(params, "delta")?;
            if seconds.is_some() == delta.is_some() {
                return Err("seek 需要 seconds 或 delta（二选一）".to_string());
            }
            ControlRequest::Seek { seconds, delta }
        }
        "volume" => {
            let value = optional_number(params, "value")?;
            let delta = optional_number(params, "delta")?;
            if value.is_some() == delta.is_some() {
                return Err("volume 需要 value 或 delta（二选一）".to_string());
            }
            ControlRequest::Volume { value, delta }
        }
        "command" => ControlRequest::Command(text_param(params, "id")?),
        "get" => ControlRequest::Get(text_param(params, "key")?),
        "set" => ControlRequest::Set {
            key: text_param(params, "key")?,
            value: params
                .get("value")
                .cloned()
                .ok_or("缺少 value 参数".to_string())?,
        },
        "resize" => ControlRequest::Resize {
            width: uint_param(params, "width")?,
            height: uint_param(params, "height")?,
        },
        "show" => ControlRequest::Show,
        "hide" => ControlRequest::Hide,
        other => {
            return Err(format!(
                "未知方法：{other}（可用：ping、version、open、add、seek、volume、command、get、set、resize、show、hide）"
            ));
        }
    };
    Ok((id, request))
}

fn text_param(params: &Value, key: &str) -> Result<String, String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("缺少 {key} 参数"))
}

fn optional_number(params: &Value, key: &str) -> Result<Option<f64>, String> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_f64()
            .map(Some)
            .ok_or_else(|| format!("{key} 必须是数字")),
    }
}

fn uint_param(params: &Value, key: &str) -> Result<u32, String> {
    params
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| format!("缺少 {key} 参数（正整数）"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_supported_methods() {
        let (id, request) = parse_request(r#"{"id":3,"method":"open","params":{"path":"/tmp/a.mkv"}}"#)
            .expect("open");
        assert_eq!(id, json!(3));
        assert_eq!(request, ControlRequest::Open("/tmp/a.mkv".to_string()));

        let (_, request) = parse_request(r#"{"method":"seek","params":{"delta":-5}}"#).expect("seek");
        assert_eq!(request, ControlRequest::Seek { seconds: None, delta: Some(-5.0) });

        let (_, request) = parse_request(r#"{"method":"set","params":{"key":"volume","value":42}}"#)
            .expect("set");
        assert_eq!(
            request,
            ControlRequest::Set {
                key: "volume".to_string(),
                value: json!(42)
            }
        );

        let (_, request) = parse_request(r#"{"method":"resize","params":{"width":800,"height":600}}"#)
            .expect("resize");
        assert_eq!(request, ControlRequest::Resize { width: 800, height: 600 });

        assert_eq!(parse_request(r#"{"method":"ping"}"#).expect("ping").1, ControlRequest::Ping);
        assert_eq!(
            parse_request(r#"{"method":"hide"}"#).expect("hide").1,
            ControlRequest::Hide
        );
    }

    #[test]
    fn rejects_bad_requests() {
        assert!(parse_request(r#"{"method":"nope"}"#).unwrap_err().contains("未知方法"));
        assert!(parse_request(r#"{"id":1}"#).unwrap_err().contains("method"));
        assert!(parse_request("not json").unwrap_err().contains("JSON"));
        assert!(parse_request(r#"{"method":"open"}"#).unwrap_err().contains("path"));
        assert!(parse_request(r#"{"method":"open","params":{"path":3}}"#)
            .unwrap_err()
            .contains("path"));
        assert!(
            parse_request(r#"{"method":"seek","params":{"seconds":1,"delta":2}}"#)
                .unwrap_err()
                .contains("二选一")
        );
        assert!(
            parse_request(r#"{"method":"volume","params":{}}"#)
                .unwrap_err()
                .contains("二选一")
        );
        assert!(parse_request(r#"{"method":"resize","params":{"width":800}}"#)
            .unwrap_err()
            .contains("height"));
    }

    #[test]
    fn builds_socket_paths() {
        let runtime = Path::new("/run/user/1000");
        assert_eq!(
            socket_path_from(Some(runtime), Path::new("/tmp"), 1000),
            PathBuf::from("/run/user/1000/flintplayer-1000.sock")
        );
        assert_eq!(
            socket_path_from(None, Path::new("/tmp"), 1000),
            PathBuf::from("/tmp/flintplayer-1000.sock")
        );
        let long = PathBuf::from("x".repeat(120));
        assert_eq!(
            socket_path_from(Some(&long), Path::new("/tmp"), 7),
            PathBuf::from("/tmp/flintplayer-7.sock")
        );
    }

    #[test]
    fn checks_peer_credentials() {
        assert!(peer_allowed(Some(1000), 1000));
        assert!(!peer_allowed(Some(0), 1000));
        assert!(!peer_allowed(None, 1000));
    }

    #[test]
    fn serves_requests_over_socket() {
        let path = std::env::temp_dir().join(format!(
            "flintplayer-control-test-{}-{}.sock",
            std::process::id(),
            line!()
        ));
        let _ = std::fs::remove_file(&path);
        let stale = UnixListener::bind(&path).expect("stale socket");
        drop(stale);
        assert!(matches!(prepare(&path), Prepare::Ready));

        let link = start_at(path.clone()).expect("control server");
        let mut client = connect_retry(&path);
        assert!(matches!(prepare(&path), Prepare::InUse));

        let consumer = std::thread::spawn(move || {
            let job = link.jobs.recv().expect("control job");
            assert_eq!(job.request, ControlRequest::Ping);
            job.reply.send(Ok(json!({ "pong": true }))).expect("reply");
        });

        client
            .write_all(b"{\"id\":7,\"method\":\"ping\",\"params\":{}}\n")
            .expect("write");
        let mut reader = BufReader::new(client);
        let mut line = String::new();
        reader.read_line(&mut line).expect("read");
        let response: Value = serde_json::from_str(line.trim()).expect("json");
        assert_eq!(response["id"], json!(7));
        assert_eq!(response["ok"], json!(true));
        assert_eq!(response["result"]["pong"], json!(true));

        consumer.join().expect("consumer");
        let _ = std::fs::remove_file(&path);
    }

    fn connect_retry(path: &Path) -> UnixStream {
        for _ in 0..200 {
            if let Ok(stream) = UnixStream::connect(path) {
                return stream;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("无法连接控制套接字 {}", path.display());
    }
}
