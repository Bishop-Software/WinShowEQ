use egui::Ui;

use crate::data::AppData;
use crate::map_con::{MapCon, MapState};

/// Wrapper around `MapCon` that owns the camera state and lays out the panel.
pub struct MapPane {
    state: MapState,
}

impl Default for MapPane {
    fn default() -> Self {
        Self { state: MapState::default() }
    }
}

impl MapPane {
    pub fn show(&mut self, ui: &mut Ui, data: &AppData) {
        MapCon::new(data, &mut self.state).show(ui);
    }
}