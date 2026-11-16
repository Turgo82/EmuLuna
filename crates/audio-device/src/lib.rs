//! Shared device delivery for software and hardware Rust sessions.
#![forbid(unsafe_code)]
use emuluna_audio_transport::{channel, Receiver, Sender, Statistics};
use sdl2::{
    audio::{AudioCallback, AudioDevice, AudioSpecDesired, AudioStatus},
    AudioSubsystem,
};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};
struct Control {
    paused: AtomicBool,
    boundary: AtomicU64,
    acknowledged: AtomicU64,
    gain: AtomicU32,
    ended: AtomicBool,
}
impl Default for Control {
    fn default() -> Self {
        Self {
            paused: AtomicBool::new(false),
            boundary: AtomicU64::new(0),
            acknowledged: AtomicU64::new(0),
            gain: AtomicU32::new(1f32.to_bits()),
            ended: AtomicBool::new(false),
        }
    }
}
struct Callback {
    receiver: Receiver,
    ready: Arc<AtomicBool>,
    control: Arc<Control>,
}
impl AudioCallback for Callback {
    type Channel = i16;
    fn callback(&mut self, output: &mut [i16]) {
        let boundary = self.control.boundary.load(Ordering::Acquire);
        if boundary != self.control.acknowledged.load(Ordering::Relaxed) {
            self.receiver.discard_queued();
            self.ready.store(false, Ordering::Release);
            self.control.acknowledged.store(boundary, Ordering::Release);
        }
        if self.control.paused.load(Ordering::Acquire) {
            output.fill(0);
            return;
        }
        if !self.ready.load(Ordering::Acquire) && self.receiver.available() >= 1920 {
            self.ready.store(true, Ordering::Release);
        }
        if self.ready.load(Ordering::Acquire) {
            self.receiver.fill_session(output, &self.control.ended);
            let gain = f32::from_bits(self.control.gain.load(Ordering::Relaxed));
            if gain != 1. {
                for sample in output {
                    *sample = (f32::from(*sample) * gain).round() as i16;
                }
            }
        } else {
            output.fill(0);
        }
    }
}
pub struct Device {
    device: Option<AudioDevice<Callback>>,
    parked: Option<Callback>,
    audio: AudioSubsystem,
    selected: Option<String>,
    outputs: Vec<String>,
    next_retry: Instant,
    statistics: Arc<Statistics>,
}
pub struct Producer {
    sender: Sender,
    ready: Arc<AtomicBool>,
    started: bool,
    started_played: u64,
    control: Arc<Control>,
}
pub struct WriteTiming {
    pub waited: Duration,
    pub clock_started: bool,
}
#[derive(Debug, Default)]
pub struct Report {
    pub missing: u64,
    pub overflow: u64,
    pub accepted: u64,
    pub played: u64,
    pub pending: u64,
    pub discarded: u64,
    pub tail_silence: u64,
}
impl Device {
    pub fn open(audio: &AudioSubsystem) -> Result<(Self, Producer), String> {
        Self::open_selected(audio, None)
    }
    pub fn outputs(audio: &AudioSubsystem) -> Vec<String> {
        (0..audio.num_audio_playback_devices().unwrap_or(0).min(256))
            .filter_map(|index| audio.audio_playback_device_name(index).ok())
            .collect()
    }
    pub fn open_selected(
        audio: &AudioSubsystem,
        selected: Option<&str>,
    ) -> Result<(Self, Producer), String> {
        if selected.is_some_and(|name| name.len() > 4096 || name.contains('\0')) {
            return Err("Invalid audio output name".into());
        }
        let (sender, receiver) = channel(4800);
        let statistics = sender.statistics.clone();
        let ready = Arc::new(AtomicBool::new(false));
        let callback_ready = ready.clone();
        let control = Arc::new(Control::default());
        let callback_control = control.clone();
        let mut callback = Some(Callback {
            receiver,
            ready: callback_ready,
            control: callback_control,
        });
        let spec = AudioSpecDesired {
            freq: Some(48000),
            channels: Some(2),
            samples: Some(512),
        };
        let device = match audio.open_playback(selected, &spec, |_| callback.take().unwrap()) {
            Ok(device) => device,
            Err(error) if selected.is_some() => {
                eprintln!("Preferred audio output unavailable; using system default: {error}");
                audio.open_playback(None, &spec, |_| callback.take().unwrap())?
            }
            Err(error) => return Err(error),
        };
        if device.spec().freq != 48000 || device.spec().channels != 2 {
            return Err("Device did not provide 48 kHz stereo".into());
        }
        eprintln!(
            "audio_freq={} audio_chunk={}",
            device.spec().freq,
            device.spec().samples
        );
        device.resume();
        Ok((
            Self {
                device: Some(device),
                parked: None,
                audio: audio.clone(),
                selected: selected.map(str::to_owned),
                outputs: Self::outputs(audio),
                next_retry: Instant::now(),
                statistics,
            },
            Producer {
                sender,
                ready,
                started: false,
                started_played: 0,
                control,
            },
        ))
    }
    pub fn available_outputs(&self) -> &[String] {
        &self.outputs
    }
    pub fn selected_output(&self) -> Option<&str> {
        self.selected.as_deref()
    }
    fn reopen(&mut self, selected: Option<&str>) -> Result<(), String> {
        if let Some(device) = self.device.take() {
            self.parked = Some(device.close_and_get_callback());
        }
        let device = self.audio.open_playback(
            selected,
            &AudioSpecDesired {
                freq: Some(48000),
                channels: Some(2),
                samples: Some(512),
            },
            |_| {
                self.parked
                    .take()
                    .expect("audio callback retained across device changes")
            },
        )?;
        device.resume();
        self.device = Some(device);
        Ok(())
    }
    /// Runs on the window thread. The core's producer and bounded queue survive
    /// device changes; the real-time callback never opens devices or allocates.
    pub fn select_output(&mut self, selected: Option<&str>) -> Result<(), String> {
        if selected.is_some_and(|name| name.len() > 4096 || name.contains('\0')) {
            return Err("Invalid audio output name".into());
        }
        let previous = self.selected.clone();
        if let Err(error) = self.reopen(selected) {
            // Keep the working choice when a requested device cannot open.
            let _ = self.reopen(previous.as_deref());
            self.next_retry = Instant::now() + Duration::from_secs(1);
            return Err(format!("Could not open audio output: {error}"));
        }
        self.selected = selected.map(str::to_owned);
        Ok(())
    }
    pub fn handle_event(&mut self, event: &sdl2::event::Event) -> Option<String> {
        use sdl2::event::Event;
        match event {
            Event::AudioDeviceRemoved {
                iscapture: false, ..
            } => {
                self.outputs = Self::outputs(&self.audio);
                let selected = self.selected.clone().filter(|s| self.outputs.contains(s));
                match self.reopen(selected.as_deref()) {
                    Ok(()) => Some("Audio output reconnected".into()),
                    Err(_) => {
                        self.next_retry = Instant::now();
                        Some("Audio output disconnected; reconnecting…".into())
                    }
                }
            }
            Event::AudioDeviceAdded {
                iscapture: false, ..
            } => {
                self.outputs = Self::outputs(&self.audio);
                if self
                    .selected
                    .as_ref()
                    .is_some_and(|s| self.outputs.contains(s))
                {
                    let selected = self.selected.clone();
                    let _ = self.reopen(selected.as_deref());
                }
                None
            }
            _ => None,
        }
    }
    pub fn service(&mut self) -> Option<String> {
        let failed = self
            .device
            .as_ref()
            .is_none_or(|d| d.status() == AudioStatus::Stopped);
        if !failed || Instant::now() < self.next_retry {
            return None;
        }
        self.next_retry = Instant::now() + Duration::from_secs(1);
        self.outputs = Self::outputs(&self.audio);
        let selected = self.selected.clone().filter(|s| self.outputs.contains(s));
        self.reopen(selected.as_deref())
            .ok()
            .map(|()| "Audio output reconnected".into())
    }
    pub fn statistics(&self) -> Arc<Statistics> {
        self.statistics.clone()
    }
    /// Call after the core worker has stopped. A final partial device block can
    /// contain silence; report it separately from starvation during playback.
    pub fn finish(&mut self, drain: bool) -> Report {
        if drain {
            // A freshly reopened route may take longer to deliver its first callbacks.
            // This bounded drain runs only after emulation has stopped.
            let until = Instant::now() + Duration::from_millis(500);
            while self.statistics.played.load(Ordering::Relaxed)
                < self
                    .statistics
                    .accepted
                    .load(Ordering::Relaxed)
                    .saturating_sub(self.statistics.discarded.load(Ordering::Relaxed))
                && Instant::now() < until
            {
                thread::sleep(Duration::from_millis(1));
            }
        }
        if let Some(device) = &self.device {
            device.pause();
        }
        let accepted = self.statistics.accepted.load(Ordering::Relaxed);
        let played = self.statistics.played.load(Ordering::Relaxed);
        let discarded = self.statistics.discarded.load(Ordering::Relaxed);
        Report {
            missing: self.statistics.missing.load(Ordering::Relaxed),
            overflow: self.statistics.overflow.load(Ordering::Relaxed),
            accepted,
            played,
            pending: accepted.saturating_sub(played).saturating_sub(discarded),
            discarded,
            tail_silence: self.statistics.tail_silence.load(Ordering::Relaxed),
        }
    }
}
impl Producer {
    pub fn set_volume(&mut self, volume: f32) {
        if volume.is_finite() {
            self.control
                .gain
                .store(volume.clamp(0., 1.).to_bits(), Ordering::Relaxed);
        }
    }
    /// Worker-only barrier. Callback acknowledges after discarding old PCM;
    /// fresh PCM cannot be accidentally cleared after the worker resumes.
    pub fn set_paused(&mut self, paused: bool, cancelled: &AtomicBool) -> Result<(), String> {
        self.control.paused.store(true, Ordering::Release);
        let boundary = self
            .control
            .boundary
            .fetch_add(1, Ordering::AcqRel)
            .wrapping_add(1);
        let start = Instant::now();
        while self.control.acknowledged.load(Ordering::Acquire) != boundary {
            if cancelled.load(Ordering::Acquire) {
                return Err("Audio transition cancelled".into());
            }
            if start.elapsed() > Duration::from_secs(2) {
                return Err("Audio device did not acknowledge playback transition".into());
            }
            thread::sleep(Duration::from_millis(1));
        }
        self.started = false;
        self.started_played = self.sender.statistics.played.load(Ordering::Relaxed);
        self.control.paused.store(paused, Ordering::Release);
        Ok(())
    }

