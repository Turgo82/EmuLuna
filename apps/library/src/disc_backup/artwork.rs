//! Cached Libretro box-art selection, matching the old BackupArt exact-title fallback.
use super::{err, run_capture};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::Path,
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        OnceLock,
    },
    time::{SystemTime, UNIX_EPOCH},
};
pub(super) fn preferred_region() -> &'static str {
    let locale = std::env::var("LC_ALL")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("LANG").ok())
        .unwrap_or_default()
        .to_ascii_uppercase();
    if locale.contains("_JP") {
        "Japan"
    } else if ["_US", "_CA", "_MX"].iter().any(|s| locale.contains(s)) {
        "USA"
    } else {
        "Europe"
    }
}
pub(crate) fn normalized_title(title: &str) -> String {
    static FOLDS: OnceLock<BTreeMap<char, String>> = OnceLock::new();
    let folds = FOLDS.get_or_init(|| {
        serde_json::from_str(include_str!("title-folds.json")).expect("title folds")
    });
    let mut plain = String::new();
    let mut chars = title.char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        if matches!(c, '(' | '[') {
            let end = if c == '(' { ')' } else { ']' };
            if let Some(offset) = title[index + c.len_utf8()..].find(end) {
                let stop = index + c.len_utf8() + offset;
                while chars.peek().is_some_and(|(i, _)| *i <= stop) {
                    chars.next();
                }
                continue;
            }
        }
        if c.is_ascii_alphanumeric() {
            plain.push(c.to_ascii_lowercase());
        } else if let Some(value) = folds.get(&c) {
            plain.push_str(value);
        } else {
            plain.push(' ');
        }
    }
    let mut words = plain
        .split_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if words
        .last()
        .is_some_and(|s| ["the", "a", "an"].contains(&s.as_str()))
    {
        words.rotate_right(1);
    }
    words.join(" ")
}
fn ranked_names(names: &[String], title: &str, region: &str) -> Vec<String> {
    let sanitized = title
        .chars()
        .map(|c| if "&*/:`<>?\\|\"".contains(c) { '_' } else { c })
        .collect::<String>()
        .to_lowercase();
    let normalized = normalized_title(title);
    let exact = names
        .iter()
        .filter(|n| {
            Path::new(n)
                .file_stem()
                .is_some_and(|s| s.to_string_lossy().to_lowercase() == sanitized)
        })
        .cloned()
        .collect::<Vec<_>>();
    let mut candidates = if exact.is_empty() {
        names
            .iter()
            .filter(|n| {
                !normalized.is_empty()
                    && Path::new(n)
                        .file_stem()
                        .is_some_and(|s| normalized_title(&s.to_string_lossy()) == normalized)
            })
            .cloned()
            .collect::<Vec<_>>()
    } else {
        exact
    };
    candidates.sort_by_key(|n| {
        (
            !n.split('(').skip(1).filter_map(|part| part.split_once(')')).any(|(tag, _)| {
                tag.split(',').any(|country| country.trim() == region)
            }),
            !n.contains("(World)"),
            !n.contains("(USA"),
            n.clone(),
        )
    });
    candidates.truncate(3);
    candidates
}
fn read_index(path: &Path) -> Option<Value> {
    let file = fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > 16 * 1024 * 1024 {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}
pub(crate) fn index(
    root: Option<&Path>,
    system: &str,
    repository: &str,
    cancel: &AtomicBool,
) -> Option<Vec<String>> {
    let path = root.map(|r| crate::storage::metadata(r).join(format!("libretro-{system}.json")));
    let saved = path.as_deref().and_then(read_index);
    let names = saved
        .as_ref()
        .and_then(|v| v["names"].as_array())
        .map(|names| {
            names
                .iter()
                .filter_map(Value::as_str)
                .filter(|s| s.len() <= 1024)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        });
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_secs_f64();
    let fresh = saved
        .as_ref()
        .and_then(|v| v["checked"].as_f64())
        .is_some_and(|t| now - t < 7. * 86400.);
    if fresh && names.is_some() {
        return names;
    }
    if cancel.load(Ordering::Relaxed) {
        return None;
    }
    let work = tempfile::tempdir().ok()?;
    let output = work.path().join("tree.json");
    let url=format!("https://api.github.com/repos/libretro-thumbnails/{repository}/git/trees/master?recursive=1");
    let downloaded = (|| {
        run_capture(
            Command::new("curl")
                .args([
                    "--fail",
                    "--silent",
                    "--location",
                    "--proto",
                    "=https",
                    "--max-time",
                    "15",
                    "--max-filesize",
                    "16777216",
                    "--output",
                ])
                .arg(&output)
                .arg(url),
            cancel,
        )
        .map_err(err)?;
        let tree = read_index(&output).ok_or("Invalid artwork index")?;
        if tree["truncated"].as_bool() == Some(true) {
            return Err("Artwork index is incomplete".to_owned());
        }
        let names = tree["tree"]
            .as_array()
            .ok_or("Missing artwork tree")?
            .iter()
            .filter(|v| v["type"] == "blob")
            .filter_map(|v| v["path"].as_str())
            .filter_map(|s| s.strip_prefix("Named_Boxarts/"))
            .filter(|s| s.ends_with(".png") && s.len() <= 1024)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if cancel.load(Ordering::Relaxed) {
            return Err("Cancelled".into());
        }
        if let Some(path) = &path {
            if let Some(parent) = path.parent() {
                if fs::create_dir_all(parent).is_ok() {
                    if let Ok(mut temp) = tempfile::NamedTempFile::new_in(parent) {
                        if let Ok(bytes) =
                            serde_json::to_vec(&serde_json::json!({"names":names,"checked":now}))
                        {
                            if temp.write_all(&bytes).is_ok() {
                                let _ = temp.persist(path);
                            }
                        }
                    }
                }
            }
        }
        Ok::<_, String>(names)
    })();
    downloaded.ok().or(names)
}
pub(super) fn urls(
    root: Option<&Path>,
    system: &str,
    repository: &str,
    title: &str,
    region: &str,
    cancel: &AtomicBool,
) -> Vec<String> {
    let Some(names) = index(root, system, repository, cancel) else {
        return vec![];
    };
    ranked_names(&names,title,region).into_iter().map(|name|{let encoded=name.bytes().map(|b|if b.is_ascii_alphanumeric()||b"-_.~".contains(&b){(b as char).to_string()}else{format!("%{b:02X}")}).collect::<String>();format!("https://raw.githubusercontent.com/libretro-thumbnails/{repository}/master/Named_Boxarts/{encoded}")}).collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_normalized_titles_handle_headers_regions_and_diacritics() {
        assert_eq!(
            normalized_title("ECCO THE DOLPHIN"),
            normalized_title("Ecco the Dolphin (USA)")
        );
        assert_eq!(
            normalized_title("Legend of Zelda, The [Rev A]"),
            "the legend of zelda"
        );
        assert_eq!(normalized_title("Pokémon"), "pokemon");
        assert_eq!(normalized_title("Strauß"), "strauss");
        assert_eq!(normalized_title("Cafe\u{301}"), "cafe");
        let names = [
            "Ecco the Dolphin (USA).png",
            "Ecco the Dolphin 2 (USA).png",
            "Ecco the Dolphin (Europe).png",
        ]
        .map(str::to_owned);
        assert_eq!(
            ranked_names(&names, "ECCO THE DOLPHIN", "USA"),
            vec![
                "Ecco the Dolphin (USA).png",
                "Ecco the Dolphin (Europe).png"
            ]
        );
    }
    #[test]
    fn existing_legacy_index_is_reused_without_network() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("metadata")).unwrap();
        let path = root.path().join("metadata/libretro-segacd.json");
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs_f64();
        fs::write(
            path,
            serde_json::to_vec(
                &serde_json::json!({"checked":now,"names":["Ecco the Dolphin (USA).png"]}),
            )
            .unwrap(),
        )
        .unwrap();
        let urls = urls(
            Some(root.path()),
            "segacd",
            "Sega_-_Mega-CD_-_Sega_CD",
            "ECCO THE DOLPHIN",
            "USA",
            &AtomicBool::new(false),
        );
        assert_eq!(urls.len(), 1);
        assert!(urls[0].ends_with("Ecco%20the%20Dolphin%20%28USA%29.png"));
    }
}
