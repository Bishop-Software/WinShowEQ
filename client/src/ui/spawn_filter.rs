use std::collections::{HashMap, HashSet};

use egui::Ui;

use crate::data::spawns::{SpawnCategory, SpawnStore};
use crate::game_data::GameData;

/// A single spawn entry fed into the filter UI, with race/class pre-resolved to display strings.
#[derive(Clone, Debug)]
pub struct FilterEntry {
    pub id: u32,
    pub name: String,
    pub race: String,
    pub class: String,
    pub level: u8,
    pub spawn_type: SpawnCategory,
}

/// Spawn-type bucket for the type filter ComboBox.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SpawnTypeFilter {
    #[default]
    All,
    Npc,
    Pc,
    Corpse,
    Pet,
}

impl SpawnTypeFilter {
    fn label(self) -> &'static str {
        match self {
            Self::All => "All Types",
            Self::Npc => "NPC",
            Self::Pc => "PC",
            Self::Corpse => "Corpse",
            Self::Pet => "Pet/Merc",
        }
    }

    fn matches(self, cat: SpawnCategory) -> bool {
        match self {
            Self::All => true,
            Self::Npc => cat == SpawnCategory::Npc,
            Self::Pc => cat == SpawnCategory::Pc,
            Self::Corpse => cat == SpawnCategory::Corpse,
            Self::Pet => cat == SpawnCategory::Pet || cat == SpawnCategory::Merc,
        }
    }
}

/// Compact filter bar rendered above the spawn list.
///
/// Call [`update_spawns`] whenever the spawn list changes (zone change or tick) to
/// rebuild the race/class options. Call [`apply_filters`] each frame to get the set of
/// visible spawn IDs. Call [`ui_compact`] to render the filter controls.
pub struct SpawnFilterUI {
    entries: Vec<FilterEntry>,
    /// Sorted unique race names across all entries.
    races: Vec<String>,
    /// Sorted unique class names across all entries (regardless of race selection).
    classes: Vec<String>,
    /// Classes present per race — used for cascading class ComboBox.
    classes_by_race: HashMap<String, Vec<String>>,

    pub selected_race: Option<String>,
    pub selected_class: Option<String>,
    pub level_min: u8,
    pub level_max: u8,
    pub spawn_type_filter: SpawnTypeFilter,
}

impl Default for SpawnFilterUI {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            races: Vec::new(),
            classes: Vec::new(),
            classes_by_race: HashMap::new(),
            selected_race: None,
            selected_class: None,
            level_min: 0,
            level_max: 255,
            spawn_type_filter: SpawnTypeFilter::All,
        }
    }
}

impl SpawnFilterUI {
    /// Rebuild race/class option lists from new spawn data.
    ///
    /// Existing selections are preserved when they still appear in the new data;
    /// stale selections are cleared.
    pub fn update_spawns(&mut self, entries: Vec<FilterEntry>) {
        let mut races: HashSet<String> = HashSet::new();
        let mut classes: HashSet<String> = HashSet::new();
        let mut classes_by_race: HashMap<String, HashSet<String>> = HashMap::new();

        for e in &entries {
            if !e.race.is_empty() && e.race != "---" {
                races.insert(e.race.clone());
                if !e.class.is_empty() && e.class != "---" {
                    classes_by_race
                        .entry(e.race.clone())
                        .or_default()
                        .insert(e.class.clone());
                }
            }
            if !e.class.is_empty() && e.class != "---" {
                classes.insert(e.class.clone());
            }
        }

        let mut races: Vec<String> = races.into_iter().collect();
        races.sort();
        let mut classes: Vec<String> = classes.into_iter().collect();
        classes.sort();
        let classes_by_race: HashMap<String, Vec<String>> = classes_by_race
            .into_iter()
            .map(|(k, v)| {
                let mut sorted: Vec<String> = v.into_iter().collect();
                sorted.sort();
                (k, sorted)
            })
            .collect();

        // Drop race selection if it no longer exists in the new data.
        if let Some(ref r) = self.selected_race {
            if !races.contains(r) {
                self.selected_race = None;
                self.selected_class = None;
            }
        }
        // Drop class selection if it no longer exists for the current race.
        if let Some(ref c) = self.selected_class {
            let valid = match &self.selected_race {
                Some(r) => classes_by_race.get(r).map(|v| v.as_slice()).unwrap_or(&[]),
                None => classes.as_slice(),
            };
            if !valid.contains(c) {
                self.selected_class = None;
            }
        }

        self.entries = entries;
        self.races = races;
        self.classes = classes;
        self.classes_by_race = classes_by_race;
    }