    pub fn statistics(&self) -> &Statistics {
        &self.sender.statistics
    }
    pub fn queued(&self) -> usize {
        self.sender.queued()
    }
    pub fn write(
        &mut self,
        samples: &[i16],
        cancelled: &AtomicBool,
    ) -> Result<WriteTiming, String> {
        let mut waited = self
            .sender
            .write_wait(samples, cancelled)
            .map_err(String::from)?;
        let mut clock_started = false;
        if self.sender.queued() >= 1920 && !self.started {
            self.ready.store(true, Ordering::Release);
            let start = Instant::now();
            while self.sender.statistics.played.load(Ordering::Relaxed) == self.started_played {
                if cancelled.load(Ordering::Acquire) {
                    return Err("Audio startup cancelled".into());
                }
                if start.elapsed() > Duration::from_secs(2) {
                    return Err("Audio device did not start consuming".into());
                }
                thread::sleep(Duration::from_millis(1));
            }
            waited += start.elapsed();
            self.started = true;
            clock_started = true;
        }
        Ok(WriteTiming {
            waited,
            clock_started,
        })
    }
}
impl Drop for Producer {
    fn drop(&mut self) {
        self.control.ended.store(true, Ordering::Release);
        // Drain short sessions, but never start an empty audio stream merely
        // because a core with no PCM output finished. EOF precedes core IO.
        if self.sender.queued() > 0 {
            self.ready.store(true, Ordering::Release);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "opens a real audio device; run with --test-threads=1"]
    fn live_output_switch_failure_recovery_and_queue_ownership() {
        use super::*;
        let sdl = sdl2::init().unwrap();
        let audio = sdl.audio().unwrap();
        let (mut device, mut producer) =
            Device::open_selected(&audio, Some("EmuLuna deliberately missing output")).unwrap();
        assert_eq!(
            device.selected_output(),
            Some("EmuLuna deliberately missing output")
        );
        device.select_output(None).unwrap();
        let stop = AtomicBool::new(false);
        for _ in 0..5 {
            producer.write(&[0; 1600], &stop).unwrap();
        }
        let stats = producer.sender.statistics.clone();
        assert!(Arc::ptr_eq(&stats, &device.statistics()));
        assert!(device
            .select_output(Some("EmuLuna deliberately missing output"))
            .is_err());
        assert_eq!(device.selected_output(), None);
        assert!(device.device.is_some());
        assert!(device.parked.is_none());
        producer.set_paused(true, &stop).unwrap();
        let name = device.available_outputs().first().cloned();
        device.select_output(name.as_deref()).unwrap();
        assert!(producer.control.paused.load(Ordering::Acquire));
        assert!(device
            .handle_event(&sdl2::event::Event::AudioDeviceRemoved {
                timestamp: 0,
                which: 0,
                iscapture: false
            })
            .is_some());
        producer.set_paused(false, &stop).unwrap();
        for _ in 0..5 {
            producer.write(&[0; 1600], &stop).unwrap();
        }
        drop(producer);
        let report = device.finish(true);
        assert_eq!(report.accepted, 8000);
        assert_eq!(report.accepted, report.played + report.discarded);
        assert_eq!(report.pending, 0);
        assert_eq!(report.overflow, 0);
    }

    use super::*;
    #[test]
    fn pause_boundary_discards_old_audio_without_underruns_then_reprimes() {
        let (mut sender, receiver) = channel(4800);
        let ready = Arc::new(AtomicBool::new(true));
        let control = Arc::new(Control::default());
        let mut callback = Callback {
            receiver,
            ready: ready.clone(),
            control: control.clone(),
        };
        sender.write(&[1000, -1000, 2000, -2000]);
        control.paused.store(true, Ordering::Release);
        control.boundary.store(1, Ordering::Release);
        let mut output = [99; 4];
        callback.callback(&mut output);
        assert_eq!(output, [0; 4]);
        assert_eq!(sender.queued(), 0);
        assert_eq!(control.acknowledged.load(Ordering::Acquire), 1);
        assert_eq!(sender.statistics.discarded.load(Ordering::Relaxed), 2);
        assert_eq!(sender.statistics.missing.load(Ordering::Relaxed), 0);
        control.paused.store(false, Ordering::Release);
        sender.write(&[3000, -3000]);
        callback.callback(&mut output);
        assert_eq!(output, [0; 4]);
        assert_eq!(sender.queued(), 1);
        assert_eq!(sender.statistics.missing.load(Ordering::Relaxed), 0);
        sender.write(&vec![1000; 1920 * 2]);
        control.gain.store(0.5f32.to_bits(), Ordering::Relaxed);
        callback.callback(&mut output);
        assert_eq!(output, [1500, -1500, 500, 500]);
        assert_eq!(sender.statistics.played.load(Ordering::Relaxed), 2);
    }
    #[test]
    fn producer_barrier_waits_for_consumer_before_new_audio() {
        let (sender, receiver) = channel(4800);
        let ready = Arc::new(AtomicBool::new(false));
        let control = Arc::new(Control::default());
        let mut callback = Callback {
            receiver,
            ready: ready.clone(),
            control: control.clone(),
        };
        let mut producer = Producer {
            sender,
            ready,
            started: false,
            started_played: 0,
            control,
        };
        producer.sender.write(&[12, 34]);
        let worker = std::thread::spawn(move || {
            producer.set_paused(false, &AtomicBool::new(false)).unwrap();
            producer.sender.write(&[56, 78]);
            producer
        });
        let until = Instant::now() + Duration::from_secs(1);
        while callback.control.acknowledged.load(Ordering::Acquire) == 0 && Instant::now() < until {
            callback.callback(&mut [0; 2]);
            std::thread::yield_now();
        }
        let producer = worker.join().unwrap();
        assert_eq!(producer.sender.queued(), 1);
        assert_eq!(producer.statistics().discarded.load(Ordering::Relaxed), 1);
        callback.ready.store(true, Ordering::Release);
        let mut output = [0; 2];
        callback.callback(&mut output);
        assert_eq!(output, [56, 78]);
    }
}
