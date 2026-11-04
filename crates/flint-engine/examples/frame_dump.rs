use std::fs::File;
use std::io::{BufWriter, Write};
use std::thread::sleep;
use std::time::{Duration, Instant};

use flint_engine::MpvPlayer;

const MEDIA: &str = "/tmp/flint-test/stereo30.mp4";
const OUTPUT: &str = "/tmp/flint-test/engine-frame.ppm";
const WIDTH: u32 = 640;
const HEIGHT: u32 = 360;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut player = MpvPlayer::new()?;
    player.open(MEDIA)?;
    player.play()?;

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut updates = 0u32;
    let mut saved = false;
    while Instant::now() < deadline {
        if let Some(frame) = player.render_frame(WIDTH, HEIGHT)? {
            updates += 1;
            if !saved {
                write_ppm(OUTPUT, WIDTH, HEIGHT, &frame)?;
                saved = true;
            }
        }
        sleep(Duration::from_millis(16));
    }

    let state = player.state();
    println!("title={:?}", state.title);
    println!(
        "position={:.3}s duration={:.3}s",
        state.position, state.duration
    );
    println!(
        "size={}x{} paused={} eof={} loaded={}",
        state.width, state.height, state.paused, state.eof, state.loaded
    );
    println!("volume={}", state.volume);
    println!("error={:?}", state.error);
    println!("frames_with_updates={updates}");
    println!("frame={}", if saved { OUTPUT } else { "<none>" });

    if updates == 0 {
        return Err("no frames were rendered".into());
    }
    Ok(())
}

fn write_ppm(path: &str, width: u32, height: u32, bgra: &[u8]) -> std::io::Result<()> {
    let mut output = BufWriter::new(File::create(path)?);
    write!(output, "P6\n{width} {height}\n255\n")?;
    for pixel in bgra.chunks_exact(4) {
        output.write_all(&[pixel[2], pixel[1], pixel[0]])?;
    }
    output.flush()
}
