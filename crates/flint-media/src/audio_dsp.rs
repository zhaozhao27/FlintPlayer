pub const EFFECT_NORMALIZE: u32 = 1 << 0;
pub const EFFECT_CRYSTALLIZE: u32 = 1 << 1;
pub const EFFECT_DENOISE: u32 = 1 << 2;
pub const EFFECT_REVERB: u32 = 1 << 3;
pub const EFFECT_VOCAL_REMOVE: u32 = 1 << 4;
pub const EFFECT_VOCAL_ENHANCE: u32 = 1 << 5;
pub const EFFECT_EQUALIZER: u32 = 1 << 6;
pub const EFFECT_ALL: u32 = (1 << 7) - 1;

pub const EQ_BANDS: [f32; 10] = [
    31.0, 62.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
];

struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

#[derive(Clone, Copy, Default)]
struct BiquadState {
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl BiquadState {
    fn process(&mut self, coefficients: &Biquad, input: f32) -> f32 {
        let output =
            coefficients.b0 * input + coefficients.b1 * self.x1 + coefficients.b2 * self.x2
                - coefficients.a1 * self.y1
                - coefficients.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = input;
        self.y2 = self.y1;
        self.y1 = output;
        output
    }
}

struct Equalizer {
    rate: u32,
    gains: [f32; EQ_BANDS.len()],
    coefficients: Vec<Biquad>,
    states: [[BiquadState; EQ_BANDS.len()]; 2],
}

impl Equalizer {
    fn new(rate: u32) -> Self {
        let gains = [0.0; EQ_BANDS.len()];
        let mut equalizer = Self {
            rate,
            gains,
            coefficients: Vec::new(),
            states: [[BiquadState::default(); EQ_BANDS.len()]; 2],
        };
        equalizer.retune();
        equalizer
    }

    fn set_gains(&mut self, gains: &[f32; EQ_BANDS.len()]) {
        if &self.gains == gains {
            return;
        }
        self.gains = *gains;
        self.retune();
    }

    fn retune(&mut self) {
        self.coefficients = EQ_BANDS
            .iter()
            .zip(self.gains.iter())
            .map(|(frequency, gain)| self.biquad(*frequency, *gain))
            .collect();
    }

    fn biquad(&self, frequency: f32, gain_db: f32) -> Biquad {
        if gain_db.abs() < 0.01 {
            return Biquad {
                b0: 1.0,
                b1: 0.0,
                b2: 0.0,
                a1: 0.0,
                a2: 0.0,
            };
        }
        let frequency = frequency.min(self.rate as f32 * 0.45);
        let amplitude = 10f32.powf(gain_db / 40.0);
        let omega = 2.0 * std::f32::consts::PI * frequency / self.rate as f32;
        let alpha = omega.sin() / 2.0;
        let cosine = omega.cos();
        let a0 = 1.0 + alpha / amplitude;
        Biquad {
            b0: (1.0 + alpha * amplitude) / a0,
            b1: -2.0 * cosine / a0,
            b2: (1.0 - alpha * amplitude) / a0,
            a1: -2.0 * cosine / a0,
            a2: (1.0 - alpha / amplitude) / a0,
        }
    }

