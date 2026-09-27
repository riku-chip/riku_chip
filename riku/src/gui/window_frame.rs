//! Marco propio de la ventana: minimizar, maximizar y cerrar bien visibles
//! (con fondo al pasar el puntero; cerrar en rojo, como en Windows),
//! arrastrar la ventana desde la barra superior (doble clic maximiza) y
//! cambiar el tamaño desde los bordes.
//!
//! El marco que dibuja Linux para una ventana de winit (WSLg, o el de
//! Wayland sin servidor de decoraciones) tiene botones tenues que no
//! responden al pasar el puntero. Con "Usar el marco del sistema" en
//! Ajustes vuelve el del sistema.

use eframe::egui::{self, pos2, vec2, Color32, CursorIcon, Rect, ResizeDirection, Sense, Stroke, ViewportCommand};

use crate::gui::tr;

/// Ancho de cada botón y alto de la barra superior.
pub(crate) const BUTTON_W: f32 = 46.0;
pub(crate) const BAR_H: f32 = 36.0;
/// Ancho de la franja de los bordes que cambia el tamaño.
const EDGE: f32 = 5.0;
/// En los bordes, cerca de una esquina, se cambian los dos lados.
const CORNER: f32 = 14.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Minimize,
    Maximize,
    Close,
}

fn maximized(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.viewport().maximized.unwrap_or(false) || i.viewport().fullscreen.unwrap_or(false))
}

/// Los tres botones, de derecha a izquierda (cerrar queda en la esquina).
/// Se llama dentro de un layout de derecha a izquierda.
pub(crate) fn controls(ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let max = maximized(&ctx);
    ui.spacing_mut().item_spacing.x = 0.0;
    if button(ui, Kind::Close, max).on_hover_text(tr!("window.close")).clicked() {
        ctx.send_viewport_cmd(ViewportCommand::Close);
    }
    let hint = if max { tr!("window.restore") } else { tr!("window.maximize") };
    if button(ui, Kind::Maximize, max).on_hover_text(hint).clicked() {
        ctx.send_viewport_cmd(ViewportCommand::Maximized(!max));
    }
    if button(ui, Kind::Minimize, max).on_hover_text(tr!("window.minimize")).clicked() {
        ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
    }
}

/// Un botón del alto de la barra, con el ícono dibujado a mano (nítido a
/// cualquier escala, sin depender de que la fuente tenga el glifo).
fn button(ui: &mut egui::Ui, kind: Kind, maximized: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(BUTTON_W, BAR_H), Sense::click());
    let v = ui.visuals();
    let hot = resp.hovered() || resp.has_focus();
    let down = resp.is_pointer_button_down_on();
    let (bg, fg) = match kind {
        Kind::Close if down => (Color32::from_rgb(0x94, 0x1f, 0x14), Color32::WHITE),
        Kind::Close if hot => (Color32::from_rgb(0xc4, 0x2b, 0x1c), Color32::WHITE),
        _ if down => (v.widgets.active.weak_bg_fill, v.strong_text_color()),
        _ if hot => (v.widgets.hovered.weak_bg_fill, v.strong_text_color()),
        _ => (Color32::TRANSPARENT, v.strong_text_color()),
    };
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, bg);
    let c = rect.center();
    let s = 5.5; // medio lado del ícono
    let stroke = Stroke::new(1.6_f32, fg);
    match kind {
        Kind::Minimize => {
            painter.line_segment([pos2(c.x - s, c.y), pos2(c.x + s, c.y)], stroke);
        }
        Kind::Maximize if maximized => {
            // Restaurar: dos cuadrados superpuestos.
            let back = Rect::from_min_size(pos2(c.x - s + 2.5, c.y - s - 0.5), vec2(2.0 * s - 2.5, 2.0 * s - 2.5));
            let front = Rect::from_min_size(pos2(c.x - s, c.y - s + 2.0), vec2(2.0 * s - 2.5, 2.0 * s - 2.5));
            painter.line_segment([back.left_top(), back.right_top()], stroke);
            painter.line_segment([back.right_top(), back.right_bottom()], stroke);
            painter.rect_filled(front, 0.0, bg);
            painter.rect_stroke(front, 1.0, stroke, egui::StrokeKind::Middle);
        }
        Kind::Maximize => {
            let r = Rect::from_center_size(c, vec2(2.0 * s, 2.0 * s));
            painter.rect_stroke(r, 1.0, stroke, egui::StrokeKind::Middle);
        }
        Kind::Close => {
            painter.line_segment([pos2(c.x - s, c.y - s), pos2(c.x + s, c.y + s)], stroke);
            painter.line_segment([pos2(c.x - s, c.y + s), pos2(c.x + s, c.y - s)], stroke);
        }
    }
    resp
}

/// La barra como asa de la ventana: arrastrar la mueve, doble clic
/// maximiza o restaura. Se registra antes que los botones de la barra para
/// que ellos tengan prioridad.
pub(crate) fn drag_area(ui: &mut egui::Ui, rect: Rect) {
    let resp = ui.interact(rect, egui::Id::new("window_drag"), Sense::click_and_drag());
    let ctx = ui.ctx();
    if resp.double_clicked() {
        ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized(ctx)));
    } else if resp.drag_started_by(egui::PointerButton::Primary) {
        ctx.send_viewport_cmd(ViewportCommand::StartDrag);
    }
}

