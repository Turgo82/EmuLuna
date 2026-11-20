use emuluna_library_model::Game;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

pub struct Version {
    pub game: usize,
    pub label: String,
}

pub struct Group {
    pub preference: String,
    pub versions: Vec<Version>,
}

pub struct Index {
    groups: Vec<Group>,
    membership: Vec<Option<(usize, usize)>>,
}

impl Index {
    pub fn new(games: &[Game]) -> Self {
        let mut candidates = BTreeMap::new();
        for (index, game) in games.iter().enumerate() {
            let filename = game
                .original_filename
                .as_deref()
                .map(Path::new)
                .filter(|path| path.file_name().is_some())
                .unwrap_or(&game.rom);
            let Some(stem) = filename.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            let (base, tags) = release_name(stem);
            candidates
                .entry((&game.system, base.to_lowercase()))
                .or_insert_with(Vec::new)
                .push((index, tags));
        }

        let mut groups = Vec::new();
        let mut membership = vec![None; games.len()];
        for ((system, base), mut members) in candidates {
            if members.len() < 2 || members.iter().all(|(_, tags)| tags.is_empty()) {
                continue;
            }
            members.sort_by_cached_key(|(index, tags)| {
                (
                    tags.iter()
                        .map(|tag| version_order(tag))
                        .collect::<Vec<_>>(),
                    &games[*index].id,
                )
            });
            let group = groups.len();
            use sha2::{Digest, Sha256};
            let preference = format!(
                "native.version.{:x}",
                Sha256::digest(format!("{system}\0{base}").as_bytes())
            );
            groups.push(Group {
                preference,
                versions: members
                    .into_iter()
                    .enumerate()
                    .map(|(rank, (game, tags))| {
                        membership[game] = Some((group, rank));
                        Version {
                            game,
                            label: if tags.is_empty() {
                                "Initial release".into()
                            } else {
                                tags.join(" · ")
                            },
                        }
                    })
                    .collect(),
            });
        }
        Self { groups, membership }
    }

    pub fn group(&self, index: usize) -> Option<&Group> {
        self.membership[index].map(|(group, _)| &self.groups[group])
    }

    pub fn for_game(&self, index: usize) -> &[Version] {
        self.group(index).map_or(&[], |group| &group.versions)
    }

    pub fn playback_index(
        &self,
        index: usize,
        games: &[Game],
        available: &[bool],
        preferences: &crate::preferences::Preferences,
    ) -> usize {
        self.group(index)
            .and_then(|group| {
                let id = preferences.get(&group.preference)?;
                group
                    .versions
                    .iter()
                    .find(|entry| available[entry.game] && games[entry.game].id == id)
            })
            .map_or(index, |entry| entry.game)
    }

    pub fn count(&self, indices: impl IntoIterator<Item = usize>) -> usize {
        indices
            .into_iter()
            .map(|index| {
                self.for_game(index)
                    .first()
                    .map_or(index, |version| version.game)
            })
            .collect::<HashSet<_>>()
            .len()
    }

    pub fn collapse(&self, visible: Vec<usize>, available: &[bool]) -> Vec<usize> {
        let mut positions = HashMap::new();
        let mut result = Vec::new();
        for index in visible {
            if let Some((group, rank)) = self.membership[index] {
                let (position, best_rank) = positions.entry(group).or_insert_with(|| {
                    result.push(index);
                    (result.len() - 1, rank)
                });
                if (!available[index], rank) < (!available[result[*position]], *best_rank) {
                    result[*position] = index;
                    *best_rank = rank;
                }
            } else {
                result.push(index);
            }
        }
        result
    }
}

fn release_name(stem: &str) -> (String, Vec<String>) {
    let mut title = stem;
    let mut qualifiers = Vec::new();
    let mut tags = Vec::new();
    while let Some((base, group)) = title.strip_suffix(')').and_then(|s| s.rsplit_once(" (")) {
        if base.is_empty() || group.contains(['(', ')']) {
            break;
        }
        if let Some(label) = version_label(group) {
            tags.push(label);
        } else {
            qualifiers.push(group);
        }
        title = base;
    }
    let mut base = title.to_owned();
    for qualifier in qualifiers.into_iter().rev() {
        base.push_str(" (");
        base.push_str(qualifier);
        base.push(')');
    }
    tags.reverse();
    (base, tags)
}

