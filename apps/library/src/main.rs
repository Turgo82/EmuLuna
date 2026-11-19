//! Native test library: read-only library data, isolated playback saves.
#![forbid(unsafe_code)]
mod actions;
mod activity;
mod alphabet;
mod artwork;
mod collections;
mod cover_search;
mod disc_backup;
mod game_table;
mod import_services;
mod importing;
mod kwin_restore;
mod launcher;
mod media;
mod notifications;
mod openvgdb;
mod preferences;
mod region;
mod removal;
mod resources;
mod resume;
mod selection;
mod settings;
mod startup;
mod storage;
mod window_state;
use eframe::egui::{self, Color32, FontId, Rect, RichText, Sense, Stroke, StrokeKind};
use emuluna_library_model::{fit_covers, Cover, Grid, Library, View};
use emuluna_theme::{self as theme, fonts};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Library,
    States,
    Screenshots,
}
impl Page {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "library" => Ok(Self::Library),
            "states" => Ok(Self::States),
            "screenshots" => Ok(Self::Screenshots),
            _ => Err("Unknown library page".into()),
        }
    }
}
fn library_tabs(ui: &mut egui::Ui, theme: &theme::Theme, selected: &mut Page) {
    const TAB_HEIGHT: f32 = 32.;
    const TAB_RADIUS: f32 = TAB_HEIGHT / 2.;
    const BORDER_WIDTH: f32 = 1.;
    egui::Frame::new()
        .fill(theme.color("navigation-background", ui.visuals().panel_fill))
        .stroke(Stroke::new(BORDER_WIDTH, theme.color("mid", Color32::GRAY)))
        .corner_radius(TAB_RADIUS + BORDER_WIDTH)
        .show(ui, |ui| {
            ui.visuals_mut().selection.bg_fill =
                theme.color("sidebar-selected-background", Color32::from_gray(70));
            ui.visuals_mut().selection.stroke.color =
                theme.color("sidebar-selected-text", ui.visuals().text_color());
            // Keep the selected pill at the same size as its hovered button.
            let widgets = &mut ui.visuals_mut().widgets;
            widgets.inactive.expansion = widgets.hovered.expansion;
            widgets.active.expansion = widgets.hovered.expansion;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.;
                for (page, label) in [
                    (Page::Library, "Library"),
                    (Page::States, "Save States"),
                    (Page::Screenshots, "Screenshots"),
                ] {
                    let response = ui
                        .add(
                            egui::Button::new(RichText::new(label).font(fonts::bold(14.)).color(
                                if *selected == page {
                                    ui.visuals().selection.stroke.color
                                } else {
                                    ui.visuals().text_color()
                                },
                            ))
                            .fill(if *selected == page {
                                ui.visuals().selection.bg_fill
                            } else {
                                Color32::TRANSPARENT
                            })
                            .stroke(Stroke::NONE)
                            .min_size(egui::vec2(
                                if page == Page::Library { 86. } else { 112. },
                                TAB_HEIGHT,
                            ))
                            .corner_radius(TAB_RADIUS),
                        )
                        .on_hover_text(label);
                    if response.hovered() && *selected != page {
                        ui.painter().rect_filled(
                            response.rect,
                            TAB_RADIUS,
                            ui.visuals().widgets.hovered.weak_bg_fill,
                        );
                        ui.painter().text(
                            response.rect.center(),
                            egui::Align2::CENTER_CENTER,
                            label,
                            fonts::bold(14.),
                            ui.visuals().text_color(),
                        );
                    }
                    if response.clicked() {
                        *selected = page;
                    }
                }
            });
        });
}
struct Arguments {
    setup: bool,
    library: PathBuf,
    page: Page,
    window_size: [f32; 2],
    restore_window: bool,
    preferences: PathBuf,
    settings_tab: Option<String>,
    data: PathBuf,
    theme: Option<String>,
    system: Option<String>,
    capture: Option<PathBuf>,
    scroll_test: Option<Duration>,
    test_play: Option<String>,
    test_resume: Option<PathBuf>,
    select_game: Option<String>,
}
fn arguments() -> Result<Arguments, String> {
    let mut args = std::env::args_os().skip(1);
    let mut result = Arguments {
        setup: false,
        library: PathBuf::new(),
        page: Page::Library,
        window_size: [1360., 900.],
        restore_window: true,
        preferences: PathBuf::new(),
        settings_tab: None,
        data: resources::path("data"),
        theme: None,
        system: None,
        capture: None,
        scroll_test: None,
        test_play: None,
        test_resume: None,
        select_game: None,
    };
    while let Some(flag) = args.next() {
        let value = args.next().ok_or("An option is missing its value")?;
        match flag.to_str() {
            Some("--window-size") => {
                result.restore_window=false;
                let (width,height)=value.to_str().ok_or("Invalid window size")?.split_once('x').ok_or("Use WIDTHxHEIGHT")?;
                let (width,height):(u32,u32)=(width.parse().map_err(|_|"Invalid width")?,height.parse().map_err(|_|"Invalid height")?);
                if !(680..=3840).contains(&width)||!(480..=2160).contains(&height){return Err("Window size exceeds preview limits".into());}
                result.window_size=[width as f32,height as f32];
            }
            Some("--page") => result.page = Page::parse(value.to_str().ok_or("Invalid page")?)?,
            Some("--settings-tab") => result.settings_tab = Some(value.into_string().map_err(|_| "Invalid settings tab")?),
            Some("--preferences") => result.preferences = value.into(),
            Some("--library") => result.library = value.into(),
            Some("--data") => result.data = value.into(),
            Some("--theme") => result.theme = Some(value.into_string().map_err(|_| "Invalid theme")?),
            Some("--system") => result.system = Some(value.into_string().map_err(|_| "Invalid console")?),
            Some("--select-game") => result.select_game = Some(value.into_string().map_err(|_| "Invalid game identity")?),
            Some("--test-resume") => result.test_resume = Some(value.into()),
            Some("--test-play") => result.test_play = Some(value.into_string().map_err(|_|"Invalid test game")?),
            Some("--capture") => result.capture = Some(value.into()),
            Some("--scroll-test") => {
                let seconds: u64 = value.to_str().ok_or("Invalid test duration")?.parse().map_err(|_| "Invalid test duration")?;
                if !(1..=30).contains(&seconds) { return Err("Scroll test must last 1..30 seconds".into()); }
                result.scroll_test = Some(Duration::from_secs(seconds));
            }
            _ => return Err("Usage: emuluna-native-library [--library DIR] [--data DIR] [--theme ID] [--system ID] [--capture NEW.png] [--scroll-test SECONDS] [--page library|states|screenshots] [--preferences FILE] [--settings-tab general|gameplay|cores]".into()),
        }
    }
    if result.library.as_os_str().is_empty() {
        if let Some(root) = storage::remembered_root()? {
            result.library = root;
        } else {
            result.library = storage::default_root();
            result.setup = true;
        }
    }
    if !result.setup {
        result.library = result.library.canonicalize().map_err(|e| {
            format!(
                "Could not open library at {}: {e}",
                result.library.display()
            )
        })?;
    }
    if result.preferences.as_os_str().is_empty() {
        result.preferences = storage::preferences_path(&result.library);
    }
    if result.test_resume.is_some() && result.test_play.is_none() {
        return Err("--test-resume requires --test-play".into());
    }
    if result.capture.as_ref().is_some_and(|p| p.exists()) {
        return Err("Capture destination must be a new file.".into());
    }
    Ok(result)
}
struct Loaded {
    library: Library,
    sizes: Vec<[u32; 2]>,
    rom_available: Vec<bool>,
    counts: HashMap<String, usize>,
    original_covers: HashMap<String, PathBuf>,
}
impl Loaded {
    fn load(root: &std::path::Path, preferences: &std::path::Path) -> Result<Self, String> {
        Library::load(root).map(|mut library| {
            let region = preferences::Preferences::load(preferences)
                .ok()
                .and_then(|prefs| prefs.get(region::KEY).map(str::to_owned))
                .or_else(|| library.settings.get(region::KEY).cloned());
            import_services::refresh_display_titles(
                &mut library,
                region.as_deref().unwrap_or(region::DEFAULT),
            );
            let original_covers: HashMap<_, _> = library
                .games
                .iter()
                .filter_map(|game| {
                    game.cover
                        .as_ref()
                        .map(|path| (game.id.clone(), path.clone()))
                })
                .collect();
            let artwork_root = settings::conversion::directory(&library.root, preferences);
            if let Ok(index) = settings::conversion::load_index(&artwork_root) {
                settings::conversion::apply(&mut library, &original_covers, &artwork_root, &index);
            }

            let mut counts = HashMap::new();
            let sizes = library
                .games
                .iter()
                .map(|game| {
                    *counts.entry(game.system.clone()).or_insert(0) += 1;
                    let fallback = library
                        .consoles
                        .iter()
                        .find(|c| c.id == game.system)
                        .map_or([256, 256], |c| c.cover_size);
                    game.cover
                        .as_deref()
                        .and_then(artwork::dimensions)
                        .unwrap_or(fallback)
                })
                .collect();
            let rom_available = library
                .games
                .iter()
                .map(|game| game.rom.is_file())
                .collect();
            Loaded {
                library,
                sizes,
                rom_available,
                counts,
                original_covers,
            }
        })
    }
}
struct App {
    storage_lock: std::fs::File,
    move_library: startup::MoveLibrary,
    preferences: preferences::Preferences,
    preferences_path: PathBuf,
    settings: settings::Settings,
    importer: importing::Importer,
    import_services: import_services::Services,
    backup: disc_backup::Backup,
    cover_search: cover_search::Picker,
    window_tracker: window_state::Tracker,
    page: Page,
    media: media::Browser,
    list_view: bool,
    resumes: resume::Index,
    restart: Option<emuluna_library_model::Game>,
    confirm_state: Option<(emuluna_library_model::Game, PathBuf)>,
    minimized_for_game: bool,
    loaded: Option<Loaded>,
    loading: mpsc::Receiver<Result<Loaded, String>>,
    error: Option<String>,
    data: PathBuf,
    theme: theme::Theme,
    themes: Vec<(String, String)>,
    explicit_theme: bool,
    art: artwork::Artwork,
    view: View,
    search: String,
    search_open: bool,
    notifications: notifications::Notifications,
    visible: Vec<usize>,
    alphabet_targets: std::collections::BTreeMap<char, usize>,
    fitted_covers: Vec<Cover>,
    grid: Grid,
    grid_width: f32,
    selected: selection::Selection,
    collection_editor: Option<collections::Editor>,
    delete_collection: Option<(i64, String)>,
    removal: removal::Removal,
    details: bool,
    show_log: bool,
    capture: Option<PathBuf>,
    capture_requested: bool,
    started: Instant,
    scroll_test: Option<Duration>,
    scroll_started: Option<Instant>,
    update_time: Duration,
    update_peak: Duration,
    update_count: u64,
    max_drawn: usize,
    last_drawn: usize,
    cover_scroll_target: Option<(Rect, egui::LayerId, f32)>,
    launcher: launcher::Launcher,
    presets: Vec<(String, String)>,
    test_preset: String,
    test_play: Option<String>,
    test_resume: Option<PathBuf>,
    test_started: bool,
}
impl App {
    fn new(ctx: &egui::Context, args: Arguments) -> Result<Self, String> {
        let storage_lock = storage::lock(&args.library)?;
        let preferences = preferences::Preferences::load(&args.preferences)?;
        fonts::install(&ctx);
        let themes = settings::discover_themes(&args.data, &args.preferences);
        let selection = args
            .theme
            .as_deref()
            .or_else(|| preferences.get("theme"))
            .unwrap_or("luna");
        let id = settings::effective_theme(&ctx, selection);
        let theme =
            theme::Theme::load(&settings::theme_root(&args.data, &args.preferences, id), id)?;
        theme.apply(&ctx);
        let (sender, loading) = mpsc::channel();
        let load_ctx = ctx.clone();
        let artwork_preferences = args.preferences.clone();
        thread::spawn(move || {
            let result = Loaded::load(&args.library, &artwork_preferences);
            let _ = sender.send(result);
            load_ctx.request_repaint();
        });
        let list_view = preferences.get("native.library_view") == Some("list");

        let mut settings = settings::Settings::default();
        if let Some(tab) = &args.settings_tab {
            settings.open_tab(tab)?;
        }
        if let Some(system) = &args.system {
            settings.select_console(system);
        }
        let test_preset = preferences
            .get("native.filter")
            .unwrap_or("nearest")
            .to_owned();
        let explicit_theme = args.theme.is_some() || preferences.get("theme").is_some();
        let view = args.system.map(View::System).unwrap_or_else(|| {
            preferences
                .get("native.last_view")
                .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
                .map_or_else(
                    || {
                        preferences
                            .get("native.last_console")
                            .map_or(View::All, |id| View::System(id.into()))
                    },
                    |value| match value["kind"].as_str() {
                        Some("system") => value["id"]
                            .as_str()
                            .map_or(View::All, |id| View::System(id.into())),
                        Some("collection") => {
                            value["id"].as_i64().map_or(View::All, View::Collection)
                        }
                        Some("recent") => View::Recent,
                        Some("favorites") => View::Favorites,
                        Some("added") => View::Added,
                        _ => View::All,
                    },
                )
        });
        let artwork_cache = storage::cover_cache(&args.preferences);
        let window_tracker = window_state::Tracker::new(
            &preferences,
            args.restore_window && args.capture.is_none() && args.scroll_test.is_none(),
        );
        Ok(Self {
            storage_lock,
            move_library: Default::default(),
            window_tracker,
            page: args.page,
            media: media::Browser::default(),
            list_view,
            resumes: resume::Index::default(),
            restart: None,
            confirm_state: None,
            minimized_for_game: false,
            preferences,
            preferences_path: args.preferences,
            settings,
            importer: Default::default(),
            import_services: Default::default(),
            backup: Default::default(),
            cover_search: Default::default(),
            loaded: None,
            loading,
            error: None,
            data: args.data,
            theme,
            themes,
            explicit_theme,
            art: artwork::Artwork::with_cache(ctx.clone(), Some(artwork_cache)),
            view,
            search: String::new(),
            search_open: false,
            notifications: Default::default(),
            visible: Vec::new(),
            alphabet_targets: Default::default(),
            fitted_covers: Vec::new(),
            grid: Grid::layout(1., &[]),
            grid_width: 0.,
            selected: {
                let mut selection = selection::Selection::default();
                selection.only(args.select_game);
                selection
            },
            collection_editor: None,
            delete_collection: None,
            removal: removal::Removal::default(),
            details: false,
            show_log: false,
            capture: args.capture,
            capture_requested: false,
            started: Instant::now(),
            scroll_test: args.scroll_test,
            scroll_started: None,
            update_time: Duration::ZERO,
            update_peak: Duration::ZERO,
            update_count: 0,
            max_drawn: 0,
            last_drawn: 0,
            cover_scroll_target: None,
            launcher: launcher::Launcher::new(ctx.clone()),
            presets: emuluna_shader_pack::Pack::open(&resources::path("build/rust/shader-pack"))
                .map(|pack| {
                    pack.keys()
                        .map(|(id, name)| (id.to_owned(), name.to_owned()))
                        .collect()
                })
                .unwrap_or_else(|_| vec![("nearest".into(), "Nearest Neighbor".into())]),
            test_preset,
            test_play: args.test_play,
            test_resume: args.test_resume,
            test_started: false,
        })
    }
    fn play(&mut self, game: emuluna_library_model::Game, frames: Option<u32>) {
        self.play_with_resume(game, frames, None);
    }
    fn play_with_resume(
        &mut self,
        game: emuluna_library_model::Game,
        frames: Option<u32>,
        resume: Option<PathBuf>,
    ) {
        if self.removal.busy() {
            return;
        }

        self.launch(game, frames, resume, false);
    }
    fn launch(
        &mut self,
        game: emuluna_library_model::Game,
        frames: Option<u32>,
        resume: Option<PathBuf>,
        start_from_boot: bool,
    ) {
        if let Some(data) = &self.loaded {
            self.launcher.start(launcher::Request {
                root: data.library.root.clone(),
                settings: {
                    let mut values = data.library.settings.clone();
                    values.extend(self.preferences.values.clone());
                    values
                },
                game,
                theme: self.theme.id.clone(),
                theme_root: settings::theme_root(
                    &self.data,
                    &self.preferences_path,
                    &self.theme.id,
                ),
                preset: self.test_preset.clone(),
                frames,
                resume,
                start_from_boot,
                progress_root: self
                    .preferences_path
                    .parent()
                    .unwrap_or(std::path::Path::new("."))
                    .join("progress"),
            });
        }
    }
    fn progress_root(&self) -> PathBuf {
        self.preferences_path
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join("progress")
    }
    fn apply_metadata(&mut self) {
        if let Some(data) = self.loaded.as_mut() {
            let region = settings::value(
                &self.preferences,
                &data.library,
                region::KEY,
                region::DEFAULT,
            )
            .to_owned();
            for console in &mut data.library.consoles {
                if let Some(name) = region::console_name(&console.id, &region) {
                    console.name = name.into();
                }
            }
            data.library
                .consoles
                .sort_by_cached_key(|console| console.name.to_lowercase());
            for (index, game) in data.library.games.iter_mut().enumerate() {
                if let Some(path) = self.preferences.get(&format!("game.{}.rom", game.id)) {
                    if game.rom != std::path::Path::new(path) {
                        game.rom = PathBuf::from(path);
                        data.rom_available[index] = game.rom.is_file();
                    }
                }

                if let Some(value) = self
                    .preferences
                    .get(&format!("game.{}.last_played", game.id))
                    .and_then(|s| s.parse::<f64>().ok())
                    .filter(|n| n.is_finite() && *n > 0.)
                {
                    game.last_played = Some(value);
                }
                if let Some(value) = self
                    .preferences
                    .get(&format!("game.{}.play_count", game.id))
                    .and_then(|s| s.parse::<u32>().ok())
                {
                    game.play_count = value;
                }

                if let Some(value) = self.preferences.get(&format!("game.{}.favorite", game.id)) {
                    game.favorite = value == "1";
                }
                if let Some(value) = self
                    .preferences
                    .get(&format!("game.{}.rating", game.id))
                    .and_then(|v| v.parse::<u8>().ok())
                    .filter(|v| *v <= 5)
                {
                    game.rating = value;
                }
            }
            data.library.refresh_smart_collections();
        }
    }
    fn save_preferences(&mut self) {
        if let Err(error) = self.preferences.save(&self.preferences_path) {
            self.launcher.message = format!("Could not save preferences: {error}");
        }
    }
    fn game_action(&mut self, action: actions::Action, ctx: &egui::Context) {
        match action {
            actions::Action::Play(game) => self.play(game, None),
            actions::Action::StartFromBoot(game) => {
                let autosave = self.loaded.as_ref().is_some_and(|data| {
                    let mut settings = data.library.settings.clone();
                    settings.extend(self.preferences.values.clone());
                    settings.get("native.keep_progress").is_none_or(|v| v != "0")
                        && launcher::autosave_enabled(&settings, &game.id)
                });
                if autosave {
                    self.restart = Some(game);
                } else {
                    self.launch(game, None, None, true);
                }
            }
            actions::Action::Details(id) => {
                self.selected.only(Some(id));
                self.details = true;
            }
            actions::Action::Favorite(id, value) => {
                self.preferences.set(
                    &format!("game.{id}.favorite"),
                    if value { "1" } else { "0" },
                );
                self.apply_metadata();
                self.save_preferences();
                self.rebuild();
            }
            actions::Action::Rating(id, value) => {
                self.preferences
                    .set(&format!("game.{id}.rating"), value.min(5).to_string());
                self.apply_metadata();
                self.save_preferences();
                self.rebuild();
            }
            actions::Action::Reveal(path) => {
                if let Err(error) = actions::open_path(path) {
                    self.launcher.message = error;
                }
            }
            actions::Action::NewCollection(ids) => {
                self.collection_editor = Some(collections::Editor::new(false, ids));
                ctx.request_repaint();
            }
            actions::Action::Remove(ids) => {
                if self.launcher.busy
                    || self.importer.busy
                    || self.backup.busy
                    || self.import_services.busy
                    || self.settings.sidebar_busy()
                    || self.removal.busy()
                {
                    self.launcher.message = "Close the current game or wait for library work to finish before removing games.".into();
                    return;
                }
                if let Some(data) = &self.loaded {
                    let native_progress =
                        storage::native_fallback(&data.library.root, &self.progress_root());
                    let cache = storage::cover_cache(&self.preferences_path);
                    let covers = data
                        .library
                        .games
                        .iter()
                        .filter_map(|game| {
                            game.cover
                                .as_ref()
                                .map(|path| (game.id.clone(), path.clone()))
                        })
                        .chain(
                            data.original_covers
                                .iter()
                                .map(|(id, path)| (id.clone(), path.clone())),
                        )
                        .collect();
                    self.removal.request(
                        ctx,
                        removal::Request {
                            root: data.library.root.clone(),
                            native_progress,
                            cache,
                            ids,
                            covers,
                        },
                    );
                }
            }
            actions::Action::FindCover(game) => {
                if let Some(data) = &self.loaded {
                    self.cover_search.open(game, &data.library.root, ctx);
                }
            }
        }
    }
    fn select_view(&mut self, view: View) {
        let saved = match &view {
            View::System(id) => serde_json::json!({"kind":"system","id":id}),
            View::Collection(id) => serde_json::json!({"kind":"collection","id":id}),
            View::Recent => serde_json::json!({"kind":"recent"}),
            View::Favorites => serde_json::json!({"kind":"favorites"}),
            View::Added => serde_json::json!({"kind":"added"}),
            View::All => serde_json::json!({"kind":"all"}),
        };
        self.preferences.set("native.last_view", saved.to_string());
        self.save_preferences();
        const LAST_CONSOLE: &str = "native.last_console";
        match &view {
            View::System(id) if self.preferences.get(LAST_CONSOLE) != Some(id.as_str()) => {
                self.preferences.set(LAST_CONSOLE, id.clone());
                self.save_preferences();
            }
            View::All if self.preferences.values.remove(LAST_CONSOLE).is_some() => {
                self.save_preferences();
            }
            _ => {}
        }
        self.view = view;
        self.selected.only(None);
        self.rebuild();
    }
    fn rebuild(&mut self) {
        if let Some(data) = &self.loaded {
            self.visible = data.library.visible(&self.view, &self.search);
            self.selected
                .retain_visible(&self.visible, &data.library.games);
            self.alphabet_targets.clear();
            for (position, &index) in self.visible.iter().enumerate() {
                self.alphabet_targets
                    .entry(alphabet::initial(&data.library.games[index].title))
                    .or_insert(position);
            }
            self.art.warm_box_art_previews(
                self.visible
                    .iter()
                    .filter_map(|&index| data.library.games[index].cover.clone())
                    .collect(),
            );
            self.media.set_games(
                self.visible
                    .iter()
                    .map(|i| data.library.games[*i].id.clone())
                    .collect(),
            );
            // Median aspect normalization depends on the selected games, not
            // window width. Resizing only reflows these fixed cover dimensions.
            let inputs: Vec<_> = self
                .visible
                .iter()
                .map(|&i| {
                    let game = &data.library.games[i];
                    let canonical = data
                        .library
                        .consoles
                        .iter()
                        .find(|c| c.id == game.system)
                        .map_or([256, 256], |c| c.cover_size);
                    (game.system.as_str(), data.sizes[i], canonical)
                })
                .collect();
            self.fitted_covers = fit_covers(&inputs);
            self.grid_width = 0.;
        }
    }
    fn layout(&mut self, width: f32) {
        if (width - self.grid_width).abs() < 0.5 {
            return;
        }
        self.grid_width = width;
        self.grid = Grid::layout(width, &self.fitted_covers);
    }

