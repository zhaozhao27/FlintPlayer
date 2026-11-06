pub struct ShaderSpec {
    pub key: &'static str,
    pub label: &'static str,
}

pub const SHADERS: [ShaderSpec; 16] = [
    ShaderSpec {
        key: "sharpen-3x3",
        label: "Sharpen 3x3",
    },
    ShaderSpec {
        key: "sharpen-5x5",
        label: "Sharpen 5x5",
    },
    ShaderSpec {
        key: "unsharp-mask",
        label: "Unsharp mask",
    },
    ShaderSpec {
        key: "contour",
        label: "Contour",
    },
    ShaderSpec {
        key: "denoise",
        label: "Denoise",
    },
    ShaderSpec {
        key: "undot",
        label: "Undot",
    },
    ShaderSpec {
        key: "levels",
        label: "Levels",
    },
    ShaderSpec {
        key: "levels-2",
        label: "Levels2",
    },
    ShaderSpec {
        key: "bt601-bt709",
        label: "BT.601 → BT.709",
    },
    ShaderSpec {
        key: "deinterlace-blend",
        label: "反交错(混合)",
    },
    ShaderSpec {
        key: "emboss",
        label: "Emboss",
    },
    ShaderSpec {
        key: "grayscale",
        label: "GrayScale",
    },
    ShaderSpec {
        key: "invert",
        label: "Invert",
    },
    ShaderSpec {
        key: "night-vision",
        label: "NightVision",
    },
    ShaderSpec {
        key: "horz-flip",
        label: "HorzFlip",
    },
    ShaderSpec {
        key: "vert-flip",
        label: "VertFlip",
    },
];

pub const OFF_LABEL: &str = "关闭";

pub fn spec(key: &str) -> Option<&'static ShaderSpec> {
    SHADERS.iter().find(|spec| spec.key == key)
}

pub fn label(key: &str) -> &str {
    spec(key).map_or(OFF_LABEL, |spec| spec.label)
}

pub fn next(key: &str) -> &'static str {
    match SHADERS.iter().position(|spec| spec.key == key) {
        Some(index) => SHADERS.get(index + 1).map_or("", |spec| spec.key),
        None => SHADERS[0].key,
    }
}

pub fn apply(key: &str, width: usize, height: usize, source: &[u8], target: &mut Vec<u8>) -> bool {
    if spec(key).is_none() || width == 0 || height == 0 {
        return false;
    }
    let stride = width * 4;
    if source.len() < stride * height {
        return false;
    }
    target.clear();
    target.resize(stride * height, 0);
    match key {
        "sharpen-3x3" => sharpen(stride, width, height, source, target),
        "sharpen-5x5" => sharpen_5x5(stride, width, height, source, target),
        "unsharp-mask" => unsharp_mask(stride, width, height, source, target),
        "contour" => contour(stride, width, height, source, target),
        "denoise" => denoise(stride, width, height, source, target),
        "undot" => undot(stride, width, height, source, target),
        "levels" => levels(source, target),
        "levels-2" if height < 720 => levels(source, target),
        "levels-2" => {
            let length = target.len();
            target.copy_from_slice(&source[..length]);
        }
        "bt601-bt709" => bt601_to_bt709(source, target),
        "deinterlace-blend" => deinterlace_blend(stride, height, source, target),
        "emboss" => emboss(stride, width, height, source, target),
        "grayscale" => grayscale(width, height, source, target),
        "invert" => invert(source, target),
        "night-vision" => night_vision(source, target),
        "horz-flip" => horz_flip(stride, width, height, source, target),
        "vert-flip" => vert_flip(stride, height, source, target),
        _ => return false,
    }
    true
}

fn sharpen(stride: usize, width: usize, height: usize, source: &[u8], target: &mut [u8]) {
    for y in 0..height {
        let above = &source[y.saturating_sub(1) * stride..][..stride];
        let row = &source[y * stride..][..stride];
        let below = &source[(y + 1).min(height - 1) * stride..][..stride];
        let out = &mut target[y * stride..][..stride];
        for x in 0..width {
            let left = x.saturating_sub(1) * 4;
            let center = x * 4;
            let right = (x + 1).min(width - 1) * 4;
            for channel in 0..3 {
                let neighbors = above[left + channel] as i32
                    + above[center + channel] as i32
                    + above[right + channel] as i32
                    + row[left + channel] as i32
                    + row[right + channel] as i32
                    + below[left + channel] as i32
                    + below[center + channel] as i32
                    + below[right + channel] as i32;
                let value = (16 * row[center + channel] as i32 - neighbors + 4) >> 3;
                out[center + channel] = value.clamp(0, 255) as u8;
            }
            out[center + 3] = row[center + 3];
        }
    }
}

