use eframe::egui::Modifiers;
use emuluna_library_model::Game;
use std::collections::HashSet;

#[derive(Default)]
pub(crate) struct Selection {
    pub primary: Option<String>,
    members: HashSet<String>,
}
impl Selection {
    pub fn only(&mut self, id: Option<String>) {
        self.members.clear();
        self.members.extend(id.iter().cloned());
        self.primary = id;
    }
    pub fn contains(&self, id: &str) -> bool {
        self.members.contains(id)
    }
    pub fn multiple(&self) -> bool {
        self.members.len() > 1
    }
    pub fn ids(&self) -> Vec<String> {
        let mut ids: Vec<_> = self.members.iter().cloned().collect();
        ids.sort_unstable();
        ids
    }
    pub fn retain_visible(&mut self, visible: &[usize], games: &[Game]) {
        let shown: HashSet<_> = visible
            .iter()
            .map(|&index| games[index].id.as_str())
            .collect();
        self.members.retain(|id| shown.contains(id.as_str()));
        if self
            .primary
            .as_ref()
            .is_some_and(|id| !self.members.contains(id))
        {
            self.primary = visible
                .iter()
                .map(|&index| &games[index].id)
                .find(|id| self.members.contains(*id))
                .cloned();
        }
    }
    pub fn click(&mut self, id: &str, modifiers: Modifiers, visible: &[usize], games: &[Game]) {
        if modifiers.shift {
            let anchor = self
                .primary
                .as_deref()
                .and_then(|anchor| visible.iter().position(|&index| games[index].id == anchor));
            let position = visible.iter().position(|&index| games[index].id == id);
            if let (Some(anchor), Some(position)) = (anchor, position) {
                if !modifiers.ctrl {
                    self.members.clear();
                }
                self.members.extend(
                    visible[anchor.min(position)..=anchor.max(position)]
                        .iter()
                        .map(|&index| games[index].id.clone()),
                );
                return;
            }
        }
        if modifiers.ctrl {
            if self.members.remove(id) {
                if self.primary.as_deref() == Some(id) {
                    self.primary = visible
                        .iter()
                        .map(|&index| &games[index].id)
                        .find(|id| self.members.contains(*id))
                        .cloned();
                }
            } else {
                self.members.insert(id.to_owned());
                self.primary = Some(id.to_owned());
            }
        } else {
            self.only(Some(id.to_owned()));
        }
    }
}
