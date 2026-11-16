//! Location selection and staged migration of whole EmuLuna libraries.
use crate::preferences::{self, Preferences};
use emuluna_library_model::Library;
use rusqlite::{Connection, DatabaseName, OpenFlags};
use serde_json::Value;
use std::{
    fs::{self, File},
    io,
    path::{Path, PathBuf},
    time::Duration,
};

pub const FOLDER: &str = "EmuLuna";
const INDEX: &str = "library.sqlite3";
const LAYOUT: &str = "settings/.library-layout";

fn home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from)
}

fn xdg(key: &str, fallback: &str) -> PathBuf {
    std::env::var_os(key)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| home().join(fallback))
}

pub fn default_root() -> PathBuf {
    home().join(FOLDER)
}

pub fn legacy_root() -> Option<PathBuf> {
    let data = xdg("XDG_DATA_HOME", ".local/share");
    let preferred = data.join("emuluna");
    if preferred.join(INDEX).is_file() {
        return Some(preferred);
    }

    // An earlier Rust build identified each library by its canonical path.
    // Use that saved association to rediscover a library in its old folder.
    let progress = legacy_settings().join("progress");
    if !progress.is_dir() {
        return None;
    }
    let mut candidates = fs::read_dir(data)
        .ok()?
        .take(512)
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.join(INDEX).is_file() && !managed(path))
        .filter_map(|path| path.canonicalize().ok())
        .filter(|path| preferences::library_progress_root(path, &progress).is_dir());
    let root = candidates.next()?;
    candidates.next().is_none().then_some(root)
}

pub fn legacy_settings() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config").join("emuluna-native")
}

fn pointer() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config").join("emuluna/library-location.json")
}

