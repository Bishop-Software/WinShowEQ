use egui::Ui;

use crate::data::AppData;

pub fn show(ui: &mut Ui, data: &AppData) {
    let mut items: Vec<_> = data.ground.iter().collect();
    items.sort_by(|a, b| a.name.cmp(&b.name));

    egui::ScrollArea::vertical()
        .id_salt("ground_scroll")
        .show(ui, |ui| {
            egui::Grid::new("ground_list")
                .num_columns(2)
                .striped(true)
                .min_col_width(80.0)
                .show(ui, |ui| {
                    ui.strong("Item");
                    ui.strong("Loc");
                    ui.end_row();

                    for item in &items {
                        ui.label(&item.name);
                        ui.label(format!("{:.0},{:.0}", item.x, item.y));
                        ui.end_row();
                    }
                });
        });
}