    /// Return the set of spawn IDs that pass all active filters.
    pub fn apply_filters(&self) -> HashSet<u32> {
        self.entries
            .iter()
            .filter(|e| self.matches(e))
            .map(|e| e.id)
            .collect()
    }

    fn matches(&self, e: &FilterEntry) -> bool {
        if let Some(ref race) = self.selected_race {
            if &e.race != race {
                return false;
            }
        }
        if let Some(ref class) = self.selected_class {
            if &e.class != class {
                return false;
            }
        }
        if e.level < self.level_min || e.level > self.level_max {
            return false;
        }
        if !self.spawn_type_filter.matches(e.spawn_type) {
            return false;
        }
        true
    }

    /// True when any filter differs from its default (no-op) value.
    pub fn is_active(&self) -> bool {
        self.selected_race.is_some()
            || self.selected_class.is_some()
            || self.level_min > 0
            || self.level_max < 255
            || self.spawn_type_filter != SpawnTypeFilter::All
    }

    /// Reset all filter selections to their defaults.
    pub fn reset(&mut self) {
        self.selected_race = None;
        self.selected_class = None;
        self.level_min = 0;
        self.level_max = 255;
        self.spawn_type_filter = SpawnTypeFilter::All;
    }

    /// Render a compact horizontal filter bar suitable for the spawn list panel header.
    pub fn ui_compact(&mut self, ui: &mut Ui) {
        // Clone option lists to avoid simultaneous borrow of self inside ComboBox closures.
        let races = self.races.clone();
        let available_classes: Vec<String> = match &self.selected_race {
            Some(r) => self.classes_by_race.get(r).cloned().unwrap_or_default(),
            None => self.classes.clone(),
        };

        ui.horizontal(|ui| {
            // Race ComboBox
            let race_label = self.selected_race.as_deref().unwrap_or("All Races");
            egui::ComboBox::from_id_salt("sf_race")
                .selected_text(race_label)
                .width(90.0)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_value(&mut self.selected_race, None, "All Races")
                        .clicked()
                    {
                        self.selected_class = None;
                    }
                    for race in &races {
                        if ui
                            .selectable_value(
                                &mut self.selected_race,
                                Some(race.clone()),
                                race,
                            )
                            .clicked()
                        {
                            self.selected_class = None;
                        }
                    }
                });

            // Class ComboBox (options cascade from selected race)
            let class_label = self.selected_class.as_deref().unwrap_or("All Classes");
            egui::ComboBox::from_id_salt("sf_class")
                .selected_text(class_label)
                .width(80.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.selected_class, None, "All Classes");
                    for class in &available_classes {
                        ui.selectable_value(
                            &mut self.selected_class,
                            Some(class.clone()),
                            class,
                        );
                    }
                });

            // Level range
            ui.label("Lvl:");
            let mut level_min = self.level_min as i32;
            let mut level_max = self.level_max as i32;
            ui.add(
                egui::DragValue::new(&mut level_min)
                    .range(0..=255)
                    .speed(0.5),
            );
            self.level_min = level_min.clamp(0, 255) as u8;
            ui.label("–");
            ui.add(
                egui::DragValue::new(&mut level_max)
                    .range(0..=255)
                    .speed(0.5),
            );
            self.level_max = level_max.clamp(0, 255) as u8;
            if self.level_min > self.level_max {
                self.level_max = self.level_min;
            }

            // Type ComboBox
            egui::ComboBox::from_id_salt("sf_type")
                .selected_text(self.spawn_type_filter.label())
                .width(75.0)
                .show_ui(ui, |ui| {
                    for t in [
                        SpawnTypeFilter::All,
                        SpawnTypeFilter::Npc,
                        SpawnTypeFilter::Pc,
                        SpawnTypeFilter::Corpse,
                        SpawnTypeFilter::Pet,
                    ] {
                        ui.selectable_value(&mut self.spawn_type_filter, t, t.label());
                    }
                });

            // Clear button — only visible when a filter is active
            if self.is_active() && ui.small_button("✕").on_hover_text("Clear filters").clicked() {
                self.reset();
            }
        });
    }
}

