use egui::Ui;

use crate::data::AppData;

pub fn show(ui: &mut Ui, data: &AppData) {
    let mut items: Vec<_> = data.ground.iter().collect();
    items.sort_by(|a, b| a.name.cmp(&b.name));

    egui::ScrollArea::vertical()
        .id_salt("ground_scroll")
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            egui::Grid::new("ground_list")
                .num_columns(4)
                .striped(true)
                .min_col_width(60.0)
                .show(ui, |ui| {
                    ui.strong("Item");
                    ui.strong("X");
                    ui.strong("Y");
                    ui.strong("Z");
                    ui.end_row();

                    for item in &items {
                        ui.label(&item.name);
                        ui.label(format!("{:.0}", item.x));
                        ui.label(format!("{:.0}", item.y));
                        ui.label(format!("{:.0}", item.z));
                        ui.end_row();
                    }
                });
        });
}