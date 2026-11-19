use super::appimage_update::{self, PreparedUpdate};
use eframe::egui;
use semver::Version;
use serde::Deserialize;
use std::{
    process::Command,
    sync::{atomic::AtomicBool, mpsc},
};

pub const AUTO: &str = "updates.check_automatically";
pub const VERSION: &str = include_str!("../../../../VERSION");
const REPO: &str = "https://github.com/Turgo82/EmuLuna";

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, PartialEq)]
enum Status {
    Current,
    Available {
        version: Version,
        page: String,
        appimage: Option<String>,
        checksum: Option<String>,
    },
}

fn compare(payload: &[u8], installed: &str) -> Result<Status, String> {
    let release: GithubRelease = serde_json::from_slice(payload).map_err(|e| e.to_string())?;
    let parse = |value: &str| {
        let value = value.trim();
        Version::parse(value.strip_prefix('v').unwrap_or(value)).map_err(|e| e.to_string())
    };
    let version = parse(&release.tag_name)?;
    if release.draft
        || release.prerelease
        || !version.pre.is_empty()
        || !version.cmp_precedence(&parse(installed)?).is_gt()
    {
        return Ok(Status::Current);
    }
    let prefix = format!("{REPO}/releases/download/{}/", release.tag_name);
    let mut appimages = release.assets.iter().filter(|asset| {
        asset.name.ends_with(".AppImage")
            && asset.browser_download_url == format!("{prefix}{}", asset.name)
    });
    let appimage = appimages.next().filter(|_| appimages.next().is_none());
    let checksum = appimage.and_then(|image| {
        let name = format!("{}.sha256", image.name);
        let url = format!("{prefix}{name}");
        release
            .assets
            .iter()
            .any(|asset| asset.name == name && asset.browser_download_url == url)
            .then_some(url)
    });
    Ok(Status::Available {
        page: format!("{REPO}/releases/tag/{}", release.tag_name),
        version,
        appimage: appimage.map(|asset| asset.browser_download_url.clone()),
        checksum,
    })
}

#[derive(Default)]
pub struct Checker {
    started: bool,
    automatic_check: bool,
    prompt_pending: bool,
    receiver: Option<mpsc::Receiver<Result<Status, String>>>,
    status: Option<Result<Status, String>>,
    browser_error: Option<String>,
    download: Option<mpsc::Receiver<Result<PreparedUpdate, String>>>,
    ready: Option<PreparedUpdate>,
}

