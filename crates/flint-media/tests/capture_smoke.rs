use flint_media::{CaptureEngine, CaptureSettings};
use std::thread;
use std::time::{Duration, Instant};

fn settings(width: u32, height: u32) -> CaptureSettings {
    CaptureSettings {
        width,
        height,
        fps: 30,
        fourcc: Some("MJPG".into()),
    }
}

#[test]
fn missing_device_reports_error_without_panicking() {
    let mut engine = CaptureEngine::new();
    let error = engine
        .start("/dev/video_definitely_missing", settings(640, 480))
        .expect_err("missing node must fail");
    assert!(!error.to_string().is_empty());
    assert!(!engine.is_running());
}

#[test]
#[ignore = "requires a physical V4L2 camera"]
fn captures_and_decodes_live_frames() {
    let mut engine = CaptureEngine::new();
    let info = engine
        .start("/dev/video0", settings(640, 480))
        .expect("open camera");
    assert_eq!(info.transport, "V4L2");
    assert!(info.width > 0 && info.height > 0);

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut frame = None;
    while Instant::now() < deadline {
        if let Some(candidate) = engine.take_latest() {
            frame = Some(candidate);
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    let frame = frame.expect("no frame produced within five seconds");
    assert_eq!(frame.rgba.len(), frame.width * frame.height * 4);
    assert!(frame.rgba.chunks_exact(4).any(|pixel| pixel[..3] != [0, 0, 0]));

    let snapshot = engine.shared().stats_snapshot();
    assert!(snapshot.frames > 0);
    assert!(snapshot.bitrate_mbps > 0.0);
    engine.stop();
    assert!(!engine.is_running());
}
