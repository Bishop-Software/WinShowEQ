use std::collections::HashSet;

use egui::{Color32, FontId, Painter, Pos2, Rect, Sense, Stroke, Ui, Vec2};

use crate::config::{FollowMode, MapOverlaySettings};
use crate::data::AppData;
use crate::data::spawns::{ConColor, SpawnCategory, SpawnInfo, con_color};
use crate::filters::FilterCategory;
use crate::game_data::GameData;
use crate::map_reader::MapData;

const SPAWN_RADIUS: f32 = 4.0;
const HOVER_RADIUS: f32 = 8.0;
const SELF_RADIUS: f32 = 6.0;
const GROUND_RADIUS: f32 = 4.0;
pub(crate) const ZOOM_STEP: f32 = 1.12;
pub(crate) const ZOOM_MIN: f32 = 0.04;
pub(crate) const ZOOM_MAX: f32 = 20.0;

/// Action produced by the map canvas, to be handled by the caller.
pub enum MapAction {
    /// User chose "Add Map Note here" — EQ-space coordinates of the right-click point.
    AddNoteAt { eq_x: f32, eq_y: f32 },
    /// User left-clicked on or near a spawn dot. `None` means empty space (clear selection).
    SelectSpawn { id: Option<u32> },
    /// User left-clicked on or near a timer crosshair (no live spawn there).
    SelectTimer { spawn_loc: String },
    /// User chose a filter action from the map canvas right-click context menu.
    AddToFilter {
        name: String,
        category: FilterCategory,
    },
}

/// Persistent camera state for the map canvas.
pub struct MapState {
    pub zoom: f32,
    pub pan: Vec2,
    /// Shift-clicked target in map-space coords; `None` when no bearing line is active.
    pub bearing_target: Option<(f32, f32)>,
    /// If set, the view snaps to center on these map-space coords on the next frame.
    pub pending_center: Option<(f32, f32)>,
    /// Map-space position saved when the right-click context menu opens.
    context_menu_pos: Option<(f32, f32)>,
}

impl Default for MapState {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            pan: Vec2::ZERO,
            bearing_target: None,
            pending_center: None,
            context_menu_pos: None,
        }
    }
}

/// Map canvas widget. Borrow `data` and `state` each frame; state persists in the caller.
pub struct MapCon<'a> {
    data: &'a AppData,
    state: &'a mut MapState,
}

impl<'a> MapCon<'a> {
    pub fn new(data: &'a AppData, state: &'a mut MapState) -> Self {
        Self { data, state }
    }

