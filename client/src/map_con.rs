use egui::{Color32, FontId, Painter, Pos2, Rect, Sense, Stroke, Ui, Vec2};

use crate::data::AppData;
use crate::data::spawns::{con_color, ConColor, SpawnCategory, SpawnInfo};
use crate::map_reader::MapData;

const SPAWN_RADIUS: f32 = 4.0;
const SELF_RADIUS: f32 = 6.0;
const GROUND_RADIUS: f32 = 4.0;
const ZOOM_STEP: f32 = 1.12;
const ZOOM_MIN: f32 = 0.04;
const ZOOM_MAX: f32 = 20.0;

/// Persistent camera state for the map canvas.
pub struct MapState {
    pub zoom: f32,
    pub pan: Vec2,
}

impl Default for MapState {
    fn default() -> Self {
        Self { zoom: 1.0, pan: Vec2::ZERO }
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
    pub fn show(self, ui: &mut Ui, z_filter: Option<(f32, f32)>) {
        let size = ui.available_size();
        let (response, painter) = ui.allocate_painter(size, Sense::click_and_drag());

        // Drag to pan
        if response.dragged() {
            self.state.pan += response.drag_delta();
        }

        // Scroll to zoom
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            let factor = if scroll > 0.0 { ZOOM_STEP } else { 1.0 / ZOOM_STEP };
            self.state.zoom = (self.state.zoom * factor).clamp(ZOOM_MIN, ZOOM_MAX);
        }

        painter.rect_filled(response.rect, 0.0, Color32::BLACK);

        let focus = self.focus_world();
        let ctx = DrawCtx {
            painter: &painter,
            rect: response.rect,
            center: response.rect.center(),
            focus,
            zoom: self.state.zoom,
            pan: self.state.pan,
        };

        draw_map_lines(&ctx, &self.data.map);
        draw_labels(&ctx, &self.data.map);
        draw_mob_trails(&ctx, self.data);
        draw_ground_items(&ctx, self.data, z_filter);
        draw_spawns(&ctx, self.data, z_filter);
        draw_self(&ctx, self.data);
        draw_annotations(&ctx, self.data);
        draw_hud(&ctx, ui, self.data);
    }

    /// World-space focus point (map coords) — the player's position, or origin.
    fn focus_world(&self) -> (f32, f32) {
        self.data.self_id
            .and_then(|id| self.data.spawns.get(id))
            .map(|s| eq_to_map(s.x, s.y))
            .unwrap_or((0.0, 0.0))
    }
}

/// Transform EQ spawn coordinates to map coordinate space (negate X and Y).
#[inline]
fn eq_to_map(eq_x: f32, eq_y: f32) -> (f32, f32) {
    (-eq_x, -eq_y)
}

fn draw_mob_trails(ctx: &DrawCtx, data: &AppData) {
    if !data.trails_enabled {
        return;
    }
    for trail in data.trails.values() {
        for &(mx, my) in trail {
            let pos = ctx.to_screen(mx, my);
            if ctx.is_visible(pos) {
                ctx.painter
                    .circle_filled(pos, 2.0, Color32::from_rgba_premultiplied(160, 100, 40, 140));
            }
        }
    }
}

fn draw_map_lines(ctx: &DrawCtx, map: &MapData) {
    for line in &map.lines {
        let p1 = ctx.to_screen(line.p1.x, line.p1.y);
        let p2 = ctx.to_screen(line.p2.x, line.p2.y);
        if ctx.either_visible(p1, p2) {
            let [r, g, b] = line.color;
            ctx.painter.line_segment(
                [p1, p2],
                Stroke::new(1.0, Color32::from_rgb(r, g, b)),
            );
        }
    }
}

fn draw_labels(ctx: &DrawCtx, map: &MapData) {
    for label in &map.labels {
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
            Color32::from_rgb(r, g, b),
        );
    }
}

fn draw_spawns(ctx: &DrawCtx, data: &AppData, z_filter: Option<(f32, f32)>) {
    let player_level = data.self_level();
    for spawn in data.spawns.iter() {
        if Some(spawn.id) == data.self_id {
            continue;
        }
        if z_filtered(spawn.z, z_filter) {
            continue;
        }
        let (mx, my) = eq_to_map(spawn.x, spawn.y);
        let pos = ctx.to_screen(mx, my);
        if !ctx.is_visible(pos) {
            continue;
        }
        let color = spawn_color(spawn, player_level);
        ctx.painter.circle_filled(pos, SPAWN_RADIUS, color);
    }
}

fn draw_self(ctx: &DrawCtx, data: &AppData) {
    let Some(id) = data.self_id else { return };
    let Some(s) = data.spawns.get(id) else { return };
    let (mx, my) = eq_to_map(s.x, s.y);
    let pos = ctx.to_screen(mx, my);

    ctx.painter.circle_filled(pos, SELF_RADIUS, Color32::WHITE);
    ctx.painter.circle_stroke(pos, SELF_RADIUS, Stroke::new(1.5, Color32::BLACK));

    // Direction arrow — EQ heading: 0 = north, 512 = full circle, increases clockwise.
    // North on screen = -Y, east = +X.
    let heading_rad = s.heading * std::f32::consts::TAU / 512.0;
    let arrow_len = SELF_RADIUS * 2.5;
    let tip = Pos2::new(
        pos.x + heading_rad.sin() * arrow_len,
        pos.y - heading_rad.cos() * arrow_len,
    );
    ctx.painter.line_segment([pos, tip], Stroke::new(2.0, Color32::WHITE));
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
            font,
            Color32::from_rgb(200, 200, 150),
        );
    }
}

/// Returns true if `z` is outside the filter range and should be hidden.
#[inline]
fn z_filtered(z: f32, filter: Option<(f32, f32)>) -> bool {
    match filter {
        Some((center, range)) => (z - center).abs() > range,
        None => false,
    }
}

/// Spawn dot color: filter flags take priority over category/con color.
fn spawn_color(spawn: &SpawnInfo, player_level: u8) -> Color32 {
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
    match spawn.spawn_category {
        SpawnCategory::Pc => Color32::from_rgb(0, 200, 255),
        SpawnCategory::Corpse => Color32::from_rgb(80, 40, 40),
        SpawnCategory::Pet | SpawnCategory::Merc => Color32::from_rgb(160, 160, 160),
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