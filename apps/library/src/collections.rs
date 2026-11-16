//! Editors for the same collection records used by the archived library.
use eframe::egui;
use emuluna_library_model::{Library, SmartRules};
use rusqlite::{params, Connection, OpenFlags};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub struct Editor {
    name: String,
    rules: Option<SmartRules>,
    games: Vec<String>,
    error: String,
    focus_name: bool,
    rename_id: Option<i64>,
}
impl Editor {
    pub fn new(smart: bool, games: Vec<String>) -> Self {
        Self {
            name: String::new(),
            rules: smart.then(SmartRules::default),
            games,
            error: String::new(),
            focus_name: true,
            rename_id: None,
        }
    }

    pub fn rename(id: i64, name: String) -> Self {
        Self {
            name,
            rename_id: Some(id),
            ..Self::new(false, Vec::new())
        }
    }

    fn save(&self, library: &Library) -> Result<i64, String> {
        let name = self.name.trim();
        if name.is_empty() {
            return Err("Enter a collection name.".into());
        }
        let rules = self
            .rules
            .as_ref()
            .map(|rules| {
                if rules.system.is_none()
                    && rules.minimum_rating.is_none()
                    && !rules.favorite
                    && !rules.never_played
                    && rules.added_days.is_none()
                    && rules.played_days.is_none()
                {
                    return Err("Select at least one rule.".into());
                }
                if rules.never_played && rules.played_days.is_some() {
                    return Err("Never played cannot be combined with a recent play date.".into());
                }
                serde_json::to_string(rules).map_err(|error| error.to_string())
            })
            .transpose()?;
        let mut db = Connection::open_with_flags(
            library.root.join("library.sqlite3"),
            OpenFlags::SQLITE_OPEN_READ_WRITE,
        )
        .map_err(|error| error.to_string())?;
        db.busy_timeout(Duration::from_secs(1))
            .map_err(|error| error.to_string())?;
        db.pragma_update(None, "foreign_keys", true)
            .map_err(|error| error.to_string())?;
        if let Some(id) = self.rename_id {
            let changed = db
                .execute(
                    "UPDATE collections SET name=?1 WHERE id=?2",
                    params![name, id],
                )
                .map_err(|error| error.to_string())?;
            return (changed == 1)
                .then_some(id)
                .ok_or_else(|| "This collection no longer exists.".into());
        }
        let tx = db.transaction().map_err(|error| error.to_string())?;
        let created = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0., |time| time.as_secs_f64());
        tx.execute(
            "INSERT INTO collections(name,rules,created) VALUES(?1,?2,?3)",
            params![name, rules, created],
        )
        .map_err(|error| error.to_string())?;
        let id = tx.last_insert_rowid();
        for game in &self.games {
            tx.execute("INSERT INTO collection_games(collection_id,game_id) SELECT ?1,id FROM games WHERE id=?2",
                params![id, game]).map_err(|error| error.to_string())?;
        }
        tx.commit().map_err(|error| error.to_string())?;
        Ok(id)
    }

    pub fn show(&mut self, ctx: &egui::Context, library: &Library) -> (bool, Option<i64>) {
        let mut close = false;
        let mut saved = None;
        let response = egui::Modal::new(egui::Id::new("new-library-collection"))
            .frame(egui::Frame::window(&ctx.style()).inner_margin(18))
            .show(ctx, |ui| {
                ui.set_width((ctx.screen_rect().width() - 52.).min(440.));
                ui.spacing_mut().interact_size.y = 30.;
                ui.heading(if self.rename_id.is_some() {
                    "Rename collection"
                } else if self.rules.is_some() {
                    "New smart collection"
                } else {
                    "New collection"
                });
                if self.rules.is_some() {
                    ui.label("Games appear automatically when they match all selected rules.");
                }
                ui.label("Collection name");
                let name = ui.add_sized(
                    [ui.available_width(), 34.],
                    egui::TextEdit::singleline(&mut self.name).char_limit(200),
                );
                if std::mem::take(&mut self.focus_name) {
                    name.request_focus();
                }
                if let Some(rules) = &mut self.rules {
                    egui::Grid::new("smart-collection-rules")
                        .spacing([14., 7.])
                        .show(ui, |ui| {
                            ui.label("System");
                            let system = library
                                .consoles
                                .iter()
                                .find(|console| Some(&console.id) == rules.system.as_ref())
                                .map_or("Any system", |console| console.name.as_str());
                            egui::ComboBox::from_id_salt("smart-system")
                                .width(220.)
                                .selected_text(system)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut rules.system, None, "Any system");
                                    for console in &library.consoles {
                                        ui.selectable_value(
                                            &mut rules.system,
                                            Some(console.id.clone()),
                                            &console.name,
                                        );
                                    }
                                });
                            ui.end_row();
                            ui.label("Rating");
                            let rating = rules.minimum_rating.map_or_else(
                                || "Any rating".into(),
                                |n| format!("{n} stars or higher"),
                            );
                            egui::ComboBox::from_id_salt("smart-rating")
                                .width(220.)
                                .selected_text(rating)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut rules.minimum_rating,
                                        None,
                                        "Any rating",
                                    );
                                    for n in 1..=5 {
                                        ui.selectable_value(
                                            &mut rules.minimum_rating,
                                            Some(n),
                                            format!("{n} stars or higher"),
                                        );
                                    }
                                });
                            ui.end_row();
                            for (label, value) in [
                                ("Added in last", &mut rules.added_days),
                                ("Played in last", &mut rules.played_days),
                            ] {
                                ui.label(label);
                                let mut days = value.unwrap_or(0);
                                if ui
                                    .add_sized(
                                        [112., 30.],
                                        egui::DragValue::new(&mut days)
                                            .range(0..=36_500)
                                            .custom_formatter(|value, _| {
                                                if value == 0. {
                                                    "Any time".into()
                                                } else {
                                                    format!("{value:.0} days")
                                                }
                                            })
                                            .custom_parser(|text| {
                                                text.trim_end_matches(" days").parse().ok()
                                            }),
                                    )
                                    .changed()
                                {
                                    *value = (days != 0).then_some(days);
                                }
                                ui.end_row();
                            }
                        });
                    ui.checkbox(&mut rules.favorite, "Favorites only");
                    ui.checkbox(&mut rules.never_played, "Never played");
                }
                if !self.error.is_empty() {
                    ui.colored_label(ui.visuals().error_fg_color, &self.error);
                }
                let enter =
                    name.lost_focus() && ctx.input(|input| input.key_pressed(egui::Key::Enter));
                ui.add_space(8.);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_sized([84., 34.], egui::Button::new("Save"))
                        .clicked()
                        || enter
                    {
                        match self.save(library) {
                            Ok(id) => {
                                saved = Some(id);
                                close = true;
                            }
                            Err(error) => self.error = error,
                        }
                    }
                    if ui
                        .add_sized([84., 34.], egui::Button::new("Cancel"))
                        .clicked()
                    {
                        close = true;
                    }
                });
            });
        (close || response.should_close(), saved)
    }
}