fn sharpen_5x5(stride: usize, width: usize, height: usize, source: &[u8], target: &mut [u8]) {
    const WEIGHTS: [[i32; 5]; 5] = [
        [25, 25, 25, 25, 25],
        [25, 50, 100, 50, 25],
        [25, 100, 0, 100, 25],
        [25, 50, 100, 50, 25],
        [25, 25, 25, 25, 25],
    ];
    for y in 0..height {
        let rows: [&[u8]; 5] = [
            &source[y.saturating_sub(2) * stride..][..stride],
            &source[y.saturating_sub(1) * stride..][..stride],
            &source[y * stride..][..stride],
            &source[(y + 1).min(height - 1) * stride..][..stride],
            &source[(y + 2).min(height - 1) * stride..][..stride],
        ];
        let out = &mut target[y * stride..][..stride];
        for x in 0..width {
            let columns = [
                x.saturating_sub(2) * 4,
                x.saturating_sub(1) * 4,
                x * 4,
                (x + 1).min(width - 1) * 4,
                (x + 2).min(width - 1) * 4,
            ];
            for channel in 0..3 {
                let mut neighbors = 0i32;
                for (row_index, row) in rows.iter().enumerate() {
                    for (column_index, column) in columns.iter().enumerate() {
                        let weight = WEIGHTS[row_index][column_index];
                        neighbors += weight * row[column + channel] as i32;
                    }
                }
                let center = rows[2][columns[2] + channel] as i32;
                let value = (2000 * center - neighbors + 500) / 1000;
                out[columns[2] + channel] = value.clamp(0, 255) as u8;
            }
            out[columns[2] + 3] = rows[2][columns[2] + 3];
        }
    }
}

fn unsharp_mask(stride: usize, width: usize, height: usize, source: &[u8], target: &mut [u8]) {
    const THRESHOLD: f32 = 0.23;
    const INTENSITY: f32 = 0.5;
    for y in 0..height {
        let above = &source[y.saturating_sub(1) * stride..][..stride];
        let row = &source[y * stride..][..stride];
        let below = &source[(y + 1).min(height - 1) * stride..][..stride];
        let out = &mut target[y * stride..][..stride];
        for x in 0..width {
            let left = x.saturating_sub(1) * 4;
            let center = x * 4;
            let right = (x + 1).min(width - 1) * 4;
            let mut diff = 0.0f32;
            let mut blurred = [0.0f32; 3];
            for channel in 0..3 {
                let blur = (4.0 * row[center + channel] as f32
                    + 2.0
                        * (above[center + channel] as f32
                            + below[center + channel] as f32
                            + row[left + channel] as f32
                            + row[right + channel] as f32)
                    + above[left + channel] as f32
                    + above[right + channel] as f32
                    + below[left + channel] as f32
                    + below[right + channel] as f32)
                    / 16.0;
                blurred[channel] = blur;
                diff += (blur - row[center + channel] as f32)
                    * match channel {
                        0 => 0.114,
                        1 => 0.587,
                        _ => 0.299,
                    };
            }
            for channel in 0..3 {
                let value = if diff.abs() < THRESHOLD {
                    row[center + channel] as f32
                } else {
                    row[center + channel] as f32
                        + INTENSITY * (row[center + channel] as f32 - blurred[channel])
                };
                out[center + channel] = value.clamp(0.0, 255.0) as u8;
            }
            out[center + 3] = row[center + 3];
        }
    }
}

fn contour(stride: usize, width: usize, height: usize, source: &[u8], target: &mut [u8]) {
    for y in 0..height {
        let above = &source[y.saturating_sub(1) * stride..][..stride];
        let row = &source[y * stride..][..stride];
        let below = &source[(y + 1).min(height - 1) * stride..][..stride];
        let out = &mut target[y * stride..][..stride];
        for x in 0..width {
            let center = x * 4;
            let left = x.saturating_sub(1) * 4;
            let right = (x + 1).min(width - 1) * 4;
            let mut magnitude = 0.0f32;
            for channel in 0..3 {
                let value = (4 * row[center + channel] as i32
                    - above[center + channel] as i32
                    - below[center + channel] as i32
                    - row[left + channel] as i32
                    - row[right + channel] as i32) as f32;
                magnitude += value * value;
            }
            let value = if magnitude.sqrt() < 1.0 { 0 } else { 255 };
            out[center..center + 3].fill(value);
            out[center + 3] = row[center + 3];
        }
    }
}

