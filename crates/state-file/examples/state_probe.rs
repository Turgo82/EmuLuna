//! Developer format interoperability check, never called by native playback.
use emuluna_state_file::{self as states, Identity, Renderer};
use std::{fs::File, io, path::Path};
fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 7 {
        return Err("Usage: state_probe wrap|verify INPUT NEW_OUTPUT CORE_ID BINARY_SHA ROM_SHA software|opengl|vulkan".into());
    }
    let renderer = match args[6].as_str() {
        "software" => Renderer::Software,
        "opengl" => Renderer::OpenGl,
        "vulkan" => Renderer::Vulkan,
        _ => return Err("Invalid renderer".into()),
    };
    let identity = Identity::new(&args[3], &args[4], &args[5], renderer)?;
    let input = Path::new(&args[1]);
    let output = Path::new(&args[2]);
    if output.exists() {
        return Err("Output must be a new file".into());
    }
    match args[0].as_str() {
        "wrap" => states::write_from_raw(output, input, &identity)?,
        "verify" => states::with_payload(input, &identity, |raw| {
            let mut source = File::open(raw).map_err(|e| e.to_string())?;
            let mut destination = File::create_new(output).map_err(|e| e.to_string())?;
            io::copy(&mut source, &mut destination).map_err(|e| e.to_string())?;
            Ok(())
        })?,
        _ => return Err("Invalid state operation".into()),
    }
    println!("STATE_PROBE_OK");
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
