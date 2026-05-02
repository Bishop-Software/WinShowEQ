use egui::Ui;

use crate::data::timers::TimerStore;

pub fn show(ui: &mut Ui, timers: &mut TimerStore) {
    let mut remove_idx: Option<usize> = None;

    egui::ScrollArea::vertical()
        .id_salt("timer_scroll")
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            egui::Grid::new("timer_list")
                .num_columns(3)
                .striped(true)
                .min_col_width(60.0)
                .show(ui, |ui| {
                    ui.strong("Name");
                    ui.strong("Loc");
                    ui.strong("Countdown");
                    ui.end_row();

                    for (i, t) in timers.iter().enumerate() {
                        let countdown = t.countdown_str();
                        let color = if t.is_spawned() {
                            egui::Color32::from_rgb(255, 80, 80)
                        } else if t.secs_remaining() < 60 {
                            egui::Color32::from_rgb(255, 210, 0)
                        } else {
                            ui.visuals().text_color()
                        };

                        ui.label(&t.name);
                        ui.label(format!("{:.0},{:.0}", t.x, t.y));
                        let resp = ui.colored_label(color, &countdown);
                        resp.context_menu(|ui| {
                            if ui.button("Remove timer").clicked() {
                                remove_idx = Some(i);
                                ui.close();
                            }
                        });
                        ui.end_row();
                    }
                });
        });

    if let Some(idx) = remove_idx {
        timers.remove(idx);
    }
}