fn denoise(stride: usize, width: usize, height: usize, source: &[u8], target: &mut [u8]) {
    const RADII: usize = 4;
    for y in 0..height {
        let center_row = &source[y * stride..][..stride];
        let out = &mut target[y * stride..][..stride];
        for x in 0..width {
            let center = x * 4;
            let mut accum = [0.0f32; 3];
            let mut count = 1.0f32;
            for channel in 0..3 {
                accum[channel] = center_row[center + channel] as f32;
            }
            for radius in 1..=RADII {
                for (dy, dx) in [
                    (-1i32, -1i32),
                    (-1, 0),
                    (-1, 1),
                    (0, -1),
                    (0, 1),
                    (1, -1),
                    (1, 0),
                    (1, 1),
                ] {
                    let ny = (y as i32 + dy * radius as i32).clamp(0, height as i32 - 1) as usize;
                    let nx = (x as i32 + dx * radius as i32).clamp(0, width as i32 - 1) as usize;
                    let sample = &source[ny * stride + nx * 4..][..4];
                    for channel in 0..3 {
                        accum[channel] += sample[channel] as f32;
                    }
                    count += 1.0;
                }
            }
            for channel in 0..3 {
                out[center + channel] = (accum[channel] / count).clamp(0.0, 255.0) as u8;
            }
            out[center + 3] = center_row[center + 3];
        }
    }
}

fn undot(stride: usize, width: usize, height: usize, source: &[u8], target: &mut [u8]) {
    const THRESHOLD: f32 = 0.2 * 255.0;
    for y in 0..height {
        let above = &source[y.saturating_sub(1) * stride..][..stride];
        let row = &source[y * stride..][..stride];
        let below = &source[(y + 1).min(height - 1) * stride..][..stride];
        let out = &mut target[y * stride..][..stride];
        for x in 0..width {
            let left = x.saturating_sub(1) * 4;
            let center = x * 4;
            let right = (x + 1).min(width - 1) * 4;
            let current = [
                row[center] as f32,
                row[center + 1] as f32,
                row[center + 2] as f32,
            ];
            let mut total = current;
            let mut count = 1.0f32;
            for (sample, offset) in [
                (above, left),
                (above, center),
                (above, right),
                (row, left),
                (row, right),
                (below, left),
                (below, center),
                (below, right),
            ] {
                let distance = (0..3)
                    .map(|channel| {
                        let delta = current[channel] - sample[offset + channel] as f32;
                        delta * delta
                    })
                    .sum::<f32>()
                    .sqrt();
                if distance > THRESHOLD {
                    for channel in 0..3 {
                        total[channel] += sample[offset + channel] as f32;
                    }
                    count += 1.0;
                }
            }
            for channel in 0..3 {
                out[center + channel] = (total[channel] / count).clamp(0.0, 255.0) as u8;
            }
            out[center + 3] = row[center + 3];
        }
    }
}

fn levels(source: &[u8], target: &mut [u8]) {
    for (src, dst) in source.chunks_exact(4).zip(target.chunks_exact_mut(4)) {
        for channel in 0..3 {
            let value = (src[channel] as i32 - 16) * 255 + 109;
            dst[channel] = (value / 219).clamp(0, 255) as u8;
        }
        dst[3] = src[3];
    }
}

fn bt601_to_bt709(source: &[u8], target: &mut [u8]) {
    for (src, dst) in source.chunks_exact(4).zip(target.chunks_exact_mut(4)) {
        let b = src[0] as f32 / 255.0;
        let g = src[1] as f32 / 255.0;
        let r = src[2] as f32 / 255.0;
        let y = 0.299 * r + 0.587 * g + 0.114 * b;
        let cb = -0.172 * r - 0.339 * g + 0.511 * b;
        let cr = 0.511 * r - 0.428 * g - 0.083 * b;
        let out_r = y + 1.540 * cr;
        let out_g = y - 0.459 * cr - 0.183 * cb;
        let out_b = y + 1.816 * cb;
        dst[0] = (out_b * 255.0).clamp(0.0, 255.0) as u8;
        dst[1] = (out_g * 255.0).clamp(0.0, 255.0) as u8;
        dst[2] = (out_r * 255.0).clamp(0.0, 255.0) as u8;
        dst[3] = src[3];
    }
}