    /// `z_filter`: if `Some((center_z, range))`, spawns with `|z - center| > range` are hidden.
    /// Returns `Some(MapAction)` if the context menu produced an action this frame.
    pub fn show(
        self,
        ui: &mut Ui,
        z_filter: Option<(f32, f32)>,
        overlay: &MapOverlaySettings,
        filtered_ids: Option<&HashSet<u32>>,
    ) -> Option<MapAction> {
        let MapCon { data, state } = self;

        let size = ui.available_size();
        let (response, painter) = ui.allocate_painter(size, Sense::click_and_drag());

        // Drag to pan
        if response.dragged() {
            state.pan += response.drag_delta();
        }

        // Scroll to zoom — guard with hovered() so scroll in other panels doesn't zoom the map
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 && response.hovered() {
            let factor = if scroll > 0.0 {
                ZOOM_STEP
            } else {
                1.0 / ZOOM_STEP
            };
            state.zoom = (state.zoom * factor).clamp(ZOOM_MIN, ZOOM_MAX);
        }

        // Snap view to a search result if requested.
        if let Some((wx, wy)) = state.pending_center.take() {
            let focus = focus_world(data);
            state.pan.x = -(wx - focus.0) * state.zoom;
            state.pan.y = (wy - focus.1) * state.zoom;
        }

        let mut action: Option<MapAction> = None;

        // Shift+left-click → set bearing target; plain click or ESC → clear it.
        let (shift_held, esc_pressed) =
            ui.input(|i| (i.modifiers.shift, i.key_pressed(egui::Key::Escape)));
        if esc_pressed {
            state.bearing_target = None;
        } else if response.clicked_by(egui::PointerButton::Primary) {
            if shift_held {
                if let Some(screen_pos) = response.interact_pointer_pos() {
                    let center = response.rect.center();
                    let focus = focus_world(data);
                    let wx = focus.0 + (screen_pos.x - center.x - state.pan.x) / state.zoom;
                    let wy = focus.1 + (center.y + state.pan.y - screen_pos.y) / state.zoom;
                    state.bearing_target = Some((wx, wy));
                }
            } else {
                state.bearing_target = None;
                if let Some(screen_pos) = response.interact_pointer_pos() {
                    let focus = focus_world(data);
                    let pan = state.pan;
                    let zoom = state.zoom;
                    let center = response.rect.center();
                    let to_screen = |wx: f32, wy: f32| {
                        Pos2::new(
                            center.x + (wx - focus.0) * zoom + pan.x,
                            center.y - (wy - focus.1) * zoom + pan.y,
                        )
                    };

                    // 1. Hit-test live spawn dots
                    let mut best_dist = HOVER_RADIUS;
                    let mut best_spawn: Option<u32> = None;
                    for spawn in data.spawns.iter() {
                        let (mx, my) = eq_to_map(spawn.x, spawn.y);
                        let d = screen_pos.distance(to_screen(mx, my));
                        if d < best_dist {
                            best_dist = d;
                            best_spawn = Some(spawn.id);
                        }
                    }

                    if let Some(id) = best_spawn {
                        action = Some(MapAction::SelectSpawn { id: Some(id) });
                    } else if overlay.show_timer_dots {
                        // 2. Hit-test timer crosshairs (auto-timers only)
                        let mut best_dist = HOVER_RADIUS;
                        let mut best_timer: Option<String> = None;
                        for timer in data.timers.iter() {
                            if !timer.is_auto || z_filtered(timer.z, z_filter) {
                                continue;
                            }
                            let (mx, my) = eq_to_map(timer.x, timer.y);
                            let d = screen_pos.distance(to_screen(mx, my));
                            if d < best_dist {
                                best_dist = d;
                                best_timer = Some(timer.spawn_loc.clone());
                            }
                        }
                        action = Some(if let Some(loc) = best_timer {
                            MapAction::SelectTimer { spawn_loc: loc }
                        } else {
                            MapAction::SelectSpawn { id: None } // empty space
                        });
                    } else {
                        action = Some(MapAction::SelectSpawn { id: None }); // empty space
                    }
                }
            }
        }

        // Right-click: save map-space position for the context menu.
        if response.secondary_clicked()
            && let Some(screen_pos) = response.interact_pointer_pos()
        {
            let center = response.rect.center();
            let focus = focus_world(data);
            let wx = focus.0 + (screen_pos.x - center.x - state.pan.x) / state.zoom;
            let wy = focus.1 + (center.y + state.pan.y - screen_pos.y) / state.zoom;
            state.context_menu_pos = Some((wx, wy));
        }

        // Apply follow mode: override pan and compute focus point.
        let focus = match overlay.follow_mode {
            FollowMode::None => focus_world(data),
            FollowMode::Player => {
                state.pan = Vec2::ZERO;
                focus_world(data)
            }
            FollowMode::Target => {
                state.pan = Vec2::ZERO;
                data.target_id
                    .and_then(|tid| data.spawns.get(tid))
                    .map(|t| eq_to_map(t.x, t.y))
                    .unwrap_or_else(|| focus_world(data))
            }
        };

        painter.rect_filled(response.rect, 0.0, Color32::BLACK);

        let ctx = DrawCtx {
            painter: &painter,
            rect: response.rect,
            center: response.rect.center(),
            focus,
            zoom: state.zoom,
            pan: state.pan,
        };

        if overlay.show_grid {
            draw_grid(&ctx);
        }
        draw_map_lines(&ctx, &data.map, overlay);
        draw_labels(&ctx, &data.map, overlay);
        draw_mob_trails(&ctx, data, filtered_ids);
        draw_ground_items(&ctx, data, z_filter);
        draw_spawns(&ctx, data, z_filter, overlay, filtered_ids);
        if overlay.show_timer_dots {
            draw_timer_dots(&ctx, data, z_filter);
        }
        draw_self(&ctx, data);
        draw_annotations(&ctx, data);
        if let Some(target) = state.bearing_target {
            draw_bearing_line(&ctx, data, target);
        }
        draw_hud(&ctx, ui, data);
        draw_follow_indicator(&ctx, overlay);

        if let Some(hover_pos) = response.hover_pos() {
            draw_hover_tooltip(ui, &ctx, data, hover_pos, z_filter, overlay);
        }

        // Context menu (shown on right-click, persists until dismissed).
        response.context_menu(|ui| {
            // Hit-test: find the nearest spawn within HOVER_RADIUS screen pixels of the
            // right-click point, using map-space distance to avoid needing a DrawCtx here.
            let nearby: Option<String> = state.context_menu_pos.and_then(|(wx, wy)| {
                let threshold = HOVER_RADIUS / state.zoom.max(0.001);
                let mut best_dist = threshold;
                let mut best_name: Option<String> = None;
                for spawn in data.spawns.iter() {
                    let (mx, my) = eq_to_map(spawn.x, spawn.y);
                    let d = ((mx - wx) * (mx - wx) + (my - wy) * (my - wy)).sqrt();
                    if d < best_dist {
                        best_dist = d;
                        best_name = Some(
                            spawn
                                .name
                                .trim_end_matches(|c: char| c.is_ascii_digit())
                                .trim_end()
                                .to_string(),
                        );
                    }
                }
                best_name
            });

            if let Some(ref name) = nearby {
                ui.label(egui::RichText::new(name).strong());
                ui.separator();
                if ui.button("Add to Hunt").clicked() {
                    action = Some(MapAction::AddToFilter {
                        name: name.clone(),
                        category: FilterCategory::Hunt,
                    });
                    ui.close();
                }
                if ui.button("Add to Caution").clicked() {
                    action = Some(MapAction::AddToFilter {
                        name: name.clone(),
                        category: FilterCategory::Caution,
                    });
                    ui.close();
                }
                if ui.button("Add to Danger").clicked() {
                    action = Some(MapAction::AddToFilter {
                        name: name.clone(),
                        category: FilterCategory::Danger,
                    });
                    ui.close();
                }
                if ui.button("Add to Rare").clicked() {
                    action = Some(MapAction::AddToFilter {
                        name: name.clone(),
                        category: FilterCategory::Rare,
                    });
                    ui.close();
                }
                ui.separator();
            }

            if let Some((wx, wy)) = state.context_menu_pos {
                if ui.button("Add Map Note here…").clicked() {
                    action = Some(MapAction::AddNoteAt {
                        eq_x: -wx,
                        eq_y: wy,
                    });
                    ui.close();
                }
                if ui.button("Center map here").clicked() {
                    state.pending_center = Some((wx, wy));
                    ui.close();
                }
            }
            if state.bearing_target.is_some() && ui.button("Clear bearing line").clicked() {
                state.bearing_target = None;
                ui.close();
            }
        });

        action
    }
}