pub fn remembered_root() -> Result<Option<PathBuf>, String> {
    match fs::read(pointer()) {
        Ok(bytes) => {
            let root: PathBuf = serde_json::from_slice(&bytes).map_err(error)?;
            if !root.is_absolute() {
                return Err("The saved library location is invalid.".into());
            }
            Ok(Some(root))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(error(e)),
    }
}

pub fn remember(root: &Path) -> Result<(), String> {
    let root = root.canonicalize().map_err(error)?;
    crate::settings::tools::atomic_json(&pointer(), &serde_json::to_value(root).map_err(error)?)
}

pub fn preferences_path(root: &Path) -> PathBuf {
    root.join("settings/settings.json")
}

pub fn managed(root: &Path) -> bool {
    root.join(LAYOUT).is_file()
}

fn managed_root(preferences: &Path) -> Option<&Path> {
    let root = preferences.parent()?.parent()?;
    (preferences == preferences_path(root) && managed(root)).then_some(root)
}

pub fn themes(preferences: &Path) -> PathBuf {
    managed_root(preferences)
        .unwrap_or_else(|| preferences.parent().unwrap_or(Path::new(".")))
        .join("themes")
}

pub fn cache(root: &Path) -> PathBuf {
    root.join(if managed(root) { ".cache" } else { "cache" })
}

pub fn metadata(root: &Path) -> PathBuf {
    if managed(root) {
        cache(root).join("metadata")
    } else {
        root.join("metadata")
    }
}

pub fn cover_cache(preferences: &Path) -> PathBuf {
    managed_root(preferences)
        .map_or_else(
            || preferences.parent().unwrap_or(Path::new(".")).join("cache"),
            cache,
        )
        .join("cover-previews")
}

pub fn converted_artwork(root: &Path, preferences: &Path) -> PathBuf {
    if managed_root(preferences) == Some(root) {
        cache(root).join("artwork")
    } else {
        preferences::library_progress_root(
            root,
            &preferences
                .parent()
                .unwrap_or(Path::new("."))
                .join("artwork"),
        )
    }
}

fn console_storage_is_link(root: &Path) -> bool {
    root.join("consoles").is_symlink()
}

pub fn native_media(
    root: &Path,
    legacy: &Path,
    system: &str,
    game: &str,
    core: &str,
    kind: &str,
) -> PathBuf {
    if legacy == root.join("settings/progress") && managed(root) {
        if console_storage_is_link(root) {
            return root
                .join("settings/progress")
                .join(game)
                .join(core)
                .join(kind);
        }
        return root
            .join("consoles")
            .join(system)
            .join(kind)
            .join(game)
            .join("native")
            .join(core);
    }
    legacy.join(game).join(core).join(kind)
}

pub fn native_fallback(root: &Path, legacy: &Path) -> PathBuf {
    if legacy == root.join("settings/progress") && managed(root) {
        legacy.to_owned()
    } else {
        preferences::library_progress_root(root, legacy)
    }
}

fn error(value: impl std::fmt::Display) -> String {
    value.to_string()
}

// Lock the actual index inode for cooperating EmuLuna processes. SQLite uses
// separate locks for its own transactions.
pub fn lock(root: &Path) -> Result<File, String> {
    let file = File::open(root.join(INDEX))
        .map_err(|e| format!("Could not open the library at {}: {e}", root.display()))?;
    file.try_lock().map_err(|e| {
        format!(
            "Close any other EmuLuna window using {}: {e}",
            root.display()
        )
    })?;
    Ok(file)
}

fn layout(root: &Path) -> Result<(), String> {
    for name in [
        "consoles", "system", "covers", "themes", "settings", "cores", "backups", ".cache",
    ] {
        let dir = root.join(name);
        if dir.is_symlink() {
            if name == "consoles" {
                continue;
            }
            return Err(format!("The {name} folder is a symbolic link. Choose a regular folder for it before migrating."));
        }
        fs::create_dir_all(dir).map_err(error)?;
    }
    let marker = root.join(LAYOUT);
    if marker.is_symlink() {
        return Err("The layout marker is a symbolic link.".into());
    }
    fs::write(marker, b"1\n").map_err(error)
}

fn destination_path(path: &Path) -> Result<PathBuf, String> {
    let name = path
        .file_name()
        .ok_or("Choose a folder name for the library.")?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    Ok(parent.canonicalize().map_err(error)?.join(name))
}

fn publish(
    destination: &Path,
    prepare: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<PathBuf, String> {
    let destination = destination_path(destination)?;
    match fs::symlink_metadata(&destination) {
        Ok(_) => {
            return Err(format!(
                "{} already exists. Choose another location or open that library.",
                destination.display()
            ))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(error(e)),
    }
    let stage = tempfile::Builder::new()
        .prefix(".emuluna-setup-")
        .tempdir_in(destination.parent().unwrap())
        .map_err(error)?;
    prepare(stage.path())?;
    Library::load(stage.path())?;
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        stage.path(),
        rustix::fs::CWD,
        &destination,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(|e| format!("Could not publish {}: {e}", destination.display()))?;
    Ok(destination)
}

pub fn create(destination: &Path, prefs: &Preferences, copy: bool) -> Result<PathBuf, String> {
    publish(destination, |stage| {
        layout(stage)?;
        let db = Connection::open(stage.join(INDEX)).map_err(error)?;
        db.execute_batch(include_str!("library_schema.sql"))
            .map_err(error)?;
        db.execute(
            "INSERT INTO settings(key,value) VALUES('copy_games',?1)",
            [if copy { "1" } else { "0" }],
        )
        .map_err(error)?;
        prefs.save(&preferences_path(stage))
    })
}

struct Rebase<'a> {
    from: &'a Path,
    to: &'a Path,
    settings: Option<&'a Path>,
}

impl Rebase<'_> {
    fn path(&self, path: &Path) -> Option<PathBuf> {
        if let Ok(relative) = path.strip_prefix(self.from) {
            let mut parts = relative.components();
            if !managed(self.from) {
                match parts.next().and_then(|part| part.as_os_str().to_str()) {
                    Some("cache") if !self.from.join(".cache").exists() => {
                        return Some(self.to.join(".cache").join(parts.as_path()))
                    }
                    Some("metadata")
                        if !self.from.join(".cache/metadata").exists()
                            && !self.from.join("cache/metadata").exists() =>
                    {
                        return Some(self.to.join(".cache").join(relative))
                    }
                    _ => {}
                }
            }
            return Some(self.to.join(relative));
        }
        self.settings
            .and_then(|root| path.strip_prefix(root).ok())
            .map(|p| {
                if p.starts_with("themes") {
                    self.to.join(p)
                } else if let Ok(p) = p.strip_prefix("cache") {
                    self.to.join(".cache").join(p)
                } else {
                    self.to.join("settings").join(p)
                }
            })
    }

    fn text(&self, value: &str) -> Option<String> {
        self.path(Path::new(value))
            .map(|p| p.to_string_lossy().into_owned())
    }
}

fn copy_tree(source: &Path, target: &Path, paths: &Rebase<'_>) -> Result<(), String> {
    let meta = fs::symlink_metadata(source).map_err(error)?;
    if meta.is_symlink() {
        let link = fs::read_link(source).map_err(error)?;
        let target_path = if link.is_absolute() {
            paths.path(&link).unwrap_or(link)
        } else {
            // Relative external links need an absolute target when the
            // library moves to a different parent or drive.
            let absolute = source.parent().unwrap().join(&link);
            let resolved = absolute.canonicalize().unwrap_or_else(|_| {
                let mut normalized = PathBuf::new();
                for component in absolute.components() {
                    if component == std::path::Component::ParentDir {
                        normalized.pop();
                    } else {
                        normalized.push(component.as_os_str());
                    }
                }
                normalized
            });
            if resolved.starts_with(paths.from) {
                link
            } else {
                resolved
            }
        };
        std::os::unix::fs::symlink(target_path, target).map_err(error)
    } else if meta.is_dir() {
        fs::create_dir(target).map_err(error)?;
        for entry in fs::read_dir(source).map_err(error)? {
            let entry = entry.map_err(error)?;
            copy_tree(&entry.path(), &target.join(entry.file_name()), paths)?;
        }
        Ok(())
    } else if meta.is_file() {
        let copied = fs::copy(source, target).map_err(error)?;
        let after = fs::metadata(source).map_err(error)?;
        if copied != meta.len()
            || after.len() != meta.len()
            || after.modified().ok() != meta.modified().ok()
        {
            return Err(format!(
                "{} changed while it was being copied. Close EmuLuna and retry.",
                source.display()
            ));
        }
        Ok(())
    } else {
        Err(format!(
            "Cannot copy the special file {}.",
            source.display()
        ))
    }
}

fn rebase_index(db: &mut Connection, paths: &Rebase<'_>) -> Result<(), String> {
    let tx = db.transaction().map_err(error)?;
    for (table, column) in [
        ("games", "rom_path"),
        ("games", "source"),
        ("games", "cover"),
        ("game_files", "path"),
        ("settings", "value"),
        ("import_issues", "path"),
    ] {
        let exists: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info(?1) WHERE name=?2)",
                [table, column],
                |row| row.get(0),
            )
            .map_err(error)?;
        if !exists {
            continue;
        }
        let changes = {
            let mut query = tx
                .prepare(&format!(
                    "SELECT rowid,{column} FROM {table} WHERE {column} LIKE '/%'"
                ))
                .map_err(error)?;
            let rows = query
                .query_map([], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(error)?;
            let mut changes = Vec::new();
            for row in rows {
                let (id, value) = row.map_err(error)?;
                if let Some(value) = paths.text(&value) {
                    changes.push((id, value));
                }
            }
            changes
        };
        for (id, value) in changes {
            tx.execute(
                &format!("UPDATE {table} SET {column}=?1 WHERE rowid=?2"),
                rusqlite::params![value, id],
            )
            .map_err(error)?;
        }
    }
    tx.commit().map_err(error)
}

