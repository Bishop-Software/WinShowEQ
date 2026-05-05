use egui::{Context, Key, ScrollArea, TextEdit};

use crate::data::AppData;

struct SearchResult {
    id: u32,
    name: String,
    level: u8,
    class: u8,
    x: f32,
    y: f32,
    z: f32,
}

pub struct SearchDialog {
    pub open: bool,
    query: String,
    results: Vec<SearchResult>,
    needs_focus: bool,
}

impl Default for SearchDialog {
    fn default() -> Self {
        Self {
            open: false,
            query: String::new(),
            results: Vec::new(),
            needs_focus: false,
        }
    }
}

impl SearchDialog {
    /// Show the search dialog. Returns `Some(spawn_id)` if the user clicked a result row.
    pub fn show(&mut self, ctx: &Context, data: &mut AppData) -> Option<u32> {
        if !self.open {
            return None;
        }

        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.close(data);
            return None;
        }

        let mut clicked_id: Option<u32> = None;
        let mut still_open = true;

        egui::Window::new("Find Spawn")
            .collapsible(false)
            .resizable(true)
            .default_size([420.0, 300.0])
            .open(&mut still_open)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Search:");
                    let resp = ui.add(
                        TextEdit::singleline(&mut self.query)
                            .hint_text("spawn name…")
                            .desired_width(280.0),
                    );
                    if self.needs_focus {
                        resp.request_focus();
                        self.needs_focus = false;
                    }
                    if resp.changed() {
                        self.rebuild_results(data);
                    }
                    if ui.button("Clear").clicked() {
                        self.query.clear();
                        self.rebuild_results(data);
                    }
                });

                ui.label(format!("{} match(es)", self.results.len()));
                ui.separator();

                const COL_NAME: f32 = 160.0;
                const COL_LVL: f32 = 32.0;
                const COL_CLASS: f32 = 80.0;
                const COL_COORD: f32 = 64.0;

                let row_h = ui.text_style_height(&egui::TextStyle::Body) + 4.0;

                // Header row
                ui.horizontal(|ui| {
                    let text_color = ui.visuals().weak_text_color();
                    for (label, width) in [
                        ("Name", COL_NAME),
                        ("Lvl", COL_LVL),
                        ("Class", COL_CLASS),
                        ("X", COL_COORD),
                        ("Y", COL_COORD),
                        ("Z", COL_COORD),
                    ] {
                        ui.add_sized(
                            [width, row_h],
                            egui::Label::new(egui::RichText::new(label).color(text_color).strong()),
                        );
                    }
                });
                ui.separator();

                ScrollArea::vertical()
                    .id_salt("search_results")
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                        for r in &self.results {
                            let row_rect = ui.horizontal(|ui| {
                                let name_color = ui.visuals().strong_text_color();
                                let dim_color = ui.visuals().text_color();
                                ui.add_sized(
                                    [COL_NAME, row_h],
                                    egui::Label::new(
                                        egui::RichText::new(&r.name).color(name_color),
                                    ).truncate(),
                                );
                                ui.add_sized(
                                    [COL_LVL, row_h],
                                    egui::Label::new(
                                        egui::RichText::new(r.level.to_string()).color(dim_color),
                                    ),
                                );
                                ui.add_sized(
                                    [COL_CLASS, row_h],
                                    egui::Label::new(
                                        egui::RichText::new(data.game_data.class_name(r.class)).color(dim_color),
                                    ).truncate(),
                                );
                                for coord in [r.x, r.y, r.z] {
                                    ui.add_sized(
                                        [COL_COORD, row_h],
                                        egui::Label::new(
                                            egui::RichText::new(format!("{:.1}", coord))
                                                .color(dim_color),
                                        ),
                                    );
                                }
                            }).response.rect;

                            let row_resp = ui.interact(
                                row_rect,
                                ui.id().with(("sr", r.id)),
                                egui::Sense::click(),
                            );
                            if row_resp.hovered() {
                                ui.painter().rect_filled(
                                    row_rect,
                                    0.0,
                                    egui::Color32::from_rgba_premultiplied(255, 255, 255, 12),
                                );
                            }
                            if row_resp.clicked() {
                                clicked_id = Some(r.id);
                            }
                        }
                    });
            });

        if !still_open {
            self.close(data);
        }

        clicked_id
    }

    pub fn open(&mut self) {
        self.open = true;
        self.needs_focus = true;
    }

    fn rebuild_results(&mut self, data: &mut AppData) {
        let q = self.query.to_lowercase();
        self.results = if q.is_empty() {
            Vec::new()
        } else {
            data.spawns
                .iter()
                .filter(|s| Some(s.id) != data.self_id && s.name.to_lowercase().contains(&q))
                .map(|s| SearchResult {
                    id: s.id,
                    name: s.name.clone(),
                    level: s.level,
                    class: s.class,
                    x: s.x,
                    y: s.y,
                    z: s.z,
                })
                .collect()
        };

        data.marked_ids = self.results.iter().map(|r| r.id).collect();
    }

    fn close(&mut self, data: &mut AppData) {
        self.open = false;
        self.query.clear();
        self.results.clear();
        data.marked_ids.clear();
    }
}