    fn process(&mut self, channel: usize, input: f32) -> f32 {
        let mut output = input;
        for (band, coefficients) in self.coefficients.iter().enumerate() {
            output = self.states[channel][band].process(coefficients, output);
        }
        output
    }
}

const COMB_DELAYS: [usize; 4] = [1116, 1188, 1277, 1356];
const ALLPASS_DELAYS: [usize; 2] = [556, 441];

struct Comb {
    buffer: Vec<f32>,
    index: usize,
    store: f32,
}

impl Comb {
    fn new(len: usize) -> Self {
        Self {
            buffer: vec![0.0; len.max(1)],
            index: 0,
            store: 0.0,
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        let output = self.buffer[self.index];
        self.store = output * 0.8 + self.store * 0.2;
        self.buffer[self.index] = input + self.store * 0.84;
        self.index = (self.index + 1) % self.buffer.len();
        output
    }
}

struct AllPass {
    buffer: Vec<f32>,
    index: usize,
}

impl AllPass {
    fn new(len: usize) -> Self {
        Self {
            buffer: vec![0.0; len.max(1)],
            index: 0,
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        let buffered = self.buffer[self.index];
        self.buffer[self.index] = input + buffered * 0.5;
        self.index = (self.index + 1) % self.buffer.len();
        buffered - input
    }
}

struct ChannelReverb {
    combs: Vec<Comb>,
    allpasses: Vec<AllPass>,
}

impl ChannelReverb {
    fn new(rate: u32, spread: usize) -> Self {
        let scale = |len: usize| (len + spread) * rate as usize / 44_100;
        Self {
            combs: COMB_DELAYS
                .iter()
                .map(|len| Comb::new(scale(*len)))
                .collect(),
            allpasses: ALLPASS_DELAYS
                .iter()
                .map(|len| AllPass::new(scale(*len)))
                .collect(),
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        let mut output = 0.0;
        for comb in &mut self.combs {
            output += comb.process(input);
        }
        output *= 0.25;
        for allpass in &mut self.allpasses {
            output = allpass.process(output);
        }
        output
    }
}

pub struct AudioEffects {
    normalize_peak: f32,
    normalize_gain: f32,
    crystal_lowpass: [f32; 2],
    denoise_env: f32,
    denoise_gain: f32,
    reverb: [ChannelReverb; 2],
    equalizer: Equalizer,
    scratch: Vec<f32>,
}

impl AudioEffects {
    pub fn new(rate: u32) -> Self {
        Self {
            normalize_peak: 0.0,
            normalize_gain: 1.0,
            crystal_lowpass: [0.0; 2],
            denoise_env: 0.0,
            denoise_gain: 1.0,
            reverb: [ChannelReverb::new(rate, 0), ChannelReverb::new(rate, 23)],
            equalizer: Equalizer::new(rate),
            scratch: Vec::new(),
        }
    }