/// Transform EQ spawn coordinates to map coordinate space.
/// Player map-space focus point — the player's position in map coords, or origin.
fn focus_world(data: &AppData) -> (f32, f32) {
    data.self_id
        .and_then(|id| data.spawns.get(id))
        .map(|s| eq_to_map(s.x, s.y))
        .unwrap_or((0.0, 0.0))
}

/// Negate X only: wire spawn.X = -file_x (opposite sign from map file first coord).
/// Wire spawn.Y = -file_y which matches MapLine.y (map_reader negates Y on load), so Y is unchanged.
#[inline]
fn eq_to_map(eq_x: f32, eq_y: f32) -> (f32, f32) {
    (-eq_x, eq_y)
}

/// Public re-export of the EQ→map coordinate transform for callers outside this module.
#[inline]
pub fn eq_to_map_pub(eq_x: f32, eq_y: f32) -> (f32, f32) {
    eq_to_map(eq_x, eq_y)
}

fn draw_mob_trails(ctx: &DrawCtx, data: &AppData, filtered_ids: Option<&HashSet<u32>>) {
    if !data.trails_enabled {
        return;
    }
    for (&spawn_id, trail) in &data.trails {
        if let Some(ids) = filtered_ids
            && !ids.contains(&spawn_id)
        {
            continue;
        }
        for &(mx, my) in trail {
            let pos = ctx.to_screen(mx, my);
            if ctx.is_visible(pos) {
                ctx.painter.circle_filled(
                    pos,
                    2.0,
                    Color32::from_rgba_premultiplied(160, 100, 40, 140),
                );
            }
        }
    }
}

/// Map files sometimes store black (0,0,0) which is invisible on the black canvas background.
/// Substitute a visible dark gray in that case.
#[inline]
fn map_color(r: u8, g: u8, b: u8) -> Color32 {
    if r == 0 && g == 0 && b == 0 {
        Color32::from_rgb(100, 100, 100)
    } else {
        Color32::from_rgb(r, g, b)
    }
}

fn layer_visible(layer: u8, overlay: &MapOverlaySettings) -> bool {
    match layer {
        1 => overlay.show_layer1,
        2 => overlay.show_layer2,
        3 => overlay.show_layer3,
        _ => true, // base layer always shown
    }
}

