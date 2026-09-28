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
#[derive(Clone, Copy, Default)]
pub(crate) struct Readout {
    /// Posición del cursor en coordenadas de mundo, si está sobre el lienzo.
    pub cursor_world: Option<(f64, f64)>,
    /// Tamaño de un píxel en unidades de mundo.
    pub px_world: Option<f64>,
    /// Etiquetas omitidas por solaparse.
    pub labels_hidden: usize,
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
    let scroll = ctx.input(|i| i.smooth_scroll_delta.y as f64);
    let wheel = scroll.abs() > f64::EPSILON && response.hovered();
    if pressed || wheel {
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
    if wheel {
        // Zoom anclado al cursor (o al centro si no hay puntero).
        let anchor = response.hover_pos().unwrap_or(rect.center());
        zoom_at_screen(&mut bs.viewport, 1.0 + scroll * 0.002, anchor, rect);
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
    if opts.legend {
        crate::gui::legend::show(ui, response.rect, bs, &stats);
    } else {
        bs.layer_hover_legend = None;
    }
    let readout = Readout {
        cursor_world: response.hover_pos().map(|p| xf.to_world(p)),
        px_world: Some(1.0 / bs.viewport.scale),
        labels_hidden: stats.labels_hidden,
    };
    // Tooltip con capa/área del polígono bajo el cursor (no mientras se
    // arrastra: estorba al hacer pan).
    if let Some(pos) = response.hover_pos().filter(|_| !response.dragged()) {
        if let Some(info) = hover_info(bs.scene.as_ref(), &bs.viewport, response.rect, pos, &hidden) {
            response.on_hover_text_at_pointer(info);
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

/// Cursor sobre el lienzo: mano abierta (se puede mover) y cerrada al arrastrar.
fn cursor_icon(ctx: &egui::Context, response: &egui::Response) {
    if response.dragged() {
        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
    } else if response.hovered() {
        ctx.set_cursor_icon(egui::CursorIcon::Grab);
    }
}