fn deinterlace_blend(stride: usize, height: usize, source: &[u8], target: &mut [u8]) {
    for y in 0..height {
        let above = &source[y.saturating_sub(1) * stride..][..stride];
        let row = &source[y * stride..][..stride];
        let below = &source[(y + 1).min(height - 1) * stride..][..stride];
        let out = &mut target[y * stride..][..stride];
        for offset in 0..stride {
            let value = 2 * row[offset] as u32 + above[offset] as u32 + below[offset] as u32;
            out[offset] = ((value + 2) >> 2) as u8;
        }
    }
}

fn emboss(stride: usize, width: usize, height: usize, source: &[u8], target: &mut [u8]) {
    for y in 0..height {
        let above = &source[y.saturating_sub(1) * stride..][..stride];
        let row = &source[y * stride..][..stride];
        let below = &source[(y + 1).min(height - 1) * stride..][..stride];
        let out = &mut target[y * stride..][..stride];
        for x in 0..width {
            let left = x.saturating_sub(1) * 4;
            let center = x * 4;
            let right = (x + 1).min(width - 1) * 4;
            let value: f32 = (0..3)
                .map(|channel| {
                    (-(above[left + channel] as i32)
                        - above[center + channel] as i32
                        - row[left + channel] as i32
                        + row[right + channel] as i32
                        + below[center + channel] as i32
                        + below[right + channel] as i32) as f32
                        / 3.0
                })
                .sum::<f32>()
                + 127.5;
            let value = value.clamp(0.0, 255.0) as u8;
            out[center..center + 3].fill(value);
            out[center + 3] = row[center + 3];
        }
    }
}

fn grayscale(width: usize, height: usize, source: &[u8], target: &mut [u8]) {
    let weights = if width < 1120 && height < 630 {
        (0.114f32, 0.587f32, 0.299f32)
    } else {
        (0.0722f32, 0.7152f32, 0.2126f32)
    };
    for (src, dst) in source.chunks_exact(4).zip(target.chunks_exact_mut(4)) {
        let value = (weights.0 * src[0] as f32
            + weights.1 * src[1] as f32
            + weights.2 * src[2] as f32)
            .clamp(0.0, 255.0) as u8;
        dst[0..3].fill(value);
        dst[3] = src[3];
    }
}

fn invert(source: &[u8], target: &mut [u8]) {
    for (src, dst) in source.chunks_exact(4).zip(target.chunks_exact_mut(4)) {
        dst[0] = 255 - src[0];
        dst[1] = 255 - src[1];
        dst[2] = 255 - src[2];
        dst[3] = src[3];
    }
}

fn night_vision(source: &[u8], target: &mut [u8]) {
    for (src, dst) in source.chunks_exact(4).zip(target.chunks_exact_mut(4)) {
        let green = 0.125 * src[0] as f32 + 0.5 * src[1] as f32 + 0.375 * src[2] as f32;
        dst[0] = 0;
        dst[1] = green.clamp(0.0, 255.0) as u8;
        dst[2] = 0;
        dst[3] = src[3];
    }
}

fn horz_flip(stride: usize, width: usize, height: usize, source: &[u8], target: &mut [u8]) {
    for y in 0..height {
        let row = &source[y * stride..][..stride];
        let out = &mut target[y * stride..][..stride];
        for x in 0..width {
            let source_offset = (width - 1 - x) * 4;
            let target_offset = x * 4;
            out[target_offset..target_offset + 4]
                .copy_from_slice(&row[source_offset..source_offset + 4]);
        }
    }
}

