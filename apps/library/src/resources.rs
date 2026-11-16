use std::path::{Path, PathBuf};

pub fn path(relative: &str) -> PathBuf {
    if let Ok(executable) = std::env::current_exe() {
        if let Some(root) = executable.parent().and_then(Path::parent) {
            if root.join(".emuluna-appimage").is_file() {
                return root.join(relative);
            }
        }
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}
