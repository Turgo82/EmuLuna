//! First-run library selection and explicit relocation.
use crate::{
    artwork::Artwork,
    preferences::Preferences,
    settings::{self, onboarding},
    storage, App, Arguments,
};
use eframe::egui;
use emuluna_library_model::Library;
use emuluna_theme::{fonts, Theme};
use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
};

pub fn command_line_migration() -> Option<Result<(), String>> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new("--migrate-library")) {
        return None;
    }
    Some((|| {
        let destination = args
            .next()
            .map(PathBuf::from)
            .unwrap_or_else(storage::default_root);
        let source = args
            .next()
            .map(PathBuf::from)
            .or_else(storage::legacy_root)
            .ok_or("No previous EmuLuna library found.")?;
        if args.next().is_some() {
            return Err("Use --migrate-library [DESTINATION] [SOURCE].".into());
        }
        let config = storage::legacy_settings();
        let legacy = (!storage::managed(&source) && config.is_dir()).then_some(config.as_path());
        println!("Copying {} to {}…", source.display(), destination.display());
        let root = storage::migrate(&source, &destination, legacy, None)?;
        storage::remember(&root)?;
        println!(
            "EmuLuna will open {}. The old copy was kept.",
            root.display()
        );
        Ok(())
    })())
}

pub enum Startup {
    Setup(Box<Setup>),
    Library(Box<App>),
}

pub struct Setup {
    args: Option<Arguments>,
    library: Library,
    theme: Theme,
    themes: Vec<(String, String)>,
    artwork: Artwork,
    wizard: onboarding::Preview,
    task: Option<mpsc::Receiver<Result<PathBuf, String>>>,
}

impl Startup {
    pub fn new(ctx: &egui::Context, args: Arguments) -> Result<Self, String> {
        if !args.setup {
            return Ok(Self::Library(Box::new(App::new(ctx, args)?)));
        }
        fonts::install(ctx);
        let theme = Theme::builtin("luna")?;
        theme.apply(ctx);
        let themes = settings::discover_themes(&args.data, &args.preferences);
        let library = Library::empty(args.library.clone())?;
        let wizard = onboarding::Preview::setup(args.library.clone(), storage::legacy_root());
        Ok(Self::Setup(Box::new(Setup {
            args: Some(args),
            library,
            theme,
            themes,
            artwork: Artwork::with_cache(ctx.clone(), None),
            wizard,
            task: None,
        })))
    }
}

impl eframe::App for Startup {
    fn raw_input_hook(&mut self, ctx: &egui::Context, input: &mut egui::RawInput) {
        if let Self::Library(app) = self {
            app.raw_input_hook(ctx, input);
        }
    }

    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        match self {
            Self::Library(app) => app.update(ctx, frame),
            Self::Setup(setup) => {
                if let Some(root) = setup.update(ctx) {
                    let mut args = setup.args.take().unwrap();
                    args.setup = false;
                    args.preferences = storage::preferences_path(&root);
                    args.library = root;
                    match App::new(ctx, args) {
                        Ok(app) => *self = Self::Library(Box::new(app)),
                        Err(error) => {
                            eprintln!("Could not open the library: {error}");
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    }
                }
            }
        }
    }
}

impl Setup {
    fn update(&mut self, ctx: &egui::Context) -> Option<PathBuf> {
        if let Some(task) = &self.task {
            match task.try_recv() {
                Ok(Ok(root)) => {
                    self.wizard.close(ctx);
                    return Some(root);
                }
                Ok(Err(error)) => {
                    self.task = None;
                    self.wizard.show_error(error);
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.task = None;
                    self.wizard
                        .show_error("Library setup stopped unexpectedly.".into());
                }
                Err(mpsc::TryRecvError::Empty) => {
                    working(ctx, "Preparing your library…");
                    return None;
                }
            }
        }
        let args = self.args.as_ref().unwrap();
        self.artwork.update(ctx);
        self.wizard.show(
            ctx,
            &mut settings::Environment {
                library: &self.library,
                presets: &[],
                themes: &self.themes,
                data: &args.data,
                theme: &self.theme,
                artwork: &mut self.artwork,
                original_covers: &Default::default(),
                preferences_path: &args.preferences,
                playing: false,
            },
        );
        if let Some(choice) = self.wizard.take_choice() {
            self.task = Some(work(ctx, move || {
                let root = if choice.existing {
                    let root = choice.folder.canonicalize().map_err(|e| e.to_string())?;
                    Library::load(&root)?;
                    if !storage::managed(&root) {
                        return Err("This library uses the older folder layout. Use Migrate library to create a portable copy.".into());
                    }
                    let _lock = storage::lock(&root)?;
                    root
                } else if let Some(source) = choice.legacy {
                    let config = storage::legacy_settings();
                    storage::migrate(
                        &source,
                        &choice.folder,
                        config.is_dir().then_some(&config),
                        None,
                    )?
                } else {
                    let mut prefs = Preferences::default();
                    prefs.set("theme", choice.theme);
                    prefs.set(crate::region::KEY, choice.region);
                    storage::create(&choice.folder, &prefs, choice.copy)?
                };
                storage::remember(&root)?;
                Ok(root)
            }));
        } else if !self.wizard.open {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        None
    }
}

fn work<T: Send + 'static>(
    ctx: &egui::Context,
    task: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> mpsc::Receiver<Result<T, String>> {
    let (tx, rx) = mpsc::channel();
    let ctx = ctx.clone();
    thread::spawn(move || {
        let _ = tx.send(task());
        ctx.request_repaint();
    });
    rx
}

fn working(ctx: &egui::Context, text: &str) {
    if ctx.input(|input| input.viewport().close_requested()) {
        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
    }
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.36);
            ui.spinner();
            ui.heading(text);
        });
    });
}