fn draw_grid(ctx: &DrawCtx) {
    let w = ctx.rect.width();
    let h = ctx.rect.height();

    // Auto-select interval so ~4–8 lines are visible across the width.
    let interval = [250.0_f32, 500.0, 1000.0, 2000.0, 5000.0]
        .iter()
        .copied()
        .find(|&i| w / (i * ctx.zoom) < 8.0)
        .unwrap_or(5000.0);

    // Visible EQ coordinate range (map space == EQ space with sign flip; DrawCtx works in map space).
    let eq_left = ctx.focus.0 - (w / 2.0 + ctx.pan.x) / ctx.zoom;
    let eq_right = ctx.focus.0 + (w / 2.0 - ctx.pan.x) / ctx.zoom;
    let eq_bottom = ctx.focus.1 - (h / 2.0 - ctx.pan.y) / ctx.zoom;
    let eq_top = ctx.focus.1 + (h / 2.0 + ctx.pan.y) / ctx.zoom;

    let grid_color = Color32::from_rgba_premultiplied(128, 128, 128, 48);
    let label_color = Color32::from_rgba_premultiplied(180, 180, 180, 160);
    let font = FontId::proportional(10.0);

    // Minimum pixel spacing between labels before we suppress them.
    let label_px_threshold = 40.0_f32;
    let show_labels = interval * ctx.zoom >= label_px_threshold;

    // Vertical lines (constant map-X / EQ north-south coordinate).
    let first_x = (eq_left / interval).ceil() * interval;
    let mut gx = first_x;
    while gx <= eq_right {
        let screen = ctx.to_screen(gx, 0.0);
        let top = Pos2::new(screen.x, ctx.rect.min.y);
        let bot = Pos2::new(screen.x, ctx.rect.max.y);
        ctx.painter
            .line_segment([top, bot], Stroke::new(1.0, grid_color));
        if show_labels {
            ctx.painter.text(
                Pos2::new(screen.x + 2.0, ctx.rect.min.y + 2.0),
                egui::Align2::LEFT_TOP,
                format!("{:.0}", gx),
                font.clone(),
                label_color,
            );
        }
        gx += interval;
    }

    // Horizontal lines (constant map-Y / EQ east-west coordinate).
    let first_y = (eq_bottom / interval).ceil() * interval;
    let mut gy = first_y;
    while gy <= eq_top {
        let screen = ctx.to_screen(0.0, gy);
        let left = Pos2::new(ctx.rect.min.x, screen.y);
        let right = Pos2::new(ctx.rect.max.x, screen.y);
        ctx.painter
            .line_segment([left, right], Stroke::new(1.0, grid_color));
        if show_labels {
            ctx.painter.text(
                Pos2::new(ctx.rect.min.x + 2.0, screen.y - 2.0),
                egui::Align2::LEFT_BOTTOM,
                format!("{:.0}", gy),
                font.clone(),
                label_color,
            );
        }
        gy += interval;
    }
}

fn draw_timer_dots(ctx: &DrawCtx, data: &AppData, z_filter: Option<(f32, f32)>) {
    let ltgray = Color32::from_rgb(160, 160, 160);
    let red = Color32::from_rgb(255, 60, 60);
    let orange = Color32::from_rgb(255, 140, 0);
    let yellow = Color32::from_rgb(255, 210, 0);
    let arm = SPAWN_RADIUS;

    // Flash state for < 30s: toggle every 500 ms using wall clock
    let flash = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| (d.as_millis() / 500) % 2 == 0)
        .unwrap_or(true);

    for timer in data.timers.iter() {
        // Only auto-learned timers (confirmed at least two respawn cycles)
        if !timer.is_auto {
            continue;
        }
        if z_filtered(timer.z, z_filter) {
            continue;
        }
        let (mx, my) = eq_to_map(timer.x, timer.y);
        let pos = ctx.to_screen(mx, my);
        if !ctx.is_visible(pos) {
            continue;
        }
        let secs = timer.secs_remaining();

        // Match C# color bands
        let color = if secs <= 0 {
            ltgray // spawned — already up
        } else if secs < 30 {
            if flash { red } else { continue } // flashing red
        } else if secs < 60 {
            red
        } else if secs < 90 {
            orange
        } else if secs < 120 {
            yellow
        } else {
            ltgray // > 2 min remaining
        };

        // Crosshair (+)
        let stroke = Stroke::new(2.0, color);
        ctx.painter.line_segment(
            [Pos2::new(pos.x - arm, pos.y), Pos2::new(pos.x + arm, pos.y)],
            stroke,
        );
        ctx.painter.line_segment(
            [Pos2::new(pos.x, pos.y - arm), Pos2::new(pos.x, pos.y + arm)],
            stroke,
        );

        // Countdown text next to the crosshair when within 2 minutes
        if secs > 0 && secs < 120 {
            ctx.painter.text(
                Pos2::new(pos.x + arm + 2.0, pos.y),
                egui::Align2::LEFT_CENTER,
                secs.to_string(),
                FontId::proportional(10.0),
                color,
            );
        }

        // Gold ring around the selected timer crosshair
        if data.selected_timer_loc.as_deref() == Some(timer.spawn_loc.as_str()) {
            ctx.painter.circle_stroke(
                pos,
                SPAWN_RADIUS + 4.0,
                Stroke::new(2.0, Color32::from_rgb(255, 200, 0)),
            );
        }
    }
}

fn draw_map_lines(ctx: &DrawCtx, map: &MapData, overlay: &MapOverlaySettings) {
    for line in &map.lines {
        if !layer_visible(line.layer, overlay) {
            continue;
        }
        let p1 = ctx.to_screen(line.p1.x, line.p1.y);
        let p2 = ctx.to_screen(line.p2.x, line.p2.y);
        if ctx.either_visible(p1, p2) {
            let [r, g, b] = line.color;
            ctx.painter
                .line_segment([p1, p2], Stroke::new(1.0, map_color(r, g, b)));
        }
    }
}