/// Build the `FilterEntry` list from the current spawn store, resolving race and class names.
/// Intended to be called once per tick when `AppData::spawns_dirty` is set.
pub fn build_filter_entries(spawns: &SpawnStore, game_data: &GameData) -> Vec<FilterEntry> {
    spawns
        .iter()
        .map(|s| FilterEntry {
            id: s.id,
            name: s.name.clone(),
            race: game_data.race_name(s.race).to_owned(),
            class: game_data.class_name(s.class),
            level: s.level,
            spawn_type: s.spawn_category,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(id: u32, race: &str, class: &str, level: u8, spawn_type: SpawnCategory) -> FilterEntry {
        FilterEntry {
            id,
            name: format!("spawn_{id}"),
            race: race.to_string(),
            class: class.to_string(),
            level,
            spawn_type,
        }
    }

    fn test_entries() -> Vec<FilterEntry> {
        vec![
            make_entry(1, "Orc", "Warrior", 20, SpawnCategory::Npc),
            make_entry(2, "Orc", "Shaman", 25, SpawnCategory::Npc),
            make_entry(3, "Gnoll", "Warrior", 15, SpawnCategory::Npc),
            make_entry(4, "Human", "Ranger", 60, SpawnCategory::Pc),
            make_entry(5, "Orc", "Warrior", 30, SpawnCategory::Corpse),
        ]
    }

    #[test]
    fn default_is_not_active() {
        let f = SpawnFilterUI::default();
        assert!(!f.is_active());
    }

    #[test]
    fn apply_filters_returns_all_when_inactive() {
        let mut f = SpawnFilterUI::default();
        f.update_spawns(test_entries());
        assert_eq!(f.apply_filters().len(), 5);
    }

    #[test]
    fn filter_by_race() {
        let mut f = SpawnFilterUI::default();
        f.update_spawns(test_entries());
        f.selected_race = Some("Orc".to_string());
        assert!(f.is_active());
        let ids = f.apply_filters();
        assert!(ids.contains(&1));
        assert!(ids.contains(&2));
        assert!(ids.contains(&5));
        assert!(!ids.contains(&3));
        assert!(!ids.contains(&4));
    }

    #[test]
    fn filter_by_class() {
        let mut f = SpawnFilterUI::default();
        f.update_spawns(test_entries());
        f.selected_class = Some("Warrior".to_string());
        let ids = f.apply_filters();
        assert!(ids.contains(&1));
        assert!(ids.contains(&3));
        assert!(ids.contains(&5));
        assert!(!ids.contains(&2));
        assert!(!ids.contains(&4));
    }

    #[test]
    fn filter_by_race_and_class() {
        let mut f = SpawnFilterUI::default();
        f.update_spawns(test_entries());
        f.selected_race = Some("Orc".to_string());
        f.selected_class = Some("Warrior".to_string());
        let ids = f.apply_filters();
        assert!(ids.contains(&1));
        assert!(ids.contains(&5));
        assert!(!ids.contains(&2));
        assert!(!ids.contains(&3));
    }

    #[test]
    fn filter_by_level_min() {
        let mut f = SpawnFilterUI::default();
        f.update_spawns(test_entries());
        f.level_min = 20;
        assert!(f.is_active());
        let ids = f.apply_filters();
        assert!(!ids.contains(&3)); // level 15
        assert!(ids.contains(&1)); // level 20
    }

    #[test]
    fn filter_by_level_max() {
        let mut f = SpawnFilterUI::default();
        f.update_spawns(test_entries());
        f.level_max = 25;
        assert!(f.is_active());
        let ids = f.apply_filters();
        assert!(ids.contains(&1));
        assert!(ids.contains(&2));
        assert!(ids.contains(&3));
        assert!(!ids.contains(&4)); // level 60
    }

    #[test]
    fn filter_by_type_npc() {
        let mut f = SpawnFilterUI::default();
        f.update_spawns(test_entries());
        f.spawn_type_filter = SpawnTypeFilter::Npc;
        assert!(f.is_active());
        let ids = f.apply_filters();
        assert!(ids.contains(&1));
        assert!(ids.contains(&2));
        assert!(ids.contains(&3));
        assert!(!ids.contains(&4)); // PC
        assert!(!ids.contains(&5)); // Corpse
    }

    #[test]
    fn filter_by_type_pc() {
        let mut f = SpawnFilterUI::default();
        f.update_spawns(test_entries());
        f.spawn_type_filter = SpawnTypeFilter::Pc;
        let ids = f.apply_filters();
        assert_eq!(ids, [4].into());
    }

    #[test]
    fn filter_by_type_corpse() {
        let mut f = SpawnFilterUI::default();
        f.update_spawns(test_entries());
        f.spawn_type_filter = SpawnTypeFilter::Corpse;
        let ids = f.apply_filters();
        assert_eq!(ids, [5].into());
    }

    #[test]
    fn update_spawns_builds_race_list() {
        let mut f = SpawnFilterUI::default();
        f.update_spawns(test_entries());
        assert!(f.races.contains(&"Orc".to_string()));
        assert!(f.races.contains(&"Gnoll".to_string()));
        assert!(f.races.contains(&"Human".to_string()));
        assert!(f.races.windows(2).all(|w| w[0] <= w[1]), "races must be sorted");
    }

    #[test]
    fn update_spawns_classes_cascade_by_race() {
        let mut f = SpawnFilterUI::default();
        f.update_spawns(test_entries());
        let orc_classes = f.classes_by_race.get("Orc").unwrap();
        assert!(orc_classes.contains(&"Warrior".to_string()));
        assert!(orc_classes.contains(&"Shaman".to_string()));
        assert!(!orc_classes.contains(&"Ranger".to_string()));
    }

    #[test]
    fn update_spawns_preserves_valid_selection() {
        let mut f = SpawnFilterUI::default();
        f.update_spawns(test_entries());
        f.selected_race = Some("Orc".to_string());
        // Re-update with same data — selection should be preserved
        f.update_spawns(test_entries());
        assert_eq!(f.selected_race, Some("Orc".to_string()));
    }

    #[test]
    fn update_spawns_clears_stale_race() {
        let mut f = SpawnFilterUI::default();
        f.update_spawns(test_entries());
        f.selected_race = Some("Orc".to_string());
        f.selected_class = Some("Warrior".to_string());
        // Update with entries that have no Orcs
        f.update_spawns(vec![make_entry(10, "Gnoll", "Warrior", 10, SpawnCategory::Npc)]);
        assert_eq!(f.selected_race, None);
        assert_eq!(f.selected_class, None);
    }

    #[test]
    fn reset_clears_all_filters() {
        let mut f = SpawnFilterUI::default();
        f.update_spawns(test_entries());
        f.selected_race = Some("Orc".to_string());
        f.level_min = 10;
        f.level_max = 50;
        f.spawn_type_filter = SpawnTypeFilter::Npc;
        f.reset();
        assert!(!f.is_active());
    }

    #[test]
    fn pet_filter_matches_merc() {
        let mut f = SpawnFilterUI::default();
        f.update_spawns(vec![
            make_entry(1, "---", "---", 1, SpawnCategory::Pet),
            make_entry(2, "---", "---", 1, SpawnCategory::Merc),
            make_entry(3, "---", "---", 1, SpawnCategory::Npc),
        ]);
        f.spawn_type_filter = SpawnTypeFilter::Pet;
        let ids = f.apply_filters();
        assert!(ids.contains(&1));
        assert!(ids.contains(&2));
        assert!(!ids.contains(&3));
    }
}