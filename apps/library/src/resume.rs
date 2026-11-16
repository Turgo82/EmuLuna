//! Background resume index: no save-file access or core hashing while painting.
use emuluna_library_model::Library;
use emuluna_state_file::{Identity, Renderer};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
    sync::mpsc,
};

pub struct Index {
    games: HashSet<String>,
    pending: Option<mpsc::Receiver<HashSet<String>>>,
    pub refresh: bool,
}
impl Default for Index {
    fn default() -> Self {
        Self {
            games: HashSet::new(),
            pending: None,
            refresh: true,
        }
    }
}
impl Index {
    pub fn idle(&self) -> bool {
        !self.refresh && self.pending.is_none()
    }
    pub fn needs_refresh(&self) -> bool {
        self.refresh && self.pending.is_none()
    }
    pub fn has(&self, id: &str) -> bool {
        self.games.contains(id)
    }
    pub fn poll(&mut self) {
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok(games) => {
                    self.games = games;
                    self.pending = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.pending = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
    }
    pub fn ensure(
        &mut self,
        library: &Library,
        progress: PathBuf,
        settings: BTreeMap<String, String>,
        ctx: &eframe::egui::Context,
    ) {
        if !self.refresh || self.pending.is_some() {
            return;
        }
        self.refresh = false;
        self.games.clear();
        let root = library.root.clone();
        let progress = crate::storage::native_fallback(&root, &progress);
        let games: Vec<_> = library
            .games
            .iter()
            .map(|g| (g.id.clone(), g.system.clone()))
            .collect();
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let mut available = HashSet::new();
            if settings
                .get("native.keep_progress")
                .is_none_or(|v| v != "0")
            {
                let catalog: serde_json::Value =
                    serde_json::from_str(include_str!("../../../data/cores.json"))
                        .expect("built-in catalog");
                let mut cores = HashMap::new();
                for (id, system) in games {
                    if !crate::launcher::autosave_enabled(&settings, &id) {
                        continue;
                    }
                    let shared = crate::launcher::autosave_path(&root, &progress, &system, &id);
                    if shared.is_file() {
                        if emuluna_state_file::inspect(&shared).is_ok_and(|identity| {
                            crate::launcher::state_core(&root, &system, &id, &identity).is_ok()
                        }) {
                            available.insert(id);
                        }
                        continue;
                    }
                    let core = cores.entry(system.clone()).or_insert_with(|| {
                        emuluna_launch_config::resolve_installed(
                            &root,
                            &system,
                            settings
                                .get(&format!("core.{system}"))
                                .map_or("auto", String::as_str),
                        )
                        .ok()
                    });
                    let Some(core) = core else {
                        continue;
                    };
                    let definition = &catalog[core.catalog_id.as_deref().unwrap_or(&core.id)];
                    let gl = definition["hardware_required"].as_bool() == Some(true)
                        || (definition.get("hardware_options").is_some()
                            && settings
                                .get("experimental.hardware_rendering")
                                .is_none_or(|v| v != "0"));
                    let expected = Identity {
                        core: format!("libretro:{}:{}", core.id, core.sha256),
                        rom: id.clone(),
                        renderer: if gl {
                            Renderer::OpenGl
                        } else {
                            Renderer::Software
                        },
                    };
                    let directory = crate::storage::native_media(
                        &root, &progress, &system, &id, &core.id, "states",
                    )
                    .join(&core.sha256);
                    let path = emuluna_state_file::readable_path(
                        &emuluna_state_file::automatic_path(&directory),
                    );
                    if emuluna_state_file::inspect(&path).is_ok_and(|actual| actual == expected) {
                        available.insert(id);
                    }
                }
            }
            let _ = tx.send(available);
            ctx.request_repaint();
        });
    }
}
