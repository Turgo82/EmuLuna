//! Same rolling byte-based estimate as the archived disc_progress.ReadEstimate.
use std::collections::VecDeque;
#[derive(Default)]
pub(super) struct Estimate {
    samples: VecDeque<(f64, u64)>,
    started: Option<f64>,
    last_advance: f64,
    previous: u64,
    total: u64,
}
pub(super) struct Reading {
    pub idle: f64,
    pub waiting: bool,
    pub rate: f64,
    pub eta: Option<f64>,
}
impl Estimate {
    pub fn update(&mut self, done: u64, total: u64, now: f64) -> Reading {
        if self.started.is_none() || total != self.total || done < self.previous {
            self.samples.clear();
            self.started = Some(now);
            self.last_advance = now;
        }
        if done > self.previous {
            self.last_advance = now;
        }
        self.previous = done;
        self.total = total;
        if self.samples.back().is_none_or(|(t, _)| now - t >= 0.5) {
            self.samples.push_back((now, done));
        }
        while self.samples.len() > 2 && self.samples.get(1).is_some_and(|(t, _)| *t < now - 30.) {
            self.samples.pop_front();
        }
        let (start, bytes) = self.samples.front().copied().unwrap_or((now, done));
        let span = now - start;
        let rate = if span > 0. {
            done.saturating_sub(bytes) as f64 / span
        } else {
            0.
        };
        let idle = (now - self.last_advance).max(0.);
        let waiting = idle >= 10. && done < total;
        let eta = (rate > 0. && now - self.started.unwrap_or(now) >= 3. && !waiting)
            .then(|| total.saturating_sub(done) as f64 / rate);
        Reading {
            idle,
            waiting,
            rate: if waiting { 0. } else { rate },
            eta,
        }
    }
}
pub(super) fn duration(seconds: f64) -> String {
    let seconds = seconds.max(0.).ceil() as u64;
    if seconds < 60 {
        return format!("{seconds} sec");
    }
    let minutes = seconds.div_ceil(60);
    if minutes < 60 {
        return format!("{minutes} min");
    }
    let (hours, minutes) = (minutes / 60, minutes % 60);
    if minutes == 0 {
        format!("{hours} hr")
    } else {
        format!("{hours} hr {minutes} min")
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn estimate_pauses_without_new_bytes_and_recovers() {
        let mut e = Estimate::default();
        assert!(e.update(0, 10000, 0.).eta.is_none());
        let r = e.update(2000, 10000, 4.);
        assert_eq!(r.eta, Some(16.));
        let r = e.update(2000, 10000, 15.);
        assert!(r.waiting);
        assert_eq!(r.rate, 0.);
        assert!(r.eta.is_none());
        let r = e.update(3000, 10000, 16.);
        assert!(!r.waiting);
        assert!(r.eta.is_some());
    }
    #[test]
    fn changing_total_or_restarting_resets_rate() {
        let mut e = Estimate::default();
        e.update(100, 1000, 0.);
        e.update(500, 1000, 4.);
        assert!(e.update(0, 2000, 5.).eta.is_none());
        assert!(!e.update(2000, 2000, 20.).waiting);
        assert_eq!(duration(65.), "2 min");
        assert_eq!(duration(3601.), "1 hr 1 min");
    }
}
