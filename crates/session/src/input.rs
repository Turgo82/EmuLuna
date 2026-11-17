//! Coherent input snapshots without making the core wait for the window thread.
use std::sync::{
    atomic::{AtomicU32, AtomicU64, Ordering::SeqCst},
    Arc,
};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub buttons: u32,
    pub axes: [i16; 4],
}
struct Shared {
    version: AtomicU64,
    buttons: AtomicU32,
    axes: AtomicU64,
    pending_presses: AtomicU32,
}
/// Single publisher; intentionally not Clone. Publication needs exclusive access.
pub struct Publisher(Arc<Shared>);
pub struct Reader {
    shared: Arc<Shared>,
    previous: Snapshot,
}
pub fn channel() -> (Publisher, Reader) {
    let shared = Arc::new(Shared {
        version: AtomicU64::new(0),
        buttons: AtomicU32::new(0),
        axes: AtomicU64::new(0),
        pending_presses: AtomicU32::new(0),
    });
    (
        Publisher(shared.clone()),
        Reader {
            shared,
            previous: Snapshot::default(),
        },
    )
}
impl Publisher {
    pub fn latch_buttons(&mut self, buttons: u32) {
        if buttons != 0 {
            self.0.pending_presses.fetch_or(buttons, SeqCst);
        }
    }

    pub fn clear(&mut self) {
        self.publish(Snapshot::default());
        self.0.pending_presses.store(0, SeqCst);
    }

    pub fn publish(&mut self, snapshot: Snapshot) {
        let axes = snapshot
            .axes
            .iter()
            .enumerate()
            .fold(0u64, |word, (index, value)| {
                word | ((*value as u16 as u64) << (index * 16))
            });
        self.0.version.fetch_add(1, SeqCst);
        self.0.buttons.store(snapshot.buttons, SeqCst);
        self.0.axes.store(axes, SeqCst);
        self.0.version.fetch_add(1, SeqCst);
    }
}
impl Reader {
    /// A key can be pressed and released before the next emulated frame.
    /// Deliver its press once even if the latest held state is already clear.
    pub fn for_frame(&mut self) -> Snapshot {
        let pending = self.shared.pending_presses.swap(0, SeqCst);
        let mut snapshot = self.latest();
        snapshot.buttons |= pending;
        snapshot
    }

    /// Bounded attempts: a preempted publisher cannot stall emulation/audio.
    /// Sequentially consistent atomics keep the sequence and both data words in
    /// one order; a failed read retains the previous complete snapshot.
    pub fn latest(&mut self) -> Snapshot {
        for _ in 0..3 {
            let before = self.shared.version.load(SeqCst);
            if before & 1 != 0 {
                continue;
            }
            let buttons = self.shared.buttons.load(SeqCst);
            let word = self.shared.axes.load(SeqCst);
            if before == self.shared.version.load(SeqCst) {
                self.previous = Snapshot {
                    buttons,
                    axes: std::array::from_fn(|i| (word >> (i * 16)) as u16 as i16),
                };
                break;
            }
        }
        self.previous
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn publisher_preemption_returns_previous_complete_state() {
        let (mut publisher, mut reader) = channel();
        let state = Snapshot {
            buttons: 8,
            axes: [-32768, 32767, -1, 1],
        };
        publisher.publish(state);
        assert_eq!(reader.latest(), state);
        publisher.0.version.fetch_add(1, SeqCst);
        publisher.0.buttons.store(16, SeqCst);
        assert_eq!(reader.latest(), state);
    }
    #[test]
    fn concurrent_updates_never_mix_buttons_and_axes() {
        let (mut publisher, mut reader) = channel();
        let worker = std::thread::spawn(move || {
            for n in 1..30000 {
                publisher.publish(Snapshot {
                    buttons: n,
                    axes: [n as i16, -(n as i16), 123, -456],
                });
            }
        });
        for _ in 0..30000 {
            let state = reader.latest();
            if state.buttons != 0 {
                assert_eq!(
                    state.axes,
                    [state.buttons as i16, -(state.buttons as i16), 123, -456]
                );
            }
        }
        worker.join().unwrap();
        assert_eq!(reader.latest().buttons, 29999);
    }
}