fn vert_flip(stride: usize, height: usize, source: &[u8], target: &mut [u8]) {
    for y in 0..height {
        let source_offset = (height - 1 - y) * stride;
        let target_offset = y * stride;
        target[target_offset..target_offset + stride]
            .copy_from_slice(&source[source_offset..source_offset + stride]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(width: usize, height: usize, pixel: [u8; 4]) -> Vec<u8> {
        pixel.repeat(width * height)
    }

    #[test]
    fn cycles_through_every_shader_then_off() {
        let mut key = "";
        let mut seen = Vec::new();
        for _ in 0..SHADERS.len() {
            key = next(key);
            seen.push(key);
        }
        assert_eq!(seen, SHADERS.iter().map(|spec| spec.key).collect::<Vec<_>>());
        assert_eq!(next(key), "");
    }

    #[test]
    fn sharpen_keeps_flat_images_flat() {
        let source = solid(8, 6, [40, 80, 160, 255]);
        for key in ["sharpen-3x3", "sharpen-5x5", "unsharp-mask", "denoise", "undot"] {
            let mut target = Vec::new();
            assert!(apply(key, 8, 6, &source, &mut target));
            assert_eq!(target, source, "{key}");
        }
    }

    #[test]
    fn levels_expands_limited_range() {
        let mut source = solid(1, 1, [16, 128, 235, 255]);
        source.extend_from_slice(&[0, 255, 64, 255]);
        let mut target = Vec::new();
        assert!(apply("levels", 2, 1, &source, &mut target));
        assert_eq!(&target[0..3], &[0, 130, 255]);
        assert_eq!(&target[4..7], &[0, 255, 56]);
        assert_eq!(target[3], 255);
    }

    #[test]
    fn levels_2_only_expands_small_frames() {
        let source = solid(2, 1, [16, 128, 235, 255]);
        let mut target = Vec::new();
        assert!(apply("levels-2", 2, 1, &source, &mut target));
        assert_eq!(&target[0..3], &[0, 130, 255]);

        let tall = solid(1, 720, [16, 128, 235, 255]);
        assert!(apply("levels-2", 1, 720, &tall, &mut target));
        assert_eq!(target, tall);
    }

    #[test]
    fn deinterlace_blend_averages_vertical_neighbors() {
        let mut source = Vec::new();
        source.extend_from_slice(&[0, 0, 0, 255]);
        source.extend_from_slice(&[0, 0, 0, 255]);
        source.extend_from_slice(&[200, 200, 200, 255]);
        let mut target = Vec::new();
        assert!(apply("deinterlace-blend", 1, 3, &source, &mut target));
        assert_eq!(&target[0..3], &[0, 0, 0]);
        assert_eq!(&target[4..7], &[50, 50, 50]);
        assert_eq!(&target[8..11], &[150, 150, 150]);
    }

    #[test]
    fn bt601_to_bt709_preserves_gray() {
        let source = solid(1, 1, [90, 90, 90, 255]);
        let mut target = Vec::new();
        assert!(apply("bt601-bt709", 1, 1, &source, &mut target));
        for channel in &target[0..3] {
            assert!((*channel as i32 - 90).abs() <= 1);
        }
    }

    #[test]
    fn contour_marks_flat_areas_black() {
        let source = solid(4, 4, [90, 90, 90, 255]);
        let mut target = Vec::new();
        assert!(apply("contour", 4, 4, &source, &mut target));
        assert_eq!(&target[0..3], &[0, 0, 0]);
        assert_eq!(target[3], 255);
    }

    #[test]
    fn emboss_flattens_to_mid_gray() {
        let source = solid(4, 4, [90, 90, 90, 255]);
        let mut target = Vec::new();
        assert!(apply("emboss", 4, 4, &source, &mut target));
        assert_eq!(&target[0..3], &[127, 127, 127]);
    }

    #[test]
    fn invert_complements_channels() {
        let source = solid(1, 1, [0, 60, 255, 255]);
        let mut target = Vec::new();
        assert!(apply("invert", 1, 1, &source, &mut target));
        assert_eq!(target, vec![255, 195, 0, 255]);
    }

    #[test]
    fn grayscale_uses_sd_weights_for_small_frames() {
        let source = solid(2, 2, [30, 60, 90, 255]);
        let mut target = Vec::new();
        assert!(apply("grayscale", 2, 2, &source, &mut target));
        assert_eq!(&target[0..3], &[65, 65, 65]);
    }

    #[test]
    fn night_vision_keeps_only_green() {
        let source = solid(1, 1, [0, 200, 0, 255]);
        let mut target = Vec::new();
        assert!(apply("night-vision", 1, 1, &source, &mut target));
        assert_eq!(target, vec![0, 100, 0, 255]);
    }

    #[test]
    fn flips_mirror_pixels() {
        let mut source = Vec::new();
        source.extend_from_slice(&[1, 2, 3, 255]);
        source.extend_from_slice(&[4, 5, 6, 255]);
        let mut target = Vec::new();
        assert!(apply("horz-flip", 2, 1, &source, &mut target));
        assert_eq!(target, vec![4, 5, 6, 255, 1, 2, 3, 255]);

        assert!(apply("vert-flip", 2, 1, &source, &mut target));
        assert_eq!(target, source);
    }

    #[test]
    fn unknown_shader_is_rejected() {
        let source = solid(1, 1, [0, 0, 0, 255]);
        let mut target = Vec::new();
        assert!(!apply("", 1, 1, &source, &mut target));
        assert!(!apply("missing", 1, 1, &source, &mut target));
    }
}
