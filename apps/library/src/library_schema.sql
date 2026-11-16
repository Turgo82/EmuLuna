PRAGMA journal_mode = WAL;
BEGIN;
CREATE TABLE games (
    id TEXT PRIMARY KEY, title TEXT NOT NULL, system TEXT NOT NULL, rom_path TEXT NOT NULL,
    source TEXT NOT NULL, added REAL NOT NULL, last_played REAL, play_count INTEGER NOT NULL DEFAULT 0,
    favorite INTEGER NOT NULL DEFAULT 0, cover TEXT, cover_revision INTEGER NOT NULL DEFAULT 0,
    original_filename TEXT, previous_rom_stem TEXT, rating INTEGER NOT NULL DEFAULT 0 CHECK(rating BETWEEN 0 AND 5)
);
CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
INSERT INTO settings VALUES ('storage.layout', 'consoles');
CREATE TABLE collections (id INTEGER PRIMARY KEY, name TEXT NOT NULL, rules TEXT, created REAL NOT NULL);
CREATE TABLE collection_games (
    collection_id INTEGER REFERENCES collections(id) ON DELETE CASCADE,
    game_id TEXT REFERENCES games(id) ON DELETE CASCADE, PRIMARY KEY(collection_id, game_id)
);
CREATE TABLE metadata (
    game_id TEXT PRIMARY KEY REFERENCES games(id) ON DELETE CASCADE,
    region TEXT NOT NULL DEFAULT '', developer TEXT NOT NULL DEFAULT '', publisher TEXT NOT NULL DEFAULT '',
    release_date TEXT NOT NULL DEFAULT '', genre TEXT NOT NULL DEFAULT '', players TEXT NOT NULL DEFAULT '',
    notes TEXT NOT NULL DEFAULT '', description TEXT NOT NULL DEFAULT ''
);
CREATE TABLE metadata_overrides (
    game_id TEXT REFERENCES games(id) ON DELETE CASCADE, field TEXT NOT NULL, PRIMARY KEY(game_id, field)
);
CREATE TABLE game_files (
    game_id TEXT REFERENCES games(id) ON DELETE CASCADE, path TEXT NOT NULL, sha256 TEXT NOT NULL,
    size INTEGER NOT NULL, PRIMARY KEY(game_id, path)
);
CREATE TABLE artwork (
    game_id TEXT PRIMARY KEY, status TEXT NOT NULL, checked REAL NOT NULL, retry_after REAL NOT NULL,
    source_url TEXT, message TEXT NOT NULL DEFAULT ''
);
CREATE TABLE import_issues (
    id INTEGER PRIMARY KEY, path TEXT NOT NULL UNIQUE, code TEXT NOT NULL, message TEXT NOT NULL,
    system TEXT, status TEXT NOT NULL DEFAULT 'open', created REAL NOT NULL, updated REAL NOT NULL
);
COMMIT;