    pub fn process(&mut self, samples: &mut [i16], flags: u32, eq_gains: &[f32; EQ_BANDS.len()]) {
        if flags & EFFECT_ALL == 0 || samples.len() < 2 {
            return;
        }
        if flags & EFFECT_EQUALIZER != 0 {
            self.equalizer.set_gains(eq_gains);
        }
        let frames = samples.len() / 2;
        self.scratch.clear();
        self.scratch.reserve(samples.len());
        for chunk in samples.chunks_exact(2) {
            let mut left = chunk[0] as f32 / 32768.0;
            let mut right = chunk[1] as f32 / 32768.0;

            if flags & EFFECT_EQUALIZER != 0 {
                left = self.equalizer.process(0, left);
                right = self.equalizer.process(1, right);
            }

            if flags & EFFECT_VOCAL_REMOVE != 0 {
                let side = (left - right) * 0.707;
                left = side;
                right = side;
            } else if flags & EFFECT_VOCAL_ENHANCE != 0 {
                let mid = (left + right) * 0.5;
                let side = (left - right) * 0.5;
                left = mid * 1.5 + side * 0.8;
                right = mid * 1.5 - side * 0.8;
            }

            if flags & EFFECT_DENOISE != 0 {
                let level = left.abs().max(right.abs());
                self.denoise_env = if level > self.denoise_env {
                    level
                } else {
                    self.denoise_env * 0.99987
                };
                let target = if self.denoise_env < 0.0035 { 0.12 } else { 1.0 };
                self.denoise_gain += (target - self.denoise_gain) * 0.001;
                left *= self.denoise_gain;
                right *= self.denoise_gain;
            }

            if flags & EFFECT_CRYSTALLIZE != 0 {
                for (channel, value) in [(&mut left, 0usize), (&mut right, 1usize)] {
                    let low = self.crystal_lowpass[value]
                        + (*channel - self.crystal_lowpass[value]) * 0.15;
                    self.crystal_lowpass[value] = low;
                    let excited = *channel + (*channel - low) * 0.7;
                    *channel = excited.tanh() * 0.95;
                }
            }

            if flags & EFFECT_NORMALIZE != 0 {
                let peak = left.abs().max(right.abs());
                self.normalize_peak = if peak > self.normalize_peak {
                    peak
                } else {
                    self.normalize_peak * 0.99992
                };
                let target = (0.9 / self.normalize_peak.max(0.03)).clamp(0.3, 4.0);
                self.normalize_gain += (target - self.normalize_gain) * 0.0005;
                left *= self.normalize_gain;
                right *= self.normalize_gain;
            }

            if flags & EFFECT_REVERB != 0 {
                let wet_left = self.reverb[0].process((left + right) * 0.5);
                let wet_right = self.reverb[1].process((left + right) * 0.5);
                left = left * 0.75 + wet_left * 0.35;
                right = right * 0.75 + wet_right * 0.35;
            }

            self.scratch.push(left.clamp(-1.0, 1.0));
            self.scratch.push(right.clamp(-1.0, 1.0));
        }

        for (sample, value) in samples
            .chunks_exact_mut(2)
            .take(frames)
            .zip(self.scratch.chunks_exact(2))
        {
            sample[0] = (value[0] * 32767.0).round() as i16;
            sample[1] = (value[1] * 32767.0).round() as i16;
        }
    }
}

impl Default for AudioEffects {
    fn default() -> Self {
        Self::new(48_000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vocal_removal_centers_difference() {
        let mut effects = AudioEffects::new(48_000);
        let mut samples = vec![10_000i16, 6_000, -8_000, -4_000];
        effects.process(&mut samples, EFFECT_VOCAL_REMOVE, &[0.0; 10]);
        assert_eq!(samples[0], samples[1]);
        assert!(samples[0] > 0);
        assert_eq!(samples[2], samples[3]);
        assert!(samples[2] < 0);
    }

    #[test]
    fn processor_keeps_zero_silent_and_all_effects_bounded() {
        let mut effects = AudioEffects::new(48_000);
        let mut silence = vec![0i16; 4096];
        effects.process(&mut silence, EFFECT_ALL, &[0.0; 10]);
        assert!(silence.iter().all(|sample| *sample == 0));

        let mut tone = Vec::with_capacity(8192);
        for index in 0..4096 {
            let value = ((index as f32 * 0.08).sin() * 30_000.0) as i16;
            tone.push(value);
            tone.push(value / 2);
        }
        effects.process(&mut tone, EFFECT_ALL, &[0.0; 10]);
        assert_eq!(tone.len(), 8192);
    }

    #[test]
    fn reverb_keeps_tail_after_impulse() {
        let mut effects = AudioEffects::new(48_000);
        let mut impulse = vec![0i16; 4410 * 2];
        impulse[0] = 30_000;
        impulse[1] = 30_000;
        effects.process(&mut impulse, EFFECT_REVERB, &[0.0; 10]);
        let tail_energy: i32 = impulse[200..]
            .iter()
            .map(|sample| i32::from(*sample).abs())
            .sum();
        assert!(tail_energy > 0);
    }

    #[test]
    fn equalizer_boosts_selected_band() {
        let mut effects = AudioEffects::new(48_000);
        let tone = |gain: f32| {
            let mut samples = Vec::with_capacity(4800 * 2);
            for index in 0..4800 {
                let value = ((index as f32 * 2.0 * std::f32::consts::PI * 125.0 / 48_000.0).sin()
                    * 8000.0
                    * gain) as i16;
                samples.push(value);
                samples.push(value);
            }
            samples
        };
        let mut flat = tone(1.0);
        effects.process(&mut flat, EFFECT_EQUALIZER, &[0.0; 10]);

        let mut gains = [0.0; 10];
        gains[2] = 12.0;
        let mut boosted = tone(1.0);
        effects.process(&mut boosted, EFFECT_EQUALIZER, &gains);

        let energy = |samples: &[i16]| -> i64 {
            samples.iter().map(|sample| i64::from(*sample).pow(2)).sum()
        };
        let flat_energy = energy(&flat[600..]);
        let boosted_energy = energy(&boosted[600..]);
        assert!(boosted_energy > flat_energy * 2);
    }
}