fn draw_labels(ctx: &DrawCtx, map: &MapData, overlay: &MapOverlaySettings) {
    if !overlay.show_zone_text {
        return;
    }
    for label in &map.labels {
        if !layer_visible(label.layer, overlay) {
            continue;
        }
        let pos = ctx.to_screen(label.pos.x, label.pos.y);
        if !ctx.is_visible(pos) {
            continue;
        }
        let font_size = match label.size {
            1 => 9.0_f32,
            3 => 14.0,
            _ => 11.0,
        };
        let [r, g, b] = label.color;
        ctx.painter.text(
            pos,
            egui::Align2::CENTER_CENTER,
            &label.text,
            FontId::proportional(font_size),
            map_color(r, g, b),
        );
    }
}

fn draw_spawns(
    ctx: &DrawCtx,
    data: &AppData,
    z_filter: Option<(f32, f32)>,
    overlay: &MapOverlaySettings,
    filtered_ids: Option<&HashSet<u32>>,
) {
    let player_level = data.self_level();
    for spawn in data.spawns.iter() {
        if Some(spawn.id) == data.self_id {
            continue;
        }
        if z_filtered(spawn.z, z_filter) {
            continue;
        }

        // Spawn filter — target is always shown regardless of filter state
        if let Some(ids) = filtered_ids
            && data.target_id != Some(spawn.id)
            && !ids.contains(&spawn.id)
        {
            continue;
        }

        // Visibility filtering
        let visible = match spawn.spawn_category {
            SpawnCategory::Npc => overlay.show_npcs,
            SpawnCategory::Pc => overlay.show_players,
            SpawnCategory::Corpse => overlay.show_corpses,
            SpawnCategory::Pet | SpawnCategory::Merc => overlay.show_pets,
        };
        if !visible {
            continue;
        }

        let (mx, my) = eq_to_map(spawn.x, spawn.y);
        let pos = ctx.to_screen(mx, my);
        if !ctx.is_visible(pos) {
            continue;
        }
        let color = spawn_color(spawn, player_level, &data.game_data);
        let is_pc = spawn.spawn_category == SpawnCategory::Pc;
        let is_corpse = spawn.spawn_category == SpawnCategory::Corpse;
        if is_corpse {
            if is_pc_corpse(&spawn.name) {
                // PC corpse: hollow yellow square (matches C# DrawRectangle with yellowPen)
                let r = SPAWN_RADIUS;
                let sq = egui::Rect::from_center_size(pos, egui::Vec2::splat(r * 2.0));
                ctx.painter.rect_stroke(
                    sq,
                    0.0,
                    Stroke::new(1.5, Color32::from_rgb(255, 210, 0)),
                    egui::StrokeKind::Middle,
                );
            } else {
                // NPC corpse: cyan crosshair (matches C# DrawLine with cyanPen)
                let d = SPAWN_RADIUS;
                ctx.painter.line_segment(
                    [Pos2::new(pos.x - d, pos.y), Pos2::new(pos.x + d, pos.y)],
                    Stroke::new(1.5, Color32::from_rgb(0, 220, 220)),
                );
                ctx.painter.line_segment(
                    [Pos2::new(pos.x, pos.y - d), Pos2::new(pos.x, pos.y + d)],
                    Stroke::new(1.5, Color32::from_rgb(0, 220, 220)),
                );
            }
        } else if is_pc {
            let r = SPAWN_RADIUS;
            let sq = egui::Rect::from_center_size(pos, egui::Vec2::splat(r * 2.0));
            ctx.painter.rect_filled(sq, 0.0, color);
            ctx.painter.rect_stroke(
                sq,
                0.0,
                Stroke::new(1.0, Color32::from_rgb(255, 0, 255)),
                egui::StrokeKind::Middle,
            );
            if data.selected_id == Some(spawn.id) {
                ctx.painter.rect_stroke(
                    sq.expand(4.0),
                    0.0,
                    Stroke::new(2.0, Color32::from_rgb(255, 200, 0)),
                    egui::StrokeKind::Outside,
                );
            } else if data.marked_ids.contains(&spawn.id) {
                ctx.painter.rect_stroke(
                    sq.expand(3.0),
                    0.0,
                    Stroke::new(1.5, Color32::WHITE),
                    egui::StrokeKind::Outside,
                );
            }
            if data.target_id == Some(spawn.id) {
                ctx.painter.rect_stroke(
                    sq.expand(6.0),
                    0.0,
                    Stroke::new(2.0, Color32::from_rgb(255, 120, 0)),
                    egui::StrokeKind::Outside,
                );
            }
        } else {
            ctx.painter.circle_filled(pos, SPAWN_RADIUS, color);
            if data.selected_id == Some(spawn.id) {
                ctx.painter.circle_stroke(
                    pos,
                    SPAWN_RADIUS + 4.0,
                    Stroke::new(2.0, Color32::from_rgb(255, 200, 0)),
                );
            } else if data.marked_ids.contains(&spawn.id) {
                ctx.painter.circle_stroke(
                    pos,
                    SPAWN_RADIUS + 3.0,
                    Stroke::new(1.5, Color32::WHITE),
                );
            }
            if data.target_id == Some(spawn.id) {
                ctx.painter.circle_stroke(
                    pos,
                    SPAWN_RADIUS + 6.0,
                    Stroke::new(2.0, Color32::from_rgb(255, 120, 0)),
                );
            }
        }

        // Map label overlay
        let label: Option<String> = if is_pc {
            if overlay.show_player_names {
                Some(spawn.name.clone())
            } else {
                None
            }
        } else {
            match (overlay.show_npc_names, overlay.show_npc_levels) {
                (true, true) => Some(format!("{} ({})", spawn.name, spawn.level)),
                (true, false) => Some(spawn.name.clone()),
                (false, true) => Some(spawn.level.to_string()),
                (false, false) => None,
            }
        };
        if let Some(text) = label {
            let label_pos = pos + Vec2::new(0.0, -(SPAWN_RADIUS + 2.0));
            ctx.painter.with_clip_rect(ctx.rect).text(
                label_pos,
                egui::Align2::CENTER_BOTTOM,
                &text,
                FontId::proportional(10.0),
                color,
            );
        }
    }
}

