use std::{
    fs,
    io::Read,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::PathBuf,
    process::{Command, Stdio},
    sync::Mutex,
};

static RESTART_UPDATE: Mutex<Option<PreparedUpdate>> = Mutex::new(None);

pub(super) struct PreparedUpdate {
    image: PathBuf,
    identity: (u64, u64),
    download: tempfile::NamedTempFile,
}

pub(super) fn current_image() -> Option<PathBuf> {
    let image = fs::canonicalize(std::env::var_os("APPIMAGE")?).ok()?;
    image.is_file().then_some(image)
}

fn download(url: &str, limit: u64, destination: &fs::File) -> Result<(), String> {
    let status = Command::new("curl")
        .args([
            "--disable",
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--connect-timeout",
            "10",
            "--max-time",
            "600",
            "--max-filesize",
        ])
        .arg(limit.to_string())
        .arg("--")
        .arg(url)
        .stdin(Stdio::null())
        .stdout(destination.try_clone().map_err(error)?)
        .status()
        .map_err(error)?;
    if !status.success() {
        return Err(format!("Could not download the update ({status})."));
    }
    if destination.metadata().map_err(error)?.len() > limit {
        return Err("The update download exceeded its size limit.".into());
    }
    Ok(())
}

pub(super) fn prepare(
    image: PathBuf,
    url: &str,
    checksum_url: &str,
) -> Result<PreparedUpdate, String> {
    let metadata = fs::metadata(&image).map_err(error)?;
    let parent = image
        .parent()
        .ok_or("The AppImage folder is unavailable.")?;
    let mut checksum = tempfile::tempfile().map_err(error)?;
    download(checksum_url, 4096, &checksum)?;
    std::io::Seek::rewind(&mut checksum).map_err(error)?;
    let mut manifest = String::new();
    checksum
        .take(4096)
        .read_to_string(&mut manifest)
        .map_err(error)?;
    let expected = manifest
        .split_whitespace()
        .next()
        .filter(|hash| hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or("The release checksum is invalid.")?;
    let mut staged = tempfile::NamedTempFile::new_in(parent).map_err(error)?;
    download(url, 1024 * 1024 * 1024, staged.as_file())?;
    if !emuluna_state_file::digest_file(staged.path())?.eq_ignore_ascii_case(expected) {
        return Err("The downloaded AppImage does not match its checksum.".into());
    }
    std::io::Seek::rewind(staged.as_file_mut()).map_err(error)?;
    let mut header = [0; 20];
    staged.read_exact(&mut header).map_err(error)?;
    if &header[..6] != b"\x7fELF\x02\x01"
        || &header[8..11] != b"AI\x02"
        || header[18..20] != [0x3e, 0]
    {
        return Err("The download is not a Linux x86-64 AppImage.".into());
    }
    staged
        .as_file()
        .set_permissions(fs::Permissions::from_mode(metadata.mode() & 0o777))
        .map_err(error)?;
    staged.as_file().sync_all().map_err(error)?;
    Ok(PreparedUpdate {
        image,
        identity: (metadata.dev(), metadata.ino()),
        download: staged,
    })
}

pub(super) fn queue_restart(update: PreparedUpdate) {
    *RESTART_UPDATE.lock().unwrap() = Some(update);
}

pub(crate) fn restart_if_ready() -> Result<(), String> {
    let Some(update) = RESTART_UPDATE.lock().unwrap().take() else {
        return Ok(());
    };
    let metadata = fs::symlink_metadata(&update.image).map_err(error)?;
    if !metadata.is_file() || (metadata.dev(), metadata.ino()) != update.identity {
        return Err("The original AppImage changed while the update was downloading.".into());
    }
    let parent = update
        .image
        .parent()
        .ok_or("The AppImage folder is unavailable.")?;
    let backup = tempfile::tempdir_in(parent).map_err(error)?;
    let previous = backup.path().join("previous.AppImage");
    if fs::hard_link(&update.image, &previous).is_err() {
        fs::copy(&update.image, &previous).map_err(error)?;
    }
    drop(update.download.persist(&update.image).map_err(error)?);

    let mut command = Command::new(&update.image);
    command.args(std::env::args_os().skip(1));
    for name in ["APPIMAGE", "APPDIR", "ARGV0", "OWD"] {
        command.env_remove(name);
    }
    if let (Some(app_dir), Some(library_path)) = (
        std::env::var_os("APPDIR"),
        std::env::var_os("LD_LIBRARY_PATH"),
    ) {
        let app_dir = PathBuf::from(app_dir);
        let paths = std::env::split_paths(&library_path).filter(|path| !path.starts_with(&app_dir));
        if let Ok(paths) = std::env::join_paths(paths) {
            if paths.is_empty() {
                command.env_remove("LD_LIBRARY_PATH");
            } else {
                command.env("LD_LIBRARY_PATH", paths);
            }
        }
        if app_dir
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("appimage_extracted_"))
        {
            command.env("APPIMAGE_EXTRACT_AND_RUN", "1");
        }
    }
    match command.spawn() {
        Ok(_) => Ok(()),
        Err(launch) => {
            fs::rename(previous, &update.image).map_err(|rollback| {
                format!("Could not restart EmuLuna: {launch}. Could not restore the previous AppImage: {rollback}.")
            })?;
            Err(format!(
                "Could not restart EmuLuna: {launch}. The previous AppImage was restored."
            ))
        }
    }
}

fn error(error: impl std::fmt::Display) -> String {
    error.to_string()
}