fn rebase_json(path: &Path, paths: &Rebase<'_>) -> Result<(), String> {
    fn visit(value: &mut Value, paths: &Rebase<'_>) -> bool {
        match value {
            Value::String(s) => paths.text(s).is_some_and(|new| {
                *s = new;
                true
            }),
            Value::Array(items) => items
                .iter_mut()
                .fold(false, |changed, item| visit(item, paths) || changed),
            Value::Object(items) => items
                .values_mut()
                .fold(false, |changed, item| visit(item, paths) || changed),
            _ => false,
        }
    }
    if path.is_file() && !path.is_symlink() {
        let mut json: Value =
            serde_json::from_slice(&fs::read(path).map_err(error)?).map_err(error)?;
        if visit(&mut json, paths) {
            crate::settings::tools::atomic_json(path, &json)?;
        }
    }
    Ok(())
}

fn migrate_native(stage: &Path, paths: &Rebase<'_>, library: &Library) -> Result<(), String> {
    let Some(config) = paths.settings else {
        return Ok(());
    };
    let old = preferences::library_progress_root(paths.from, &config.join("progress"));
    if old.is_dir() {
        let retained = stage.join("backups/previous-native-progress");
        copy_tree(&old, &retained, paths)?;
        for game in &library.games {
            if game.id.len() != 64
                || !game.id.bytes().all(|b| b.is_ascii_hexdigit())
                || game.system.is_empty()
                || !game
                    .system
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
            {
                continue;
            }
            let dir = retained.join(&game.id);
            if !fs::symlink_metadata(&dir).is_ok_and(|m| m.is_dir()) {
                continue;
            }
            for core in fs::read_dir(dir).map_err(error)? {
                let core = core.map_err(error)?;
                if !core.file_type().map_err(error)?.is_dir() {
                    continue;
                }
                for kind in ["saves", "states", "screenshots"] {
                    let source = core.path().join(kind);
                    if !fs::symlink_metadata(&source).is_ok_and(|m| m.is_dir()) {
                        continue;
                    }
                    let target = native_media(
                        stage,
                        &stage.join("settings/progress"),
                        &game.system,
                        &game.id,
                        &core.file_name().to_string_lossy(),
                        kind,
                    );
                    // Never write through a symlink from the old library or overwrite existing media.
                    if target
                        .ancestors()
                        .take_while(|p| *p != stage)
                        .any(Path::is_symlink)
                        || target.exists()
                    {
                        return Err(format!(
                            "Existing native media at {} prevents a safe migration.",
                            target.strip_prefix(stage).unwrap_or(&target).display()
                        ));
                    }
                    fs::create_dir_all(target.parent().unwrap()).map_err(error)?;
                    fs::rename(source, target).map_err(error)?;
                }
            }
        }
    }

    let converted = preferences::library_progress_root(paths.from, &config.join("artwork"));
    if converted.is_dir() && !stage.join(".cache/artwork").exists() {
        copy_tree(&converted, &stage.join(".cache/artwork"), paths)?;
    }
    let mut prefs = Preferences::load(&config.join("settings.json"))?;
    for value in prefs.values.values_mut() {
        if let Some(new) = paths.text(value) {
            *value = new;
        }
    }
    prefs.save(&preferences_path(stage))?;
    let themes = config.join("themes");
    if themes.is_dir() {
        for entry in fs::read_dir(themes).map_err(error)? {
            let entry = entry.map_err(error)?;
            let target = stage.join("themes").join(entry.file_name());
            if fs::symlink_metadata(&target).is_ok() {
                // Retain a conflicting legacy theme separately.
                let backups = stage.join("backups/previous-native-themes");
                fs::create_dir_all(&backups).map_err(error)?;
                copy_tree(&entry.path(), &backups.join(entry.file_name()), paths)?;
            } else {
                copy_tree(&entry.path(), &target, paths)?;
            }
        }
    }
    Ok(())
}

