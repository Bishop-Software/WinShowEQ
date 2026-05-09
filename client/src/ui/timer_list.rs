use egui::Ui;

use crate::data::AppData;

/// Returns true if "Clear all timers" was requested (caller must delete the obs file).
pub fn show(
    ui: &mut Ui,
    data: &mut AppData,
    sort_column: &mut Option<usize>,
    sort_ascending: &mut bool,
) -> bool {
    const HEADERS: &[&str] = &["Name", "Loc", "Countdown", "Count"];
    let row_h = ui.text_style_height(&egui::TextStyle::Body) + 4.0;
    let mut col_widths = data.timer_list_column_widths.clone();

    // Header row with resizable columns
    ui.horizontal(|ui| {
        for (col_idx, (header, width)) in HEADERS.iter().zip(col_widths.iter_mut()).enumerate() {
            let indicator = match sort_column {
                Some(c) if *c == col_idx => if *sort_ascending { " ▲" } else { " ▼" },
                _ => "",
            };
            let label = format!("{}{}", header, indicator);

            // Allocate space for header and detect clicks
            let header_rect = ui.allocate_space(egui::vec2(*width, row_h)).1;
            let header_resp = ui.interact(header_rect, ui.id().with("header").with(col_idx), egui::Sense::click());

            // Display header as plain text
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

            // Resize handle between columns (except after last column)
            if col_idx < HEADERS.len() - 1 {
                let sep_width = 4.0;
                let sep_rect = ui.allocate_space(egui::vec2(sep_width, row_h)).1;
                let sep_sense = ui.interact(
                    sep_rect,
                    ui.id().with("resize").with(col_idx),
                    egui::Sense::drag(),
                );
                if sep_sense.dragged() {
                    let delta = sep_sense.drag_delta().x;
                    *width = (*width + delta).max(30.0);
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

    // Collect and sort timers
    let mut timers_with_idx: Vec<_> = data.timers.iter().enumerate().collect();
    if let Some(col) = sort_column {
        timers_with_idx.sort_by(|a, b| {
            let cmp = match col {
                0 => a.1.name.cmp(&b.1.name),
                1 => {
                    let a_loc = (a.1.x as i32, a.1.y as i32);
                    let b_loc = (b.1.x as i32, b.1.y as i32);
                    a_loc.cmp(&b_loc)
                }
                2 => a.1.secs_remaining().partial_cmp(&b.1.secs_remaining()).unwrap_or(std::cmp::Ordering::Equal),
                _ => std::cmp::Ordering::Equal,
            };
            if *sort_ascending { cmp } else { cmp.reverse() }
        });
    }

    // Data rows
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

                let row_rect = ui.horizontal(|ui| {
                    let name_cell = if t.is_auto {
                        format!("{} [A]", t.name)
                    } else {
                        t.name.clone()
                    };
                    let cells = [
                        name_cell,
                        format!("{:.0},{:.0}", t.x, t.y),
                        countdown,
                        if t.spawn_count > 0 { t.spawn_count.to_string() } else { String::new() },
                    ];

                    for (col_idx, text) in cells.iter().enumerate() {
                        let cell_color = if col_idx == 2 { color } else { ui.visuals().text_color() };
                        let (_, cell_rect) = ui.allocate_space(egui::vec2(col_widths[col_idx], row_h));
                        ui.painter().with_clip_rect(cell_rect).text(
                            egui::pos2(cell_rect.min.x + 4.0, cell_rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            text,
                            egui::FontId::default(),
                            cell_color,
                        );

                        // Add matching resize handle spacing
                        if col_idx < cells.len() - 1 {
                            ui.allocate_space(egui::vec2(4.0, row_h));
                        }
                    }
                }).response.rect;

                // Right-click context menu
                let row_resp = ui.interact(row_rect, ui.id().with("timer").with(i), egui::Sense::click());
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