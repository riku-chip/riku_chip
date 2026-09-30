//! El lienzo de una escena: mover, zoom (rueda, teclado), encuadre,
//! animaciones e inercia, y el pintado. Devuelve lo que la barra de estado
//! muestra en el próximo cuadro.

use eframe::egui;
use viewer_core::viewport::Viewport;

use crate::gui::content::SceneState;
use crate::gui::motion::{Inertia, ViewAnimation};
use crate::gui::scene_painter::{fit_bbox, fit_scene, focus_area, hover_info, paint_scene, zoom_at_screen, PaintOptions, ScreenXform};
use crate::gui::theme::CanvasTheme;

/// Preferencias que afectan al lienzo.
#[derive(Clone, Copy)]
pub(crate) struct CanvasOptions {
    /// Sin animaciones ni inercia (movimiento reducido).
    pub reduce_motion: bool,
    /// Dos dedos en el touchpad (o la rueda) mueven la vista en vez de
    /// hacer zoom; el zoom queda en pellizcar o Ctrl + rueda.
    pub scroll_pans: bool,
    pub labels: bool,
    /// Leyenda de las capas a la vista.
    pub legend: bool,
    /// Resumir en bloques lo menor a un píxel.
    pub simplify: bool,
    pub block_px: f64,
    /// `RIKU_PROFILE`: imprimir tiempos de pintado por cuadro.
    pub profile: bool,
}

/// Lecturas del lienzo para la barra de estado.
#[derive(Clone, Default)]
pub(crate) struct Readout {
    /// Posición del cursor en coordenadas de mundo, si está sobre el lienzo.
    pub cursor_world: Option<(f64, f64)>,
    /// Tamaño de un píxel en unidades de mundo.
    pub px_world: Option<f64>,
    /// Etiquetas omitidas por solaparse.
    pub labels_hidden: usize,
    /// Doble clic en una instancia: la entrada (sub-celda o sub-esquemático)
    /// a abrir.
    pub enter: Option<String>,
}

