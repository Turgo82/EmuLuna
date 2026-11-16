//! KDE Wayland can restore its own minimized windows through KWin scripting.
use std::{
    io::Write,
    process::{Command, Stdio},
    sync::OnceLock,
};

pub fn wayland() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
        && !std::env::var("WINIT_UNIX_BACKEND").is_ok_and(|v| v.eq_ignore_ascii_case("x11"))
}
pub fn available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        wayland()
            && std::env::var("XDG_CURRENT_DESKTOP")
                .is_ok_and(|d| d.split(':').any(|s| s.eq_ignore_ascii_case("KDE")))
            && call(&[
                "introspect",
                "--session",
                "--dest",
                "org.kde.KWin",
                "--object-path",
                "/Scripting",
            ])
            .is_ok_and(|output| output.contains("loadScript"))
    })
}
fn call(args: &[&str]) -> Result<String, String> {
    let result = Command::new("gdbus")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    if result.status.success() {
        Ok(String::from_utf8_lossy(&result.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&result.stderr).trim().to_owned())
    }
}
fn script(pid: u32) -> String {
    format!("for (const w of workspace.windowList()) {{ if (w.pid === {pid} && String(w.resourceClass).toLowerCase() === 'emuluna' && String(w.caption).startsWith('EmuLuna — Native Preview')) {{ w.minimized = false; workspace.activeWindow = w; break; }} }}")
}
pub fn restore() -> Result<(), String> {
    let mut file = tempfile::Builder::new()
        .prefix("emuluna-kwin-")
        .suffix(".js")
        .tempfile()
        .map_err(|e| e.to_string())?;
    file.write_all(script(std::process::id()).as_bytes())
        .map_err(|e| e.to_string())?;
    let name = format!("emuluna-restore-{}", std::process::id());
    let path = file.path().to_str().ok_or("Invalid KWin script path")?;
    let output = call(&[
        "call",
        "--session",
        "--dest",
        "org.kde.KWin",
        "--object-path",
        "/Scripting",
        "--method",
        "org.kde.kwin.Scripting.loadScript",
        path,
        &name,
    ])?;
    let id = output
        .trim()
        .trim_start_matches('(')
        .split(',')
        .next()
        .and_then(|s| s.trim().parse::<i32>().ok())
        .filter(|id| *id >= 0)
        .ok_or("KWin rejected the restore script")?;
    let object = format!("/Scripting/Script{id}");
    let result = call(&[
        "call",
        "--session",
        "--dest",
        "org.kde.KWin",
        "--object-path",
        &object,
        "--method",
        "org.kde.kwin.Script.run",
    ]);
    let _ = call(&[
        "call",
        "--session",
        "--dest",
        "org.kde.KWin",
        "--object-path",
        "/Scripting",
        "--method",
        "org.kde.kwin.Scripting.unloadScript",
        &name,
    ]);
    result.map(|_| ())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn script_targets_only_library_owned_by_current_process() {
        let s = script(12345);
        assert!(s.contains("w.pid === 12345"));
        assert!(s.contains("'emuluna'"));
        assert!(s.contains("EmuLuna — Native Preview"));
        assert!(s.contains("w.minimized = false"));
    }
}