fn draw_self(ctx: &DrawCtx, data: &AppData) {
    let Some(id) = data.self_id else { return };
    let Some(s) = data.spawns.get(id) else { return };
    let (mx, my) = eq_to_map(s.x, s.y);
    let pos = ctx.to_screen(mx, my);

    ctx.painter.circle_filled(pos, SELF_RADIUS, Color32::WHITE);
    ctx.painter
        .circle_stroke(pos, SELF_RADIUS, Stroke::new(1.5, Color32::BLACK));

    // Direction arrow — EQ heading: 0 = north, 128 = west, 256 = south, 384 = east,
    // 512 = full circle. Increases counter-clockwise. Matches C# MySEQ xSin/xCos convention.
    let heading_rad = s.heading * std::f32::consts::TAU / 512.0;
    let arrow_len = SELF_RADIUS * 2.5;
    let tip = Pos2::new(
        pos.x - heading_rad.sin() * arrow_len,
        pos.y - heading_rad.cos() * arrow_len,
    );
    ctx.painter
        .line_segment([pos, tip], Stroke::new(2.0, Color32::WHITE));
}

fn draw_ground_items(ctx: &DrawCtx, data: &AppData, z_filter: Option<(f32, f32)>) {
    for item in data.ground.iter() {
        if z_filtered(item.z, z_filter) {
            continue;
        }
        let (mx, my) = eq_to_map(item.x, item.y);
        let pos = ctx.to_screen(mx, my);
        if !ctx.is_visible(pos) {
            continue;
        }
        let r = GROUND_RADIUS;
        // Diamond shape
        let pts = [
            Pos2::new(pos.x, pos.y - r),
            Pos2::new(pos.x + r, pos.y),
            Pos2::new(pos.x, pos.y + r),
            Pos2::new(pos.x - r, pos.y),
        ];
        for i in 0..4 {
            ctx.painter.line_segment(
                [pts[i], pts[(i + 1) % 4]],
                Stroke::new(1.5, Color32::YELLOW),
            );
        }
    }
}

fn draw_annotations(ctx: &DrawCtx, data: &AppData) {
    for ann in &data.annotations.items {
        let (mx, my) = eq_to_map(ann.x, ann.y);
        let pos = ctx.to_screen(mx, my);
        if !ctx.is_visible(pos) {
            continue;
        }
        let [r, g, b] = ann.color;
        let color = Color32::from_rgb(r, g, b);
        let font_size = ann.size as f32;
        // Small diamond marker
        let d = 4.0_f32;
        let pts = [
            Pos2::new(pos.x, pos.y - d),
            Pos2::new(pos.x + d, pos.y),
            Pos2::new(pos.x, pos.y + d),
            Pos2::new(pos.x - d, pos.y),
        ];
        for i in 0..4 {
            ctx.painter
                .line_segment([pts[i], pts[(i + 1) % 4]], Stroke::new(1.5, color));
        }
        ctx.painter.text(
            pos + Vec2::new(6.0, 0.0),
            egui::Align2::LEFT_CENTER,
            &ann.text,
            FontId::proportional(font_size),
            color,
        );
    }
}

/// Overlay: zone name and world time in the top-left corner.
fn draw_hud(ctx: &DrawCtx, _ui: &mut Ui, data: &AppData) {
    let top_left = ctx.rect.min + Vec2::new(6.0, 4.0);
    let font = FontId::proportional(13.0);

    if !data.zone_name.is_empty() {
        ctx.painter.text(
            top_left,
            egui::Align2::LEFT_TOP,
            &data.zone_name,
            font.clone(),
            Color32::WHITE,
        );
    }

    if data.world_time != Default::default() {
        let time_pos = top_left + Vec2::new(0.0, 18.0);
        ctx.painter.text(
            time_pos,
            egui::Align2::LEFT_TOP,
            data.world_time.display(),
            font.clone(),
            Color32::from_rgb(200, 200, 150),
        );
    }

    if let Some(pos) = data.player_pos() {
        // EQ /loc order is Y, X, Z
        let loc_text = format!("/loc {:.0}, {:.0}, {:.0}", pos.1, pos.0, pos.2);
        let loc_pos = top_left + Vec2::new(0.0, 36.0);
        ctx.painter.text(
            loc_pos,
            egui::Align2::LEFT_TOP,
            loc_text,
            font,
            Color32::from_rgb(150, 220, 150),
        );
    }
}

