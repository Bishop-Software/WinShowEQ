use egui::Ui;

use crate::data::AppData;
use crate::data::spawns::SpawnCategory;

pub fn show(ui: &mut Ui, data: &AppData) {
    let player_pos = data.player_pos();

    let mut spawns: Vec<_> = data
        .spawns
        .iter()
        .filter(|s| Some(s.id) != data.self_id)
        .collect();

    if let Some((px, py, _)) = player_pos {
        spawns.sort_by(|a, b| {
            let da = a.distance_2d(px, py);
            let db = b.distance_2d(px, py);
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        });
    } else {
        spawns.sort_by(|a, b| a.name.cmp(&b.name));
    }

    egui::ScrollArea::vertical()
        .id_salt("spawn_scroll")
        .show(ui, |ui| {
            egui::Grid::new("spawn_list")
                .num_columns(5)
                .striped(true)
                .min_col_width(36.0)
                .show(ui, |ui| {
                    ui.strong("Name");
                    ui.strong("Lvl");
                    ui.strong("Cat");
                    ui.strong("Loc");
                    ui.strong("Dist");
                    ui.end_row();

                    for s in &spawns {
                        let dist_str = match player_pos {
                            Some((px, py, _)) => format!("{:.0}", s.distance_2d(px, py)),
                            None => "-".to_owned(),
                        };
                        let cat = match s.spawn_category {
                            SpawnCategory::Pc => "PC",
                            SpawnCategory::Npc => "NPC",
                            SpawnCategory::Corpse => "Cor",
                            SpawnCategory::Pet => "Pet",
                            SpawnCategory::Merc => "Mrc",
                        };
                        let color = if s.is_danger {
                            egui::Color32::from_rgb(255, 80, 80)
                        } else if s.is_caution {
                            egui::Color32::from_rgb(255, 160, 0)
                        } else if s.is_hunt {
                            egui::Color32::from_rgb(0, 220, 120)
                        } else if s.is_alert {
                            egui::Color32::from_rgb(200, 0, 255)
                        } else {
                            ui.visuals().text_color()
                        };

                        ui.colored_label(color, &s.name);
                        ui.label(s.level.to_string());
                        ui.label(cat);
                        ui.label(format!("{:.0},{:.0}", s.x, s.y));
                        ui.label(dist_str);
                        ui.end_row();
                    }
                });
        });
}