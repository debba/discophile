//! Sample-rate conversion to the device rate, playback speed, and channel mapping.
//!
//! Speed is a resampling ratio change, so audiobooks and podcasts at 1.5x also sound higher.
//! Pitch is not preserved.

use rubato::{Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction};

pub const MIN_SPEED: f32 = 0.5;
pub const MAX_SPEED: f32 = 4.0;
/// Input frames per resampler call.
const CHUNK: usize = 1024;

pub fn clamp_speed(speed: f32) -> f32 {
    if speed.is_finite() { speed.clamp(MIN_SPEED, MAX_SPEED) } else { 1.0 }
}

/// Converts interleaved audio of one rate and channel count into the device rate at a speed.
/// Output length matches `input frames × ratio` exactly once `finish` is called, so joined tracks
/// neither overlap nor leave a gap.
pub struct Resample {
    in_rate: u32,
    out_rate: u32,
    channels: usize,
    speed: f32,
    inner: Option<SincFixedIn<f32>>,
    /// Input not yet resampled, per channel. Rubato's sinc resampler adds no net delay (tested).
    pending: Vec<Vec<f32>>,
    expected: f64,
    produced: u64,
}

impl Resample {
    pub fn new(in_rate: u32, out_rate: u32, channels: usize, speed: f32) -> Self {
        let mut resample = Self {
            in_rate: in_rate.max(1),
            out_rate: out_rate.max(1),
            channels: channels.max(1),
            speed: clamp_speed(speed),
            inner: None,
            pending: vec![Vec::new(); channels.max(1)],
            expected: 0.0,
            produced: 0,
        };
        if resample.in_rate != resample.out_rate || resample.speed != 1.0 {
            resample.build();
        }
        resample
    }

    pub fn in_rate(&self) -> u32 {
        self.in_rate
    }

    /// Output frames per input frame at the current speed.
    pub fn ratio(&self) -> f64 {
        f64::from(self.out_rate) / f64::from(self.in_rate) / f64::from(self.speed)
    }

    #[cfg(test)]
    pub fn is_passthrough(&self) -> bool {
        self.inner.is_none()
    }

    fn build(&mut self) {
        let parameters = SincInterpolationParameters {
            sinc_len: 128,
            f_cutoff: 0.95,
            oversampling_factor: 128,
            interpolation: SincInterpolationType::Cubic,
            window: WindowFunction::BlackmanHarris2,
        };
        let base = f64::from(self.out_rate) / f64::from(self.in_rate);
        // Relative ratios from 1/MAX_SPEED to 1/MIN_SPEED must be allowed.
        match SincFixedIn::<f32>::new(base, f64::from(MAX_SPEED), parameters, CHUNK, self.channels) {
            Ok(mut inner) => {
                if self.speed != 1.0 {
                    let _ = inner.set_resample_ratio_relative(1.0 / f64::from(self.speed), false);
                }
                self.inner = Some(inner);
            }
            Err(error) => log::warn!("Resampler unavailable: {error}"),
        }
    }

    pub fn set_speed(&mut self, speed: f32) {
        let speed = clamp_speed(speed);
        if speed == self.speed {
            return;
        }
        self.speed = speed;
        match &mut self.inner {
            Some(inner) => {
                let _ = inner.set_resample_ratio_relative(1.0 / f64::from(speed), true);
            }
            None => self.build(),
        }
    }

