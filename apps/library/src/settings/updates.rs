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
    let mut appimages = release.assets.into_iter().filter(|asset| {
        asset.name.ends_with(".AppImage") && asset.browser_download_url.starts_with(&prefix)
    });
    let appimage = appimages
        .next()
        .filter(|_| appimages.next().is_none())
        .map(|asset| asset.browser_download_url);
    Ok(Status::Available {
        page: format!("{REPO}/releases/tag/{}", release.tag_name),
        version,
        appimage,
    })
}

#[derive(Default)]
pub struct Checker {
    started: bool,
    receiver: Option<mpsc::Receiver<Result<Status, String>>>,
    status: Option<Result<Status, String>>,
    browser_error: Option<String>,
}

impl Checker {
    fn start(&mut self, ctx: &egui::Context) {
        if self.receiver.is_some() {
            return;
        }
        self.started = true;
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
            ctx.request_repaint();
        });
    }

    pub fn poll(&mut self, ctx: &egui::Context, automatic: bool) {
        if automatic && !self.started {
            self.start(ctx);
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
            ctx.request_repaint();
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
                self.receiver.is_none(),
                egui::Button::new("Check for updates"),
            )
            .clicked()
        {
            self.start(ui.ctx());
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
