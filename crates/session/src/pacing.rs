//! Monotonic worker pacing. Scheduler oversleep does not accumulate every frame.
use std::time::{Duration, Instant};
pub struct Pacer {
    deadline: Instant,
}
impl Pacer {
    pub fn new(now: Instant) -> Self {
        Self { deadline: now }
    }
    /// Audio backpressure suspends the producer's deadline. A device's first
    /// consumption establishes a fresh epoch. Large stalls rebase instead of
    /// running an unbounded catch-up burst; ordinary scheduler jitter does not.
    pub fn advance(
        &mut self,
        now: Instant,
        period: Duration,
        audio_wait: Duration,
        clock_started: bool,
    ) -> Duration {
        if clock_started {
            self.deadline = now;
        } else {
            self.deadline += audio_wait;
        }
        self.deadline += period;
        if now.saturating_duration_since(self.deadline) > Duration::from_millis(100) {
            self.deadline = now;
        }
        self.deadline.saturating_duration_since(now)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn small_scheduler_delays_do_not_slow_every_frame() {
        let base = Instant::now();
        let period = Duration::from_millis(20);
        let mut pacer = Pacer::new(base);
        assert_eq!(pacer.advance(base, period, Duration::ZERO, false), period);
        for index in 1..1000 {
            let now = base + period * index + Duration::from_micros(500);
            assert_eq!(
                pacer.advance(now, period, Duration::ZERO, false),
                period - Duration::from_micros(500)
            );
        }
    }
    #[test]
    fn startup_and_audio_wait_are_not_unbounded_catchup() {
        let base = Instant::now();
        let period = Duration::from_millis(20);
        let mut pacer = Pacer::new(base);
        assert_eq!(
            pacer.advance(base + Duration::from_secs(1), period, Duration::ZERO, true),
            period
        );
        assert_eq!(
            pacer.advance(
                base + Duration::from_millis(1040),
                period,
                Duration::from_millis(20),
                false
            ),
            period
        );
        assert_eq!(
            pacer.advance(base + Duration::from_secs(3), period, Duration::ZERO, false),
            Duration::ZERO
        );
    }
}
