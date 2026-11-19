use rusqlite::{Connection, OpenFlags, OptionalExtension};
use std::path::Path;

const FIELDS: &[(&str, &str)] = &[
    ("region", "Region"),
    ("developer", "Developer"),
    ("publisher", "Publisher"),
    ("release_date", "Release date"),
    ("genre", "Genre"),
    ("players", "Players"),
    ("description", "Description"),
    ("notes", "Notes"),
];

pub type Fields = Vec<(&'static str, String)>;

pub fn load(root: &Path, game_id: &str) -> rusqlite::Result<Fields> {
    let db = Connection::open_with_flags(
        root.join("library.sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    let has_metadata: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = 'metadata')",
        [],
        |row| row.get(0),
    )?;
    if !has_metadata {
        return Ok(Vec::new());
    }

    db.query_row(
        "SELECT * FROM metadata WHERE game_id = ?1",
        [game_id],
        |row| {
            let mut fields = Vec::new();
            for &(column, label) in FIELDS {
                // Older libraries can have only a subset of these columns.
                if let Ok(index) = row.as_ref().column_index(column) {
                    if let Some(value) = row.get::<_, Option<String>>(index)? {
                        if !value.trim().is_empty() {
                            fields.push((label, value));
                        }
                    }
                }
            }
            Ok(fields)
        },
    )
    .optional()
    .map(Option::unwrap_or_default)
}

pub fn for_game(
    root: &Path,
    game: &emuluna_library_model::Game,
    preferred_region: &str,
) -> rusqlite::Result<Fields> {
    let saved = load(root, &game.id)?;
    if !saved.is_empty() {
        return Ok(saved);
    }
    let Some(filename) = game.rom.file_name().and_then(|name| name.to_str()) else {
        return Ok(saved);
    };
    let catalog: serde_json::Value =
        serde_json::from_str(include_str!("../../../data/systems.json")).expect("console catalog");
    let Some(systems) = crate::openvgdb::system_names(&catalog, &game.system) else {
        return Ok(saved);
    };
    let path = crate::storage::metadata(root).join("openvgdb.sqlite");
    if !path.is_file() {
        return Ok(saved);
    }
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    db.query_row(
        "SELECT regionName, releaseDeveloper, releasePublisher, releaseDate, releaseGenre, releaseDescription
         FROM ROMs JOIN SYSTEMS USING (systemID) JOIN RELEASES USING (romID)
         LEFT JOIN REGIONS ON regionLocalizedID = REGIONS.regionID
         WHERE romFileName = ?1 COLLATE NOCASE
           AND systemShortName IN (SELECT value FROM json_each(?2))
         ORDER BY (regionName = ?3 COLLATE NOCASE) DESC, releaseID
         LIMIT 1",
        rusqlite::params![filename, systems, preferred_region],
        |row| {
            let mut fields = Vec::new();
            for (index, label) in ["Region", "Developer", "Publisher", "Release date", "Genre", "Description"]
                .into_iter().enumerate()
            {
                if let Some(value) = row.get::<_, Option<String>>(index)? {
                    if !value.trim().is_empty() {
                        fields.push((label, value));
                    }
                }
            }
            Ok(fields)
        },
    )
    .optional()
    .map(Option::unwrap_or_default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_populated_fields_from_legacy_library() -> rusqlite::Result<()> {
        let dir = tempfile::tempdir().unwrap();
        let db = Connection::open(dir.path().join("library.sqlite3"))?;
        assert!(load(dir.path(), "game")?.is_empty());
        db.execute_batch(
            "CREATE TABLE metadata(game_id TEXT PRIMARY KEY, publisher TEXT, notes TEXT);
             INSERT INTO metadata VALUES ('game', 'Publisher', '  ');",
        )?;
        assert_eq!(
            load(dir.path(), "game")?,
            vec![("Publisher", "Publisher".into())]
        );
        assert!(load(dir.path(), "other")?.is_empty());
        Ok(())
    }
}