/// Dibuja la escena en todo el espacio disponible y atiende los gestos.
pub(crate) fn show(ui: &mut egui::Ui, bs: &mut SceneState, opts: CanvasOptions) -> Readout {
    let ctx = ui.ctx().clone();
    let available = ui.available_size_before_wrap();
    let response = ui.allocate_response(available, egui::Sense::click_and_drag());
    cursor_icon(&ctx, &response);
    let rect = response.rect;
    let (w, h) = (rect.width() as f64, rect.height() as f64);
    let animate = !opts.reduce_motion;

    // El usuario manda: tocar el lienzo o usar la rueda frena cualquier
    // animación o inercia en el valor que tenga en pantalla (interrumpible,
    // sin saltos).
    let pressed = response.hovered() && ctx.input(|i| i.pointer.any_pressed());
    // Rueda o dos dedos (x: a los lados, y: arriba/abajo) y pellizco o
    // Ctrl + rueda (egui los junta en `zoom_delta`, sin desplazamiento).
    let (scroll, pinch) = ctx.input(|i| (i.smooth_scroll_delta, i.zoom_delta() as f64));
    let (sx, sy) = (scroll.x as f64, scroll.y as f64);
    let scrolled = response.hovered() && (sx.abs() > f64::EPSILON || sy.abs() > f64::EPSILON);
    let pinched = response.hovered() && (pinch - 1.0).abs() > 1e-6;
    if pressed || scrolled || pinched {
        bs.anim = None;
        bs.inertia = None;
    }
    if response.dragged() {
        let delta = response.drag_delta();
        bs.viewport.pan_by_screen(delta.x as f64, delta.y as f64);
        bs.fitted_size = None;
        ctx.request_repaint();
    }
    // Al soltar, la vista sigue a la velocidad del puntero y desacelera
    // (proyección de momento).
    if response.drag_stopped() && animate {
        let v = ctx.input(|i| i.pointer.velocity());
        bs.inertia = Inertia::from_release(v.x as f64, v.y as f64);
    }
    // Zoom anclado al cursor (o al centro si no hay puntero).
    let anchor = response.hover_pos().unwrap_or(rect.center());
    if pinched {
        zoom_at_screen(&mut bs.viewport, pinch, anchor, rect);
    }
    if scrolled {
        match scroll_action(sx, sy, opts.scroll_pans) {
            ScrollAction::Pan(dx, dy) => bs.viewport.pan_by_screen(dx, dy),
            ScrollAction::ZoomAndPan { zoom, dx } => {
                zoom_at_screen(&mut bs.viewport, zoom, anchor, rect);
                bs.viewport.pan_by_screen(dx, 0.0);
            }
        }
    }
    if scrolled || pinched {
        bs.fitted_size = None;
        ctx.request_repaint();
    }

    // Cambio de vista hacia `target`: animado si lo pidió el usuario (y no
    // hay movimiento reducido), inmediato si no.
    let go_to = |bs: &mut SceneState, target: Viewport, user: bool| {
        if user && animate {
            bs.anim = Some(ViewAnimation::to(target));
        } else {
            bs.viewport = target;
        }
        bs.inertia = None;
    };

    // Encuadre al cargar / "Encuadrar", y de nuevo si el lienzo cambia de
    // tamaño (paneles, ventana) mientras el usuario no haya movido la vista.
    let size = rect.size();
    let resized = bs.anim.is_none() && bs.fitted_size.is_some_and(|s| s != size);
    if (bs.needs_fit || resized) && size.x > 0.0 && size.y > 0.0 {
        let mut target = bs.viewport;
        fit_scene(&mut target, bs.scene.as_ref(), rect);
        let user = std::mem::take(&mut bs.animate_fit);
        go_to(bs, target, user);
        bs.needs_fit = false;
        bs.fitted_size = Some(size);
    }
    // Clic en un cambio: encuadrarlo con contexto alrededor. Es una vista
    // elegida, no se re-encuadra sola al redimensionar.
    if let Some(area) = bs.focus.take() {
        let mut target = bs.viewport;
        fit_bbox(&mut target, &focus_area(&area, &bs.scene.bbox()), bs.scene.y_axis(), rect);
        go_to(bs, target, true);
        bs.fitted_size = None;
    }
    // Zoom por teclado (+/−) sobre el centro del lienzo.
    if let Some(factor) = bs.pending_zoom.take() {
        let mut target = bs.anim.map_or(bs.viewport, |a| a.target());
        zoom_at_screen(&mut target, factor, rect.center(), rect);
        go_to(bs, target, true);
        bs.fitted_size = None;
    }

    // Avanzar animación e inercia (tiempo real, no por frame).
    let dt = ctx.input(|i| i.stable_dt).clamp(0.001, 0.05) as f64;
    if let Some(mut anim) = bs.anim.take() {
        if !anim.step(&mut bs.viewport, dt, w, h) {
            bs.anim = Some(anim);
        }
        ctx.request_repaint();
    }
    if let Some(mut it) = bs.inertia.take() {
        let ((dx, dy), alive) = it.step(dt);
        bs.viewport.pan_by_screen(dx, dy);
        bs.fitted_size = None;
        if alive {
            bs.inertia = Some(it);
        }
        ctx.request_repaint();
    }
    let hidden = bs.hidden_keys();
    let paint = PaintOptions {
        theme: CanvasTheme::from_visuals(ui.visuals()),
        labels: opts.labels,
        lod: opts.simplify,
        block_px: opts.block_px,
        focus: bs.focus_key(),
    };
    let t_paint = std::time::Instant::now();
    let stats = ui
        .scope_builder(egui::UiBuilder::new().max_rect(response.rect), |ui| {
            paint_scene(ui, bs.scene.as_ref(), &bs.viewport, &hidden, paint)
        })
        .inner;
    if opts.profile {
        eprintln!(
            "PROFILE paint_scene {:.1} ms · entre cuadros {:.1} ms · {} elementos · nivel {:?}",
            t_paint.elapsed().as_secs_f64() * 1e3,
            ctx.input(|i| i.unstable_dt) as f64 * 1e3,
            stats.elements,
            stats.lod_level
        );
    }
    let xf = ScreenXform::new(response.rect, &bs.viewport, bs.scene.y_axis());
    // Clic en un polígono: resaltar su red (otro clic en la misma, o en el
    // vacío, la suelta).
    if response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            let (x, y) = xf.to_world(pos);
            let hit = crate::gui::scene_painter::pick_at(bs.scene.as_ref(), (x, y), &hidden).and_then(|el| bs.scene.net_at(x, y, Some(el.layer())));
            bs.net_focus = match hit {
                Some(h) if bs.net_focus.as_ref().is_some_and(|f| f.name == h.name) => None,
                other => other,
            };
        }
    }
    if let Some(net) = &bs.net_focus {
        paint_net(&ui.painter_at(response.rect), &xf, net, ui.visuals());
    }
    if let Some(mark) = &bs.mark {
        paint_mark(&ui.painter_at(response.rect), &xf, mark, ui.visuals());
    }
    // Doble clic en una instancia de una sub-celda o de un sub-esquemático:
    // entrar (la más interna, si se anidan).
    let scene = bs.scene.clone();
    let link_under = |pos: egui::Pos2| {
        let (x, y) = xf.to_world(pos);
        viewer_core::link_at(scene.links(), x, y)
    };
    let enter = response
        .double_clicked()
        .then(|| response.interact_pointer_pos())
        .flatten()
        .and_then(|pos| link_under(pos).map(|l| l.entry.clone()));
    if opts.legend {
        crate::gui::legend::show(ui, response.rect, bs, &stats);
    } else {
        bs.layer_hover_legend = None;
    }
    let readout = Readout {
        cursor_world: response.hover_pos().map(|p| xf.to_world(p)),
        px_world: Some(1.0 / bs.viewport.scale),
        labels_hidden: stats.labels_hidden,
        enter,
    };
    // Tooltip con capa/área del polígono bajo el cursor (no mientras se
    // arrastra: estorba al hacer pan).
    if let Some(pos) = response.hover_pos().filter(|_| !response.dragged()) {
        let info = hover_info(bs.scene.as_ref(), &bs.viewport, response.rect, pos, &hidden);
        let hint = link_under(pos).map(|l| crate::gui::tr!("canvas.enter", what = l.label));
        let text: Vec<String> = info.into_iter().chain(hint).collect();
        if !text.is_empty() {
            response.on_hover_text_at_pointer(text.join("\n"));
        }
    }
    readout
}