fn draw_follow_indicator(ctx: &DrawCtx, overlay: &MapOverlaySettings) {
    let label = match overlay.follow_mode {
        FollowMode::None => return,
        FollowMode::Player => "Following: Player",
        FollowMode::Target => "Following: Target",
    };
    let pos = Pos2::new(ctx.rect.min.x + 6.0, ctx.rect.max.y - 6.0);
    ctx.painter.text(
        pos,
        egui::Align2::LEFT_BOTTOM,
        label,
        FontId::proportional(12.0),
        Color32::from_rgba_premultiplied(180, 220, 255, 200),
    );
}

enum HoverHit<'a> {
    Spawn(&'a SpawnInfo),
    Ground(&'a crate::data::ground::GroundItem),
    Timer(&'a crate::data::timers::SpawnTimer),
}

fn draw_hover_tooltip(
    ui: &mut Ui,
    ctx: &DrawCtx,
    data: &AppData,
    hover_pos: Pos2,
    z_filter: Option<(f32, f32)>,
    overlay: &MapOverlaySettings,
) {
    let mut best_dist = f32::MAX;
    let mut hit: Option<HoverHit<'_>> = None;

    for spawn in data.spawns.iter() {
        if z_filtered(spawn.z, z_filter) {
            continue;
        }
        let (mx, my) = eq_to_map(spawn.x, spawn.y);
        let screen_pos = ctx.to_screen(mx, my);
        if !ctx.is_visible(screen_pos) {
            continue;
        }
        let dist = hover_pos.distance(screen_pos);
        if dist <= HOVER_RADIUS && dist < best_dist {
            best_dist = dist;
            hit = Some(HoverHit::Spawn(spawn));
        }
    }

    for item in data.ground.iter() {
        if z_filtered(item.z, z_filter) {
            continue;
        }
        let (mx, my) = eq_to_map(item.x, item.y);
        let screen_pos = ctx.to_screen(mx, my);
        if !ctx.is_visible(screen_pos) {
            continue;
        }
        let dist = hover_pos.distance(screen_pos);
        if dist <= HOVER_RADIUS && dist < best_dist {
            best_dist = dist;
            hit = Some(HoverHit::Ground(item));
        }
    }

    if overlay.show_timer_dots {
        for timer in data.timers.iter() {
            let (mx, my) = eq_to_map(timer.x, timer.y);
            let screen_pos = ctx.to_screen(mx, my);
            if !ctx.is_visible(screen_pos) {
                continue;
            }
            let dist = hover_pos.distance(screen_pos);
            if dist <= HOVER_RADIUS && dist < best_dist {
                best_dist = dist;
                hit = Some(HoverHit::Timer(timer));
            }
        }
    }

    let Some(hit) = hit else { return };

    let player = data.self_id.and_then(|id| data.spawns.get(id));
    let player_dist = |x: f32, y: f32| -> String {
        player
            .map(|s| {
                let dx = x - s.x;
                let dy = y - s.y;
                format!("{:.0}", (dx * dx + dy * dy).sqrt())
            })
            .unwrap_or_else(|| "?".to_owned())
    };

    #[allow(deprecated)]
    egui::show_tooltip_at_pointer(
        ui.ctx(),
        ui.layer_id(),
        egui::Id::new("map_hover_tooltip"),
        |ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            match hit {
                HoverHit::Spawn(s) => {
                    ui.label(format!("{} ({})", s.name, s.level));
                    let invis = if s.hidden != 0 { "Invis" } else { "Visible" };
                    ui.label(format!(
                        "{} / {}",
                        data.game_data.race_name(s.race),
                        data.game_data.class_name(s.class)
                    ));
                    ui.label(format!("Invisible: {invis}"));
                    ui.label(format!("Speed: {:.1}", s.speed));
                    ui.label(format!("Dist: {}", player_dist(s.x, s.y)));
                    ui.label(format!("Loc: {:.2}, {:.2}, {:.2}", s.x, s.y, s.z));
                }
                HoverHit::Ground(g) => {
                    ui.label(&g.name);
                    ui.label(format!("Dist: {}", player_dist(g.x, g.y)));
                }
                HoverHit::Timer(t) => {
                    ui.label(egui::RichText::new(&t.name).strong());
                    if t.is_alive() {
                        ui.label("Alive — timer starts on kill");
                    } else if t.is_spawned() {
                        ui.label("Unknown — check spawn point");
                    } else {
                        ui.label(format!("Respawn in: {}", t.countdown_str()));
                    }
                }
            }
        },
    );
}

fn draw_bearing_line(ctx: &DrawCtx, data: &AppData, target: (f32, f32)) {
    let Some(id) = data.self_id else { return };
    let Some(s) = data.spawns.get(id) else { return };
    let (player_mx, player_my) = eq_to_map(s.x, s.y);
    let (target_mx, target_my) = target;

    let player_screen = ctx.to_screen(player_mx, player_my);
    let target_screen = ctx.to_screen(target_mx, target_my);

    let color = Color32::from_rgb(255, 220, 0);
    ctx.painter
        .line_segment([player_screen, target_screen], Stroke::new(1.5, color));

    // Small crosshair circle at target
    ctx.painter
        .circle_stroke(target_screen, 5.0, Stroke::new(1.5, color));

    // Distance in EQ units (map-space distance == EQ-space distance; eq_to_map only flips sign)
    let dx = target_mx - player_mx;
    let dy = target_my - player_my;
    let distance = (dx * dx + dy * dy).sqrt();

    // Bearing: angle clockwise from north. Map north = +y, east = +x.
    let bearing_deg = dx.atan2(dy).to_degrees();
    let bearing_deg = if bearing_deg < 0.0 {
        bearing_deg + 360.0
    } else {
        bearing_deg
    };

    let label = format!(
        "{:.0} units  {:.0}°  {}",
        distance,
        bearing_deg,
        to_cardinal(bearing_deg)
    );
    ctx.painter.text(
        target_screen + Vec2::new(8.0, -8.0),
        egui::Align2::LEFT_BOTTOM,
        &label,
        FontId::proportional(12.0),
        color,
    );
}

fn to_cardinal(degrees: f32) -> &'static str {
    let idx = ((degrees + 22.5) / 45.0) as usize % 8;
    ["N", "NE", "E", "SE", "S", "SW", "W", "NW"][idx]
}

