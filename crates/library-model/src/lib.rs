//! Read-only library snapshots and deterministic cover layout for the native UI.
#![forbid(unsafe_code)]
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
};
#[derive(Clone, Debug)]
pub struct Game {
    pub id: String,
    pub title: String,
    pub system: String,
    pub rom: PathBuf,
    pub original_filename: Option<String>,
    pub cover: Option<PathBuf>,
    pub rating: u8,
    pub favorite: bool,
    pub added: f64,
    pub last_played: Option<f64>,
    pub play_count: u32,
    search: String,
    sort_title: String,
}
impl Game {
    pub fn set_display_title(&mut self, title: String) {
        self.sort_title = title.to_lowercase();
        self.search.push(' ');
        self.search.push_str(&self.sort_title);
        self.title = title;
    }
}
#[derive(Clone, Debug)]
pub struct Console {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub cover_size: [u32; 2],
}
#[derive(Clone, Debug)]
pub struct Collection {
    pub id: i64,
    pub name: String,
    pub games: HashSet<String>,
    pub rules: Option<SmartRules>,
}
#[derive(Clone, Debug, Default, serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct SmartRules {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum_rating: Option<u8>,
    pub favorite: bool,
    pub never_played: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub added_days: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub played_days: Option<u32>,
}
impl SmartRules {
    fn matches(&self, game: &Game, now: f64) -> bool {
        self.system
            .as_ref()
            .is_none_or(|system| game.system == *system)
            && self
                .minimum_rating
                .is_none_or(|rating| game.rating >= rating)
            && (!self.favorite || game.favorite)
            && (!self.never_played || game.play_count == 0)
            && self
                .added_days
                .is_none_or(|days| game.added >= now - f64::from(days) * 86400.)
            && self.played_days.is_none_or(|days| {
                game.last_played
                    .is_some_and(|date| date >= now - f64::from(days) * 86400.)
            })
    }
}
#[derive(Debug)]
pub struct Library {
    pub root: PathBuf,
    pub games: Vec<Game>,
    pub consoles: Vec<Console>,
    pub collections: Vec<Collection>,
    pub theme: String,
    pub settings: BTreeMap<String, String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum View {
    All,
    System(String),
    Favorites,
    Recent,
    Added,
    Collection(i64),
}
fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn resolve(root: &Path, value: String) -> PathBuf {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        root.join(path)
    }
}
fn console_catalog() -> Result<Vec<Console>, String> {
    let catalog: Value =
        serde_json::from_str(include_str!("../../../data/systems.json")).map_err(error)?;
    let mut consoles: Vec<_> = catalog
        .as_object()
        .ok_or("Invalid console catalog")?
        .iter()
        .map(|(id, v)| Console {
            id: id.clone(),
            name: v["name"].as_str().unwrap_or(id).into(),
            icon: v["icon"].as_str().unwrap_or("").into(),
            cover_size: std::array::from_fn(|i| {
                v["cover_size"][i].as_u64().unwrap_or(256).clamp(1, 8192) as u32
            }),
        })
        .collect();
    consoles.sort_by_cached_key(|c| c.name.to_lowercase());
    Ok(consoles)
}
impl Library {
    pub fn empty(root: PathBuf) -> Result<Self, String> {
        Ok(Self {
            root,
            games: Vec::new(),
            consoles: console_catalog()?,
            collections: Vec::new(),
            theme: "luna".into(),
            settings: BTreeMap::new(),
        })
    }

