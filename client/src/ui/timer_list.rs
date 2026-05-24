use chrono::Local;
use egui::Ui;

use crate::data::AppData;

const HEADERS: &[&str] = &[
    "Name",
    "Remain",
    "Interval",
    "Zone",
    "X",
    "Y",
    "Z",
    "Count",
    "Spawn Time",
    "Kill Time",
];

fn format_timestamp(dt: chrono::DateTime<chrono::Utc>) -> String {
    let local: chrono::DateTime<Local> = dt.into();
    local.format("%-I:%M %p %-m/%-d/%Y").to_string()
}

/// Returns true if "Clear all timers" was requested (caller must delete the obs file).
pub fn show(
    ui: &mut Ui,
    data: &mut AppData,
    sort_column: &mut Option<usize>,
    sort_ascending: &mut bool,
) -> bool {
    let row_h = ui.text_style_height(&egui::TextStyle::Body) + 4.0;
    let mut col_widths = data.timer_list_column_widths.clone();

    // Header row with resizable columns
    ui.horizontal(|ui| {
        for (col_idx, (header, width)) in HEADERS.iter().zip(col_widths.iter_mut()).enumerate() {
            let indicator = match sort_column {
                Some(c) if *c == col_idx => {
                    if *sort_ascending {
                        " ▲"
                    } else {
                        " ▼"
                    }
                }
                _ => "",
            };
            let label = format!("{}{}", header, indicator);

            let header_rect = ui.allocate_space(egui::vec2(*width, row_h)).1;
            let header_resp = ui.interact(
                header_rect,
                ui.id().with("header").with(col_idx),
                egui::Sense::click(),
            );

            let text_color = if header_resp.hovered() {
                egui::Color32::WHITE
            } else {
                ui.visuals().text_color()
            };
            ui.painter().text(
                header_rect.left_top() + egui::vec2(4.0, 2.0),
                egui::Align2::LEFT_TOP,
                &label,
                egui::FontId::default(),
                text_color,
            );

            if header_resp.clicked() {
                if *sort_column == Some(col_idx) {
                    *sort_ascending = !*sort_ascending;
                } else {
                    *sort_column = Some(col_idx);
                    *sort_ascending = true;
                }
            }

            if col_idx < HEADERS.len() - 1 {
                let sep_rect = ui.allocate_space(egui::vec2(4.0, row_h)).1;
                let sep_sense = ui.interact(
                    sep_rect,
                    ui.id().with("resize").with(col_idx),
                    egui::Sense::drag(),
                );
                if sep_sense.dragged() {
                    *width = (*width + sep_sense.drag_delta().x).max(30.0);
                }
                if sep_sense.hovered() {
                    ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::ResizeHorizontal);
                }
            }
        }
    });

    data.timer_list_column_widths = col_widths.clone();
    ui.separator();

    let mut remove_idx: Option<usize> = None;
    let mut clear_all = false;

    let mut timers_with_idx: Vec<_> = data.timers.iter().enumerate().collect();
    if let Some(col) = sort_column {
        timers_with_idx.sort_by(|a, b| {
            let cmp = match col {
                0 => a.1.name.cmp(&b.1.name),
                1 => a.1.secs_remaining().cmp(&b.1.secs_remaining()),
                2 => a.1.respawn_secs.cmp(&b.1.respawn_secs),
                3 => a.1.zone.cmp(&b.1.zone),
                4 => {
                    a.1.x
                        .partial_cmp(&b.1.x)
                        .unwrap_or(std::cmp::Ordering::Equal)
                }
                5 => {
                    a.1.y
                        .partial_cmp(&b.1.y)
                        .unwrap_or(std::cmp::Ordering::Equal)
                }
                6 => {
                    a.1.z
                        .partial_cmp(&b.1.z)
                        .unwrap_or(std::cmp::Ordering::Equal)
                }
                7 => a.1.spawn_count.cmp(&b.1.spawn_count),
                8 => a.1.spawn_time.cmp(&b.1.spawn_time),
                9 => a.1.killed_at.cmp(&b.1.killed_at),
                _ => std::cmp::Ordering::Equal,
            };
            if *sort_ascending { cmp } else { cmp.reverse() }
        });
    }

    egui::ScrollArea::vertical()
        .id_salt("timer_scroll")
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            for (i, t) in timers_with_idx {
                let countdown = t.countdown_str();
                let color = if t.is_spawned() {
                    egui::Color32::from_rgb(255, 80, 80)
                } else if t.secs_remaining() < 60 {
                    egui::Color32::from_rgb(255, 210, 0)
                } else {
                    ui.visuals().text_color()
                };

                let name_cell = if t.is_auto {
                    format!("{} [A]", t.name)
                } else {
                    t.name.clone()
                };

                let cells: [String; 10] = [
                    name_cell,
                    countdown,
                    t.respawn_secs.to_string(),
                    t.zone.clone(),
                    format!("{:.2}", t.x),
                    format!("{:.2}", t.y),
                    format!("{:.2}", t.z),
                    if t.spawn_count > 0 {
                        t.spawn_count.to_string()
                    } else {
                        String::new()
                    },
                    t.spawn_time.map(format_timestamp).unwrap_or_default(),
                    format_timestamp(t.killed_at),
                ];

                let row_rect = ui
                    .horizontal(|ui| {
                        for (col_idx, text) in cells.iter().enumerate() {
                            let cell_color = if col_idx == 1 {
                                color
                            } else {
                                ui.visuals().text_color()
                            };
                            let (_, cell_rect) =
                                ui.allocate_space(egui::vec2(col_widths[col_idx], row_h));
                            ui.painter().with_clip_rect(cell_rect).text(
                                egui::pos2(cell_rect.min.x + 4.0, cell_rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                text,
                                egui::FontId::default(),
                                cell_color,
                            );
                            if col_idx < cells.len() - 1 {
                                ui.allocate_space(egui::vec2(4.0, row_h));
                            }
                        }
                    })
                    .response
                    .rect;

                let row_resp = ui.interact(
                    row_rect,
                    ui.id().with("timer").with(i),
                    egui::Sense::click(),
                );
                row_resp.context_menu(|ui| {
                    if ui.button("Remove timer").clicked() {
                        remove_idx = Some(i);
                        ui.close();
                    }
                    if ui.button("Clear all timers").clicked() {
                        clear_all = true;
                        ui.close();
                    }
                });
            }
        });

    if clear_all {
        data.timers.clear_all();
        data.observer.reset_zone();
        return true;
    } else if let Some(idx) = remove_idx {
        data.timers.remove(idx);
    }
    false
}