    /// Converts `input` (interleaved, `channels` wide) and appends the result to `out`.
    pub fn push(&mut self, input: &[f32], out: &mut Vec<f32>) {
        let Some(_) = self.inner else {
            out.extend_from_slice(input);
            return;
        };
        let frames = input.len() / self.channels;
        for frame in input.chunks_exact(self.channels) {
            for (channel, sample) in frame.iter().enumerate() {
                self.pending[channel].push(*sample);
            }
        }
        self.expected += frames as f64 * self.ratio();
        while let Some(inner) = &mut self.inner {
            let needed = inner.input_frames_next();
            if self.pending[0].len() < needed {
                break;
            }
            let block: Vec<&[f32]> = self.pending.iter().map(|c| &c[..needed]).collect();
            let result = inner.process(&block, None);
            for channel in &mut self.pending {
                channel.drain(..needed);
            }
            match result {
                Ok(planes) => self.emit(&planes, out, false),
                Err(error) => {
                    log::warn!("Resampling failed: {error}");
                    break;
                }
            }
        }
    }

    /// Flushes what is still inside the filter at the end of the stream.
    pub fn finish(&mut self, out: &mut Vec<f32>) {
        if self.inner.is_none() {
            return;
        }
        let target = self.expected.round() as u64;
        if !self.pending[0].is_empty() {
            let pending = std::mem::replace(&mut self.pending, vec![Vec::new(); self.channels]);
            if let Some(inner) = &mut self.inner
                && let Ok(planes) = inner.process_partial(Some(&pending), None)
            {
                self.emit(&planes, out, true);
            }
        }
        // Each empty call pushes zeros through the filter; bounded in case the ratio misbehaves.
        let mut guard = 0;
        while self.produced < target && guard < 64 {
            guard += 1;
            let Some(inner) = &mut self.inner else { break };
            match inner.process_partial::<Vec<f32>>(None, None) {
                Ok(planes) => self.emit(&planes, out, true),
                Err(_) => break,
            }
        }
    }

    fn emit(&mut self, planes: &[Vec<f32>], out: &mut Vec<f32>, limit: bool) {
        let frames = planes.first().map_or(0, Vec::len);
        let mut take = frames;
        if limit {
            let target = self.expected.round() as u64;
            take = take.min(target.saturating_sub(self.produced) as usize);
        }
        out.reserve(take * self.channels);
        for frame in 0..take {
            for plane in planes {
                out.push(plane[frame]);
            }
        }
        self.produced += take as u64;
    }

    /// Forgets buffered input, for example after a seek.
    pub fn reset(&mut self) {
        for channel in &mut self.pending {
            channel.clear();
        }
        if let Some(inner) = &mut self.inner {
            inner.reset();
            if self.speed != 1.0 {
                let _ = inner.set_resample_ratio_relative(1.0 / f64::from(self.speed), false);
            }
        }
        self.expected = 0.0;
        self.produced = 0;
    }
}