    fn capture(&mut self, ctx: &egui::Context) {
        let Some(path) = self.capture.as_ref() else {
            return;
        };
        let screenshot = ctx.input(|input| {
            input.events.iter().find_map(|event| {
                if let egui::Event::Screenshot { image, .. } = event {
                    Some(image.clone())
                } else {
                    None
                }
            })
        });
        if let Some(image) = screenshot {
            use image::ImageEncoder;
            let result = (|| -> Result<(), String> {
                let output = std::fs::File::create_new(path).map_err(|e| e.to_string())?;
                let bytes: Vec<u8> = image
                    .pixels
                    .iter()
                    .flat_map(|pixel| pixel.to_array())
                    .collect();
                image::codecs::png::PngEncoder::new(output)
                    .write_image(
                        &bytes,
                        image.width() as u32,
                        image.height() as u32,
                        image::ExtendedColorType::Rgba8,
                    )
                    .map_err(|e| e.to_string())
            })();
            eprintln!(
                "library_capture={path:?} result={result:?} cached_artwork_bytes={}",
                self.art.bytes()
            );
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else if !self.capture_requested
            && self.loaded.is_some()
            && self.art.idle()
            && self.resumes.idle()
            && !self.media.busy
            && self.started.elapsed() > Duration::from_secs(2)
        {
            self.capture_requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        } else {
            ctx.request_repaint_after(Duration::from_millis(30));
        }
        if self.started.elapsed() > Duration::from_secs(15) {
            eprintln!("Library capture timed out");
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}
impl eframe::App for App {
    fn raw_input_hook(&mut self, ctx: &egui::Context, input: &mut egui::RawInput) {
        let Some((rect, layer, row_step)) = self.cover_scroll_target else {
            return;
        };
        if input.viewport_id != egui::ViewportId::ROOT {
            return;
        }
        let mut pointer = ctx.input(|state| state.pointer.hover_pos());
        for event in &mut input.events {
            match event {
                egui::Event::PointerMoved(pos) => pointer = Some(*pos),
                egui::Event::PointerGone => pointer = None,
                egui::Event::MouseWheel { unit, delta, modifiers }
                    if *unit == egui::MouseWheelUnit::Line
                        && delta.y != 0.
                        && !modifiers.shift
                        && !modifiers.ctrl
                        && !modifiers.command
                        && !modifiers.mac_cmd
                        && pointer.is_some_and(|pos| {
                            rect.contains(pos) && ctx.layer_id_at(pos) == Some(layer)
                        }) =>
                {
                    *unit = egui::MouseWheelUnit::Point;
                    delta.y *= row_step * 0.5;
                }
                _ => {}
            }
        }
    }

    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.cover_scroll_target = None;
        if self.move_library.show(
            ctx,
            self.loaded.as_ref().map(|data| data.library.root.as_path()),
            &self.storage_lock,
            self.launcher.busy
                || self.importer.busy
                || self.backup.busy
                || self.import_services.busy
                || self.settings.sidebar_busy()
                || self.removal.busy(),
            &mut self.launcher.message,
        ) {
            return;
        }
        let update_started = Instant::now();
        if self.window_tracker.update(ctx, &mut self.preferences) {
            self.save_preferences();
        }
        let dropped = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect::<Vec<_>>()
        });
        if !dropped.is_empty() {
            self.importer.drop_paths(dropped);
        }
        if !self.importer.busy
            && ctx.input_mut(|i| {
                i.consume_shortcut(&egui::KeyboardShortcut::new(
                    egui::Modifiers::CTRL | egui::Modifiers::SHIFT,
                    egui::Key::O,
                ))
            })
        {
            self.importer.request(true);
        } else if !self.importer.busy
            && ctx.input_mut(|i| {
                i.consume_shortcut(&egui::KeyboardShortcut::new(
                    egui::Modifiers::CTRL,
                    egui::Key::O,
                ))
            })
        {
            self.importer.request(false);
        }
        let old_agreement = self
            .preferences
            .get("disc_backup.agreement_version")
            .unwrap_or("")
            .to_owned();
        let backup_changed = self.backup.show(
            ctx,
            self.loaded.as_ref().map(|d| d.library.root.as_path()),
            &mut self.preferences,
        );
        if old_agreement
            != self
                .preferences
                .get("disc_backup.agreement_version")
                .unwrap_or("")
        {
            self.save_preferences();
        }
        let cover_changed = self.loaded.as_ref().is_some_and(|data| {
            let quality = self
                .preferences
                .get("artwork.webp_quality")
                .or_else(|| {
                    data.library
                        .settings
                        .get("artwork.webp_quality")
                        .map(String::as_str)
                })
                .and_then(|value| value.parse().ok())
                .unwrap_or(85);
            self.cover_search.show(ctx, &data.library.root, quality)
        });
        let services_changed = self.import_services.poll(ctx);
        let import_changed = self
            .importer
            .show(ctx, self.loaded.as_ref().map(|d| d.library.root.as_path()));
        let removal_blocked = self.launcher.busy
            || self.importer.busy
            || self.backup.busy
            || self.import_services.busy
            || self.settings.sidebar_busy();
        let removed = self.removal.show(ctx, removal_blocked);
        if !self.removal.message.is_empty() {
            self.launcher.message = std::mem::take(&mut self.removal.message);
        }
        if let Some(removed) = &removed {
            for id in &removed.ids {
                let prefix = format!("game.{id}.");
                self.preferences
                    .values
                    .retain(|key, _| !key.starts_with(&prefix));
            }
            self.selected.only(None);
            self.resumes.refresh = true;
            self.media.refresh = true;
            self.save_preferences();
            self.launcher.message =
                format!("Removed {} game(s) from the library.", removed.ids.len());
            if let Some(warning) = &removed.warning {
                self.launcher.message.push(' ');
                self.launcher.message.push_str(warning);
            }
        }
        if let Some((id, name)) = self.delete_collection.clone() {
            let response = egui::Modal::new(egui::Id::new("delete-library-collection"))
                .frame(egui::Frame::window(&ctx.style()).inner_margin(18))
                .show(ctx, |ui| {
                    ui.set_max_width(380.);
                    ui.spacing_mut().interact_size.y = 34.;
                    ui.heading(format!("Delete {name}?"));
                    ui.label("Games in this collection will stay in your library.");
                    ui.add_space(8.);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Delete collection").clicked() {
                            self.delete_collection = None;
                            if let Some(data) = &mut self.loaded {
                                match collections::delete(&data.library, id) {
                                    Ok(()) => {
                                        data.library
                                            .collections
                                            .retain(|collection| collection.id != id);
                                        if self.view == View::Collection(id) {
                                            self.select_view(View::All);
                                        }
                                    }
                                    Err(error) => {
                                        self.launcher.message =
                                            format!("Could not delete collection: {error}")
                                    }
                                }
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            self.delete_collection = None;
                        }
                    });
                });
            if response.should_close() {
                self.delete_collection = None;
            }
        }
        let mut collection_saved = None;
        if let (Some(editor), Some(data)) = (&mut self.collection_editor, &self.loaded) {
            let (close, saved) = editor.show(ctx, &data.library);
            collection_saved = saved;
            if close {
                self.collection_editor = None;
            }
        }
        if backup_changed
            || import_changed
            || services_changed
            || cover_changed
            || removed.is_some()
            || collection_saved.is_some()
        {
            if let Some(data) = &self.loaded {
                match Loaded::load(&data.library.root, &self.preferences_path) {
                    Ok(updated) => {
                        if backup_changed || import_changed {
                            let known: std::collections::HashSet<_> = data
                                .library
                                .games
                                .iter()
                                .map(|game| game.id.as_str())
                                .collect();
                            let games = updated
                                .library
                                .games
                                .iter()
                                .filter(|game| !known.contains(game.id.as_str()))
                                .cloned()
                                .collect();
                            let mut values = updated.library.settings.clone();
                            values.extend(self.preferences.values.clone());
                            self.import_services
                                .queue(updated.library.root.clone(), games, values);
                            if import_changed {
                                self.launcher.message = self.importer.message.clone();
                            }
                        }
                        if services_changed {
                            self.launcher.message = self.import_services.message.clone();
                        }
                        self.loaded = Some(updated);
                        self.apply_metadata();
                        if let Some(id) = collection_saved {
                            self.select_view(View::Collection(id));
                        } else {
                            self.rebuild();
                        }
                    }
                    Err(error) => {
                        self.launcher.message = format!("Could not refresh the library: {error}")
                    }
                }
            }
        }
        let was_playing = self.launcher.busy;
        self.launcher.poll();
        if let Some(system) = self.launcher.controls_request.take() {
            self.settings.select_console(&system);
            let _ = self.settings.open_tab("controls");
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        if let Some(filter) = self.launcher.completed_filter.take() {
            self.preferences.set("native.filter", filter.clone());
            self.test_preset = filter;
            self.save_preferences();
        }
        if let Some(options) = self.launcher.completed_options.take() {
            for (key, value) in options {
                let console_option = (key.starts_with("core_option.")
                    || key == "n64.automatic_borders"
                    || key == "n64.preserve_crop_aspect"
                    || key == "integer_scale")
                    && value.len() <= 128;
                let shader_setting = (key.starts_with("shader_parameters.")
                    && value.len() <= 16384)
                    || (key.starts_with("filter_resolution.")
                        && matches!(value.as_str(), "0" | "720" | "1080"));
                let game_autosave = matches!(value.as_str(), "0" | "1")
                    && key
                        .strip_prefix("game.")
                        .and_then(|key| key.strip_suffix(".autosave"))
                        .is_some_and(|id| {
                            id.len() == 64 && id.bytes().all(|byte| byte.is_ascii_hexdigit())
                        });
                if key.len() <= 300 && (console_option || shader_setting || game_autosave) {
                    self.preferences.set(&key, value);
                }
            }
            self.save_preferences();
        }
        let minimize_library = self
            .preferences
            .get("experimental.minimize_library_during_game")
            .or_else(|| {
                self.loaded.as_ref().and_then(|data| {
                    data.library
                        .settings
                        .get("experimental.minimize_library_during_game")
                        .map(String::as_str)
                })
            })
            .unwrap_or("1")
            == "1";
        if self.minimized_for_game
            && (!self.launcher.busy || !minimize_library || self.settings.open)
        {
            self.minimized_for_game = false;
            let needs_restore = self
                .launcher
                .restore_on_exit
                .swap(false, std::sync::atomic::Ordering::AcqRel);
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            if self.launcher.busy && needs_restore && kwin_restore::available() {
                std::thread::spawn(|| {
                    if let Err(error) = kwin_restore::restore() {
                        eprintln!("Could not restore the library: {error}");
                    }
                });
            }
        } else if minimize_library
            && self.launcher.busy
            && self.launcher.ready
            && !self.minimized_for_game
            && !self.settings.open
            && self.test_play.is_none()
            // On KDE, KWin restores the Wayland window; elsewhere leave it visible.
            && (!kwin_restore::wayland() || kwin_restore::available())
            && !ctx.input(|input| input.viewport().minimized.unwrap_or(false))
        {
            self.minimized_for_game = true;
            self.launcher
                .restore_on_exit
                .store(true, std::sync::atomic::Ordering::Release);
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }
        if was_playing && !self.launcher.busy {
            self.media.refresh = true;
            self.resumes.refresh = true;
        }
        self.media.poll(ctx, &mut self.art);
        if self.media.poll_deletion(&mut self.art) {
            self.resumes.refresh = true;
        }
        if let Some(id) = self.launcher.started_game.take() {
            let count = self
                .loaded
                .as_ref()
                .and_then(|data| data.library.games.iter().find(|g| g.id == id))
                .map_or(0, |game| game.play_count);
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0., |d| d.as_secs_f64());
            self.preferences
                .set(&format!("game.{id}.last_played"), now.to_string());
            self.preferences.set(
                &format!("game.{id}.play_count"),
                count.saturating_add(1).to_string(),
            );
            self.apply_metadata();
            self.save_preferences();
            self.rebuild();
        }
        if self.test_play.is_some() && self.loaded.is_some() {
            if !self.test_started {
                self.test_started = true;
                let game = self
                    .loaded
                    .as_ref()
                    .unwrap()
                    .library
                    .games
                    .iter()
                    .find(|g| Some(&g.id) == self.test_play.as_ref())
                    .cloned();
                if let Some(game) = game {
                    let resume = self.test_resume.take();
                    self.play_with_resume(game, Some(180), resume);
                } else {
                    TEST_FAILED.store(true, std::sync::atomic::Ordering::Release);
                    eprintln!("native_launch_test failed: game was not found");
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            } else if !self.launcher.busy {
                self.test_play = None;
                if self.launcher.last_success == Some(true) {
                    eprintln!("{}", self.launcher.diagnostic_tail());
                    eprintln!("NATIVE_LIBRARY_LAUNCH_OK");
                } else {
                    TEST_FAILED.store(true, std::sync::atomic::Ordering::Release);
                    eprintln!(
                        "native_launch_test failed: {}\n{}",
                        self.launcher.message,
                        self.launcher.diagnostic_tail()
                    );
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        self.last_drawn = 0;
        self.art.update(ctx);
        if self.loaded.is_none() && self.error.is_none() {
            match self.loading.try_recv() {
                Ok(Ok(data)) => {
                    if !self.explicit_theme {
                        let selection = if data
                            .library
                            .settings
                            .get("appearance.use_system_theme")
                            .is_some_and(|s| s == "1")
                        {
                            "system"
                        } else {
                            &data.library.theme
                        };
                        let id = settings::effective_theme(ctx, selection);
                        if let Ok(theme) = theme::Theme::load(
                            &settings::theme_root(&self.data, &self.preferences_path, id),
                            id,
                        ) {
                            theme.apply(ctx);
                            self.theme = theme;
                        }
                    }
                    eprintln!(
                        "library_loaded={} header_scan_ms={:.1}",
                        data.library.games.len(),
                        self.started.elapsed().as_secs_f64() * 1000.
                    );
                    if let View::System(id) = &self.view {
                        if !data
                            .library
                            .consoles
                            .iter()
                            .any(|console| &console.id == id)
                        {
                            self.view = View::All;
                        }
                    }
                    if let View::Collection(id) = self.view {
                        if !data
                            .library
                            .collections
                            .iter()
                            .any(|collection| collection.id == id)
                        {
                            self.view = View::All;
                        }
                    }
                    self.loaded = Some(data);
                    self.apply_metadata();
                    self.rebuild();
                    ctx.request_repaint();
                }
                Ok(Err(error)) => self.error = Some(error),
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.error = Some("The library could not be loaded.".into())
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        let old_page = self.page;
        let old_list = self.list_view;
        self.resumes.poll();
        if let Some(data) = self
            .loaded
            .as_ref()
            .filter(|_| self.resumes.needs_refresh())
        {
            let mut values = data.library.settings.clone();
            values.extend(self.preferences.values.clone());
            self.resumes
                .ensure(&data.library, self.progress_root(), values, ctx);
        }
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::Comma)) {
            self.settings.open = true;
            if let View::System(system) = &self.view {
                self.settings.select_console(system);
            }
            self.settings.refresh_audio();
        }
        let mut changed_search = false;
        let session_message = self.launcher.message.as_str();
        let session_notice = if matches!(session_message, "Game running" | "Game closed")
            || session_message.starts_with("Opening ") && session_message.ends_with('…')
        {
            ""
        } else {
            session_message
        };
        self.notifications.update("session", session_notice, false);
        self.notifications.update(
            "settings",
            &self.settings.message,
            self.settings.sidebar_busy(),
        );
        self.notifications
            .update("import", &self.importer.message, self.importer.busy);
        self.notifications.update(
            "artwork",
            &self.import_services.message,
            self.import_services.busy,
        );
        self.notifications
            .update("media", &self.media.message, false);

        if let Some(data) = &self.loaded {
            self.settings.poll_updates(ctx, &self.preferences, &data.library);
        }
        self.notifications
            .update("updates", &self.settings.update_notification(), false);

        egui::TopBottomPanel::top("toolbar")
            .exact_height(66.)
            .frame(
                egui::Frame::new()
                    .fill(
                        self.theme
                            .color("navigation-background", Color32::from_gray(34)),
                    )
                    .inner_margin(egui::Margin::symmetric(20, 12)),
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    let menu = egui::menu::menu_custom_button(
                        ui,
                        egui::Button::new("")
                            .frame(false)
                            .min_size(egui::vec2(32., 32.)),
                        |ui| {
                            ui.set_min_width(205.);
                            ui.menu_button("Library", |ui| {
                                for (label, folder) in
                                    [("Import games…", false), ("Import folder…", true)]
                                {
                                    if ui
                                        .add_enabled(
                                            self.loaded.is_some() && !self.importer.busy,
                                            egui::Button::new(label),
                                        )
                                        .clicked()
                                    {
                                        self.importer.request(folder);
                                        ui.close_menu();
                                    }
                                }
                                ui.separator();

                                if ui
                                    .add_enabled(
                                        self.loaded.is_some(),
                                        egui::Button::new("Back up disc…"),
                                    )
                                    .clicked()
                                {
                                    self.backup.request();
                                    ui.close_menu();
                                }
                                if ui
                                    .add_enabled(
                                        self.loaded.is_some(),
                                        egui::Button::new("Import issues…"),
                                    )
                                    .clicked()
                                {
                                    if let Some(data) = &self.loaded {
                                        self.importer.show_issues(&data.library.root);
                                    }
                                    ui.close_menu();
                                }
                                if ui.button("Open library folder").clicked() {
                                    if let Some(data) = &self.loaded {
                                        if let Err(error) =
                                            actions::open_path(data.library.root.clone())
                                        {
                                            self.launcher.message = error;
                                        }
                                    }
                                    ui.close_menu();
                                }
                                ui.separator();
                                if ui.button("Quit").clicked() {
                                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                                }
                            });
                            if ui
                                .add_enabled(
                                    self.selected.primary.is_some(),
                                    egui::Button::new("Game information…"),
                                )
                                .clicked()
                            {
                                self.details = true;
                                ui.close_menu();
                            }
                            ui.separator();
                            if ui.button("Settings…                 Ctrl+,").clicked() {
                                self.settings.open = true;
                                self.settings.refresh_audio();
                                ui.close_menu();
                            }
                            if ui.button("Session log").clicked() {
                                self.show_log = true;
                                ui.close_menu();
                            }
                            if ui.button("About EmuLuna…").clicked() {
                                self.settings.about_open = true;
                                ui.close_menu();
                            }
                        },
                    );
                    let center = menu.response.rect.center();
                    for y in [-6., 0., 6.] {
                        ui.painter().line_segment(
                            [center + egui::vec2(-8., y), center + egui::vec2(8., y)],
                            Stroke::new(1.6_f32, ui.visuals().text_color()),
                        );
                    }
                    menu.response.on_hover_text("Application menu");
                    ui.add_space(216.);
                    let accent = ui.visuals().selection;
                    ui.visuals_mut().selection.bg_fill = self
                        .theme
                        .color("navigation-selected-background", accent.bg_fill);
                    ui.visuals_mut().selection.stroke.color = self
                        .theme
                        .color("navigation-selected-text", ui.visuals().text_color());
                    if actions::toolbar_button(
                        ui,
                        actions::ToolbarIcon::Grid,
                        "Grid view",
                        !self.list_view,
                    )
                    .clicked()
                    {
                        self.list_view = false;
                    }
                    if actions::toolbar_button(
                        ui,
                        actions::ToolbarIcon::List,
                        "List view",
                        self.list_view,
                    )
                    .clicked()
                    {
                        self.list_view = true;
                    }
                    ui.visuals_mut().selection = accent;
                    let tab_left = (260. + ctx.screen_rect().width()) * 0.5 - 155.;
                    ui.add_space((tab_left - ui.next_widget_position().x).max(12.));
                    library_tabs(ui, &self.theme, &mut self.page);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        self.notifications.show(ui, |ui| {
                            if let Some(page) = self.settings.update_release_page() {
                                if ui.link("View EmuLuna release notes").clicked() {
                                    if let Err(error) = actions::open_url(page) {
                                        self.launcher.message = error;
                                    }
                                }
                            }

                            if self.backup.busy {
                                self.backup.sidebar(ui);
                            }
                            if self.importer.busy {
                                self.importer.sidebar(ui);
                            }
                            if self.import_services.busy {
                                activity::sidebar(
                                    ui,
                                    "Updating library",
                                    &self.import_services.message,
                                    (0, 0),
                                    None,
                                );
                            }
                            if self.settings.sidebar_busy() {
                                self.settings.sidebar_activity(ui);
                            }
                            let mut close = false;
                            ui.horizontal_wrapped(|ui| {
                                if let Some(data) = &self.loaded {
                                    if !self.importer.busy && ui.button("Import issues…").clicked()
                                    {
                                        self.importer.show_issues(&data.library.root);
                                        close = true;
                                    }
                                }
                                if self.launcher.log.is_some() && ui.button("Session log").clicked()
                                {
                                    self.show_log = true;
                                    close = true;
                                }
                            });
                            close
                        });
                        let search_id = egui::Id::new("library-search-field");
                        let mut focus_search = ctx.input_mut(|input| {
                            input.consume_key(egui::Modifiers::CTRL, egui::Key::F)
                        });
                        let search_button = actions::toolbar_button(
                            ui,
                            actions::ToolbarIcon::Search,
                            "Search (Ctrl+F)",
                            self.search_open,
                        );
                        if search_button.clicked() {
                            if self.search_open && self.search.is_empty() {
                                self.search_open = false;
                                search_button.request_focus();
                            } else {
                                focus_search = true;
                            }
                        }
                        if focus_search {
                            self.search_open = true;
                        }
                        if self.search_open
                            && ctx.memory(|memory| {
                                memory.has_focus(search_id)
                                    || memory.had_focus_last_frame(search_id)
                            })
                            && ctx.input_mut(|input| {
                                input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)
                            })
                        {
                            changed_search |= !self.search.is_empty();
                            self.search.clear();
                            self.search_open = false;
                            search_button.request_focus();
                        }
                        let search_slide = ctx.animate_bool_with_time_and_easing(
                            search_id.with("slide"),
                            self.search_open,
                            0.18,
                            egui::emath::easing::quadratic_out,
                        );
                        if self.search_open || search_slide > 0. {
                            let frame = actions::search_frame(ui, &self.theme);
                            let size = egui::vec2(166., 30.) + frame.total_margin().sum();
                            let (field_rect, _) =
                                ui.allocate_exact_size(size, egui::Sense::hover());
                            let slide = egui::vec2(size.x * (1. - search_slide), 0.);
                            let mut field_ui = ui.new_child(
                                egui::UiBuilder::new()
                                    .id_salt("sliding-search")
                                    .max_rect(field_rect.translate(slide))
                                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
                            );
                            field_ui.set_clip_rect(ui.clip_rect().intersect(field_rect));
                            let search_field = frame.show(&mut field_ui, |ui| {
                                ui.set_width(166.);
                                ui.spacing_mut().item_spacing.x = 4.;
                                ui.with_layout(
                                    egui::Layout::left_to_right(egui::Align::Center),
                                    |ui| {
                                        let response = ui
                                            .add_sized(
                                                [132., 30.],
                                                actions::search_input(&mut self.search)
                                                    .id(search_id)
                                                    .interactive(self.search_open),
                                            )
                                            .on_hover_text("Escape to clear and close");
                                        if focus_search {
                                            response.request_focus();
                                            if let Some(mut state) =
                                                egui::TextEdit::load_state(ctx, search_id)
                                            {
                                                use egui::text::{CCursor, CCursorRange};
                                                state.cursor.set_char_range(Some(
                                                    CCursorRange::two(
                                                        CCursor::new(0),
                                                        CCursor::new(self.search.chars().count()),
                                                    ),
                                                ));
                                                egui::TextEdit::store_state(ctx, search_id, state);
                                            }
                                        }
                                        changed_search |= response.changed();
                                        if !self.search.is_empty()
                                            && actions::toolbar_button(
                                                ui,
                                                actions::ToolbarIcon::Clear,
                                                "Clear search",
                                                false,
                                            )
                                            .clicked()
                                        {
                                            self.search.clear();
                                            changed_search = true;
                                            response.request_focus();
                                        }
                                        response
                                    },
                                )
                                .inner
                            });
                            if self.search_open
                                && !focus_search
                                && self.search.is_empty()
                                && (search_field.response.clicked_elsewhere()
                                    || search_field.inner.lost_focus())
                            {
                                search_field.inner.surrender_focus();
                                self.search_open = false;
                                ctx.request_repaint();
                            }
                        }
                    });
                });
            });
        if old_list != self.list_view {
            self.preferences.set(
                "native.library_view",
                if self.list_view { "list" } else { "grid" },
            );
            self.save_preferences();
        }
        if old_page != self.page {
            self.media.refresh = true;
        }
        if self.page != Page::Library {
            let kind = if self.page == Page::States {
                media::Kind::States
            } else {
                media::Kind::Screenshots
            };
            self.media.select(kind);
            if let Some(data) = &self.loaded {
                let native = storage::native_fallback(&data.library.root, &self.progress_root());
                self.media.ensure(&data.library, native, ctx);
            }
        }
        if changed_search {
            self.rebuild();
        }
        let mut next_view = None;
        {
            egui::SidePanel::left("consoles")
                .exact_width(260.)
                .resizable(false)
                .frame(
                    egui::Frame::new()
                        .fill(
                            self.theme
                                .color("sidebar-background", Color32::from_gray(35)),
                        )
                        .inner_margin(egui::Margin::symmetric(12, 18)),
                )
                .show(ctx, |ui| {
                    ui.spacing_mut().item_spacing.y = 3.;
                    let active = self.settings.sidebar_busy()
                        || self.importer.busy
                        || self.backup.busy
                        || self.import_services.busy;
                    let popup_id = egui::Id::new("sidebar-add-menu");
                    if active && ctx.memory(|memory| memory.is_popup_open(popup_id)) {
                        ctx.memory_mut(|memory| memory.close_popup());
                    }
                    let recent = if active {
                        None
                    } else {
                        activity::Recent::get(ctx)
                    };
                    let expansion = ctx.animate_bool_with_time(
                        egui::Id::new("sidebar-activity-height"),
                        active || recent.is_some(),
                        0.16,
                    );
                    let activity_height = egui::lerp(34. ..=82., expansion);
                    egui::ScrollArea::vertical()
                        .max_height((ui.available_height() - activity_height - 8.).max(30.))
                        .id_salt("sidebar")
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new("Consoles")
                                    .strong()
                                    .color(self.theme.color("placeholder-text", Color32::GRAY)),
                            );
                            ui.add_space(8.);
                            if let Some(data) = &self.loaded {
                                for console in &data.library.consoles {
                                    let count = data.counts.get(&console.id).copied().unwrap_or(0);
                                    if count == 0
                                        && self
                                            .preferences
                                            .get("library.hide_empty_consoles")
                                            .or_else(|| {
                                                data.library
                                                    .settings
                                                    .get("library.hide_empty_consoles")
                                                    .map(String::as_str)
                                            })
                                            .unwrap_or("1")
                                            != "0"
                                    {
                                        continue;
                                    }
                                    let selected = self.view == View::System(console.id.clone());
                                    let (rect, response) = ui.allocate_exact_size(
                                        egui::vec2(ui.available_width(), 24.),
                                        Sense::click(),
                                    );
                                    if selected || response.hovered() {
                                        ui.painter().rect_filled(
                                            rect,
                                            5.,
                                            if selected {
                                                self.theme.color(
                                                    "sidebar-selected-background",
                                                    ui.visuals().selection.bg_fill,
                                                )
                                            } else {
                                                ui.visuals().widgets.hovered.weak_bg_fill
                                            },
                                        );
                                    }
                                    if let Some(texture) = self
                                        .art
                                        .texture(&self.data.join("icons").join(&console.icon))
                                    {
                                        let image = Rect::from_center_size(
                                            egui::pos2(rect.left() + 17., rect.center().y),
                                            egui::vec2(22., 22.),
                                        );
                                        ui.painter().image(
                                            texture,
                                            image,
                                            Rect::from_min_max(
                                                egui::Pos2::ZERO,
                                                egui::pos2(1., 1.),
                                            ),
                                            Color32::WHITE,
                                        );
                                    }
                                    let color = if selected {
                                        self.theme.color(
                                            "sidebar-selected-text",
                                            ui.visuals().text_color(),
                                        )
                                    } else {
                                        ui.visuals().text_color()
                                    };
                                    ui.painter().text(
                                        egui::pos2(rect.left() + 34., rect.center().y),
                                        egui::Align2::LEFT_CENTER,
                                        &console.name,
                                        fonts::bold(14.),
                                        color,
                                    );
                                    if response.clicked() {
                                        next_view = Some(View::System(console.id.clone()));
                                    }
                                    response.context_menu(|ui| {
                                        if ui.button("Controller settings…").clicked() {
                                            self.settings.select_console(&console.id);
                                            let _ = self.settings.open_tab("controls");
                                            ui.close_menu();
                                        }
                                        if ui.button("Select core…").clicked() {
                                            self.settings.select_console(&console.id);
                                            let _ = self.settings.open_tab("cores");
                                            ui.close_menu();
                                        }
                                    });
                                }
                            }
                            ui.add_space(12.);
                            ui.separator();
                            ui.add_space(10.);
                            ui.label(
                                RichText::new("Collections")
                                    .strong()
                                    .color(self.theme.color("placeholder-text", Color32::GRAY)),
                            );
                            ui.add_space(8.);
                            if let Some(data) = &self.loaded {
                                let added_cutoff = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .map_or(0., |d| d.as_secs_f64())
                                    - 30. * 86400.;
                                for (label, view, icon) in [
                                    ("All Games", View::All, actions::CollectionIcon::All),
                                    (
                                        "Recently Played",
                                        View::Recent,
                                        actions::CollectionIcon::Recent,
                                    ),
                                    (
                                        "Favorites",
                                        View::Favorites,
                                        actions::CollectionIcon::Favorite,
                                    ),
                                    (
                                        "Recently Added",
                                        View::Added,
                                        actions::CollectionIcon::Added,
                                    ),
                                ] {
                                    let count = data
                                        .library
                                        .games
                                        .iter()
                                        .filter(|game| match view {
                                            View::Favorites => game.favorite,
                                            View::Recent => game.last_played.is_some(),
                                            View::Added => game.added >= added_cutoff,
                                            _ => true,
                                        })
                                        .count();
                                    if actions::collection_row(
                                        ui,
                                        label,
                                        count,
                                        icon,
                                        self.view == view,
                                        &self.theme,
                                    )
                                    .clicked()
                                    {
                                        next_view = Some(view);
                                    }
                                }
                                for collection in &data.library.collections {
                                    let view = View::Collection(collection.id);
                                    let response = actions::collection_row(
                                        ui,
                                        &collection.name,
                                        collection.games.len(),
                                        actions::CollectionIcon::Folder,
                                        self.view == view,
                                        &self.theme,
                                    );
                                    if response.clicked() {
                                        next_view = Some(view);
                                    }
                                    response.context_menu(|ui| {
                                        ui.set_width(186.);
                                        ui.spacing_mut().item_spacing.y = 0.;
                                        if actions::menu_row(ui, "Rename…", "", true).clicked() {
                                            self.collection_editor =
                                                Some(collections::Editor::rename(
                                                    collection.id,
                                                    collection.name.clone(),
                                                ));
                                            ui.close_menu();
                                        }
                                        if actions::menu_row(ui, "Delete…", "", true).clicked() {
                                            self.delete_collection =
                                                Some((collection.id, collection.name.clone()));
                                            ui.close_menu();
                                        }
                                    });
                                }
                            }
                        });
                    ui.add_space((ui.available_height() - activity_height).max(0.));
                    if self.backup.busy {
                        self.backup.sidebar(ui);
                    } else if self.importer.busy {
                        self.importer.sidebar(ui);
                    } else if self.import_services.busy {
                        activity::sidebar(
                            ui,
                            "Updating library",
                            &self.import_services.message,
                            (0, 0),
                            None,
                        );
                    } else if self.settings.sidebar_busy() {
                        self.settings.sidebar_activity(ui);
                    } else if let Some(recent) = recent {
                        recent.show(ui);
                    } else {
                        let plus = ui.add_sized([32., 32.], egui::Button::new("").frame(false));
                        if plus.clicked() {
                            ctx.memory_mut(|memory| memory.toggle_popup(popup_id));
                        }
                        let menu_open = ctx.memory(|memory| memory.is_popup_open(popup_id));
                        if plus.hovered() || menu_open {
                            ui.painter().rect_filled(
                                plus.rect,
                                7.,
                                ui.visuals().widgets.hovered.weak_bg_fill,
                            );
                        }
                        if menu_open {
                            let mut close = false;
                            let menu = egui::Area::new(popup_id)
                                .order(egui::Order::Foreground)
                                .movable(false)
                                .fade_in(false)
                                .pivot(egui::Align2::LEFT_BOTTOM)
                                .fixed_pos(plus.rect.left_top() - egui::vec2(0., 4.))
                                .show(ctx, |ui| {
                                    egui::Frame::popup(ui.style())
                                        .fill(ui.visuals().window_fill)
                                        .corner_radius(7.)
                                        .inner_margin(6)
                                        .show(ui, |ui| {
                                            ui.set_width(250.);
                                            ui.spacing_mut().item_spacing.y = 0.;
                                            for (label, shortcut, folder) in [
                                                ("Import games…", "Ctrl+O", false),
                                                ("Import folder…", "Ctrl+Shift+O", true),
                                            ] {
                                                if actions::menu_row(
                                                    ui,
                                                    label,
                                                    shortcut,
                                                    self.loaded.is_some(),
                                                )
                                                .clicked()
                                                {
                                                    self.importer.request(folder);
                                                    close = true;
                                                }
                                            }
                                            if actions::menu_row(
                                                ui,
                                                "Back up disc…",
                                                "",
                                                self.loaded.is_some(),
                                            )
                                            .clicked()
                                            {
                                                self.backup.request();
                                                close = true;
                                            }
                                            ui.separator();
                                            for (label, smart) in [
                                                ("New collection…", false),
                                                ("New smart collection…", true),
                                            ] {
                                                if actions::menu_row(
                                                    ui,
                                                    label,
                                                    "",
                                                    self.loaded.is_some(),
                                                )
                                                .clicked()
                                                {
                                                    self.collection_editor = Some(
                                                        collections::Editor::new(smart, vec![]),
                                                    );
                                                    close = true;
                                                }
                                            }
                                        });
                                });
                            if close
                                || (!plus.clicked() && menu.response.clicked_elsewhere())
                                || ctx.input(|input| input.key_pressed(egui::Key::Escape))
                            {
                                ctx.memory_mut(|memory| memory.close_popup());
                            }
                        }
                        let center = plus.rect.center();
                        let stroke = Stroke::new(1.8_f32, ui.visuals().text_color());
                        ui.painter().line_segment(
                            [center - egui::vec2(7., 0.), center + egui::vec2(7., 0.)],
                            stroke,
                        );
                        ui.painter().line_segment(
                            [center - egui::vec2(0., 7.), center + egui::vec2(0., 7.)],
                            stroke,
                        );
                        plus.on_hover_text("Add games or collections");
                    }
                });
        }
        if let Some(view) = next_view {
            self.select_view(view);
        }
        let mut game_action = None;
        let mut media_action = None;
        let mut alphabet_jump = None;
        let show_alphabet = self
            .preferences
            .get("library.show_alphabet_index")
            .or_else(|| {
                self.loaded.as_ref().and_then(|data| {
                    data.library
                        .settings
                        .get("library.show_alphabet_index")
                        .map(String::as_str)
                })
            })
            .unwrap_or("1")
            == "1";
        if self.page == Page::Library && !self.visible.is_empty() && show_alphabet {
            let available = ctx.available_rect().shrink2(egui::vec2(0., 20.));
            let strip = Rect::from_min_size(
                egui::pos2(
                    available.right() - 28. - ctx.style().spacing.scroll.bar_width - 4.,
                    available.top(),
                ),
                egui::vec2(28., available.height()),
            );
            egui::Area::new(egui::Id::new("library-alphabet"))
                .order(egui::Order::Middle)
                .movable(false)
                .fixed_pos(strip.min)
                .show(ctx, |ui| {
                    ui.set_min_size(strip.size());
                    ui.set_max_size(strip.size());
                    alphabet_jump = alphabet::show(ui, &self.alphabet_targets);
                });
            if let Some(position) = alphabet_jump {
                self.selected.only(
                    self.loaded
                        .as_ref()
                        .map(|data| data.library.games[self.visible[position]].id.clone()),
                );
            }
        }
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(self.theme.color("base", Color32::from_gray(32)))
                    .inner_margin(egui::Margin {
                        top: if self.page == Page::Library && self.list_view {
                            0
                        } else {
                            20
                        },
                        bottom: 20,
                        ..egui::Margin::ZERO
                    }),
            )
            .show(ctx, |ui| {
                if let Some(error) = &self.error {
                    ui.centered_and_justified(|ui| {
                        ui.label(error);
                    });
                    return;
                }
                let Some(data) = self.loaded.as_ref() else {
                    ui.centered_and_justified(|ui| {
                        ui.spinner();
                    });
                    ctx.request_repaint_after(Duration::from_millis(30));
                    return;
                };
                let cover_radius = if settings::value(
                    &self.preferences,
                    &data.library,
                    "library.rounded_box_art",
                    "1",
                ) == "1"
                {
                    8
                } else {
                    0
                };
                if self.page != Page::Library {
                    media_action = self.media.show(
                        ui,
                        &mut self.art,
                        self.list_view,
                        self.launcher.busy,
                        cover_radius,
                    );
                    return;
                }
                if self.list_view {
                    let data = self.loaded.as_ref().unwrap();
                    let scroll_gutter = ui.spacing().scroll.allocated_width();
                    let table_width = (ui.available_width()
                        - scroll_gutter
                        - if show_alphabet && !self.visible.is_empty() {
                            28.
                        } else {
                            0.
                        })
                    .max(1.);
                    game_table::header(ui, table_width);
                    ui.spacing_mut().item_spacing.y = 0.;
                    if self.visible.is_empty() {
                        ui.centered_and_justified(|ui| ui.label("No games found"));
                        return;
                    }
                    let mut scroll = egui::ScrollArea::vertical()
                        .id_salt(("game-list", format!("{:?}", self.view), &self.search))
                        .auto_shrink([false, false]);
                    if let Some(position) = alphabet_jump {
                        scroll = scroll.vertical_scroll_offset(
                            position as f32 * (40. + ui.spacing().item_spacing.y),
                        );
                    }
                    scroll.show_rows(ui, 40., self.visible.len(), |ui, rows| {
                        for row in rows {
                            let index = self.visible[row];
                            let game = &data.library.games[index];
                            let (rect, response) = ui
                                .allocate_exact_size(egui::vec2(table_width, 40.), Sense::click());
                            let selected = self.selected.contains(&game.id);
                            if selected || response.hovered() {
                                ui.painter().rect_filled(
                                    rect,
                                    4.,
                                    if selected {
                                        self.theme.color(
                                            "sidebar-selected-background",
                                            ui.visuals().selection.bg_fill,
                                        )
                                    } else {
                                        ui.visuals().widgets.hovered.weak_bg_fill
                                    },
                                );
                            }
                            if !selected && !response.hovered() && row % 2 == 1 {
                                ui.painter().rect_filled(
                                    rect,
                                    0.,
                                    self.theme
                                        .color("alternate-base", ui.visuals().faint_bg_color),
                                );
                            }
                            let thumb = Rect::from_min_size(
                                egui::pos2(
                                    rect.left() + game_table::COVER_LEFT_INSET,
                                    rect.center().y - game_table::COVER_SIZE.y / 2.,
                                ),
                                game_table::COVER_SIZE,
                            );
                            if let Some(texture) = game
                                .cover
                                .as_deref()
                                .and_then(|p| self.art.box_art_texture(p))
                            {
                                let size = game
                                    .cover
                                    .as_deref()
                                    .and_then(|p| self.art.size(p))
                                    .unwrap_or(game_table::COVER_SIZE);
                                let scale = (thumb.width() / size.x).min(thumb.height() / size.y);
                                egui::Image::new((texture, size))
                                    .corner_radius(cover_radius / 2)
                                    .paint_at(
                                        ui,
                                        Rect::from_center_size(thumb.center(), size * scale),
                                    );
                            }
                            let system = data
                                .library
                                .consoles
                                .iter()
                                .find(|console| console.id == game.system)
                                .map_or(game.system.as_str(), |console| console.name.as_str());
                            let core_key = format!("core.{}", game.system);
                            let core = self
                                .preferences
                                .get(&core_key)
                                .or_else(|| {
                                    data.library.settings.get(&core_key).map(String::as_str)
                                })
                                .unwrap_or("auto");
                            let rating = if game.rating == 0 {
                                "—".into()
                            } else {
                                "★".repeat(game.rating as usize)
                            };
                            game_table::paint(
                                ui,
                                rect,
                                [
                                    &game.title,
                                    system,
                                    &rating,
                                    &game_table::date(game.last_played.unwrap_or(0.)),
                                    &game_table::date(game.added),
                                    &game.play_count.to_string(),
                                    game_table::core_name(core, &game.system),
                                    if data.rom_available[index] {
                                        "Available"
                                    } else {
                                        "Missing"
                                    },
                                ],
                                false,
                                self.theme.color("rating-filled", Color32::YELLOW),
                            );
                            let modifiers = ui.input(|input| input.modifiers);
                            if response.clicked() {
                                self.selected.click(
                                    &game.id,
                                    modifiers,
                                    &self.visible,
                                    &data.library.games,
                                );
                            }
                            if response.double_clicked() && !modifiers.ctrl && !modifiers.shift {
                                game_action = Some(actions::Action::Play(game.clone()));
                            }
                            if response.secondary_clicked() && !self.selected.contains(&game.id) {
                                self.selected.only(Some(game.id.clone()));
                            }
                            response.context_menu(|ui| {
                                actions::menu(
                                    ui,
                                    game,
                                    self.launcher.busy,
                                    self.resumes.has(&game.id),
                                    if self.selected.contains(&game.id) {
                                        self.selected.ids()
                                    } else {
                                        vec![game.id.clone()]
                                    },
                                    &mut game_action,
                                )
                            });
                        }
                    });
                    return;
                }
                let width = ui.available_width()
                    - ui.spacing().scroll.bar_width
                    - 8.
                    - if show_alphabet { 28. } else { 0. };
                self.layout(width);
                let data = self.loaded.as_ref().unwrap();
                if self.visible.is_empty() {
                    ui.centered_and_justified(|ui| {
                        ui.label("No games found");
                    });
                    return;
                }
                let row_height = self.grid.row_height;
                let row_step = row_height + ui.spacing().item_spacing.y;
                let mut scroll = egui::ScrollArea::vertical()
                    .id_salt(("covers", format!("{:?}", self.view), &self.search))
                    .auto_shrink([false, false]);
                if let Some(position) = alphabet_jump {
                    if let Some(row) = self
                        .grid
                        .rows
                        .iter()
                        .position(|row| row.iter().any(|place| place.index == position))
                    {
                        scroll = scroll.vertical_scroll_offset(row as f32 * row_step);
                    }
                }
                let grid = scroll.show_rows(ui, row_height, self.grid.rows.len(), |ui, rows| {
                    let nearby =
                        rows.start.saturating_sub(2)..(rows.end + 2).min(self.grid.rows.len());
                    for row in rows.clone() {
                        let (row_rect, _) =
                            ui.allocate_exact_size(egui::vec2(width, row_height), Sense::hover());
                        for place in &self.grid.rows[row] {
                            self.last_drawn += 1;
                            let game = &data.library.games[self.visible[place.index]];
                            let art = Rect::from_min_size(
                                row_rect.min + egui::vec2(place.x, 8.),
                                egui::vec2(place.cover.width, place.cover.height),
                            );
                            let response = ui.interact(
                                art.expand2(egui::vec2(5., 30.)),
                                ui.id().with(&game.id),
                                Sense::click(),
                            );
                            if self.selected.contains(&game.id) || ui.rect_contains_pointer(art) {
                                ui.painter().rect_stroke(
                                    art.expand(4.),
                                    cover_radius + 4,
                                    Stroke::new(
                                        2_f32,
                                        self.theme
                                            .color("highlight", ui.visuals().selection.bg_fill),
                                    ),
                                    StrokeKind::Outside,
                                );
                            }
                            if let Some(texture) = game
                                .cover
                                .as_deref()
                                .and_then(|p| self.art.box_art_texture(p))
                            {
                                egui::Image::new((texture, art.size()))
                                    .corner_radius(cover_radius)
                                    .paint_at(ui, art);
                            } else {
                                ui.painter().rect_filled(
                                    art,
                                    cover_radius,
                                    self.theme.color("alternate-base", Color32::from_gray(45)),
                                );
                                ui.painter().text(
                                    art.center(),
                                    egui::Align2::CENTER_CENTER,
                                    game.system.to_uppercase(),
                                    FontId::proportional(20.),
                                    self.theme.color("placeholder-text", Color32::GRAY),
                                );
                            }
                            let mut job = egui::text::LayoutJob::simple(
                                game.title.clone(),
                                if self.theme.strong_titles() {
                                    fonts::bold(14.)
                                } else {
                                    FontId::proportional(14.)
                                },
                                ui.visuals().text_color(),
                                art.width() + 20.,
                            );
                            job.wrap.max_rows = 2;
                            job.halign = egui::Align::Center;
                            let galley = ui.painter().layout_job(job);
                            let text_height = galley.size().y;
                            ui.painter().galley(
                                egui::pos2(art.center().x, art.bottom() + 7.),
                                galley,
                                ui.visuals().text_color(),
                            );
                            let star_y = art.bottom() + 7. + text_height + 3.;
                            let star_width = 14.;
                            let start = art.center().x - 2. * star_width;
                            for star in 0..5 {
                                ui.painter().text(
                                    egui::pos2(start + star as f32 * star_width, star_y),
                                    egui::Align2::CENTER_TOP,
                                    if star < game.rating { "★" } else { "☆" },
                                    FontId::proportional(15.),
                                    self.theme.color(
                                        if star < game.rating {
                                            "rating-filled"
                                        } else {
                                            "rating-empty"
                                        },
                                        Color32::GRAY,
                                    ),
                                );
                            }
                            for star in 0..5 {
                                let rect = Rect::from_center_size(
                                    egui::pos2(start + star as f32 * star_width, star_y + 8.),
                                    egui::vec2(14., 18.),
                                );
                                if ui
                                    .interact(
                                        rect,
                                        ui.id().with((&game.id, "rating", star)),
                                        Sense::click(),
                                    )
                                    .on_hover_text(format!("Rate {} stars", star + 1))
                                    .clicked()
                                {
                                    game_action =
                                        Some(actions::Action::Rating(game.id.clone(), star + 1));
                                }
                            }
                            let selecting =
                                ui.input(|input| input.modifiers.ctrl || input.modifiers.shift);
                            if !selecting
                                && (self.scroll_test.is_some()
                                    || actions::play_button_visible(
                                        ui,
                                        &response,
                                        !self.selected.multiple()
                                            && self.selected.contains(&game.id),
                                    ))
                            {
                                let button = Rect::from_center_size(
                                    art.center(),
                                    egui::vec2(
                                        if self.resumes.has(&game.id) {
                                            144.
                                        } else {
                                            100.
                                        },
                                        36.,
                                    ),
                                );
                                if let Some(action) = actions::hover_game_controls(
                                    ui,
                                    game,
                                    button,
                                    self.launcher.busy,
                                    self.resumes.has(&game.id),
                                ) {
                                    game_action = Some(action);
                                }
                            }
                            let modifiers = ui.input(|input| input.modifiers);
                            if response.clicked() {
                                self.selected.click(
                                    &game.id,
                                    modifiers,
                                    &self.visible,
                                    &data.library.games,
                                );
                            }
                            if response.double_clicked() && !modifiers.ctrl && !modifiers.shift {
                                game_action = Some(actions::Action::Play(game.clone()));
                            }
                            if response.secondary_clicked() && !self.selected.contains(&game.id) {
                                self.selected.only(Some(game.id.clone()));
                            }
                            response.context_menu(|ui| {
                                actions::menu(
                                    ui,
                                    game,
                                    self.launcher.busy,
                                    self.resumes.has(&game.id),
                                    if self.selected.contains(&game.id) {
                                        self.selected.ids()
                                    } else {
                                        vec![game.id.clone()]
                                    },
                                    &mut game_action,
                                )
                            });
                        }
                    }
                    // Visible art is queued first, followed by two nearby rows on either side.
                    for row in nearby.filter(|row| !rows.contains(row)) {
                        for place in &self.grid.rows[row] {
                            if let Some(path) = data.library.games[self.visible[place.index]]
                                .cover
                                .as_deref()
                            {
                                self.art.box_art_texture(path);
                            }
                        }
                    }
                    if self.scroll_test.is_some() {
                        ui.scroll_with_delta(egui::vec2(0., -48.));
                    }
                });
                self.cover_scroll_target = Some((grid.inner_rect, ui.layer_id(), row_step));
            });
        if let Some(action) = game_action {
            self.game_action(action, ctx);
        }
        if let Some(action) = media_action {
            match action {
                media::Action::Open(path) => {
                    if let Err(error) = actions::open_path(path) {
                        self.launcher.message = error;
                    }
                }
                media::Action::Delete(kind, entry) => self.media.request_delete(kind, entry),
                media::Action::Resume {
                    game,
                    path,
                    autosave,
                } => {
                    if let Some(game) = self
                        .loaded
                        .as_ref()
                        .and_then(|data| data.library.games.iter().find(|g| g.id == game))
                        .cloned()
                    {
                        let data = self.loaded.as_ref().unwrap();
                        let mut settings = data.library.settings.clone();
                        settings.extend(self.preferences.values.clone());
                        if !autosave
                            && launcher::autosave_enabled(&settings, &game.id)
                            && settings
                                .get("native.keep_progress")
                                .is_none_or(|v| v != "0")
                        {
                            self.confirm_state = Some((game, path));
                        } else {
                            self.play_with_resume(game, None, Some(path));
                        }
                    }
                }
            }
        }
        self.media.show_delete(ctx, self.launcher.busy);
        let mut settings_changed = false;
        if let Some(data) = &self.loaded {
            let settings_open = self.settings.open;
            let mut show_settings = |settings_ctx: &egui::Context| {
                self.settings.show(
                    settings_ctx,
                    &mut self.preferences,
                    &mut settings::Environment {
                        library: &data.library,
                        presets: &self.presets,
                        themes: &self.themes,
                        data: &self.data,
                        theme: &self.theme,
                        artwork: &mut self.art,
                        original_covers: &data.original_covers,
                        preferences_path: &self.preferences_path,
                        playing: self.launcher.busy,
                    },
                )
            };
            let changed = if settings_open {
                ctx.show_viewport_immediate(
                    egui::ViewportId::from_hash_of("settings-window"),
                    window_state::app_viewport()
                        .with_title("Settings — EmuLuna")
                        .with_inner_size([1160.0, 760.0])
                        .with_min_inner_size([680.0, 480.0]),
                    |settings_ctx, _| show_settings(settings_ctx),
                )
            } else {
                show_settings(ctx)
            };
            if changed {
                settings_changed = true;
                self.resumes.refresh = true;
                if let Some(id) = self.preferences.get("theme") {
                    let id = settings::effective_theme(ctx, id);
                    match theme::Theme::load(
                        &settings::theme_root(&self.data, &self.preferences_path, id),
                        id,
                    ) {
                        Ok(theme) => {
                            theme.apply(ctx);
                            self.theme = theme;
                            self.explicit_theme = true;
                        }
                        Err(error) => self.settings.message = error,
                    }
                }
                self.test_preset = self
                    .preferences
                    .get("native.filter")
                    .unwrap_or("nearest")
                    .into();
                match self.preferences.save(&self.preferences_path) {
                    Ok(()) => {}
                    Err(error) => {
                        self.settings.message = format!("Could not save settings: {error}")
                    }
                }
            }
        }
        if std::mem::take(&mut self.settings.move_library) {
            self.settings.open = false;
            self.move_library.request(ctx);
        }
        if let Some((root, index)) = self.settings.take_converted_artwork() {
            if let Some(data) = self.loaded.as_mut() {
                settings::conversion::apply(
                    &mut data.library,
                    &data.original_covers,
                    &root,
                    &index,
                );
            }
            for entry in index.values() {
                self.art.invalidate(&root.join(&entry.file));
            }
            self.rebuild();
        }
        if self.settings.reload_themes {
            self.settings.reload_themes = false;
            self.themes = settings::discover_themes(&self.data, &self.preferences_path);
        }
        if self.settings.rebuild_artwork {
            self.settings.rebuild_artwork = false;
            let cache = storage::cover_cache(&self.preferences_path);
            self.art = artwork::Artwork::with_cache(ctx.clone(), Some(cache));
        }
        if self.preferences.get("theme") == Some("system")
            || (!self.explicit_theme
                && self.loaded.as_ref().is_some_and(|data| {
                    data.library
                        .settings
                        .get("appearance.use_system_theme")
                        .is_some_and(|s| s == "1")
                }))
        {
            let id = settings::effective_theme(ctx, "system");
            if self.theme.id != id {
                if let Ok(theme) = theme::Theme::load(
                    &settings::theme_root(&self.data, &self.preferences_path, id),
                    id,
                ) {
                    theme.apply(ctx);
                    self.theme = theme;
                }
            }
        }
        if settings_changed {
            self.apply_metadata();
            self.rebuild();
        }
        if let Some(game) = self.restart.clone() {
            let response = egui::Modal::new(egui::Id::new("confirm-restart")).show(ctx, |ui| {
                ui.set_max_width(380.);
                ui.heading(format!("Restart {}?", game.title));
                ui.label("Start from the beginning instead of resuming? Your in-game saves are kept. The next automatic save will replace the current resume point.");
                ui.add_space(12.);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() { self.restart = None; }
                    if ui.add_enabled(!self.launcher.busy, egui::Button::new("Restart")).clicked() {
                        self.restart = None;
                        self.launch(game, None, None, true);
                    }
                });
            });
            if response.should_close() {
                self.restart = None;
            }
        }
        if let Some((game, path)) = self.confirm_state.clone() {
            let response = egui::Modal::new(egui::Id::new("confirm-library-state")).show(ctx, |ui| {
                ui.set_max_width(380.);
                ui.heading(format!("Load a save for {}?", game.title));
                ui.label("Autosave is on. When you close the game, the next autosave will replace your current resume point.");
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        self.confirm_state = None;
                    }
                    if ui.add_enabled(!self.launcher.busy, egui::Button::new("Load state")).clicked() {
                        self.confirm_state = None;
                        self.play_with_resume(game, None, Some(path));
                    }
                });
            });
            if response.should_close() {
                self.confirm_state = None;
            }
        }
        let mut play_game = None;
        if self.details {
            if let Some(game) = self.loaded.as_ref().and_then(|data| {
                data.library
                    .games
                    .iter()
                    .find(|g| Some(&g.id) == self.selected.primary.as_ref())
            }) {
                egui::Window::new(emuluna_theme::fonts::window_title("Game details"))
                    .collapsible(false)
                    .resizable(false)
                    .open(&mut self.details)
                    .show(ctx, |ui| {
                        ui.heading(&game.title);
                        ui.label(&game.system);
                        ui.label(format!("Played {} times", game.play_count));
                        ui.add_space(8.);
                        ui.label(game.rom.to_string_lossy());
                        ui.add_space(12.);
                        egui::ComboBox::from_id_salt("test-filter")
                            .selected_text(
                                self.presets
                                    .iter()
                                    .find(|(id, _)| id == &self.test_preset)
                                    .map_or("Nearest Neighbor", |(_, name)| name.as_str()),
                            )
                            .show_ui(ui, |ui| {
                                for (id, name) in &self.presets {
                                    ui.selectable_value(&mut self.test_preset, id.clone(), name);
                                }
                            });
                        if ui
                            .add_enabled(
                                !self.launcher.busy,
                                egui::Button::new(if self.resumes.has(&game.id) {
                                    "Resume"
                                } else {
                                    "Play"
                                }),
                            )
                            .clicked()
                        {
                            play_game = Some(game.clone());
                        }
                        ui.weak(
                            "Each launch uses fresh temporary saves. Your existing progress stays unchanged.",
                        );
                    });
            }
        }
        if let Some(game) = play_game {
            self.play(game, None);
        }
        if self.show_log {
            egui::Window::new(emuluna_theme::fonts::window_title("Session log"))
                .open(&mut self.show_log)
                .default_width(720.)
                .default_height(360.)
                .vscroll(true)
                .show(ctx, |ui| {
                    if let Some(path) = self.launcher.log.clone() {
                        ui.label(path.to_string_lossy());
                        if ui.button("Show log folder").clicked() {
                            if let Some(parent) = path.parent() {
                                if let Err(error) = actions::open_path(parent.to_owned()) {
                                    self.launcher.message = error;
                                }
                            }
                        }
                        ui.separator();
                        ui.monospace(self.launcher.diagnostic_tail());
                    } else {
                        ui.label("No game session log is available yet.");
                    }
                });
        }
        let elapsed = update_started.elapsed();
        self.update_time += elapsed;
        self.update_peak = self.update_peak.max(elapsed);
        self.update_count += 1;
        self.max_drawn = self.max_drawn.max(self.last_drawn);
        if let Some(duration) = self.scroll_test {
            if self.loaded.is_some() {
                let started = *self.scroll_started.get_or_insert_with(Instant::now);
                if started.elapsed() >= duration {
                    eprintln!("native_ui_test frames={} mean_update_ms={:.3} peak_update_ms={:.3} max_drawn_games={} cached_artwork_bytes={}", self.update_count, self.update_time.as_secs_f64()*1000./self.update_count as f64, self.update_peak.as_secs_f64()*1000., self.max_drawn, self.art.bytes());
                    assert!(self.art.bytes() <= 256 * 1024 * 1024 + 1024 * 96 * 96 * 4);
                    self.scroll_test = None;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            ctx.request_repaint_after(Duration::from_millis(16));
        }
        if !self.launcher.busy {
            self.settings.show_update_prompt(ctx);
        }
        if !self.launcher.busy
            && !self.backup.busy
            && !self.importer.busy
            && !self.import_services.busy
            && !self.settings.sidebar_busy()
            && !self.removal.busy()
        {
            self.settings.restart_for_update(ctx);
        }
        self.capture(ctx);
    }
}
static TEST_FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
fn main() {
    if let Some(result) = startup::command_line_migration() {
        if let Err(error) = result {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }
    let args = match arguments() {
        Ok(args) => args,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        viewport: window_state::app_viewport()
            .with_inner_size(args.window_size)
            .with_min_inner_size([850., 560.]),
        ..Default::default()
    };
    let mut options = options;
    if args.restore_window && args.capture.is_none() && args.scroll_test.is_none() {
        if let Ok(prefs) = preferences::Preferences::load(&args.preferences) {
            options.viewport = window_state::restore(options.viewport, &prefs);
        }
    }
    if let Err(error) = eframe::run_native(
        "EmuLuna — Native Preview",
        options,
        Box::new(move |cc| {
            startup::Startup::new(&cc.egui_ctx, args)
                .map(|app| Box::new(app) as Box<dyn eframe::App>)
                .map_err(|e| e.into())
        }),
    ) {
        eprintln!("{error}");
        std::process::exit(1);
    }
    if TEST_FAILED.load(std::sync::atomic::Ordering::Acquire) {
        std::process::exit(1);
    }
    if let Err(error) = settings::appimage_update::restart_if_ready() {
        eprintln!("EmuLuna update: {error}");
        std::process::exit(1);
    }
}
