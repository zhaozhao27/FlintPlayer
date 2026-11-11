use ffmpeg_next as ffmpeg;
use std::path::Path;

pub fn frame_count(path: &Path) -> Result<i64, String> {
    let mut context =
        ffmpeg::format::input(&path).map_err(|error| format!("打开文件失败：{error}"))?;
    let index = context
        .streams()
        .find(|stream| stream.parameters().medium() == ffmpeg::media::Type::Video)
        .map(|stream| stream.index())
        .ok_or_else(|| "文件中没有视频轨".to_string())?;
    let mut count = 0i64;
    for (stream, _) in context.packets() {
        if stream.index() == index {
            count += 1;
        }
    }
    Ok(count)
}