/// La red resaltada: el resto atenuado y sus polígonos en amarillo encima.
fn paint_net(painter: &egui::Painter, xf: &ScreenXform, net: &viewer_core::NetHit, visuals: &egui::Visuals) {
    painter.rect_filled(painter.clip_rect(), 0.0, visuals.extreme_bg_color.gamma_multiply(0.65));
    let yellow = egui::Color32::from_rgb(255, 205, 40);
    let stroke = egui::Stroke::new(1.5, yellow);
    for poly in &net.outline {
        if poly.len() < 3 {
            continue;
        }
        let screen: Vec<egui::Pos2> = poly.iter().map(|&(x, y)| xf.to_screen(x, y)).collect();
        crate::gui::polygon_fill::paint_filled_polygon(painter, poly, screen, yellow.gamma_multiply(0.45), stroke);
    }
}

/// Lo resaltado de un LVS: el resto atenuado, las redes rellenas y los
/// dispositivos recuadrados, en el mismo amarillo que una red.
fn paint_mark(painter: &egui::Painter, xf: &ScreenXform, mark: &crate::gui::content::Mark, visuals: &egui::Visuals) {
    painter.rect_filled(painter.clip_rect(), 0.0, visuals.extreme_bg_color.gamma_multiply(0.65));
    let yellow = egui::Color32::from_rgb(255, 205, 40);
    let stroke = egui::Stroke::new(2.0, yellow);
    for poly in mark.fills.iter().filter(|p| p.len() >= 3) {
        let screen: Vec<egui::Pos2> = poly.iter().map(|&(x, y)| xf.to_screen(x, y)).collect();
        crate::gui::polygon_fill::paint_filled_polygon(painter, poly, screen, yellow.gamma_multiply(0.6), egui::Stroke::new(1.0, yellow));
    }
    for b in &mark.boxes {
        let rect = egui::Rect::from_two_pos(xf.to_screen(b.min_x, b.min_y), xf.to_screen(b.max_x, b.max_y)).expand(3.0);
        painter.rect_filled(rect, 2.0, yellow.gamma_multiply(0.12));
        painter.rect_stroke(rect, 2.0, stroke, egui::StrokeKind::Outside);
    }
}

/// Cursor sobre el lienzo: mano abierta (se puede mover) y cerrada al arrastrar.
fn cursor_icon(ctx: &egui::Context, response: &egui::Response) {
    if response.dragged() {
        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
    } else if response.hovered() {
        ctx.set_cursor_icon(egui::CursorIcon::Grab);
    }
}

/// Qué hace un desplazamiento de la rueda o de dos dedos en el touchpad.
#[derive(Debug, PartialEq)]
enum ScrollAction {
    /// Mover la vista (como arrastrar el contenido).
    Pan(f64, f64),
    /// Zoom con lo vertical (como la rueda del mouse) y mover con lo
    /// horizontal (dos dedos a los lados, o Shift + rueda).
    ZoomAndPan { zoom: f64, dx: f64 },
}

/// `scroll_pans`: el ajuste "Dos dedos / rueda: mover". Por defecto la
/// rueda hace zoom; en WSLg el touchpad llega igual que la rueda, así que
/// no se puede decidir solo por el tipo de evento.
fn scroll_action(dx: f64, dy: f64, scroll_pans: bool) -> ScrollAction {
    if scroll_pans {
        ScrollAction::Pan(dx, dy)
    } else {
        ScrollAction::ZoomAndPan { zoom: 1.0 + dy * 0.002, dx }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_rueda_hace_zoom_y_lo_horizontal_mueve() {
        let ScrollAction::ZoomAndPan { zoom, dx } = scroll_action(0.0, 50.0, false) else { panic!("zoom") };
        assert!((zoom - 1.1).abs() < 1e-9 && dx == 0.0, "{zoom} {dx}");
        assert_eq!(scroll_action(30.0, 0.0, false), ScrollAction::ZoomAndPan { zoom: 1.0, dx: 30.0 });
    }

    #[test]
    fn en_modo_mover_los_dos_ejes_mueven() {
        assert_eq!(scroll_action(30.0, -20.0, true), ScrollAction::Pan(30.0, -20.0));
    }
}