impl Checker {
    fn start(&mut self, ctx: &egui::Context, automatic: bool) {
        if self.receiver.is_some() || self.installing() {
            return;
        }
        self.started = true;
        self.automatic_check = automatic;
        self.prompt_pending = false;
        self.status = None;
        self.browser_error = None;
        let (tx, rx) = mpsc::channel();
        self.receiver = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = crate::disc_backup::run_capture(
                Command::new("curl").args([
                    "--disable",
                    "--fail",
                    "--silent",
                    "--show-error",
                    "--proto",
                    "=https",
                    "--connect-timeout",
                    "5",
                    "--max-time",
                    "12",
                    "--max-filesize",
                    "1048576",
                    "--header",
                    "Accept: application/vnd.github+json",
                    "--user-agent",
                    "EmuLuna",
                    "https://api.github.com/repos/Turgo82/EmuLuna/releases/latest",
                ]),
                &AtomicBool::new(false),
            )
            .and_then(|body| compare(&body, VERSION));
            if let Err(error) = &result {
                eprintln!("Update check failed: {error}");
            }
            let _ = tx.send(result);
            ctx.request_repaint_of(egui::ViewportId::ROOT);
        });
    }

    pub fn poll(&mut self, ctx: &egui::Context, automatic: bool) {
        if let Some(receiver) = &self.download {
            let result = match receiver.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("The update download ended unexpectedly.".into()))
                }
            };
            if let Some(result) = result {
                self.download = None;
                match result {
                    Ok(update) => self.ready = Some(update),
                    Err(error) => self.browser_error = Some(error),
                }
            }
        }
        if automatic && !self.started {
            self.start(ctx, true);
        }
        if let Some(receiver) = &self.receiver {
            self.status = match receiver.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Empty) => return,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("Update check ended unexpectedly".into()))
                }
            };
            self.receiver = None;
            self.prompt_pending =
                self.automatic_check && matches!(self.status, Some(Ok(Status::Available { .. })));
            ctx.request_repaint();
        }
    }

    fn can_install(&self) -> bool {
        std::env::var_os("APPIMAGE").is_some()
            && matches!(
                &self.status,
                Some(Ok(Status::Available {
                    checksum: Some(_),
                    ..
                }))
            )
    }

    fn installing(&self) -> bool {
        self.download.is_some() || self.ready.is_some()
    }

    fn start_install(&mut self, ctx: &egui::Context) {
        let Some(Ok(Status::Available {
            appimage: Some(url),
            checksum: Some(checksum),
            ..
        })) = &self.status
        else {
            return;
        };
        let Some(image) = appimage_update::current_image() else {
            self.browser_error = Some("Could not locate the running AppImage.".into());
            return;
        };
        let (url, checksum) = (url.clone(), checksum.clone());
        let (tx, rx) = mpsc::channel();
        self.download = Some(rx);
        self.browser_error = None;
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(appimage_update::prepare(image, &url, &checksum));
            ctx.request_repaint_of(egui::ViewportId::ROOT);
        });
    }

    pub fn restart_if_ready(&mut self, ctx: &egui::Context) {
        if let Some(update) = self.ready.take() {
            appimage_update::queue_restart(update);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    pub fn show_prompt(&mut self, ctx: &egui::Context) {
        if !self.prompt_pending {
            return;
        }
        let Some(Ok(Status::Available {
            version,
            page,
            appimage,
            ..
        })) = &self.status
        else {
            return;
        };
        let response = egui::Modal::new(egui::Id::new("app-update-available")).show(ctx, |ui| {
            ui.set_max_width(400.);
            ui.heading(format!("EmuLuna {version} is available"));
            if let Some(error) = &self.browser_error {
                ui.label(error);
            }
            if self.installing() {
                ui.label(if self.ready.is_some() {
                    "Update ready. EmuLuna will restart when current work finishes."
                } else {
                    "Downloading and verifying the update…"
                });
                return (false, None, false);
            }
            ui.add_space(10.);
            ui.horizontal(|ui| {
                let later = ui.button("Later").clicked();
                let release = ui
                    .button("Release notes")
                    .clicked()
                    .then_some(page.as_str());
                let install = self.can_install() && ui.button("Update and restart").clicked();
                let download = appimage
                    .as_deref()
                    .filter(|_| !self.can_install() && ui.button("Download AppImage").clicked());
                (later, download.or(release), install)
            })
            .inner
        });
        let dismissed = response.should_close();
        let (later, link, install) = response.inner;
        let mut opened = false;
        if let Some(url) = link {
            self.browser_error = crate::actions::open_url(url).err();
            opened = self.browser_error.is_none();
        }
        if install {
            self.start_install(ctx);
        }
        if !self.installing() && (later || opened || dismissed) {
            self.prompt_pending = false;
        }
    }

    pub fn notification(&self) -> String {
        match &self.status {
            Some(Ok(Status::Available { version, .. })) => {
                format!("EmuLuna {version} is available.")
            }
            _ => String::new(),
        }
    }

    pub fn links(&self) -> Option<(&str, Option<&str>)> {
        match &self.status {
            Some(Ok(Status::Available { page, appimage, .. })) => Some((page, appimage.as_deref())),
            _ => None,
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui) {
        if ui
            .add_enabled(
                self.receiver.is_none() && !self.installing(),
                egui::Button::new("Check for updates"),
            )
            .clicked()
        {
            self.start(ui.ctx(), false);
        }
        if self.receiver.is_some() {
            ui.label("Checking GitHub Releases…");
        }
        match &self.status {
            Some(Ok(Status::Current)) => {
                ui.label("EmuLuna is up to date.");
            }
            Some(Ok(Status::Available { version, .. })) => {
                ui.strong(format!("EmuLuna {version} is available."));
            }
            Some(Err(_)) => {
                ui.label("Could not check for updates. Try again later.");
            }
            None => {}
        }
        if let Some((page, appimage)) = self.links() {
            let mut error = None;
            for (label, url) in [
                ("Release notes", Some(page)),
                ("Download AppImage", appimage),
            ] {
                if let Some(url) = url {
                    if ui.link(label).clicked() {
                        error = crate::actions::open_url(url).err();
                    }
                }
            }
            if error.is_some() {
                self.browser_error = error;
            }
        }
        if let Some(error) = &self.browser_error {
            ui.label(error);
        }
        if self.can_install()
            && ui
                .add_enabled(!self.installing(), egui::Button::new("Update and restart"))
                .clicked()
        {
            self.start_install(ui.ctx());
        }
        if self.installing() {
            ui.label(if self.ready.is_some() {
                "Update ready. EmuLuna will restart when current work finishes."
            } else {
                "Downloading and verifying the update…"
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_stable_version_precedence_and_project_downloads() {
        let release = |tag: &str, prerelease: bool, draft: bool| {
            serde_json::to_vec(&serde_json::json!({
                "tag_name": tag, "draft": draft, "prerelease": prerelease,
                "assets": [{"name":"EmuLuna.AppImage","browser_download_url":format!("{REPO}/releases/download/{tag}/EmuLuna.AppImage")}],
            })).unwrap()
        };
        assert!(matches!(
            compare(&release("v0.20.10", false, false), "0.20.2"),
            Ok(Status::Available {
                appimage: Some(_),
                ..
            })
        ));
        for payload in [
            release("v0.20.2", false, false),
            release("v0.19.9", false, false),
            release("v0.21.0-rc.1", true, false),
            release("v0.21.0", false, true),
        ] {
            assert_eq!(compare(&payload, "0.20.2"), Ok(Status::Current));
        }
    }
}
