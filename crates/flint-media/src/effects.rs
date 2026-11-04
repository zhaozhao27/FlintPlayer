#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Adjust {
    pub brightness: i16,
    pub contrast: i16,
    pub saturation: i16,
}

impl Adjust {
    pub fn is_neutral(self) -> bool {
        self == Self::default()
    }

    pub fn clamped(self) -> Self {
        Self {
            brightness: self.brightness.clamp(-100, 100),
            contrast: self.contrast.clamp(-100, 100),
            saturation: self.saturation.clamp(-100, 100),
        }
    }
}

pub fn apply_rgba(width: usize, height: usize, rgba: &mut [u8], adjust: Adjust) -> bool {
    let adjust = adjust.clamped();
    if adjust.is_neutral() || width == 0 || height == 0 || rgba.len() < width * height * 4 {
        return false;
    }
    let brightness = f32::from(adjust.brightness) * 2.55;
    let contrast = (100.0 + f32::from(adjust.contrast)) / 100.0;
    let saturation = (100.0 + f32::from(adjust.saturation)) / 100.0;
    for pixel in rgba.chunks_exact_mut(4).take(width * height) {
        let mut r = pixel[0] as f32 + brightness;
        let mut g = pixel[1] as f32 + brightness;
        let mut b = pixel[2] as f32 + brightness;
        r = (r - 128.0) * contrast + 128.0;
        g = (g - 128.0) * contrast + 128.0;
        b = (b - 128.0) * contrast + 128.0;
        let luma = 0.299 * r + 0.587 * g + 0.114 * b;
        pixel[0] = (luma + (r - luma) * saturation).clamp(0.0, 255.0) as u8;
        pixel[1] = (luma + (g - luma) * saturation).clamp(0.0, 255.0) as u8;
        pixel[2] = (luma + (b - luma) * saturation).clamp(0.0, 255.0) as u8;
    }
    true
}

pub fn rotation_swaps_axes(degrees: u16) -> bool {
    matches!(degrees % 360, 90 | 270)
}

pub fn rotate_rgba(
    width: usize,
    height: usize,
    source: &[u8],
    degrees: u16,
    out: &mut Vec<u8>,
) -> Option<(usize, usize)> {
    if width == 0 || height == 0 || source.len() < width * height * 4 {
        return None;
    }
    let degrees = degrees % 360;
    let (out_width, out_height) = match degrees {
        0 => (width, height),
        90 | 270 => (height, width),
        180 => (width, height),
        _ => return None,
    };
    out.clear();
    out.resize(out_width * out_height * 4, 0);
    for y in 0..height {
        for x in 0..width {
            let (dest_x, dest_y) = match degrees {
                0 => (x, y),
                90 => (height - 1 - y, x),
                180 => (width - 1 - x, height - 1 - y),
                _ => (y, width - 1 - x),
            };
            let source_index = (y * width + x) * 4;
            let dest_index = (dest_y * out_width + dest_x) * 4;
            out[dest_index..dest_index + 4]
                .copy_from_slice(&source[source_index..source_index + 4]);
        }
    }
    Some((out_width, out_height))
}

pub fn flip_rgba(
    width: usize,
    height: usize,
    source: &[u8],
    horizontal: bool,
    vertical: bool,
    out: &mut Vec<u8>,
) -> bool {
    if width == 0 || height == 0 || source.len() < width * height * 4 || (!horizontal && !vertical)
    {
        return false;
    }
    out.clear();
    out.resize(width * height * 4, 0);
    for y in 0..height {
        let source_y = if vertical { height - 1 - y } else { y };
        for x in 0..width {
            let source_x = if horizontal { width - 1 - x } else { x };
            let source_index = (source_y * width + source_x) * 4;
            let dest_index = (y * width + x) * 4;
            out[dest_index..dest_index + 4]
                .copy_from_slice(&source[source_index..source_index + 4]);
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(width: usize, height: usize, color: [u8; 4]) -> Vec<u8> {
        color
            .iter()
            .copied()
            .cycle()
            .take(width * height * 4)
            .collect()
    }

    #[test]
    fn brightness_shifts_channels() {
        let mut pixels = solid(2, 2, [100, 100, 100, 255]);
        let adjust = Adjust {
            brightness: 20,
            ..Adjust::default()
        };
        assert!(apply_rgba(2, 2, &mut pixels, adjust));
        assert_eq!(pixels[0], 151);
    }

    #[test]
    fn neutral_adjust_is_noop() {
        let mut pixels = solid(2, 2, [10, 20, 30, 255]);
        assert!(!apply_rgba(2, 2, &mut pixels, Adjust::default()));
        assert_eq!(pixels, solid(2, 2, [10, 20, 30, 255]));
    }

    #[test]
    fn saturation_below_zero_produces_gray() {
        let mut pixels = solid(1, 1, [200, 40, 40, 255]);
        let adjust = Adjust {
            saturation: -100,
            ..Adjust::default()
        };
        assert!(apply_rgba(1, 1, &mut pixels, adjust));
        assert_eq!(pixels[0], pixels[1]);
        assert_eq!(pixels[1], pixels[2]);
    }

    #[test]
    fn rotates_quarter_turn_clockwise() {
        let source = vec![1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255];
        let mut out = Vec::new();
        let (width, height) = rotate_rgba(2, 2, &source, 90, &mut out).unwrap();
        assert_eq!((width, height), (2, 2));
        assert_eq!(out[0..4], [7, 8, 9, 255]);
        assert_eq!(out[4..8], [1, 2, 3, 255]);
        assert_eq!(out[8..12], [10, 11, 12, 255]);
        assert_eq!(out[12..16], [4, 5, 6, 255]);
    }

    #[test]
    fn rotation_swaps_dimensions() {
        let source = vec![0u8; 3 * 2 * 4];
        let mut out = Vec::new();
        let (width, height) = rotate_rgba(3, 2, &source, 270, &mut out).unwrap();
        assert_eq!((width, height), (2, 3));
        assert_eq!(out.len(), 24);
    }

    #[test]
    fn flips_horizontally() {
        let source = vec![1, 0, 0, 255, 2, 0, 0, 255];
        let mut out = Vec::new();
        assert!(flip_rgba(2, 1, &source, true, false, &mut out));
        assert_eq!(out[0], 2);
        assert_eq!(out[4], 1);
    }

    #[test]
    fn flips_vertically() {
        let source = vec![1, 0, 0, 255, 2, 0, 0, 255];
        let mut out = Vec::new();
        assert!(flip_rgba(1, 2, &source, false, true, &mut out));
        assert_eq!(out[0], 2);
        assert_eq!(out[4], 1);
    }

    #[test]
    fn flip_without_axis_is_noop() {
        let source = vec![1, 0, 0, 255];
        let mut out = Vec::new();
        assert!(!flip_rgba(1, 1, &source, false, false, &mut out));
    }

    #[test]
    fn rejects_bad_input() {
        let mut out = Vec::new();
        assert!(rotate_rgba(0, 2, &[], 90, &mut out).is_none());
        assert!(rotate_rgba(2, 2, &[0u8; 4], 90, &mut out).is_none());
        assert!(rotate_rgba(2, 2, &[0u8; 16], 45, &mut out).is_none());
    }
}
