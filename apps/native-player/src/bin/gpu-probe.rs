#![forbid(unsafe_code)]
//! Hardware transport diagnostic; counts audio but does not play it.
use emuluna_gl_presenter::{Filter, Presenter};
use std::path::PathBuf;
fn run() -> Result<(), String> {
    let mut args: Vec<_> = std::env::args_os().skip(1).collect();
    let scanlines = if args.last().is_some_and(|s| s == "--scanlines") {
        args.pop();
        true
    } else {
        false
    };
    if args.len() != 6 {
        return Err("Usage: gpu-probe HOST_SO CORE_SO ROM NEW_SAVE_DIR BIOS_DIR FRAMES [--scanlines] (diagnostic: no sound)".into());
    }
    let paths: Vec<_> = args[..5].iter().map(PathBuf::from).collect();
    let frames = args[5]
        .to_str()
        .ok_or("Invalid frames")?
        .parse()
        .map_err(|_| "Invalid frames")?;
    let paths: [PathBuf; 5] = paths.try_into().map_err(|_| "Invalid paths")?;
    let sdl = sdl2::init()?;
    let video = sdl.video()?;
    let mut presenter = Presenter::new(&video)?;
    if scanlines {
        presenter.set_filter(Filter::Scanlines);
    }
    println!("{:?}", presenter.hardware_core_probe(paths, frames)?);
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