    /// Read a consistent snapshot, including committed WAL entries from native
    /// import/background workers. This connection never writes game data.
    pub fn load(root: &Path) -> Result<Self, String> {
        let root = root.canonicalize().map_err(error)?;
        let db = root.join("library.sqlite3");
        if !db.is_file() {
            return Err("This folder does not contain an EmuLuna library.".into());
        }
        let connection = Connection::open_with_flags(
            db,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(error)?;
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(error)?;
        connection.execute_batch("BEGIN DEFERRED").map_err(error)?;
        if !connection
            .is_readonly(rusqlite::DatabaseName::Main)
            .map_err(error)?
        {
            return Err("Library preview requires a read-only connection".into());
        }
        let count: u64 = connection
            .query_row("SELECT COUNT(*) FROM games", [], |r| r.get(0))
            .map_err(error)?;
        if count > 100_000 {
            return Err("This preview supports up to 100,000 games.".into());
        }
        let mut statement=connection.prepare("SELECT id,title,system,rom_path,cover,rating,favorite,added,last_played,play_count,original_filename FROM games").map_err(error)?;
        let games = statement
            .query_map([], |row| {
                let title: String = row.get(1)?;
                let system: String = row.get(2)?;
                let rom: String = row.get(3)?;
                let original: Option<String> = row.get(10)?;
                Ok(Game {
                    id: row.get(0)?,
                    search: format!("{title} {system} {rom} {}", original.as_deref().unwrap_or_default())
                        .to_lowercase(),
                    sort_title: title.to_lowercase(),
                    title,
                    system,
                    rom: resolve(&root, rom),
                    original_filename: original,
                    cover: row
                        .get::<_, Option<String>>(4)?
                        .filter(|s| !s.is_empty())
                        .map(|p| resolve(&root, p)),
                    rating: row.get::<_, i64>(5)?.clamp(0, 5) as u8,
                    favorite: row.get::<_, i64>(6)? != 0,
                    added: row.get(7)?,
                    last_played: row.get(8)?,
                    play_count: row.get::<_, i64>(9)?.clamp(0, u32::MAX as i64) as u32,
                })
            })
            .map_err(error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(error)?;
        let mut statement = connection
            .prepare("SELECT id,name,rules FROM collections ORDER BY name COLLATE NOCASE,id")
            .or_else(|_| {
                connection
                    .prepare("SELECT id,name,NULL FROM collections ORDER BY name COLLATE NOCASE,id")
            })
            .map_err(error)?;
        let mut collections = statement
            .query_map([], |row| {
                Ok(Collection {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    games: HashSet::new(),
                    rules: row
                        .get::<_, Option<String>>(2)?
                        .and_then(|json| serde_json::from_str(&json).ok()),
                })
            })
            .map_err(error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(error)?;
        let mut statement = connection
            .prepare("SELECT collection_id,game_id FROM collection_games")
            .map_err(error)?;
        for entry in statement
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(error)?
        {
            let (id, game) = entry.map_err(error)?;
            if let Some(collection) = collections.iter_mut().find(|c| c.id == id) {
                collection.games.insert(game);
            }
        }
        let mut settings = BTreeMap::new();
        let mut bytes = 0usize;
        let mut statement = connection
            .prepare("SELECT key,value FROM settings LIMIT 10001")
            .map_err(error)?;
        for row in statement
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(error)?
        {
            let (key, value) = row.map_err(error)?;
            bytes += key.len() + value.len();
            if settings.len() >= 10000 || bytes > 4 * 1024 * 1024 {
                return Err("Library settings exceed the preview limit".into());
            }
            settings.insert(key, value);
        }
        let theme = settings
            .get("appearance.theme")
            .cloned()
            .unwrap_or_else(|| "luna".into());
        let consoles = console_catalog()?;
        let mut library = Self {
            root,
            games,
            consoles,
            collections,
            theme,
            settings,
        };
        library.refresh_smart_collections();
        Ok(library)
    }
    pub fn refresh_smart_collections(&mut self) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0., |d| d.as_secs_f64());
        for collection in &mut self.collections {
            if let Some(rules) = &collection.rules {
                collection.games = self
                    .games
                    .iter()
                    .filter(|game| rules.matches(game, now))
                    .map(|game| game.id.clone())
                    .collect();
            }
        }
    }
    pub fn visible(&self, view: &View, search: &str) -> Vec<usize> {
        let search = search.trim().to_lowercase();
        let added_cutoff = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0., |d| d.as_secs_f64())
            - 30. * 86400.;
        let collection = if let View::Collection(id) = view {
            self.collections.iter().find(|c| c.id == *id)
        } else {
            None
        };
        let mut indices: Vec<_> = self
            .games
            .iter()
            .enumerate()
            .filter(|(_, g)| {
                let included = match view {
                    View::All => true,
                    View::Added => g.added >= added_cutoff,
                    View::System(s) => g.system == *s,
                    View::Favorites => g.favorite,
                    View::Recent => g.last_played.is_some(),
                    View::Collection(_) => collection.is_some_and(|c| c.games.contains(&g.id)),
                };
                included && (search.is_empty() || g.search.contains(&search))
            })
            .map(|(i, _)| i)
            .collect();
        indices.sort_by(|&a, &b| {
            let (a, b) = (&self.games[a], &self.games[b]);
            let order = match view {
                View::Recent => b
                    .last_played
                    .partial_cmp(&a.last_played)
                    .unwrap_or(std::cmp::Ordering::Equal),
                View::Added => b.added.total_cmp(&a.added),
                _ => std::cmp::Ordering::Equal,
            };
            order
                .then_with(|| a.sort_title.cmp(&b.sort_title))
                .then_with(|| a.id.cmp(&b.id))
        });
        indices
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Cover {
    pub width: f32,
    pub height: f32,
}
#[derive(Clone, Copy, Debug)]
pub struct Placement {
    pub index: usize,
    pub x: f32,
    pub cover: Cover,
}
pub struct Grid {
    pub rows: Vec<Vec<Placement>>,
    pub row_height: f32,
}
impl Grid {
    /// Fixed art sizes, shared left edge, equal bounded gaps within each row.
    pub fn layout(width: f32, covers: &[Cover]) -> Self {
        let available = (width - 56.).max(1.);
        let mut rows = Vec::new();
        let mut row = Vec::new();
        let mut total = 0.;
        let mut last_gap = 35f32;
        let finish = |row: &mut Vec<Placement>,
                      rows: &mut Vec<Vec<Placement>>,
                      total: f32,
                      last_gap: &mut f32,
                      final_row: bool| {
            if row.is_empty() {
                return;
            }
            let mut gap = ((available - total) / row.len() as f32).clamp(35., 64.);
            if final_row {
                gap = gap.min(*last_gap);
            } else {
                *last_gap = gap;
            }
            let mut x = 28.;
            for item in row.iter_mut() {
                item.x = x;
                x += item.cover.width + gap;
            }
            rows.push(std::mem::take(row));
        };
        for (index, &cover) in covers.iter().enumerate() {
            if !row.is_empty() && total + cover.width + (row.len() + 1) as f32 * 35. > available {
                finish(&mut row, &mut rows, total, &mut last_gap, false);
                total = 0.;
            }
            row.push(Placement {
                index,
                x: 0.,
                cover,
            });
            total += cover.width;
        }
        finish(&mut row, &mut rows, total, &mut last_gap, true);
        Self {
            rows,
            row_height: covers.iter().map(|c| c.height).fold(0f32, f32::max) + 82.,
        }
    }
}
/// Same console-normalized cover sizing as the existing grid. Geometry is
/// independent of window width; only row wrapping and gaps change on resize.
pub fn fit_covers(games: &[(&str, [u32; 2], [u32; 2])]) -> Vec<Cover> {
    let mut ratios: HashMap<&str, Vec<f32>> = HashMap::new();
    for &(system, size, _) in games {
        ratios
            .entry(system)
            .or_default()
            .push(size[0] as f32 / size[1].max(1) as f32);
    }
    let mut heights = HashMap::new();
    for (&system, ratios) in &mut ratios {
        ratios.sort_by(f32::total_cmp);
        let mid = ratios.len() / 2;
        let median = if ratios.len() % 2 == 0 {
            (ratios[mid - 1] + ratios[mid]) * 0.5
        } else {
            ratios[mid]
        };
        let normal = ratios
            .iter()
            .copied()
            .filter(|r| *r <= median * 1.2)
            .fold(1f32, f32::max);
        heights.insert(system, (256. / normal).floor());
    }
    games
        .iter()
        .map(|&(system, size, canonical)| {
            let max_height = (canonical[1] as f32).min(heights[system]);
            let scale = (256. / size[0].max(1) as f32).min(max_height / size[1].max(1) as f32);
            Cover {
                width: (size[0] as f32 * scale).floor().max(1.),
                height: (size[1] as f32 * scale).floor().max(1.),
            }
        })
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_art_and_shared_left_edge_across_widths() {
        let art = vec![
            Cover {
                width: 150.,
                height: 256.,
            },
            Cover {
                width: 256.,
                height: 256.,
            },
            Cover {
                width: 155.,
                height: 256.,
            },
            Cover {
                width: 150.,
                height: 256.,
            },
        ];
        for width in [320., 650., 1100., 1900.] {
            let grid = Grid::layout(width, &art);
            for row in grid.rows {
                assert_eq!(row[0].x, 28.);
                let gaps: Vec<_> = row
                    .windows(2)
                    .map(|p| p[1].x - p[0].x - p[0].cover.width)
                    .collect();
                if let Some(first) = gaps.first() {
                    assert!((35.0..=64.0).contains(first));
                    assert!(gaps.iter().all(|g| (g - first).abs() < 0.001));
                }
                for item in row {
                    assert_eq!(item.cover.width, art[item.index].width);
                }
            }
        }
    }
    #[test]
    fn landscape_and_portrait_n64_covers_share_height_without_stretching() {
        let covers = fit_covers(&[
            ("n64", [320, 224], [320, 224]),
            ("n64", [224, 320], [320, 224]),
            ("n64", [330, 224], [320, 224]),
        ]);
        assert!(covers
            .iter()
            .all(|c| (c.height - covers[0].height).abs() <= 1.));
        assert!(covers.iter().all(|c| c.width <= 256.));
        assert!(covers[1].width < covers[0].width);
    }
    #[test]
    fn library_snapshot_never_writes_or_creates_sidecars() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "emuluna-readonly-{}-{unique} #?",
            std::process::id()
        ));
        std::fs::create_dir(&root).unwrap();
        let db = root.join("library.sqlite3");
        let c = Connection::open(&db).unwrap();
        c.execute_batch("CREATE TABLE games(id TEXT,title TEXT,system TEXT,rom_path TEXT,cover TEXT,rating INTEGER,favorite INTEGER,added REAL,last_played REAL,play_count INTEGER,original_filename TEXT); CREATE TABLE settings(key TEXT,value TEXT); CREATE TABLE collections(id INTEGER,name TEXT); CREATE TABLE collection_games(collection_id INTEGER,game_id TEXT); INSERT INTO games VALUES('a','A literal % name','nes','consoles/nes/roms/game.nes','covers/a.webp',9,1,1,NULL,0,NULL),('b','Z game','snes','/external/game.sfc',NULL,2,0,2,10,3,'old name'); INSERT INTO collections VALUES(1,'My games'); INSERT INTO collection_games VALUES(1,'a');").unwrap();
        drop(c);
        let original = std::fs::read(&db).unwrap();
        let library = Library::load(&root).unwrap();
        assert_eq!(library.games[0].rating, 5);
        assert_eq!(library.visible(&View::All, "%"), vec![0]);
        assert_eq!(library.visible(&View::Favorites, ""), vec![0]);
        assert_eq!(library.visible(&View::Recent, ""), vec![1]);
        assert_eq!(library.visible(&View::Collection(1), ""), vec![0]);
        assert_eq!(library.visible(&View::All, "old name"), vec![1]);
        assert!(library.games[0].rom.starts_with(&root));
        assert_eq!(library.games[1].rom, Path::new("/external/game.sfc"));
        assert_eq!(std::fs::read(&db).unwrap(), original);
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        let writer = Connection::open(&db).unwrap();
        writer
            .execute_batch(
                "PRAGMA journal_mode=WAL; UPDATE games SET title='Updated in WAL' WHERE id='a';",
            )
            .unwrap();
        assert!(
            std::fs::metadata(root.join("library.sqlite3-wal"))
                .unwrap()
                .len()
                > 0
        );
        let snapshot = Library::load(&root).unwrap();
        assert_eq!(snapshot.games[0].title, "Updated in WAL");
        drop(writer);
        std::fs::remove_dir_all(root).unwrap();
    }
}
