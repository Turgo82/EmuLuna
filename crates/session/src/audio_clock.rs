//! Smooth native resampler correction; never changes the core's frame clock.
use std::time::Duration;
pub struct AudioRateControl {
    target: f64,
    level: Option<f64>,
    ratio: f64,
}
impl AudioRateControl {
    pub fn new(target_frames: f64) -> Self {
        let target = if target_frames.is_finite() {
            target_frames.clamp(960., 12000.)
        } else {
            2400.
        };
        Self {
            target,
            level: None,
            ratio: 1.,
        }
    }
    pub fn reset(&mut self) {
        self.level = None;
        self.ratio = 1.;
    }
    pub fn update(&mut self, queued_frames: f64, period: Duration) -> f64 {
        if !queued_frames.is_finite() {
            self.reset();
            return self.ratio;
        }
        let seconds = period.as_secs_f64();
        let alpha = (seconds / 0.25).clamp(0., 1.);
        let level = self.level.map_or(queued_frames, |level| {
            level + alpha * (queued_frames - level)
        });
        self.level = Some(level);
        let desired = 1. + (((self.target - level) / self.target) * 0.005).clamp(-0.005, 0.005);
        let slew = seconds * 0.006;
        self.ratio += (desired - self.ratio).clamp(-slew, slew);
        self.ratio
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn follows_sink_drift_without_changing_emulation_speed() {
        for drift in [-0.002, 0.002] {
            let mut control = AudioRateControl::new(2400.);
            let mut queue = 2400f64;
            let (mut low, mut high) = (queue, queue);
            let mut ratio = 1.;
            for _ in 0..7200 {
                ratio = control.update(queue, Duration::from_secs_f64(1. / 60.));
                queue += 800. * ratio - 800. * (1. + drift);
                low = low.min(queue);
                high = high.max(queue);
                assert!((0.995..=1.005).contains(&ratio));
            }
            assert!(low > 1200. && high < 4000., "{low}..{high}");
            assert!((ratio - (1. + drift)).abs() < 0.0002);
        }
    }
    #[test]
    fn callback_block_jitter_is_smoothed_and_invalid_statistics_reset() {
        let mut control = AudioRateControl::new(2400.);
        let mut tail = Vec::new();
        for index in 0..600 {
            let ratio = control.update(
                if index % 2 == 0 { 2144. } else { 2656. },
                Duration::from_secs_f64(1. / 60.),
            );
            if index >= 480 {
                tail.push(ratio);
            }
        }
        let low = tail.iter().copied().fold(f64::INFINITY, f64::min);
        let high = tail.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        assert!(high - low < 0.0001);
        assert_eq!(control.update(f64::NAN, Duration::from_millis(16)), 1.);
    }
}
