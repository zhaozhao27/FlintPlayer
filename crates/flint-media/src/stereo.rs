use crate::Frame;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StereoMode {
    #[default]
    Off,
    AnaglyphSideBySide,
    AnaglyphTopBottom,
    LeftSideBySide,
    RightSideBySide,
    LeftTopBottom,
    RightTopBottom,
}

impl StereoMode {
    pub fn from_key(key: &str) -> Self {
        match key {
            "anaglyph-sbs" => Self::AnaglyphSideBySide,
            "anaglyph-tab" => Self::AnaglyphTopBottom,
            "left-sbs" => Self::LeftSideBySide,
            "right-sbs" => Self::RightSideBySide,
            "left-tab" => Self::LeftTopBottom,
            "right-tab" => Self::RightTopBottom,
            _ => Self::Off,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::AnaglyphSideBySide => "anaglyph-sbs",
            Self::AnaglyphTopBottom => "anaglyph-tab",
            Self::LeftSideBySide => "left-sbs",
            Self::RightSideBySide => "right-sbs",
            Self::LeftTopBottom => "left-tab",
            Self::RightTopBottom => "right-tab",
        }
    }

    pub fn is_off(self) -> bool {
        matches!(self, Self::Off)
    }

    fn is_horizontal(self) -> bool {
        matches!(
            self,
            Self::AnaglyphSideBySide | Self::LeftSideBySide | Self::RightSideBySide
        )
    }

    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::AnaglyphSideBySide,
            Self::AnaglyphSideBySide => Self::AnaglyphTopBottom,
            _ => Self::Off,
        }
    }
}

pub fn transform_into(frame: &Frame, mode: StereoMode, out: &mut Vec<u8>) -> bool {
    transform_rgba(frame.width, frame.height, &frame.rgba, mode, out)
}

pub fn transform_rgba(
    width: usize,
    height: usize,
    source: &[u8],
    mode: StereoMode,
    out: &mut Vec<u8>,
) -> bool {
    if mode.is_off() {
        return false;
    }
    let horizontal = mode.is_horizontal();
    if (horizontal && width < 2) || (!horizontal && height < 2) {
        return false;
    }
    let (eye_w, eye_h) = if horizontal {
        (width / 2, height)
    } else {
        (width, height / 2)
    };
    if eye_w == 0 || eye_h == 0 {
        return false;
    }
    if source.len() < width * height * 4 {
        return false;
    }
    let eye_offset = if horizontal { eye_w } else { eye_h * width };
    out.clear();
    out.resize(width * height * 4, 255);
    let anaglyph = matches!(
        mode,
        StereoMode::AnaglyphSideBySide | StereoMode::AnaglyphTopBottom
    );
    let right_eye = matches!(
        mode,
        StereoMode::RightSideBySide | StereoMode::RightTopBottom
    );
    for y in 0..height {
        let eye_y = y * eye_h / height;
        let row_start = y * width;
        let eye_row = eye_y * width;
        for x in 0..width {
            let eye_x = x * eye_w / width;
            let left = eye_row + eye_x;
            let target = (row_start + x) * 4;
            if anaglyph {
                let right = left + eye_offset;
                if right * 4 + 3 >= source.len() {
                    return false;
                }
                out[target] = source[left * 4];
                out[target + 1] = source[right * 4 + 1];
                out[target + 2] = source[right * 4 + 2];
            } else {
                let index = if right_eye { left + eye_offset } else { left };
                if index * 4 + 3 >= source.len() {
                    return false;
                }
                out[target..target + 4].copy_from_slice(&source[index * 4..index * 4 + 4]);
            }
            out[target + 3] = 255;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn frame(width: usize, height: usize, pixels: &[[u8; 4]]) -> Frame {
        Frame {
            width,
            height,
            rgba: pixels.iter().flatten().copied().collect(),
            nv12: None,
            pts: 0.0,
            captured_at: Instant::now(),
            serial: 0,
        }
    }

    #[test]
    fn anaglyph_combines_left_red_with_right_green_blue() {
        let pixels = [
            [200, 10, 10, 255],
            [0, 0, 0, 255],
            [20, 40, 220, 255],
            [0, 0, 0, 255],
        ];
        let source = frame(4, 1, &pixels);
        let mut out = Vec::new();
        assert!(transform_into(
            &source,
            StereoMode::AnaglyphSideBySide,
            &mut out
        ));
        assert_eq!(&out[0..4], &[200, 40, 220, 255]);
    }

    #[test]
    fn left_and_right_eyes_scale_to_full_frame() {
        let pixels = [
            [10, 20, 30, 255],
            [10, 20, 30, 255],
            [40, 50, 60, 255],
            [40, 50, 60, 255],
        ];
        let source = frame(4, 1, &pixels);
        let mut out = Vec::new();
        assert!(transform_into(
            &source,
            StereoMode::LeftSideBySide,
            &mut out
        ));
        assert_eq!(&out[0..4], &[10, 20, 30, 255]);
        assert_eq!(&out[4..8], &[10, 20, 30, 255]);
        assert!(transform_into(
            &source,
            StereoMode::RightSideBySide,
            &mut out
        ));
        assert_eq!(&out[0..4], &[40, 50, 60, 255]);
        assert_eq!(&out[4..8], &[40, 50, 60, 255]);
    }

    #[test]
    fn top_bottom_picks_correct_eye_rows() {
        let pixels = [
            [1, 2, 3, 255],
            [1, 2, 3, 255],
            [4, 5, 6, 255],
            [4, 5, 6, 255],
            [7, 8, 9, 255],
            [7, 8, 9, 255],
            [10, 11, 12, 255],
            [10, 11, 12, 255],
        ];
        let source = frame(2, 4, &pixels);
        let mut out = Vec::new();
        assert!(transform_into(&source, StereoMode::LeftTopBottom, &mut out));
        assert_eq!(&out[0..4], &[1, 2, 3, 255]);
        assert_eq!(&out[4..8], &[1, 2, 3, 255]);
        assert!(transform_into(
            &source,
            StereoMode::RightTopBottom,
            &mut out
        ));
        assert_eq!(&out[0..4], &[7, 8, 9, 255]);
        assert_eq!(&out[4..8], &[7, 8, 9, 255]);
    }

    #[test]
    fn off_mode_leaves_buffer_untouched() {
        let pixels = [[0, 0, 0, 255]; 4];
        let source = frame(2, 2, &pixels);
        let mut out = Vec::new();
        assert!(!transform_into(&source, StereoMode::Off, &mut out));
        assert!(out.is_empty());
    }

    #[test]
    fn cycles_off_anaglyph_sbs_anaglyph_tab() {
        assert_eq!(StereoMode::Off.next(), StereoMode::AnaglyphSideBySide);
        assert_eq!(
            StereoMode::AnaglyphSideBySide.next(),
            StereoMode::AnaglyphTopBottom
        );
        assert_eq!(StereoMode::AnaglyphTopBottom.next(), StereoMode::Off);
        assert_eq!(StereoMode::from_key("left-tab").key(), "left-tab");
    }
}