fn version_label(tag: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    if let Some(value) = lower.strip_prefix("rev ") {
        if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
            return Some(format!("Rev {}", value.to_ascii_uppercase()));
        }
    }
    if let Some(value) = lower.strip_prefix('v') {
        if value.as_bytes().first().is_some_and(u8::is_ascii_digit)
            && value.split('.').all(|part| {
                !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_alphanumeric())
            })
        {
            return Some(format!("v{value}"));
        }
    }
    if lower == "alt" {
        return Some("Alt".into());
    }
    if let Some(value) = lower.strip_prefix("alt ") {
        if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
            return Some(format!("Alt {}", value.to_ascii_uppercase()));
        }
    }
    None
}

fn version_order(label: &str) -> (bool, Vec<(Option<u64>, String)>) {
    let parts = label
        .trim_start_matches('v')
        .split([' ', '.'])
        .map(|part| match part.parse::<u64>() {
            Ok(number) => (Some(number), String::new()),
            Err(_) => (None, part.to_owned()),
        })
        .collect();
    (label.starts_with("Alt"), parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_only_explicit_release_tags() {
        assert_eq!(
            release_name("Mystic Defender (USA, Europe) (Rev A)"),
            ("Mystic Defender (USA, Europe)".into(), vec!["Rev A".into()])
        );
        assert_eq!(
            release_name("Example (Japan) (v1.10) (Alt 2) (Beta)"),
            (
                "Example (Japan) (Beta)".into(),
                vec!["v1.10".into(), "Alt 2".into()]
            )
        );
        assert_eq!(
            release_name("Example (Disc 2) (Vintage Edition)"),
            ("Example (Disc 2) (Vintage Edition)".into(), vec![])
        );
        assert!(version_order("v1.2") < version_order("v1.10"));
    }

    #[test]
    fn groups_revisions_without_losing_other_regions_or_their_roms() {
        let dir = tempfile::tempdir().unwrap();
        let db = rusqlite::Connection::open(dir.path().join("library.sqlite3")).unwrap();
        db.execute_batch(include_str!("library_schema.sql"))
            .unwrap();
        for (id, system, filename) in [
            ("initial", "genesis", "Mystic Defender (USA, Europe).md"),
            (
                "rev-a",
                "genesis",
                "Mystic Defender (USA, Europe) (Rev A).md",
            ),
            ("alt", "genesis", "Mystic Defender (USA, Europe) (Alt).md"),
            ("japan", "genesis", "Mystic Defender (Japan).md"),
            ("other-console", "snes", "Mystic Defender (USA, Europe).sfc"),
        ] {
            db.execute(
                "INSERT INTO games(id,title,system,rom_path,source,original_filename,added)
                 VALUES (?1, 'Mystic Defender', ?2, ?1, '', ?3, 1)",
                rusqlite::params![id, system, filename],
            )
            .unwrap();
        }
        let library = emuluna_library_model::Library::load(dir.path()).unwrap();
        let index = Index::new(&library.games);
        let locate = |id| library.games.iter().position(|game| game.id == id).unwrap();
        let initial = locate("initial");
        let revision = locate("rev-a");
        let alt = locate("alt");
        let group = index.for_game(initial);
        assert_eq!(
            group
                .iter()
                .map(|v| (v.game, v.label.as_str()))
                .collect::<Vec<_>>(),
            [
                (initial, "Initial release"),
                (revision, "Rev A"),
                (alt, "Alt")
            ]
        );
        let mut available = vec![true; library.games.len()];
        let visible: Vec<_> = (0..library.games.len()).collect();
        assert_eq!(index.count(visible.iter().copied()), 3);
        let collapsed = index.collapse(visible.clone(), &available);
        assert_eq!(collapsed.len(), 3);
        assert!(collapsed.contains(&initial));
        assert!(collapsed.contains(&locate("japan")));
        assert!(collapsed.contains(&locate("other-console")));
        let mut preferences = crate::preferences::Preferences::default();
        let group_key = &index.group(initial).unwrap().preference;
        let playback = |available: &[bool], preferences: &crate::preferences::Preferences| {
            index.playback_index(initial, &library.games, available, preferences)
        };
        assert_eq!(playback(&available, &preferences), initial);
        preferences.set(group_key, "rev-a");
        assert_eq!(playback(&available, &preferences), revision);
        available[revision] = false;
        assert_eq!(playback(&available, &preferences), initial);
        available[revision] = true;
        preferences.set(group_key, "japan");
        assert_eq!(playback(&available, &preferences), initial);
        available[initial] = false;
        assert!(index.collapse(visible, &available).contains(&revision));
        assert_eq!(index.collapse(vec![alt], &available), [alt]);
    }
}