/// Returns true if a corpse spawn was likely a player character.
/// Matches C# heuristic: no underscore, doesn't start with "a " or "an ".
/// NPC names always start with an article or contain underscores; player names never do.
#[inline]
fn is_pc_corpse(name: &str) -> bool {
    !name.contains('_') && !name.starts_with("a ") && !name.starts_with("an ")
}

/// Returns true if `z` is outside the filter range and should be hidden.
#[inline]
fn z_filtered(z: f32, filter: Option<(f32, f32)>) -> bool {
    match filter {
        Some((center, range)) => (z - center).abs() > range,
        None => false,
    }
}

/// Spawn dot color: filter flags take priority, then named color overrides, then con-color.
fn spawn_color(spawn: &SpawnInfo, player_level: u8, game_data: &GameData) -> Color32 {
    if spawn.is_danger {
        return Color32::from_rgb(255, 50, 50);
    }
    if spawn.is_caution {
        return Color32::from_rgb(255, 140, 0);
    }
    if spawn.is_hunt {
        return Color32::from_rgb(0, 255, 120);
    }
    if spawn.is_rare {
        return Color32::from_rgb(220, 0, 255);
    }
    if let Some([r, g, b]) = game_data.spawn_color_override(&spawn.name) {
        return Color32::from_rgb(r, g, b);
    }
    match spawn.spawn_category {
        SpawnCategory::Pc => con_to_color(con_color(player_level, spawn.level)),
        SpawnCategory::Corpse => Color32::from_rgb(80, 40, 40),
        SpawnCategory::Pet | SpawnCategory::Merc => {
            con_to_color(con_color(player_level, spawn.level))
        }
        SpawnCategory::Npc => con_to_color(con_color(player_level, spawn.level)),
    }
}

fn con_to_color(c: ConColor) -> Color32 {
    match c {
        ConColor::Gray => Color32::from_rgb(128, 128, 128),
        ConColor::Green => Color32::from_rgb(0, 200, 0),
        ConColor::LightBlue => Color32::from_rgb(0, 200, 255),
        ConColor::Blue => Color32::from_rgb(64, 128, 255),
        ConColor::White => Color32::WHITE,
        ConColor::Yellow => Color32::from_rgb(255, 210, 0),
        ConColor::Red => Color32::from_rgb(255, 50, 50),
    }
}

// --- coordinate transform ---

struct DrawCtx<'a> {
    painter: &'a Painter,
    rect: Rect,
    center: Pos2,
    /// World-space (map coords) point to center the view on.
    focus: (f32, f32),
    zoom: f32,
    pan: Vec2,
}

impl DrawCtx<'_> {
    /// World space (map coords) → egui screen space.
    /// Map Y increases north; screen Y increases down — so Y is flipped.
    fn to_screen(&self, wx: f32, wy: f32) -> Pos2 {
        Pos2::new(
            self.center.x + (wx - self.focus.0) * self.zoom + self.pan.x,
            self.center.y - (wy - self.focus.1) * self.zoom + self.pan.y,
        )
    }

    fn is_visible(&self, p: Pos2) -> bool {
        self.rect.expand(64.0).contains(p)
    }

    fn either_visible(&self, a: Pos2, b: Pos2) -> bool {
        let r = self.rect.expand(64.0);
        r.contains(a) || r.contains(b)
    }
}