/// Maps interleaved audio from `from` channels to `to` channels and appends it to `out`.
/// Mono is copied to both front channels; surround is folded down to stereo.
pub fn map_channels(input: &[f32], from: usize, to: usize, out: &mut Vec<f32>) {
    let (from, to) = (from.max(1), to.max(1));
    if from == to {
        out.extend_from_slice(input);
        return;
    }
    out.reserve(input.len() / from * to);
    for frame in input.chunks_exact(from) {
        match (from, to) {
            (_, 1) => out.push(frame.iter().sum::<f32>() / from as f32),
            (1, _) => {
                out.push(frame[0]);
                out.push(frame[0]);
                out.extend(std::iter::repeat_n(0.0, to - 2));
            }
            (2, _) => {
                out.extend_from_slice(frame);
                out.extend(std::iter::repeat_n(0.0, to - 2));
            }
            (_, 2) => {
                // Front left, front right, centre, LFE (dropped), then alternating surrounds.
                const SIDE: f32 = std::f32::consts::FRAC_1_SQRT_2;
                let centre = frame.get(2).copied().unwrap_or(0.0) * SIDE;
                let (mut left, mut right) = (frame[0] + centre, frame[1] + centre);
                for (index, sample) in frame.iter().enumerate().skip(4) {
                    if index % 2 == 0 {
                        left += sample * SIDE;
                    } else {
                        right += sample * SIDE;
                    }
                }
                let scale = 1.0 / (1.0 + SIDE + if from > 4 { SIDE } else { 0.0 });
                out.push(left * scale);
                out.push(right * scale);
            }
            _ => {
                for channel in 0..to {
                    out.push(frame.get(channel).copied().unwrap_or(0.0));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(resample: &mut Resample, frames: usize, channels: usize, piece: usize) -> Vec<f32> {
        let input: Vec<f32> = (0..frames * channels).map(|i| ((i / channels) as f32 * 0.01).sin()).collect();
        let mut out = Vec::new();
        for chunk in input.chunks(piece * channels) {
            resample.push(chunk, &mut out);
        }
        resample.finish(&mut out);
        out
    }

    #[test]
    fn passthrough_when_rates_match() {
        let mut resample = Resample::new(48_000, 48_000, 2, 1.0);
        assert!(resample.is_passthrough());
        let out = run(&mut resample, 1000, 2, 333);
        assert_eq!(out.len(), 2000);
    }

    #[test]
    fn output_length_is_exact_for_any_chunking() {
        for piece in [1, 100, 1024, 4096, 10_000] {
            let mut resample = Resample::new(44_100, 48_000, 2, 1.0);
            let out = run(&mut resample, 44_100, 2, piece);
            assert_eq!(out.len() / 2, 48_000, "piece {piece}");
        }
    }

    #[test]
    fn downsampling_length_is_exact() {
        let mut resample = Resample::new(96_000, 44_100, 1, 1.0);
        let out = run(&mut resample, 9_600, 1, 777);
        assert_eq!(out.len(), 4_410);
    }

    #[test]
    fn speed_shortens_output() {
        let mut resample = Resample::new(48_000, 48_000, 2, 2.0);
        assert!(!resample.is_passthrough());
        let out = run(&mut resample, 48_000, 2, 512);
        assert_eq!(out.len() / 2, 24_000);
    }

    #[test]
    fn steps_stay_in_place() {
        // A step at input frame `at` must appear near output frame `at × ratio`: no added delay,
        // or gapless joins would drift.
        for (from, to, at) in [(44_100, 88_200, 1000usize), (48_000, 44_100, 4800), (44_100, 48_000, 3000)] {
            let mut resample = Resample::new(from, to, 1, 1.0);
            let mut input = vec![0.0f32; at * 4];
            input[at..].iter_mut().for_each(|s| *s = 1.0);
            let mut out = Vec::new();
            for chunk in input.chunks(700) {
                resample.push(chunk, &mut out);
            }
            resample.finish(&mut out);
            let first = out.iter().position(|s| *s > 0.5).unwrap() as f64;
            let expected = at as f64 * f64::from(to) / f64::from(from);
            assert!((first - expected).abs() <= 2.0, "{from}->{to}: step at {first}, expected {expected}");
        }
    }

    #[test]
    fn reset_restarts_counting() {
        let mut resample = Resample::new(44_100, 48_000, 2, 1.0);
        let _ = run(&mut resample, 10_000, 2, 512);
        resample.reset();
        let out = run(&mut resample, 4_410, 2, 512);
        assert_eq!(out.len() / 2, 4_800);
    }

    #[test]
    fn channel_mapping() {
        let mut out = Vec::new();
        map_channels(&[0.5, -0.5], 1, 2, &mut out);
        assert_eq!(out, [0.5, 0.5, -0.5, -0.5]);
        out.clear();
        map_channels(&[1.0, 0.0], 2, 1, &mut out);
        assert_eq!(out, [0.5]);
        out.clear();
        map_channels(&[1.0, 1.0, 1.0, 1.0, 1.0, 1.0], 6, 2, &mut out);
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|s| (*s - 1.0).abs() < 1e-5));
        out.clear();
        map_channels(&[0.25, 0.75], 2, 4, &mut out);
        assert_eq!(out, [0.25, 0.75, 0.0, 0.0]);
    }
}
