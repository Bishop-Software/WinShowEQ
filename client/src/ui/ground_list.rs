use egui::Ui;

use crate::data::AppData;

pub fn show(
    ui: &mut Ui,
    data: &mut AppData,
    sort_column: &mut Option<usize>,
    sort_ascending: &mut bool,
) {
    const HEADERS: &[&str] = &["Item", "X", "Y", "Z"];
    let row_h = ui.text_style_height(&egui::TextStyle::Body) + 4.0;
    let mut col_widths = data.ground_list_column_widths.clone();

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

    data.ground_list_column_widths = col_widths.clone();
    ui.separator();

    let mut items: Vec<_> = data.ground.iter().collect();
    if let Some(col) = sort_column {
        items.sort_by(|a, b| {
            let cmp = match col {
                0 => a.name.cmp(&b.name),
                1 => a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal),
                2 => a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal),
                3 => a.z.partial_cmp(&b.z).unwrap_or(std::cmp::Ordering::Equal),
                _ => std::cmp::Ordering::Equal,
            };
            if *sort_ascending { cmp } else { cmp.reverse() }
        });
    } else {
        items.sort_by(|a, b| a.name.cmp(&b.name));
    }

    // Data rows
    egui::ScrollArea::vertical()
        .id_salt("ground_scroll")
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            for item in &items {
                let cells = [
                    item.name.clone(),
                    format!("{:.0}", item.x),
                    format!("{:.0}", item.y),
                    format!("{:.0}", item.z),
                ];

                ui.horizontal(|ui| {
                    for (col_idx, text) in cells.iter().enumerate() {
                        ui.add_sized(
                            [col_widths[col_idx], row_h],
                            egui::Label::new(text).truncate(),
                        );

                        // Add matching resize handle spacing
                        if col_idx < cells.len() - 1 {
                            ui.allocate_space(egui::vec2(4.0, row_h));
                        }
                    }
                });
            }
        });
}