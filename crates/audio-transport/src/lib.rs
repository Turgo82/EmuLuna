//! Preallocated stereo transport; no locks or allocations in the consumer.
#![forbid(unsafe_code)]
use rtrb::{Consumer, Producer, RingBuffer};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
#[derive(Default)]
pub struct Statistics {
    pub missing: AtomicU64,
    pub overflow: AtomicU64,
    pub accepted: AtomicU64,
    pub played: AtomicU64,
    pub discarded: AtomicU64,
    pub tail_silence: AtomicU64,
}
pub struct Sender {
    producer: Producer<[i16; 2]>,
    pub statistics: Arc<Statistics>,
}
pub struct Receiver {
    consumer: Consumer<[i16; 2]>,
    pub statistics: Arc<Statistics>,
}
pub fn channel(capacity_frames: usize) -> (Sender, Receiver) {
    let (producer, consumer) = RingBuffer::new(capacity_frames);
    let statistics = Arc::new(Statistics::default());
    (
        Sender {
            producer,
            statistics: statistics.clone(),
        },
        Receiver {
            consumer,
            statistics,
        },
    )
}
impl Sender {
    pub fn write(&mut self, samples: &[i16]) {
        let frames = samples.as_chunks::<2>().0;
        let (accepted, remainder) = self.producer.push_partial_slice(frames);
        self.statistics
            .accepted
            .fetch_add(accepted.len() as u64, Ordering::Relaxed);
        self.statistics
            .overflow
            .fetch_add(remainder.len() as u64, Ordering::Relaxed);
    }
    /// Worker-side backpressure. Never call from the device callback.
    pub fn write_wait(
        &mut self,
        samples: &[i16],
        cancelled: &std::sync::atomic::AtomicBool,
    ) -> Result<std::time::Duration, &'static str> {
        use std::time::{Duration, Instant};
        let mut waited = Duration::ZERO;
        let mut remaining = samples.as_chunks::<2>().0;
        let mut stalled = None;
        while !remaining.is_empty() {
            let (accepted, remainder) = self.producer.push_partial_slice(remaining);
            if !accepted.is_empty() {
                self.statistics
                    .accepted
                    .fetch_add(accepted.len() as u64, Ordering::Relaxed);
                stalled = None;
            }
            remaining = remainder;
            if remaining.is_empty() {
                break;
            }
            if cancelled.load(Ordering::Acquire)
                || stalled.get_or_insert_with(Instant::now).elapsed() > Duration::from_secs(2)
            {
                return Err("Audio delivery cancelled or device stopped consuming");
            }
            let sleep = Instant::now();
            std::thread::sleep(Duration::from_millis(1));
            waited += sleep.elapsed();
        }
        Ok(waited)
    }
    pub fn queued(&self) -> usize {
        self.producer.buffer().capacity() - self.producer.slots()
    }
}
impl Receiver {
    /// Consumer-only discontinuity: called after the producer has stopped
    /// writing and requested a boundary. Each discarded stereo frame remains
    /// accounted for, rather than being reported as played or missing.
    pub fn discard_queued(&mut self) {
        let count = self.consumer.slots();
        if count > 0 {
            if let Ok(chunk) = self.consumer.read_chunk(count) {
                chunk.commit_all();
                self.statistics
                    .discarded
                    .fetch_add(count as u64, Ordering::Relaxed);
            }
        }
    }
    pub fn available(&self) -> usize {
        self.consumer.slots()
    }
    pub fn fill(&mut self, output: &mut [i16]) {
        self.fill_counted(output, None);
    }
    pub fn fill_session(&mut self, output: &mut [i16], ended: &AtomicBool) {
        self.fill_counted(output, Some(ended));
    }
    fn fill_counted(&mut self, output: &mut [i16], ended: Option<&AtomicBool>) {
        let (played, missing) = {
            let frames = output.as_chunks_mut::<2>().0;
            let (filled, remainder) = self.consumer.pop_partial_slice(frames);
            remainder.fill([0, 0]);
            (filled.len(), remainder.len())
        };
        if !output.len().is_multiple_of(2) {
            *output.last_mut().unwrap() = 0;
        }
        let played_total = self
            .statistics
            .played
            .fetch_add(played as u64, Ordering::Relaxed)
            .saturating_add(played as u64);
        if missing > 0 {
            // Acquire EOF before reading the final accepted count. Silence is
            // tail only when every published frame was played or intentionally
            // discarded; queued but not yet played audio still means starvation.
            let at_end = ended.is_some_and(|flag| flag.load(Ordering::Acquire))
                && played_total.saturating_add(self.statistics.discarded.load(Ordering::Relaxed))
                    >= self.statistics.accepted.load(Ordering::Relaxed);
            let counter = if at_end {
                &self.statistics.tail_silence
            } else {
                &self.statistics.missing
            };
            counter.fetch_add(missing as u64, Ordering::Relaxed);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overflow_and_starvation_preserve_channel_alignment() {
        let (mut tx, mut rx) = channel(2);
        tx.write(&[1, -1, 2, -2, 3, -3]);
        let mut out = [99; 6];
        rx.fill(&mut out);
        assert_eq!(out, [1, -1, 2, -2, 0, 0]);
        assert_eq!(tx.statistics.overflow.load(Ordering::Relaxed), 1);
        assert_eq!(tx.statistics.missing.load(Ordering::Relaxed), 1);
    }
    #[test]
    fn cancelled_backpressure_returns_without_losing_accounting() {
        let (mut tx, _rx) = channel(1);
        let cancelled = std::sync::atomic::AtomicBool::new(true);
        assert!(tx.write_wait(&[1, -1, 2, -2], &cancelled).is_err());
        assert_eq!(tx.statistics.accepted.load(Ordering::Relaxed), 1);
        assert_eq!(tx.queued(), 1);
    }
    #[test]
    fn backpressure_delivers_a_block_larger_than_the_ring() {
        let (mut tx, mut rx) = channel(2);
        let worker = std::thread::spawn(move || {
            tx.write_wait(
                &[1, -1, 2, -2, 3, -3, 4, -4],
                &std::sync::atomic::AtomicBool::new(false),
            )
            .unwrap();
            tx
        });
        let until = std::time::Instant::now() + std::time::Duration::from_secs(1);
        let mut received = Vec::new();
        while received.len() < 8 {
            assert!(std::time::Instant::now() < until);
            if rx.available() > 0 {
                let mut pair = [0; 2];
                rx.fill(&mut pair);
                received.extend(pair);
            } else {
                std::thread::yield_now();
            }
        }
        let tx = worker.join().unwrap();
        assert_eq!(received, [1, -1, 2, -2, 3, -3, 4, -4]);
        assert_eq!(tx.statistics.accepted.load(Ordering::Relaxed), 4);
        assert_eq!(tx.statistics.overflow.load(Ordering::Relaxed), 0);
    }
    #[test]
    fn producer_keeps_running_without_a_video_consumer() {
        let (mut tx, mut rx) = channel(8);
        let worker = std::thread::spawn(move || {
            tx.write(&[5, -5, 6, -6]);
            tx
        });
        let tx = worker.join().unwrap();
        assert_eq!(tx.queued(), 2);
        let mut out = [0; 4];
        rx.fill(&mut out);
        assert_eq!(out, [5, -5, 6, -6]);
    }
    #[test]
    fn batched_stereo_wraps_ring_and_handles_an_odd_output_tail() {
        let (mut tx, mut rx) = channel(5);
        tx.write(&[1, -1, 2, -2, 3, -3]);
        let mut first = [0; 4];
        rx.fill(&mut first);
        assert_eq!(first, [1, -1, 2, -2]);
        tx.write(&[4, -4, 5, -5, 6, -6, 7, -7]);
        let mut second = [99; 11];
        rx.fill(&mut second);
        assert_eq!(second, [3, -3, 4, -4, 5, -5, 6, -6, 7, -7, 0]);
        assert_eq!(tx.statistics.accepted.load(Ordering::Relaxed), 7);
        assert_eq!(tx.statistics.played.load(Ordering::Relaxed), 7);
        assert_eq!(tx.statistics.missing.load(Ordering::Relaxed), 0);
        assert_eq!(tx.statistics.overflow.load(Ordering::Relaxed), 0);
    }
    #[test]
    fn eof_tail_is_separate_from_starvation_and_accounts_for_discards() {
        let (mut tx, mut rx) = channel(4);
        let ended = AtomicBool::new(false);
        tx.write(&[1, -1, 2, -2]);
        let mut output = [99; 6];
        rx.fill_session(&mut output, &ended);
        assert_eq!(tx.statistics.missing.load(Ordering::Relaxed), 1);
        tx.write(&[3, -3]);
        rx.discard_queued();
        tx.write(&[4, -4]);
        ended.store(true, Ordering::Release);
        rx.fill_session(&mut output, &ended);
        assert_eq!(output, [4, -4, 0, 0, 0, 0]);
        assert_eq!(tx.statistics.missing.load(Ordering::Relaxed), 1);
        assert_eq!(tx.statistics.tail_silence.load(Ordering::Relaxed), 2);
        assert_eq!(tx.statistics.discarded.load(Ordering::Relaxed), 1);
    }
}
