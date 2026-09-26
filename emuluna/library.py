"""SQLite game library, imports and persistent user preferences."""
import hashlib
import json
import os
from pathlib import Path
import sqlite3
import shutil
import tempfile
import time
import zipfile
import zlib

MAX_ROM = 512 * 1024 * 1024


class ImportProblem(ValueError):
    def __init__(self, code, message):
        super().__init__(message)
        self.code = code


from .systems import SYSTEMS, EXTENSIONS, EXTENSION_SYSTEMS
from .content import DISC_FILES, inventory, content_id, copy_content
from .disc_detection import companion_cue, detect_disc, probe_track


def default_data_dir():
    base = Path(os.environ.get("XDG_DATA_HOME", Path.home() / ".local/share"))
    preferred = base / "emuluna"
    legacy = base / "openemu-linux"
    # Reuse the user's existing library in place; never silently move ROMs/saves.
    if not (preferred / "library.sqlite3").exists() and (legacy / "library.sqlite3").is_file():
        return legacy
    return preferred


class Library:
    def __init__(self, root=None):
        self.root = Path(root or os.environ.get("EMULUNA_DATA_DIR") or os.environ.get("OPENEMU_DATA_DIR") or default_data_dir()).resolve()
        for folder in ["roms", "saves", "states", "covers", "screenshots"]:
            (self.root / folder).mkdir(parents=True, exist_ok=True)
        self.db = sqlite3.connect(self.root / "library.sqlite3", timeout=10)
        self.db.row_factory = sqlite3.Row
        self.db.execute("PRAGMA foreign_keys=ON")
        self.db.execute("PRAGMA journal_mode=WAL")
        self.db.execute("""CREATE TABLE IF NOT EXISTS games (
            id TEXT PRIMARY KEY, title TEXT NOT NULL, system TEXT NOT NULL,
            rom_path TEXT NOT NULL, source TEXT NOT NULL, added REAL NOT NULL,
            last_played REAL, play_count INTEGER NOT NULL DEFAULT 0,
            favorite INTEGER NOT NULL DEFAULT 0, cover TEXT)""")
        if "cover_revision" not in {row[1] for row in self.db.execute("PRAGMA table_info(games)")}:
            self.db.execute("ALTER TABLE games ADD COLUMN cover_revision INTEGER NOT NULL DEFAULT 0")
        columns = {row[1] for row in self.db.execute("PRAGMA table_info(games)")}
        for column in ("original_filename", "previous_rom_stem"):
            if column not in columns:
                self.db.execute(f"ALTER TABLE games ADD COLUMN {column} TEXT")
        self.db.execute("""CREATE TABLE IF NOT EXISTS artwork (
            game_id TEXT PRIMARY KEY, status TEXT NOT NULL, checked REAL NOT NULL,
            retry_after REAL NOT NULL, source_url TEXT, message TEXT NOT NULL DEFAULT '')""")
        self.db.execute("CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)")
        self.db.commit()
        self.migrate()

    def migrate(self):
        """Additive, transactional migrations; existing ROMs and saves stay in place."""
        self.db.execute("CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY, applied REAL NOT NULL)")
        migrations = {1: [
            "ALTER TABLE games ADD COLUMN rating INTEGER NOT NULL DEFAULT 0 CHECK(rating BETWEEN 0 AND 5)",
            "CREATE TABLE collections (id INTEGER PRIMARY KEY, name TEXT NOT NULL, rules TEXT, created REAL NOT NULL)",
            "CREATE TABLE collection_games (collection_id INTEGER REFERENCES collections(id) ON DELETE CASCADE, game_id TEXT REFERENCES games(id) ON DELETE CASCADE, PRIMARY KEY(collection_id, game_id))",
            "CREATE TABLE metadata (game_id TEXT PRIMARY KEY REFERENCES games(id) ON DELETE CASCADE, region TEXT NOT NULL DEFAULT '', developer TEXT NOT NULL DEFAULT '', publisher TEXT NOT NULL DEFAULT '', release_date TEXT NOT NULL DEFAULT '', genre TEXT NOT NULL DEFAULT '', players TEXT NOT NULL DEFAULT '', notes TEXT NOT NULL DEFAULT '')",
            "CREATE INDEX collection_games_game ON collection_games(game_id)",
            "CREATE INDEX games_added ON games(added)",
            "CREATE INDEX games_last_played ON games(last_played)",
        ], 2: [
            "CREATE TABLE import_issues (id INTEGER PRIMARY KEY, path TEXT NOT NULL UNIQUE, code TEXT NOT NULL, message TEXT NOT NULL, system TEXT, status TEXT NOT NULL DEFAULT 'open', created REAL NOT NULL, updated REAL NOT NULL)",
        ], 3: [
            "ALTER TABLE metadata ADD COLUMN description TEXT NOT NULL DEFAULT ''",
            "CREATE TABLE metadata_overrides (game_id TEXT REFERENCES games(id) ON DELETE CASCADE, field TEXT NOT NULL, PRIMARY KEY(game_id,field))",
            "CREATE TABLE metadata_lookups (game_id TEXT PRIMARY KEY REFERENCES games(id) ON DELETE CASCADE, provider TEXT NOT NULL, status TEXT NOT NULL, checked REAL NOT NULL, retry_after REAL NOT NULL, source_url TEXT, payload TEXT, message TEXT NOT NULL DEFAULT '')",
            "CREATE TABLE rom_hashes (game_id TEXT PRIMARY KEY REFERENCES games(id) ON DELETE CASCADE, md5 TEXT NOT NULL, sha1 TEXT NOT NULL, crc32 TEXT NOT NULL)",
        ], 4: [
            "CREATE TABLE game_files (game_id TEXT REFERENCES games(id) ON DELETE CASCADE, path TEXT NOT NULL, sha256 TEXT NOT NULL, size INTEGER NOT NULL, PRIMARY KEY(game_id,path))",
        ]}
        # Lock before checking the version: importer and artwork connections may
        # open the same older library together on startup.
        self.db.execute("BEGIN IMMEDIATE")
        try:
            applied = {row[0] for row in self.db.execute("SELECT version FROM schema_migrations")}
            for version, statements in migrations.items():
                if version not in applied:
                    for statement in statements:
                        self.db.execute(statement)
                    if version == 3:
                        for field in ("region", "developer", "publisher", "release_date", "genre", "players", "notes"):
                            self.db.execute(f"INSERT INTO metadata_overrides SELECT game_id,? FROM metadata WHERE {field}<>''", (field,))
                        for game in self.db.execute("SELECT * FROM games").fetchall():
                            if game["title"] != Path(game["original_filename"] or game["rom_path"]).stem:
                                self.db.execute("INSERT INTO metadata_overrides VALUES(?, 'title')", (game["id"],))
                    self.db.execute("INSERT INTO schema_migrations VALUES(?,?)", (version, time.time()))
            self.db.commit()
        except Exception:
            self.db.rollback()
            raise

    def import_file(self, path, system_override=None):
        requested = Path(path).expanduser().absolute()
        name = requested.name
        path = requested.resolve()
        if system_override is not None and system_override not in SYSTEMS:
            raise ImportProblem("unknown_system", "Choose an available system before retrying this file.")
        if path.stat().st_size == 0:
            raise ImportProblem("empty_file", "This file is empty. Choose a complete ROM file.")
        if requested.suffix.lower() == ".zip":
            result = []
            with zipfile.ZipFile(path) as z:
                if any(Path(i.filename).suffix.lower() in DISC_FILES for i in z.infolist()):
                    raise ImportProblem('unsupported_archive', 'Extract this disc archive first, then import its CUE, CCD, M3U or disc image. Keep its folders and tracks together.')
                candidates = [i for i in z.infolist() if Path(i.filename).suffix.lower() in EXTENSIONS and not i.is_dir()]
                if not candidates:
                    raise ImportProblem("unsupported_archive", "This ZIP has no recognized ROMs. Extract it, then import a ROM and choose its system if needed.")
                if len(candidates) > 100 or sum(i.file_size for i in candidates) > 128 * 1024 * 1024:
                    raise ValueError("The archive is too large. Extract and import individual ROMs.")
                for info in candidates:
                    if info.file_size > MAX_ROM:
                        raise ValueError("A ROM in this archive exceeds 512 MiB.")
                    with z.open(info) as f:
                        data = f.read(MAX_ROM + 1)
                    result.append(self._import_bytes(Path(info.filename).name, data, str(requested), system=system_override))
            return result
        if requested.suffix.lower() in (".7z", ".rar", ".gz", ".tar"):
            raise ImportProblem("unsupported_archive", "Extract this archive first, then import the ROM files. ZIP archives can be imported directly.")
        if requested.suffix.lower() == '.bin' and (not system_override or SYSTEMS[system_override].media == 'disc'):
            try:
                cue = companion_cue(requested)
                if cue:
                    return self.import_file(cue, system_override=system_override)
                if probe_track(requested) or system_override:
                    raise ImportProblem('missing_cue', 'This appears to be a disc track. Import its matching CUE sheet so track order and audio are preserved. Keep the CUE and all BIN files together.')
            except ValueError as error:
                if isinstance(error, ImportProblem):
                    raise
                raise ImportProblem('invalid_disc', str(error)) from error
        if requested.suffix.lower() in DISC_FILES:
            try:
                detected = detect_disc(requested)
            except (ValueError, UnicodeError) as error:
                raise ImportProblem('invalid_disc', str(error)) from error
            if detected and system_override and detected != system_override:
                raise ImportProblem('invalid_disc', f'This disc identifies as {SYSTEMS[detected].name}, not {SYSTEMS[system_override].name}. Retry with automatic detection.')
            system = system_override or detected or EXTENSIONS.get(requested.suffix.lower())
            if not system:
                raise ImportProblem('unknown_disc', 'Choose the console for this disc game, then import it. You can select several discs for the same console at once. All referenced files will be kept together.')
            if SYSTEMS[system].media != 'disc':
                raise ImportProblem('invalid_disc', 'Choose a disc-based console for this file.')
            if requested.suffix.lower() == '.iso' and 'iso' not in SYSTEMS[system].extensions:
                raise ImportProblem('invalid_disc', f'{SYSTEMS[system].name} needs a CUE sheet or CHD image with the available cores. Import the matching CUE sheet for this ISO, or convert the complete disc to CHD.')
            return [self.import_content(requested, system)]
        if requested.suffix.lower() not in EXTENSIONS and system_override is None:
            raise ImportProblem("unknown_system", "The system could not be identified from this filename. Choose its system, then retry. The filename will stay unchanged.")
        if path.stat().st_size > MAX_ROM:
            raise ValueError("The ROM exceeds 512 MiB.")
        with path.open("rb") as file:
            data = file.read(MAX_ROM + 1)
        return [self._import_bytes(name, data, str(requested), system=system_override,
                                  external=self.setting("copy_games", "1") != "1")]

    def import_content(self, source, system):
        try:
            rows = inventory(source)
        except (ValueError, UnicodeError) as error:
            raise ImportProblem('invalid_disc', str(error)) from error
        digest = content_id(rows)
        if self.get(digest):
            return digest
        managed = self.setting('copy_games', '1') == '1'
        target = copy_content(source, self.root / 'roms' / system / digest, rows) if managed else source
        stored = str(target.relative_to(self.root)) if managed else str(target)
        with self.db:
            self.db.execute("INSERT OR IGNORE INTO games (id,title,system,rom_path,source,added,original_filename) VALUES(?,?,?,?,?,?,?)",
                (digest, source.stem, system, stored, str(source), time.time(), source.name))
            self.db.executemany("INSERT OR IGNORE INTO game_files VALUES(?,?,?,?)",
                [(digest, row['path'], row['sha256'], row['size']) for row in rows])
        return digest

    def content_rows(self, game_id):
        return [dict(row) for row in self.db.execute('SELECT path,sha256,size FROM game_files WHERE game_id=?', (game_id,))]

    def _import_bytes(self, name, data, source, *, system=None, external=False):
        suffix = Path(name).suffix.lower()
        system = system or EXTENSIONS.get(suffix)
        if system is None:
            choices = ', '.join(SYSTEMS[key].name for key in EXTENSION_SYSTEMS.get(suffix, []))
            raise ImportProblem("unknown_system", "Choose the system for this file" + (": " + choices if choices else "") + ". Its filename will stay unchanged.")
        minimum = 32768 if system == "snes" else (192 if system == "gba" else (0x150 if system in ("gb", "gbc") else 1))
        if len(data) > MAX_ROM or len(data) < minimum:
            raise ImportProblem("invalid_rom", f"{name} has an invalid ROM size for {SYSTEMS[system].name}. Choose a complete ROM and the correct system.")
        if system in ("gb", "gbc") and data[0x143] & 0x80:
            system = "gbc"
        digest = hashlib.sha256(data).hexdigest()
        existing = self.get(digest)
        if existing:
            if self.legacy_name(existing):
                self.restore_rom_name(existing, name, data)
            return digest
        dest = Path(source) if external else self.write_rom(digest, name, data, system=system)
        stored_path = str(dest) if external else str(dest.relative_to(self.root))
        with self.db:
            self.db.execute("""INSERT OR IGNORE INTO games
                (id,title,system,rom_path,source,added,original_filename) VALUES(?,?,?,?,?,?,?)""",
                (digest, Path(name).stem, system, stored_path, source, time.time(), name))
            self.db.execute("INSERT OR IGNORE INTO rom_hashes VALUES(?,?,?,?)",
                (digest, hashlib.md5(data, usedforsecurity=False).hexdigest(),
                 hashlib.sha1(data, usedforsecurity=False).hexdigest(), f"{zlib.crc32(data):08x}"))
        return digest

    def write_rom(self, digest, name, data, *, system=None):
        if not name or Path(name).name != name or name in (".", ".."):
            raise ValueError("Invalid ROM filename")
        # The folder is content-addressed; the filename itself stays unchanged.
        folder = self.root / "roms"
        if system:
            if system not in SYSTEMS:
                raise ValueError("Unknown system")
            folder /= system
        dest = folder / digest / name
        dest.parent.mkdir(parents=True, exist_ok=True)
        fd, tmp = tempfile.mkstemp(dir=dest.parent)
        try:
            with os.fdopen(fd, "wb") as f:
                f.write(data)
            os.replace(tmp, dest)
        finally:
            Path(tmp).unlink(missing_ok=True)
        return dest

    @staticmethod
    def legacy_name(game):
        path = Path(game["rom_path"])
        return path.parent == Path("roms") and path.stem == game["id"]

    def needs_filename_restore(self):
        return any(self.legacy_name(row) for row in self.games())

    def restore_rom_name(self, game, name, data):
        if hashlib.sha256(data).hexdigest() != game["id"]:
            raise ValueError("The imported ROM no longer matches its library identity.")
        dest = self.write_rom(game["id"], name, data, system=game["system"])
        with self.db:
            count = self.db.execute("""UPDATE games SET rom_path=?, original_filename=?, previous_rom_stem=?
                WHERE id=? AND rom_path=?""", (str(dest.relative_to(self.root)), name,
                Path(game["rom_path"]).stem, game["id"], game["rom_path"])).rowcount
        # Retain the old copy: a running core or artwork worker may still use it.
        return bool(count)

    def restore_filenames(self):
        restored, unavailable = 0, 0
        for game in self.games():
            if not self.legacy_name(game):
                continue
            try:
                name = game["original_filename"]
                source = Path(game["source"])
                if not name and source.suffix.lower() in EXTENSIONS:
                    name = source.name
                if not name and source.suffix.lower() == ".zip" and source.is_file():
                    with zipfile.ZipFile(source) as archive:
                        entries = [i for i in archive.infolist() if not i.is_dir() and Path(i.filename).suffix.lower() in EXTENSIONS]
                        if len(entries) > 100 or sum(i.file_size for i in entries) > 128 * 1024 * 1024:
                            raise ValueError("Archive too large for filename recovery")
                        for entry in entries:
                            if entry.file_size > MAX_ROM:
                                continue
                            with archive.open(entry) as file:
                                payload = file.read(MAX_ROM + 1)
                            if hashlib.sha256(payload).hexdigest() == game["id"]:
                                name = Path(entry.filename).name
                                break
                if not name or name == Path(game["rom_path"]).name:
                    unavailable += 1
                    continue
                with (self.root / game["rom_path"]).open("rb") as file:
                    data = file.read(MAX_ROM + 1)
                restored += int(self.restore_rom_name(game, name, data))
            except (OSError, ValueError, zipfile.BadZipFile, RuntimeError):
                unavailable += 1
        return restored, unavailable

    def prepare_save_filenames(self, game, directory):
        """Called after acquiring the game lock, before any core opens its saves."""
        previous = game["previous_rom_stem"]
        current = Path(game["rom_path"]).stem
        directory = Path(directory)
        if not previous or previous == current or not directory.is_dir():
            return
        for source in directory.iterdir():
            if not source.is_file() or not source.name.startswith(previous + "."):
                continue
            target = directory / (current + source.name[len(previous):])
            if target.exists():
                continue
            fd, temporary = tempfile.mkstemp(dir=directory)
            try:
                with os.fdopen(fd, "wb") as output, source.open("rb") as original:
                    shutil.copyfileobj(original, output)
                try:
                    os.link(temporary, target)
                except FileExistsError:
                    pass
            finally:
                Path(temporary).unlink(missing_ok=True)

    def get(self, game_id):
        return self.db.execute("SELECT * FROM games WHERE id=?", (game_id,)).fetchone()

    def validate_game(self, game_id):
        game = self.get(game_id)
        if game is None:
            raise ValueError("This game is no longer in the library.")
        path = self.root / game["rom_path"]
        if not path.is_file():
            raise ValueError("The ROM file is missing. Right-click the game and choose Locate missing ROM to reconnect it.")
        if rows := self.content_rows(game_id):
            expected = sorted(rows, key=lambda row: row['path'])
            if sorted(inventory(path), key=lambda row: row['path']) != expected:
                raise ValueError('A disc file changed since import. Restore the original disc set before playing.')
            return path
        with path.open("rb") as file:
            digest = hashlib.file_digest(file, 'sha256').hexdigest()
        if digest != game_id:
            raise ValueError("The ROM file has changed since import. Locate the original ROM, or import the changed file as a separate game.")
        return path

    def consolidate(self, game_id):
        game = self.get(game_id)
        source = self.validate_game(game_id)
        if not Path(game["rom_path"]).is_absolute():
            return False
        if rows := self.content_rows(game_id):
            target = copy_content(source, self.root / 'roms' / game['system'] / game_id, rows)
            with self.db:
                self.db.execute('UPDATE games SET rom_path=? WHERE id=?', (str(target.relative_to(self.root)), game_id))
            return True
        with source.open("rb") as file:
            data = file.read(MAX_ROM + 1)
        if hashlib.sha256(data).hexdigest() != game_id:
            raise ValueError("The ROM changed while it was being copied. Try again after closing any program editing it.")
        target = self.write_rom(game_id, source.name, data, system=game["system"])
        with self.db:
            self.db.execute("UPDATE games SET rom_path=? WHERE id=? AND rom_path=?",
                            (str(target.relative_to(self.root)), game_id, game["rom_path"]))
        return True

    def relink(self, game_id, path):
        game = self.get(game_id)
        if game is None:
            raise ValueError("This game is no longer in the library.")
        requested = Path(path).expanduser().absolute()
        if rows := self.content_rows(game_id):
            if sorted(inventory(requested), key=lambda row: row['path']) != sorted(rows, key=lambda row: row['path']):
                raise ValueError('Choose the original disc set with matching filenames and hashes.')
            target = requested if Path(game['rom_path']).is_absolute() else copy_content(requested, self.root / 'roms' / game['system'] / game_id, rows)
            stored = str(target) if Path(game['rom_path']).is_absolute() else str(target.relative_to(self.root))
            with self.db:
                self.db.execute('UPDATE games SET rom_path=?,source=? WHERE id=?', (stored, str(requested), game_id))
            return
        with requested.open("rb") as file:
            data = file.read(MAX_ROM + 1)
        if hashlib.sha256(data).hexdigest() != game_id:
            raise ValueError("This file is a different ROM. Choose the original game's file; its SHA-256 must match to preserve saves safely.")
        # Finish an older naming transition before starting another one. Keep
        # every old copy and never overwrite a newer, already named save.
        self.prepare_save_filenames(game, self.root / "saves" / game_id)
        if Path(game["rom_path"]).is_absolute():
            stored = str(requested)
        else:
            stored = str(self.write_rom(game_id, requested.name, data, system=game["system"]).relative_to(self.root))
        with self.db:
            self.db.execute("UPDATE games SET rom_path=?,source=?,original_filename=?,previous_rom_stem=? WHERE id=?",
                (stored, str(requested), requested.name, Path(game["rom_path"]).stem, game_id))

    def record_import_issue(self, path, code, message, system=None):
        now = time.time()
        with self.db:
            self.db.execute("""INSERT INTO import_issues(path,code,message,system,created,updated) VALUES(?,?,?,?,?,?)
                ON CONFLICT(path) DO UPDATE SET code=excluded.code,message=excluded.message,system=excluded.system,
                status='open',updated=excluded.updated""", (str(path), code, message, system, now, now))

    def import_issues(self):
        return self.db.execute("SELECT * FROM import_issues WHERE status='open' ORDER BY created,id").fetchall()

    def resolve_import_issue(self, path, status="resolved"):
        with self.db:
            self.db.execute("UPDATE import_issues SET status=?,updated=? WHERE path=?", (status, time.time(), str(path)))

    def games(self, system=None, search="", favorites=False, recent=False, *,
              collection=None, added=False, never=False):
        query, args = "SELECT games.* FROM games WHERE 1=1", []
        if search:
            # Bound LIKE values keep filename wildcard characters literal.
            pattern = "%" + search.replace("\\", "\\\\").replace("%", "\\%").replace("_", "\\_") + "%"
            fields = ["title", "system", "rom_path", "original_filename"]
            clauses = [f"COALESCE({field},'') LIKE ? ESCAPE '\\'" for field in fields]
            args.extend([pattern] * len(fields))
            clauses.append("EXISTS (SELECT 1 FROM metadata m WHERE m.game_id=games.id AND "
                           "(m.region || ' ' || m.developer || ' ' || m.publisher || ' ' || m.release_date || ' ' || m.genre || ' ' || m.players || ' ' || m.notes || ' ' || m.description) LIKE ? ESCAPE '\\')")
            args.append(pattern)
            keys = [key for key, value in SYSTEMS.items() if search.casefold() in value.name.casefold()]
            if keys:
                clauses.append("system IN (" + ",".join("?" for _ in keys) + ")")
                args.extend(keys)
            query += " AND (" + " OR ".join(clauses) + ")"
        if system:
            query += " AND system=?"
            args.append(system)
        if favorites:
            query += " AND favorite=1"
        if recent:
            query += " AND last_played IS NOT NULL"
        if added:
            query += " AND added>=?"
            args.append(time.time() - 30 * 86400)
        if never:
            query += " AND play_count=0"
        if collection is not None:
            record = self.collection(collection)
            if not record:
                return []
            if record["rules"] is None:
                query += " AND id IN (SELECT game_id FROM collection_games WHERE collection_id=?)"
                args.append(collection)
            else:
                rules = self.validate_rules(json.loads(record["rules"]))
                for key, value in rules.items():
                    if key == "system":
                        query += " AND system=?"
                        args.append(value)
                    elif key == "minimum_rating":
                        query += " AND rating>=?"
                        args.append(value)
                    elif key in ("added_days", "played_days"):
                        query += " AND " + ("added" if key == "added_days" else "last_played") + ">=?"
                        args.append(time.time() - value * 86400)
                    elif key == "favorite" and value:
                        query += " AND favorite=1"
                    elif key == "never_played" and value:
                        query += " AND play_count=0"
        query += (" ORDER BY last_played DESC, title COLLATE NOCASE" if recent else
                  " ORDER BY added DESC, title COLLATE NOCASE" if added else " ORDER BY title COLLATE NOCASE")
        return self.db.execute(query, args).fetchall()

    def rate(self, game_ids, rating):
        if type(rating) is not int or not 0 <= rating <= 5:
            raise ValueError("Choose a rating from 0 to 5 stars.")
        with self.db:
            self.db.executemany("UPDATE games SET rating=? WHERE id=?", [(rating, key) for key in game_ids])

    @staticmethod
    def validate_rules(rules):
        if not isinstance(rules, dict) or not rules:
            raise ValueError("Choose at least one smart collection rule.")
        for key, value in rules.items():
            if key == "system" and isinstance(value, str) and value in SYSTEMS:
                continue
            if key == "minimum_rating" and type(value) is int and 0 <= value <= 5:
                continue
            if key in ("added_days", "played_days") and type(value) is int and 1 <= value <= 36500:
                continue
            if key in ("favorite", "never_played") and type(value) is bool:
                continue
            raise ValueError("Invalid smart collection rule.")
        return rules

    def collections(self):
        return self.db.execute("SELECT * FROM collections ORDER BY name COLLATE NOCASE, id").fetchall()

    def sidebar_counts(self):
        """Return collection counts with aggregate SQL instead of loading games."""
        cutoff = time.time() - 30 * 86400
        total, recent, favorites, added = self.db.execute("""SELECT COUNT(*),
            COALESCE(SUM(last_played IS NOT NULL),0), COALESCE(SUM(favorite=1),0),
            COALESCE(SUM(added>=?),0) FROM games""", (cutoff,)).fetchone()
        counts = {"all": total, "recent": recent, "favorites": favorites, "added": added}
        for record in self.collections():
            if record["rules"] is None:
                count = self.db.execute("SELECT COUNT(*) FROM collection_games WHERE collection_id=?",
                                        (record["id"],)).fetchone()[0]
            else:
                clauses, args = [], []
                for key, value in self.validate_rules(json.loads(record["rules"])).items():
                    if key == "system":
                        clauses.append("system=?"); args.append(value)
                    elif key == "minimum_rating":
                        clauses.append("rating>=?"); args.append(value)
                    elif key in ("added_days", "played_days"):
                        clauses.append(("added" if key == "added_days" else "last_played") + ">=?")
                        args.append(time.time() - value * 86400)
                    elif key == "favorite" and value:
                        clauses.append("favorite=1")
                    elif key == "never_played" and value:
                        clauses.append("play_count=0")
                count = self.db.execute("SELECT COUNT(*) FROM games WHERE " + " AND ".join(clauses), args).fetchone()[0]
            counts[f"collection:{record['id']}"] = count
        return counts

    def collection(self, collection_id):
        return self.db.execute("SELECT * FROM collections WHERE id=?", (collection_id,)).fetchone()

    def save_collection(self, name, rules=None, collection_id=None):
        name = name.strip()
        if not name:
            raise ValueError("Enter a collection name.")
        encoded = json.dumps(self.validate_rules(rules)) if rules is not None else None
        with self.db:
            if collection_id is None:
                return self.db.execute("INSERT INTO collections(name,rules,created) VALUES(?,?,?)",
                                       (name, encoded, time.time())).lastrowid
            self.db.execute("UPDATE collections SET name=?, rules=? WHERE id=?", (name, encoded, collection_id))
        return collection_id

    def delete_collection(self, collection_id):
        with self.db:
            self.db.execute("DELETE FROM collections WHERE id=?", (collection_id,))

    def add_to_collection(self, collection_id, game_ids):
        record = self.collection(collection_id)
        if not record or record["rules"] is not None:
            raise ValueError("Games can be added to regular collections only.")
        with self.db:
            self.db.executemany("INSERT OR IGNORE INTO collection_games SELECT ?,id FROM games WHERE id=?",
                                [(collection_id, key) for key in game_ids])

    def remove_from_collection(self, collection_id, game_ids):
        with self.db:
            self.db.executemany("DELETE FROM collection_games WHERE collection_id=? AND game_id=?",
                                [(collection_id, key) for key in game_ids])

    def metadata(self, game_id):
        row = self.db.execute("SELECT * FROM metadata WHERE game_id=?", (game_id,)).fetchone()
        return dict(row) if row else {}

    def update_metadata(self, game_id, values):
        allowed = {"region", "developer", "publisher", "release_date", "genre", "players", "notes", "description"}
        if not values.keys() <= allowed or any(not isinstance(value, str) for value in values.values()):
            raise ValueError("Invalid game information.")
        with self.db:
            self.db.execute("INSERT OR IGNORE INTO metadata(game_id) VALUES(?)", (game_id,))
            for key, value in values.items():
                self.db.execute(f"UPDATE metadata SET {key}=? WHERE game_id=?", (value.strip(), game_id))
                self.db.execute("INSERT OR IGNORE INTO metadata_overrides VALUES(?,?)", (game_id, key))

    def metadata_candidates(self, force=False, game_ids=None):
        rows = self.db.execute("""SELECT games.*, COALESCE(metadata_lookups.retry_after,0) AS retry_after
            FROM games LEFT JOIN metadata_lookups ON games.id=metadata_lookups.game_id ORDER BY added""").fetchall()
        return [row for row in rows if (game_ids is None or row["id"] in game_ids)
                and (force or row["retry_after"] <= time.time())]

    def metadata_lookup(self, game_id):
        return self.db.execute("SELECT * FROM metadata_lookups WHERE game_id=?", (game_id,)).fetchone()

    def metadata_result(self, game_id, provider, status, *, payload=None, source_url=None, message="", hashes=None):
        """Publish a provider result while retaining every explicitly edited field."""
        allowed = {"title", "region", "developer", "publisher", "release_date", "genre", "players", "description"}
        values = {key: str(value).strip() for key, value in (payload or {}).items() if key in allowed and value is not None}
        with self.db:
            if not self.db.in_transaction:
                self.db.execute("BEGIN IMMEDIATE")
            if self.get(game_id) is None:
                return False
            protected = {r[0] for r in self.db.execute("SELECT field FROM metadata_overrides WHERE game_id=?", (game_id,))}
            self.db.execute("INSERT OR IGNORE INTO metadata(game_id) VALUES(?)", (game_id,))
            for key, value in values.items():
                if key in protected or not value:
                    continue
                if key == "title":
                    self.db.execute("UPDATE games SET title=? WHERE id=?", (value, game_id))
                else:
                    self.db.execute(f"UPDATE metadata SET {key}=? WHERE game_id=?", (value, game_id))
            if hashes:
                self.db.execute("INSERT OR REPLACE INTO rom_hashes VALUES(?,?,?,?)", (game_id, hashes["md5"], hashes["sha1"], hashes["crc32"]))
            cached = self.metadata_lookup(game_id)
            stored = values
            if status != "matched" and cached and cached["provider"] == provider and cached["payload"]:
                stored = json.loads(cached["payload"])
                source_url = cached["source_url"]
            delay = 3600 if status == "failed" else 30 * 86400
            self.db.execute("""INSERT OR REPLACE INTO metadata_lookups VALUES(?,?,?,?,?,?,?,?)""",
                            (game_id, provider, status, time.time(), time.time() + delay,
                             source_url, json.dumps(stored), message))
        return True

    def use_automatic_metadata(self, game_id):
        lookup = self.metadata_lookup(game_id)
        if not lookup or not json.loads(lookup["payload"] or "{}"):
            return False
        with self.db:
            values = json.loads(lookup["payload"])
            # Reset only fields that have a downloaded replacement. Notes and
            # unavailable provider fields stay under the user's control.
            for key, value in values.items():
                if value:
                    self.db.execute("DELETE FROM metadata_overrides WHERE game_id=? AND field=?", (game_id, key))
            self.metadata_result(game_id, lookup["provider"], "matched", payload=values, source_url=lookup["source_url"])
        return True

    def hashes(self, game_id):
        row = self.db.execute("SELECT * FROM rom_hashes WHERE game_id=?", (game_id,)).fetchone()
        return dict(row) if row else {}

    def played(self, game_id):
        with self.db:
            self.db.execute("UPDATE games SET last_played=?, play_count=play_count+1 WHERE id=?", (time.time(), game_id))

    def favorite(self, game_id):
        with self.db:
            self.db.execute("UPDATE games SET favorite=1-favorite WHERE id=?", (game_id,))

    def rename(self, game_id, title):
        if title.strip():
            with self.db:
                self.db.execute("UPDATE games SET title=? WHERE id=?", (title.strip(), game_id))
                self.db.execute("INSERT OR IGNORE INTO metadata_overrides VALUES(?, 'title')", (game_id,))

    def remove(self, game_id):
        # Removing a library entry preserves the ROM, battery saves and states.
        with self.db:
            self.db.execute("DELETE FROM games WHERE id=?", (game_id,))
            self.db.execute("DELETE FROM artwork WHERE game_id=?", (game_id,))
            self.db.execute("DELETE FROM settings WHERE key LIKE ? ESCAPE '\\'",
                            ("core\\_option." + game_id + ".%",))

    def setting(self, key, default=""):
        row = self.db.execute("SELECT value FROM settings WHERE key=?", (key,)).fetchone()
        return row[0] if row else default

    def set_setting(self, key, value):
        with self.db:
            self.db.execute("INSERT OR REPLACE INTO settings VALUES(?,?)", (key, str(value)))

    def artwork_candidates(self, force=False, game_ids=None, *, replace=False):
        rows = self.db.execute("""SELECT games.*, COALESCE(artwork.retry_after,0) AS retry_after
            FROM games LEFT JOIN artwork ON games.id=artwork.game_id ORDER BY added""").fetchall()
        return [row for row in rows if (game_ids is None or row["id"] in game_ids)
                and (replace or not row["cover"] or not (self.root / row["cover"]).is_file())
                and (force or row["retry_after"] <= time.time())]

    def artwork_result(self, game_id, status, *, url=None, message=""):
        delay = 30 * 86400 if status == "not_found" else 3600
        with self.db:
            self.db.execute("""INSERT OR REPLACE INTO artwork VALUES(?,?,?,?,?,?)""",
                            (game_id, status, time.time(), time.time() + delay, url, message))

    def artwork_info(self, game_id):
        return self.db.execute("SELECT * FROM artwork WHERE game_id=?", (game_id,)).fetchone()

    def set_manual_cover(self, game_id, path):
        with self.db:
            self.db.execute("UPDATE games SET cover=?, cover_revision=cover_revision+1 WHERE id=?", (str(path), game_id))
            self.db.execute("DELETE FROM artwork WHERE game_id=?", (game_id,))

    def set_downloaded_cover(self, game_id, path, previous_cover, previous_revision=0):
        # A cover chosen or game removed while a download runs wins the race.
        with self.db:
            updated = self.db.execute("""UPDATE games SET cover=?, cover_revision=cover_revision+1
                WHERE id=? AND cover IS ? AND cover_revision=?""",
                                      (str(path), game_id, previous_cover, previous_revision)).rowcount
        return bool(updated)

    def close(self):
        self.db.close()