pub fn migrate(
    source: &Path,
    destination: &Path,
    old_settings: Option<&Path>,
    held_lock: Option<File>,
) -> Result<PathBuf, String> {
    let source = source.canonicalize().map_err(error)?;
    let destination = destination_path(destination)?;
    if destination.starts_with(&source) {
        return Err("Choose a location outside the current library.".into());
    }
    let _lock = match held_lock {
        Some(file) => file,
        None => lock(&source)?,
    };
    let library = Library::load(&source)?;
    let paths = Rebase {
        from: &source,
        to: &destination,
        settings: old_settings,
    };
    publish(&destination, |stage| {
        let mut entries = fs::read_dir(&source)
            .map_err(error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(error)?;
        entries.sort_by_key(|entry| entry.file_name() == "metadata");
        for entry in entries {
            let name = entry.file_name();
            if matches!(
                name.to_str(),
                Some(
                    INDEX
                        | "library.sqlite3-shm"
                        | "library.sqlite3-wal"
                        | "library.sqlite3-journal"
                )
            ) {
                continue;
            }
            let source_path = entry.path();
            let mapped = paths
                .path(&source_path)
                .unwrap_or_else(|| destination.join(&name));
            let target = stage.join(mapped.strip_prefix(&destination).map_err(error)?);
            let parent = target.parent().unwrap();
            if parent != stage && parent.is_symlink() {
                return Err("A linked cache folder prevents safe migration.".into());
            }
            fs::create_dir_all(parent).map_err(error)?;
            copy_tree(&entry.path(), &target, &paths)?;
        }
        let src = Connection::open_with_flags(source.join(INDEX), OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(error)?;
        src.busy_timeout(Duration::from_secs(5)).map_err(error)?;
        src.backup(DatabaseName::Main, stage.join(INDEX), None)
            .map_err(error)?;
        let mut db = Connection::open(stage.join(INDEX)).map_err(error)?;
        rebase_index(&mut db, &paths)?;
        let check: String = db
            .query_row("PRAGMA quick_check", [], |row| row.get(0))
            .map_err(error)?;
        if check != "ok" {
            return Err(format!(
                "The copied index did not pass its integrity check: {check}"
            ));
        }
        layout(stage)?;
        for name in [
            "cores/installed.json",
            "cores/history.json",
            "console-location.json",
            "settings/settings.json",
        ] {
            rebase_json(&stage.join(name), &paths)?;
        }
        if !console_storage_is_link(stage) {
            for console in &library.consoles {
                let directory = stage.join("consoles").join(&console.id);
                if !directory.is_symlink() {
                    rebase_json(&directory.join("games.json"), &paths)?;
                }
            }
        }
        migrate_native(stage, &paths, &library)?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_keeps_games_settings_media_and_external_paths() {
        let sandbox = tempfile::tempdir().unwrap();
        let source = sandbox.path().join("previous-library");
        let config = sandbox.path().join("emuluna-native");
        let target = sandbox.path().join("EmuLuna");
        fs::create_dir(&source).unwrap();
        let game = "a".repeat(64);
        let external = "b".repeat(64);
        let managed_rom = source.join("consoles/nes/roms/game.nes");
        let external_rom = sandbox.path().join("Drive/game.nes");
        let cover = source.join("covers/game.webp");
        let bios = source.join("system/custom");
        for file in [&managed_rom, &external_rom, &cover, &bios.join("bios.bin")] {
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, b"fixture").unwrap();
        }

        let db = Connection::open(source.join(INDEX)).unwrap();
        db.execute_batch(include_str!("library_schema.sql"))
            .unwrap();
        db.execute(
            "INSERT INTO games(id,title,system,rom_path,source,added,cover,rating,favorite)
             VALUES(?1,'Local','nes',?2,?2,0,?3,4,1)",
            rusqlite::params![game, managed_rom.to_str(), cover.to_str()],
        )
        .unwrap();
        db.execute(
            "INSERT INTO games(id,title,system,rom_path,source,added)
             VALUES(?1,'External','nes',?2,?2,0)",
            rusqlite::params![external, external_rom.to_str()],
        )
        .unwrap();
        db.execute_batch(
            "INSERT INTO collections VALUES (1,'Example',NULL,0);
             INSERT INTO collection_games SELECT 1,id FROM games WHERE title='Local';",
        )
        .unwrap();
        db.execute(
            "INSERT INTO settings VALUES ('bios_directory',?1)",
            [bios.to_str()],
        )
        .unwrap();

        let mut prefs = Preferences::default();
        prefs.set("theme", "classic");
        prefs.set(&format!("game.{game}.rom"), managed_rom.to_str().unwrap());
        prefs.save(&config.join("settings.json")).unwrap();
        let themes = config.join("themes/custom");
        fs::create_dir_all(&themes).unwrap();
        fs::write(themes.join("theme.json"), b"{}").unwrap();
        let native = preferences::library_progress_root(&source, &config.join("progress"))
            .join(&game)
            .join("nestopia");
        for (kind, filename) in [
            ("saves", "battery.srm"),
            ("states", "revision/auto.oesavestate"),
            ("screenshots", "test.png"),
        ] {
            let file = native.join(kind).join(filename);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, kind).unwrap();
        }

        let locked = lock(&source).unwrap();
        assert!(migrate(&source, &target, Some(&config), None).is_err());
        assert!(!target.exists());
        drop(locked);
        // Keep the SQLite connection alive to exercise the WAL snapshot.
        let root = migrate(&source, &target, Some(&config), None).unwrap();
        let library = Library::load(&root).unwrap();
        assert!(managed(&root));
        assert_eq!(library.games.len(), 2);
        let local = library.games.iter().find(|g| g.id == game).unwrap();
        assert_eq!(local.rom, root.join("consoles/nes/roms/game.nes"));
        assert_eq!(local.cover, Some(root.join("covers/game.webp")));
        assert_eq!(local.rating, 4);
        assert!(library.collections[0].games.contains(&game));
        assert_eq!(
            library.games.iter().find(|g| g.id == external).unwrap().rom,
            external_rom
        );
        assert_eq!(
            library.settings["bios_directory"],
            root.join("system/custom").to_str().unwrap()
        );
        let prefs = Preferences::load(&preferences_path(&root)).unwrap();
        assert_eq!(prefs.get("theme"), Some("classic"));
        assert_eq!(prefs.get(&format!("game.{game}.rom")), local.rom.to_str());
        assert!(root.join("themes/custom/theme.json").is_file());
        assert!(root.join("system/custom/bios.bin").is_file());
        assert!(managed_rom.is_file());
        assert!(native.join("saves/battery.srm").is_file());
        for (kind, name) in [
            ("saves", "battery.srm"),
            ("states", "revision/auto.oesavestate"),
            ("screenshots", "test.png"),
        ] {
            let file = native_media(
                &root,
                &root.join("settings/progress"),
                "nes",
                &game,
                "nestopia",
                kind,
            )
            .join(name);
            assert_eq!(fs::read_to_string(file).unwrap(), kind);
        }
        assert!(migrate(&source, &target, Some(&config), None).is_err());

        let second = sandbox.path().join("Moved");
        let second = migrate(&root, &second, None, None).unwrap();
        let library = Library::load(&second).unwrap();
        assert_eq!(
            library.games.iter().find(|g| g.id == game).unwrap().rom,
            second.join("consoles/nes/roms/game.nes")
        );
        assert!(second
            .join(format!(
                "consoles/nes/states/{game}/native/nestopia/revision/auto.oesavestate"
            ))
            .is_file());
    }

    #[test]
    fn creates_an_empty_library_without_overwriting_an_existing_directory() {
        let parent = tempfile::tempdir().unwrap();
        let path = parent.path().join(FOLDER);
        let root = create(&path, &Preferences::default(), false).unwrap();
        let library = Library::load(&root).unwrap();
        assert!(library.games.is_empty());
        assert!(!library.consoles.is_empty());
        assert_eq!(library.settings["copy_games"], "0");
        assert!(create(&root, &Preferences::default(), true).is_err());
        assert_eq!(Library::load(&root).unwrap().settings["copy_games"], "0");
    }
}
