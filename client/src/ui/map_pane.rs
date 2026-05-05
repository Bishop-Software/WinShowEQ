use egui::Ui;

use crate::data::AppData;
use crate::map_con::{MapCon, MapState};

/// Wrapper around `MapCon` that owns the camera state and Z-filter controls.
pub struct MapPane {
    pub state: MapState,
    z_filter_enabled: bool,
    z_range: f32,
}

impl Default for MapPane {
    fn default() -> Self {
        Self {
            state: MapState::default(),
            z_filter_enabled: false,
            z_range: 50.0,
        }
    }
}

impl MapPane {
    pub fn show(&mut self, ui: &mut Ui, data: &AppData) {
        // Controls strip at the top of the panel.
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.z_filter_enabled, "Z Filter");
            if self.z_filter_enabled {
                ui.add(
                    egui::Slider::new(&mut self.z_range, 0.0..=3500.0)
                        .prefix("±")
                        .suffix(" z"),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(format!(
                    "{} spawns  {}",
                    data.spawns.len(),
                    data.zone_name
                ));
            });
        });
        ui.separator();

        let z_filter = if self.z_filter_enabled {
            let center = data
                .self_id
                .and_then(|id| data.spawns.get(id))
                .map(|s| s.z)
                .unwrap_or(0.0);
            Some((center, self.z_range))
        } else {
            None
        };

        MapCon::new(data, &mut self.state).show(ui, z_filter);
    }
}