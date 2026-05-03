use egui::Ui;

use crate::data::AppData;
use crate::data::spawns::{SpawnCategory, class_name};
use crate::filters::FilterCategory;

/// Action returned when the user selects a context menu item on a spawn row.
pub enum SpawnAction {
    AddTimer { name: String, x: f32, y: f32, z: f32 },
    AddToFilter { name: String, category: FilterCategory },
    AddMapText { x: f32, y: f32, z: f32 },
}

pub fn show(
    ui: &mut Ui,
    data: &mut AppData,
    sort_column: &mut Option<usize>,
    sort_ascending: &mut bool,
) -> Option<SpawnAction> {
    const HEADERS: &[&str] = &[
        "Name", "Last Name", "Lvl", "Class", "Race", "Type", "Owner", "Invis", "Speed", "X",
        "Y", "Z", "Dist", "ID", "Time",
    ];

    let player_pos = data.player_pos();

    let mut spawns: Vec<_> = data
        .spawns
        .iter()
        .filter(|s| Some(s.id) != data.self_id)
        .collect();

    // Apply sorting
    if let Some(col) = sort_column {
        spawns.sort_by(|a, b| {
            let cmp = match col {
                0 => a.name.cmp(&b.name),
                1 => a.last_name.cmp(&b.last_name),
                2 => a.level.cmp(&b.level),
                3 => class_name(a.class).cmp(class_name(b.class)),
                4 => data.game_data.race_name(a.race).cmp(data.game_data.race_name(b.race)),
                5 => {
                    let cat_a = spawn_category_str(a.spawn_category);
                    let cat_b = spawn_category_str(b.spawn_category);
                    cat_a.cmp(cat_b)
                }
                6 => {
                    let owner_a = owner_name_str(data, a.owner_id);
                    let owner_b = owner_name_str(data, b.owner_id);
                    owner_a.cmp(owner_b)
                }
                7 => (a.hidden != 0).cmp(&(b.hidden != 0)),
                8 => a.speed.partial_cmp(&b.speed).unwrap_or(std::cmp::Ordering::Equal),
                9 => a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal),
                10 => a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal),
                11 => a.z.partial_cmp(&b.z).unwrap_or(std::cmp::Ordering::Equal),
                12 => {
                    if let (Some((px, py, _)), Some((px2, py2, _))) = (player_pos, player_pos) {
                        let da = a.distance_2d(px, py);
                        let db = b.distance_2d(px2, py2);
                        da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                    } else {
                        std::cmp::Ordering::Equal
                    }
                }
                13 => a.id.cmp(&b.id),
                14 => a.first_seen.cmp(&b.first_seen),
                _ => std::cmp::Ordering::Equal,
            };
            if *sort_ascending {
                cmp
            } else {
                cmp.reverse()
            }
        });
    } else if let Some((px, py, _)) = player_pos {
        spawns.sort_by(|a, b| {
            let da = a.distance_2d(px, py);
            let db = b.distance_2d(px, py);
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    let mut col_widths = data.spawn_list_column_widths.clone();
    let row_h = ui.text_style_height(&egui::TextStyle::Body) + 4.0;

    // Frozen header row with resizable columns
    ui.horizontal(|ui| {
        for (col_idx, (header, width)) in HEADERS.iter().zip(col_widths.iter_mut()).enumerate() {
            let indicator = match sort_column {
                Some(c) if *c == col_idx => if *sort_ascending { " ▲" } else { " ▼" },
                _ => "",
            };
            let label_text = format!("{}{}", header, indicator);

            // Allocate space for the header cell and detect clicks
            let header_rect = ui.allocate_space(egui::vec2(*width, row_h)).1;
            let header_resp = ui.interact(header_rect, ui.id().with("header").with(col_idx), egui::Sense::click());

            // Display header as plain text (not a button)
            let text_color = if header_resp.hovered() {
                egui::Color32::WHITE
            } else {
                ui.visuals().text_color()
            };
            ui.painter().text(
                header_rect.left_top() + egui::vec2(4.0, 2.0),
                egui::Align2::LEFT_TOP,
                label_text,
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

            // Add resize handle between columns (except after last column)
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

    // Store updated column widths back to data
    data.spawn_list_column_widths = col_widths.clone();

    ui.separator();

    let mut action: Option<SpawnAction> = None;

    // Data rows (scrolled)
    egui::ScrollArea::both()
        .id_salt("spawn_scroll")
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            for s in &spawns {
                let dist_str = match player_pos {
                    Some((px, py, _)) => format!("{:.0}", s.distance_2d(px, py)),
                    None => "-".to_owned(),
                };
                let color = if s.is_danger {
                    egui::Color32::from_rgb(255, 80, 80)
                } else if s.is_caution {
                    egui::Color32::from_rgb(255, 160, 0)
                } else if s.is_hunt {
                    egui::Color32::from_rgb(0, 220, 120)
                } else if s.is_rare {
                    egui::Color32::from_rgb(200, 0, 255)
                } else {
                    ui.visuals().text_color()
                };

                let cells: [String; 15] = [
                    s.name.clone(),
                    s.last_name.clone(),
                    s.level.to_string(),
                    class_name(s.class).to_owned(),
                    data.game_data.race_name(s.race).to_owned(),
                    spawn_category_str(s.spawn_category).to_owned(),
                    owner_name_str(data, s.owner_id).to_owned(),
                    if s.hidden != 0 { "Y".to_owned() } else { String::new() },
                    format!("{:.1}", s.speed),
                    format!("{:.2}", s.x),
                    format!("{:.2}", s.y),
                    format!("{:.2}", s.z),
                    dist_str,
                    s.id.to_string(),
                    s.first_seen.format("%H:%M:%S").to_string(),
                ];

                let row_rect = ui.horizontal(|ui| {
                    for (i, text) in cells.iter().enumerate() {
                        let cell_color = if i == 0 { color } else { ui.visuals().text_color() };
                        ui.add_sized(
                            [col_widths[i], row_h],
                            egui::Label::new(
                                egui::RichText::new(text).color(cell_color)
                            ).truncate(),
                        );

                        // Add matching resize handle spacing (except after last column)
                        if i < cells.len() - 1 {
                            ui.allocate_space(egui::vec2(4.0, row_h));
                        }
                    }
                }).response.rect;

                // Capture name/pos before the closure borrows s
                let spawn_name = s.name.clone();
                let (sx, sy, sz) = (s.x, s.y, s.z);

                // ui.interact() gives the rect a click sense so context_menu fires on right-click
                let row_resp = ui.interact(row_rect, ui.id().with(s.id), egui::Sense::click());
                row_resp.context_menu(|ui| {
                    if ui.button("Add Timer…").clicked() {
                        action = Some(SpawnAction::AddTimer {
                            name: spawn_name.clone(),
                            x: sx, y: sy, z: sz,
                        });
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Add to Hunt").clicked() {
                        action = Some(SpawnAction::AddToFilter {
                            name: spawn_name.clone(),
                            category: FilterCategory::Hunt,
                        });
                        ui.close();
                    }
                    if ui.button("Add to Caution").clicked() {
                        action = Some(SpawnAction::AddToFilter {
                            name: spawn_name.clone(),
                            category: FilterCategory::Caution,
                        });
                        ui.close();
                    }
                    if ui.button("Add to Danger").clicked() {
                        action = Some(SpawnAction::AddToFilter {
                            name: spawn_name.clone(),
                            category: FilterCategory::Danger,
                        });
                        ui.close();
                    }
                    if ui.button("Add to Rare").clicked() {
                        action = Some(SpawnAction::AddToFilter {
                            name: spawn_name.clone(),
                            category: FilterCategory::Rare,
                        });
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Add Map Text").clicked() {
                        action = Some(SpawnAction::AddMapText { x: sx, y: sy, z: sz });
                        ui.close();
                    }
                });
            }
        });

    action
}

fn spawn_category_str(cat: SpawnCategory) -> &'static str {
    match cat {
        SpawnCategory::Pc => "PC",
        SpawnCategory::Npc => "NPC",
        SpawnCategory::Corpse => "Corpse",
        SpawnCategory::Pet => "Pet",
        SpawnCategory::Merc => "Merc",
    }
}

fn owner_name_str(data: &AppData, owner_id: u32) -> &str {
    if owner_id != 0 {
        data.spawns
            .get(owner_id)
            .map(|o| o.name.as_str())
            .unwrap_or("?")
    } else {
        ""
    }
}