/// Cambiar el tamaño desde los bordes (no con la ventana maximizada). Van
/// encima de todo para que el panel o el lienzo de abajo no tomen el clic.
pub(crate) fn resize_edges(ctx: &egui::Context) {
    if maximized(ctx) {
        return;
    }
    let screen = ctx.content_rect();
    let strips = [
        ("resize_n", Rect::from_min_max(screen.left_top(), pos2(screen.right(), screen.top() + EDGE))),
        ("resize_s", Rect::from_min_max(pos2(screen.left(), screen.bottom() - EDGE), screen.right_bottom())),
        ("resize_w", Rect::from_min_max(screen.left_top(), pos2(screen.left() + EDGE, screen.bottom()))),
        ("resize_e", Rect::from_min_max(pos2(screen.right() - EDGE, screen.top()), screen.right_bottom())),
    ];
    for (id, rect) in strips {
        egui::Area::new(egui::Id::new(id))
            .order(egui::Order::Foreground)
            .fixed_pos(rect.min)
            .interactable(true)
            .show(ctx, |ui| {
                let (_, resp) = ui.allocate_exact_size(rect.size(), Sense::drag());
                let Some(pos) = resp.hover_pos().or_else(|| resp.interact_pointer_pos()) else { return };
                let dir = direction(screen, pos);
                ui.ctx().set_cursor_icon(cursor(dir));
                if resp.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_pressed()) {
                    ui.ctx().send_viewport_cmd(ViewportCommand::BeginResize(dir));
                }
            });
    }
}

/// Qué lado (o esquina) se cambia desde `pos`, en el borde de `screen`.
fn direction(screen: Rect, pos: egui::Pos2) -> ResizeDirection {
    let near = |a: f32, b: f32, d: f32| (a - b).abs() <= d;
    let west = near(pos.x, screen.left(), CORNER);
    let east = near(pos.x, screen.right(), CORNER);
    let north = near(pos.y, screen.top(), CORNER);
    let south = near(pos.y, screen.bottom(), CORNER);
    // Qué borde se tocó de verdad (la franja fina); la esquina, con margen.
    let on_x = near(pos.x, screen.left(), EDGE) || near(pos.x, screen.right(), EDGE);
    let on_y = near(pos.y, screen.top(), EDGE) || near(pos.y, screen.bottom(), EDGE);
    match (north, south, west, east) {
        (true, _, true, _) if on_x || on_y => ResizeDirection::NorthWest,
        (true, _, _, true) if on_x || on_y => ResizeDirection::NorthEast,
        (_, true, true, _) if on_x || on_y => ResizeDirection::SouthWest,
        (_, true, _, true) if on_x || on_y => ResizeDirection::SouthEast,
        _ if on_y && pos.y < screen.center().y => ResizeDirection::North,
        _ if on_y => ResizeDirection::South,
        _ if pos.x < screen.center().x => ResizeDirection::West,
        _ => ResizeDirection::East,
    }
}

fn cursor(dir: ResizeDirection) -> CursorIcon {
    match dir {
        ResizeDirection::North => CursorIcon::ResizeNorth,
        ResizeDirection::South => CursorIcon::ResizeSouth,
        ResizeDirection::East => CursorIcon::ResizeEast,
        ResizeDirection::West => CursorIcon::ResizeWest,
        ResizeDirection::NorthEast => CursorIcon::ResizeNorthEast,
        ResizeDirection::SouthEast => CursorIcon::ResizeSouthEast,
        ResizeDirection::NorthWest => CursorIcon::ResizeNorthWest,
        ResizeDirection::SouthWest => CursorIcon::ResizeSouthWest,
    }
}

/// Un borde fino alrededor de la ventana sin marco (sin él se confunde con
/// lo que tiene detrás). No hace falta maximizada.
pub(crate) fn outline(ctx: &egui::Context) {
    if maximized(ctx) {
        return;
    }
    let layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("window_outline"));
    let color = ctx.global_style().visuals.widgets.noninteractive.bg_stroke.color;
    ctx.layer_painter(layer).rect_stroke(ctx.content_rect(), 0.0, Stroke::new(1.0_f32, color), egui::StrokeKind::Inside);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn esquinas_y_lados() {
        let s = Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 800.0));
        assert_eq!(direction(s, pos2(2.0, 2.0)), ResizeDirection::NorthWest);
        assert_eq!(direction(s, pos2(10.0, 2.0)), ResizeDirection::NorthWest, "cerca de la esquina, por arriba");
        assert_eq!(direction(s, pos2(998.0, 790.0)), ResizeDirection::SouthEast);
        assert_eq!(direction(s, pos2(500.0, 1.0)), ResizeDirection::North);
        assert_eq!(direction(s, pos2(500.0, 799.0)), ResizeDirection::South);
        assert_eq!(direction(s, pos2(1.0, 400.0)), ResizeDirection::West);
        assert_eq!(direction(s, pos2(999.0, 400.0)), ResizeDirection::East);
    }
}
