//! Diagnostic runner, not the graphical player. No device audio or GPU presentation yet.
use emuluna_libretro_host::{Core, Frame};
use std::{
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 6 {
        return Err(
            "Usage: emuluna-player-probe HOST_SO CORE_SO ROM TEMP_SAVE_DIR BIOS_DIR FRAMES".into(),
        );
    }
    let paths: Vec<_> = args[..5].iter().map(PathBuf::from).collect();
    let count: u32 = args[5]
        .to_str()
        .ok_or("Invalid frame count")?
        .parse()
        .map_err(|_| "Invalid frame count")?;
    if count == 0 || count > 36000 {
        return Err("Frame count must be 1..36000".into());
    }
    // Explicit disposable directory prevents accidentally modifying an existing library/save directory.
    std::fs::create_dir(&paths[3]).map_err(|e| format!("Save directory must be new: {e}"))?;
    let worker = thread::spawn(move || -> Result<(), String> {
        let mut core = Core::open(&paths[0], &paths[1], &paths[2], &paths[3], &paths[4])?;
        let start = Instant::now();
        let mut deadline = start;
        let mut samples = 0usize;
        let mut work = Duration::ZERO;
        let mut dimensions = (0, 0);
        let mut frame = Frame::default();
        let mut growth_after_warmup = 0;
        let mut executed = 0;
        for index in 0..count {
            let capacity = (frame.pixels.capacity(), frame.audio.capacity());
            let before = Instant::now();
            if core.run_into(0, &mut frame)? == emuluna_libretro_host::FrameOutcome::Shutdown {
                break;
            }
            executed += 1;
            if index >= 10 && capacity != (frame.pixels.capacity(), frame.audio.capacity()) {
                growth_after_warmup += 1;
            }
            work += before.elapsed();
            samples += frame.audio.len() / 2;
            dimensions = (frame.width, frame.height);
            deadline += Duration::from_secs_f64(1.0 / core.fps()?);
            if let Some(wait) = deadline.checked_duration_since(Instant::now()) {
                thread::sleep(wait);
            }
        }
        println!("frames={executed} requested_frames={count} elapsed_ms={:.2} mean_run_and_snapshot_ms={:.3} audio_frames={samples} size={}x{} buffer_growth_after_warmup={growth_after_warmup}",start.elapsed().as_secs_f64()*1000.,work.as_secs_f64()*1000./executed.max(1) as f64,dimensions.0,dimensions.1);
        Ok(())
    });
    worker.join().map_err(|_| "Core worker panicked")?
}