#[derive(Default)]
pub struct MoveLibrary {
    picker: Option<mpsc::Receiver<Result<Option<PathBuf>, String>>>,
    target: Option<PathBuf>,
    copying: Option<mpsc::Receiver<Result<PathBuf, String>>>,
    moved: Option<PathBuf>,
}

impl MoveLibrary {
    pub fn request(&mut self, ctx: &egui::Context) {
        if self.picker.is_none() && self.target.is_none() {
            self.picker = Some(work(ctx, || {
                settings::tools::pick_path(
                    settings::tools::Picker::Folder,
                    "Choose where to move the EmuLuna folder",
                )
            }));
        }
    }

    // Returns true while the main library must stay idle.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        library: Option<&Path>,
        lock: &File,
        busy: bool,
        message: &mut String,
    ) -> bool {
        if let Some(picker) = &self.picker {
            match picker.try_recv() {
                Ok(result) => {
                    self.picker = None;
                    match result {
                        Ok(folder) => self.target = folder.map(|path| path.join(storage::FOLDER)),
                        Err(error) => *message = error,
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.picker = None;
                    *message = "The folder picker stopped unexpectedly.".into();
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if let Some(copying) = &self.copying {
            match copying.try_recv() {
                Ok(result) => {
                    self.copying = None;
                    match result {
                        Ok(path) => self.moved = Some(path),
                        Err(error) => *message = error,
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.copying = None;
                    *message = "The library copy stopped unexpectedly.".into();
                }
                Err(mpsc::TryRecvError::Empty) => {
                    working(ctx, "Moving your library…");
                    return true;
                }
            }
        }
        if let Some(path) = &self.moved {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(ui.available_height() * 0.32);
                    ui.heading("Library moved");
                    ui.monospace(path.display().to_string());
                    ui.label("Restart EmuLuna to open the library at its new location.");
                    ui.label("The previous folder was kept as a backup.");
                    if ui.button("Close EmuLuna").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
            });
            return true;
        }
        if let (Some(target), Some(library)) = (&self.target, library) {
            let mut accepted = false;
            let mut cancelled = false;
            let response =
                egui::Modal::new(egui::Id::new("move-library-confirm")).show(ctx, |ui| {
                    ui.heading("Move library?");
                    ui.label("EmuLuna will copy the library and use the new folder after restart.");
                    ui.monospace(target.display().to_string());
                    ui.weak("The current folder will be kept as a backup.");
                    if busy {
                        ui.label("Close the current game and wait for library tasks to finish.");
                    }
                    ui.horizontal(|ui| {
                        cancelled = ui.button("Cancel").clicked();
                        accepted = ui
                            .add_enabled(!busy, egui::Button::new("Move library"))
                            .clicked();
                    });
                });
            if accepted {
                let target = self.target.take().unwrap();
                let source = library.to_owned();
                match lock.try_clone() {
                    Ok(lock) => {
                        self.copying = Some(work(ctx, move || {
                            let root = storage::migrate(&source, &target, None, Some(lock))?;
                            storage::remember(&root)?;
                            Ok(root)
                        }));
                    }
                    Err(error) => *message = error.to_string(),
                }
            } else if cancelled || response.should_close() {
                self.target = None;
            }
        }
        self.copying.is_some()
    }
}