pub fn delete(library: &Library, id: i64) -> Result<(), String> {
    let mut db = Connection::open_with_flags(
        library.root.join("library.sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_WRITE,
    )
    .map_err(|error| error.to_string())?;
    db.busy_timeout(Duration::from_secs(1))
        .map_err(|error| error.to_string())?;
    let tx = db.transaction().map_err(|error| error.to_string())?;
    tx.execute("DELETE FROM collection_games WHERE collection_id=?1", [id])
        .map_err(|error| error.to_string())?;
    tx.execute("DELETE FROM collections WHERE id=?1", [id])
        .map_err(|error| error.to_string())?;
    tx.commit().map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renaming_keeps_rules_and_deleting_keeps_games() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("library.sqlite3");
        let db = Connection::open(&path).unwrap();
        db.execute_batch(
            "CREATE TABLE collections(id INTEGER PRIMARY KEY, name TEXT, rules TEXT);
             CREATE TABLE collection_games(collection_id INTEGER, game_id TEXT);
             CREATE TABLE games(id TEXT PRIMARY KEY);
             INSERT INTO collections VALUES (1, 'Original', '{\"favorite\":true}'), (2, 'Other', NULL);
             INSERT INTO games VALUES ('game');
             INSERT INTO collection_games VALUES (1, 'game'), (2, 'game');",
        ).unwrap();
        let library = Library {
            root: folder.path().to_owned(),
            games: Vec::new(),
            consoles: Vec::new(),
            collections: Vec::new(),
            theme: String::new(),
            settings: Default::default(),
        };
        assert_eq!(Editor::rename(1, " New ".into()).save(&library).unwrap(), 1);
        let record = db
            .query_row(
                "SELECT name, rules FROM collections WHERE id=1",
                [],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .unwrap();
        assert_eq!(record, ("New".into(), "{\"favorite\":true}".into()));
        delete(&library, 1).unwrap();
        let remaining = db.query_row(
            "SELECT (SELECT sum(id) FROM collections), (SELECT sum(collection_id) FROM collection_games), (SELECT count(*) FROM games)",
            [], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?)),
        ).unwrap();
        assert_eq!(remaining, (2, 2, 1));
    }
}
