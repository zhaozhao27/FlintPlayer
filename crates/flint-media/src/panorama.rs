use crate::Frame;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub yaw: f32,
    pub pitch: f32,
    pub fov: f32,
}

impl Default for View {
    fn default() -> Self {
        Self {
            yaw: 0.0,
            pitch: 0.0,
            fov: 90f32.to_radians(),
        }
    }
}

impl View {
    pub fn clamped(self) -> Self {
        Self {
            yaw: wrap_angle(self.yaw),
            pitch: self.pitch.clamp(-MAX_PITCH, MAX_PITCH),
            fov: self.fov.clamp(MIN_FOV, MAX_FOV),
        }
    }
}

const MAX_PITCH: f32 = std::f32::consts::FRAC_PI_2 - 0.017_453_3; // 89°
const MIN_FOV: f32 = std::f32::consts::FRAC_PI_6; // 30°
const MAX_FOV: f32 = 2.0 * std::f32::consts::FRAC_PI_3; // 120°

fn wrap_angle(angle: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    let wrapped = angle % tau;
    if wrapped > std::f32::consts::PI {
        wrapped - tau
    } else if wrapped < -std::f32::consts::PI {
        wrapped + tau
    } else {
        wrapped
    }
}

pub struct Map {
    width: usize,
    height: usize,
    u: Vec<f32>,
    v: Vec<f32>,
    view: View,
}

impl Map {
    pub fn build(width: usize, height: usize, view: View) -> Option<Self> {
        if width < 2 || height < 2 {
            return None;
        }
        let view = view.clamped();
        let aspect = width as f32 / height as f32;
        let tan_half = (view.fov * 0.5).tan();
        let (sy, cy) = view.yaw.sin_cos();
        let (sp, cp) = view.pitch.sin_cos();
        let mut u = Vec::with_capacity(width * height);
        let mut v = Vec::with_capacity(width * height);
        for y in 0..height {
            let ny = (1.0 - 2.0 * (y as f32 + 0.5) / height as f32) * tan_half;
            let y1 = ny * cp - sp;
            let z1 = ny * sp + cp;
            for x in 0..width {
                let nx = (2.0 * (x as f32 + 0.5) / width as f32 - 1.0) * aspect * tan_half;
                let xw = nx * cy - z1 * sy;
                let zw = nx * sy + z1 * cy;
                let lon = xw.atan2(zw);
                let lat = y1.atan2((xw * xw + zw * zw).sqrt());
                u.push(
                    ((lon / std::f32::consts::TAU + 0.5) * width as f32 - 0.5)
                        .rem_euclid(width as f32),
                );
                v.push(
                    ((0.5 - lat / std::f32::consts::PI) * height as f32 - 0.5)
                        .clamp(0.0, height as f32 - 1.0),
                );
            }
        }
        Some(Self {
            width,
            height,
            u,
            v,
            view,
        })
    }

    pub fn matches(&self, width: usize, height: usize, view: View) -> bool {
        self.width == width && self.height == height && self.view == view.clamped()
    }

    pub fn render(&self, frame: &Frame, out: &mut Vec<u8>) -> bool {
        let (width, height) = (frame.width, frame.height);
        if width != self.width || height != self.height {
            return false;
        }
        let source = &frame.rgba;
        if source.len() < width * height * 4 {
            return false;
        }
        out.clear();
        out.resize(width * height * 4, 255);
        for y in 0..height {
            let row = y * width;
            for x in 0..width {
                let index = row + x;
                let u = self.u[index];
                let v = self.v[index];
                let y0 = v.floor();
                let y1 = (y0 + 1.0).min(height as f32 - 1.0);
                let x0 = u.floor();
                let x1 = (x0 + 1.0).rem_euclid(width as f32);
                let fx = u - x0;
                let fy = v - y0;
                let x0 = x0 as usize;
                let x1 = x1 as usize;
                let y0 = y0 as usize;
                let y1 = y1 as usize;
                let p00 = (y0 * width + x0) * 4;
                let p10 = (y0 * width + x1) * 4;
                let p01 = (y1 * width + x0) * 4;
                let p11 = (y1 * width + x1) * 4;
                let target = index * 4;
                for channel in 0..3 {
                    let top = source[p00 + channel] as f32 * (1.0 - fx)
                        + source[p10 + channel] as f32 * fx;
                    let bottom = source[p01 + channel] as f32 * (1.0 - fx)
                        + source[p11 + channel] as f32 * fx;
                    out[target + channel] = (top * (1.0 - fy) + bottom * fy).round() as u8;
                }
                out[target + 3] = 255;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn frame(width: usize, height: usize) -> Frame {
        let mut rgba = vec![0u8; width * height * 4];
        for y in 0..height {
            for x in 0..width {
                let index = (y * width + x) * 4;
                rgba[index] = (x * 255 / width) as u8;
                rgba[index + 1] = (y * 255 / height) as u8;
                rgba[index + 2] = 128;
                rgba[index + 3] = 255;
            }
        }
        Frame {
            width,
            height,
            rgba,
            pts: 0.0,
            captured_at: Instant::now(),
            serial: 0,
        }
    }

    fn assert_close(actual: &[u8], expected: &[u8]) {
        for (a, e) in actual.iter().zip(expected) {
            assert!(
                (*a as i16 - *e as i16).abs() <= 8,
                "expected {expected:?}, got {actual:?}"
            );
        }
    }

    #[test]
    fn center_of_view_samples_center_of_image() {
        let source = frame(64, 32);
        let map = Map::build(64, 32, View::default()).unwrap();
        let mut out = Vec::new();
        assert!(map.render(&source, &mut out));
        let index = (16 * 64 + 32) * 4;
        let expected = (16 * 64 + 32) * 4;
        assert_close(&out[index..index + 3], &source.rgba[expected..expected + 3]);
    }

    #[test]
    fn yaw_rotates_horizontally() {
        let source = frame(64, 32);
        let map = Map::build(
            64,
            32,
            View {
                yaw: 90f32.to_radians(),
                ..View::default()
            },
        )
        .unwrap();
        let mut out = Vec::new();
        assert!(map.render(&source, &mut out));
        let center = (16 * 64 + 32) * 4;
        let expected = (16 * 64 + 16) * 4;
        assert_close(
            &out[center..center + 3],
            &source.rgba[expected..expected + 3],
        );
    }

    #[test]
    fn rejects_mismatched_frame_size() {
        let source = frame(32, 32);
        let map = Map::build(64, 32, View::default()).unwrap();
        let mut out = Vec::new();
        assert!(!map.render(&source, &mut out));
    }

    #[test]
    fn clamps_pitch_and_fov() {
        let view = View {
            yaw: 7.0,
            pitch: 3.0,
            fov: 0.1,
        }
        .clamped();
        assert!(view.pitch <= MAX_PITCH);
        assert!(view.fov >= MIN_FOV);
        assert!(view.yaw.abs() <= std::f32::consts::PI);
    }
}
