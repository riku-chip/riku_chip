//! Painter genérico para cualquier `RenderableScene` de viewer-core.
//!
//! No sabe de qué backend viene la escena — dibuja los 5 primitivos neutros
//! (Line/Rect/Circle/Polygon/Text). Las features ricas de cada formato
//! (fantasmas, anotaciones, wires/junctions de Xschem) se quedan en su
//! painter específico (`sch_painter.rs`).
//!
//! Sistemas de coordenadas:
//! - **mundo**: el de la escena; Y-up o Y-down según `scene.y_axis()`.
//! - **vista**: Y-down, relativo a la esquina del panel. Es donde vive el
//!   `Viewport` (pan/zoom), de modo que `fit_to(w, h)` centra en el panel.
//! - **pantalla**: vista + `rect.min` del panel dentro de la ventana.
//!
//! `ScreenXform` concentra las tres conversiones para que dibujo, culling,
//! auto-fit y zoom usen exactamente la misma transformación.

use std::collections::HashSet;

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Shape, Stroke, StrokeKind};
use viewer_core::{
    bbox::BoundingBox,
    diff::{Annotation, AnnotationShape, ChangeKind},
    element::{DrawElement, HAlign, Layer, VAlign},
    fill::strip_closing_point,
    index::{Fill, LodQuery, SceneIndex},
    paint::Rgba,
    scene::{RenderableScene, TextStyle},
    viewport::{screen_to_world, world_to_screen, Viewport, YAxis},
};

use crate::gui::label_layout::{place, LabelCandidate, PILL_PADDING};
use crate::gui::polygon_fill::paint_filled_polygon;
use crate::gui::theme::CanvasTheme;

/// Paleta neutral mínima por layer. Se usa cuando la escena no provee su
/// propio `LayerPaint` — suficiente para inspección genérica.
fn neutral_layer_color(layer: Layer) -> Color32 {
    match layer {
        1 => Color32::from_rgb(180, 180, 200),    // wire/primary
        2 => Color32::from_rgb(120, 120, 140),    // grid
        3 => Color32::from_rgb(220, 220, 160),    // text
        4 => Color32::from_rgb(120, 200, 255),    // pin
        5 => Color32::from_rgb(180, 200, 255),    // label
        6 => Color32::from_rgb(220, 180, 120),    // component
        _ => {
            // Hash simple para layers desconocidos (GDS puede tener muchos).
            let r = ((layer.wrapping_mul(131)) & 0xFF) as u8;
            let g = ((layer.wrapping_mul(197)) & 0xFF) as u8;
            let b = ((layer.wrapping_mul(263)) & 0xFF) as u8;
            Color32::from_rgb(r.saturating_add(80), g.saturating_add(80), b.saturating_add(80))
        }
    }
}

pub fn to_color32(c: Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
}

/// Colores de una capa: `(relleno, contorno)`. Usa el `LayerPaint` de la
/// escena si existe; si no, la paleta neutral opaca.
fn layer_colors(scene: &dyn RenderableScene, layer: Layer) -> (Color32, Color32) {
    match scene.layer_paint(layer) {
        Some(p) => (to_color32(p.fill), to_color32(p.stroke)),
        None => {
            let c = neutral_layer_color(layer);
            (c, c)
        }
    }
}

/// Transformación mundo ↔ pantalla de una escena pintada dentro de `rect`.
#[derive(Clone, Copy)]
pub struct ScreenXform {
    rect: Rect,
    vp: Viewport,
    y_axis: YAxis,
}

impl ScreenXform {
    pub fn new(rect: Rect, vp: &Viewport, y_axis: YAxis) -> Self {
        Self { rect, vp: *vp, y_axis }
    }

    pub fn to_screen(&self, x: f64, y: f64) -> Pos2 {
        let (vx, vy) = world_to_screen(&self.vp, x, self.y_axis.flip_y(y));
        Pos2::new(self.rect.min.x + vx as f32, self.rect.min.y + vy as f32)
    }

    /// Pantalla → mundo (inverso de `to_screen`).
    pub fn to_world(&self, pos: Pos2) -> (f64, f64) {
        let (x, vy) = screen_to_world(
            &self.vp,
            (pos.x - self.rect.min.x) as f64,
            (pos.y - self.rect.min.y) as f64,
        );
        (x, self.y_axis.flip_y(vy))
    }

    /// Región del mundo visible en `rect`, usada como bbox de culling.
    pub fn visible_world_bbox(&self) -> BoundingBox {
        let (x1, y1) = screen_to_world(&self.vp, 0.0, 0.0);
        let (x2, y2) = screen_to_world(&self.vp, self.rect.width() as f64, self.rect.height() as f64);
        self.y_axis.flip_bbox(&BoundingBox::from_points((x1, y1), (x2, y2)))
    }
}

/// Ajusta `vp` para que toda la escena quepa centrada en `rect`.
pub fn fit_scene(vp: &mut Viewport, scene: &dyn RenderableScene, rect: Rect) {
    fit_bbox(vp, &scene.bbox(), scene.y_axis(), rect);
}

/// Ajusta `vp` para que `world_bbox` (coordenadas de mundo) quepa centrada en `rect`.
pub fn fit_bbox(vp: &mut Viewport, world_bbox: &BoundingBox, y_axis: YAxis, rect: Rect) {
    let view_bbox = y_axis.flip_bbox(world_bbox);
    vp.fit_to(&view_bbox, rect.width() as f64, rect.height() as f64);
}

/// Elemento con área bajo el punto de mundo `(x, y)`: el de más arriba (el
/// último que se pinta) entre los que tienen relleno visible. Las capas de
/// solo contorno (boundary, implantes) cubren celdas enteras y taparían todo:
/// solo se eligen si no hay nada relleno debajo. Ignora capas ocultas,
/// líneas y textos.
pub fn pick_at<'a>(
    scene: &'a dyn RenderableScene,
    (x, y): (f64, f64),
    hidden: &HashSet<Layer>,
) -> Option<&'a DrawElement> {
    let (mut filled, mut outline) = (None, None);
    let mut consider = |el: &'a DrawElement| {
        if !hidden.contains(&el.layer()) && el.contains_point(x, y) {
            let see_through = scene.layer_paint(el.layer()).is_some_and(|p| p.fill.a == 0);
            if see_through { outline = Some(el) } else { filled = Some(el) }
        }
    };
    match scene.indexed() {
        // Con índice: solo los elementos cuyo bbox contiene el punto.
        Some((index, elements)) => {
            for i in index.candidates_at(x, y) {
                consider(&elements[i as usize]);
            }
        }
        // bbox puntual: el culling de la escena descarta casi todo sin probarlo.
        None => scene.visit(&BoundingBox::point(x, y), &mut |el| {
            consider(el);
            true
        }),
    }
    filled.or(outline)
}

/// Texto del tooltip para el elemento bajo `pos` (pantalla), o `None` si no
/// hay ninguno: capa, área y tamaño, con la unidad de la escena.
pub fn hover_info(
    scene: &dyn RenderableScene,
    vp: &Viewport,
    rect: Rect,
    pos: Pos2,
    hidden: &HashSet<Layer>,
) -> Option<String> {
    let xf = ScreenXform::new(rect, vp, scene.y_axis());
    let el = pick_at(scene, xf.to_world(pos), hidden)?;
    let layer = match scene.layer_paint(el.layer()) {
        Some(p) => p.name.clone(),
        None => format!("capa {}", el.layer()),
    };
    let unit = scene.world_unit().map(|u| format!(" {u}")).unwrap_or_default();
    let b = el.bounding_box();
    let mut text = format!("{layer}\n{:.3} × {:.3}{unit}", b.width(), b.height());
    if let Some(a) = el.area() {
        let sq = scene.world_unit().map(|u| format!(" {u}²")).unwrap_or_default();
        text.push_str(&format!("\nárea {a:.4}{sq}"));
    }
    Some(text)
}

/// Zona a encuadrar para mostrar `target` con contexto: 25 % de margen y,
/// como mínimo, el 15 % del lado mayor de la escena (un contacto de 0.17 µm
/// no debe llenar la pantalla sin referencia). Un margen mayor aleja de más
/// los cambios grandes (p.ej. uno repartido en varias instancias).
pub fn focus_area(target: &BoundingBox, scene: &BoundingBox) -> BoundingBox {
    let min_side = scene.width().max(scene.height()) * 0.15;
    let w = (target.width() * 1.25).max(min_side);
    let h = (target.height() * 1.25).max(min_side);
    let (cx, cy) = target.center();
    BoundingBox::from_points((cx - w * 0.5, cy - h * 0.5), (cx + w * 0.5, cy + h * 0.5))
}

/// Zoom que mantiene fijo el punto de mundo bajo `pos` (pantalla absoluta).
pub fn zoom_at_screen(vp: &mut Viewport, factor: f64, pos: Pos2, rect: Rect) {
    vp.zoom_at(factor, (pos.x - rect.min.x) as f64, (pos.y - rect.min.y) as f64);
}

/// Opciones de pintado que dependen de la UI, no de la escena.
#[derive(Clone, Copy)]
pub struct PaintOptions {
    pub theme: CanvasTheme,
    /// Mostrar etiquetas de texto.
    pub labels: bool,
    /// Resumir en bloques lo menor a un píxel (escenas con índice).
    pub lod: bool,
    /// Lado máximo de un bloque, en píxeles.
    pub block_px: f64,
}

/// Qué pasó al pintar, para informar en la barra de estado.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PaintStats {
    /// Etiquetas visibles que no se dibujaron por falta de lugar.
    pub labels_hidden: usize,
    /// Elementos dibujados uno a uno y nivel de la pirámide usado (si hubo).
    pub elements: usize,
    pub lod_level: Option<usize>,
}

/// Alto de las etiquetas en pantalla. Se deriva del tamaño en el mundo pero
/// se acota: con zoom grande no deben tapar el layout y con zoom chico deben
/// seguir leyéndose.
const LABEL_PX: std::ops::RangeInclusive<f32> = 10.0..=14.0;
/// Si el tamaño "natural" (mundo × escala) cae por debajo de esto, el zoom es
/// tan lejano que las etiquetas serían solo ruido: no se dibujan.
const LABEL_MIN_NATURAL_PX: f32 = 3.0;

/// Pinta `scene` en todo el espacio disponible. Las capas en `hidden` se omiten.
pub fn paint_scene(
    ui: &mut egui::Ui,
    scene: &dyn RenderableScene,
    vp: &Viewport,
    hidden: &HashSet<Layer>,
    opts: PaintOptions,
) -> PaintStats {
    let available = ui.available_size_before_wrap();
    let (response, painter) = ui.allocate_painter(available, egui::Sense::hover());
    let rect = response.rect;
    let painter = painter.with_clip_rect(rect);
    let theme = opts.theme;

    painter.rect_filled(rect, 0.0, theme.background);

    if scene.is_empty() {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "Escena vacía.",
            FontId::proportional(16.0),
            theme.muted,
        );
        return PaintStats::default();
    }

    // Fantasmas de la versión anterior (diff) debajo de todo; geometría;
    // marcas del diff; y las etiquetas al final, por encima de todo y sin
    // pisarse entre sí.
    let xf = ScreenXform::new(rect, vp, scene.y_axis());
    let drawn_text = scene.text_style() == TextStyle::Drawn;
    let ghost = ghost_color(&theme);
    for el in scene.ghost() {
        draw_tinted(&painter, &xf, vp.scale, el, ghost, drawn_text);
    }
    let mut labels: Vec<(LabelCandidate, f32)> = Vec::new();
    let mut visitor = |el: &DrawElement| -> bool {
        if hidden.contains(&el.layer()) {
            return true;
        }
        match el {
            DrawElement::Text { .. } if drawn_text => {
                let (_, stroke) = theme.layer_colors(Color32::TRANSPARENT, layer_colors(scene, el.layer()).1);
                draw_text(&painter, &xf, vp.scale, el, stroke);
            }
            DrawElement::Text { x, y, content, size, .. } => {
                let natural = (*size * vp.scale) as f32;
                if opts.labels && natural >= LABEL_MIN_NATURAL_PX {
                    let (_, stroke) = layer_colors(scene, el.layer());
                    let px = natural.clamp(*LABEL_PX.start(), *LABEL_PX.end());
                    labels.push((
                        LabelCandidate { anchor: xf.to_screen(*x, *y), text: content.clone(), color: stroke },
                        px,
                    ));
                }
            }
            _ => draw_element(&painter, &xf, vp.scale, scene, el, &theme),
        }
        true
    };
    let (mut drawn, mut lod_level) = (0, None);
    match scene.indexed() {
        Some((index, elements)) => {
            let q = LodQuery {
                bbox: xf.visible_world_bbox(),
                px_world: 1.0 / vp.scale.max(f64::MIN_POSITIVE),
                lod: opts.lod,
                block_px: opts.block_px,
                budget: viewer_core::index::BUDGET,
                // Etiquetas ilegibles u ocultas: ni se entregan. El texto de un
                // esquemático se dibuja siempre (con un mínimo de tamaño).
                min_text_px: match (drawn_text, opts.labels) {
                    (true, _) => 0.0,
                    (false, true) => f64::from(LABEL_MIN_NATURAL_PX),
                    (false, false) => f64::INFINITY,
                },
            };
            let vis = index.visible(elements, &q, &|l| hidden.contains(&l));
            (drawn, lod_level) = (vis.elements.len(), vis.level);
            if let Some(level) = vis.level {
                paint_coverage(&painter, &xf, scene, index, level, vis.span, hidden, &theme);
            }
            let mut batch = LayerBatch::default();
            for &i in &vis.elements {
                let el = &elements[i as usize];
                if let DrawElement::Polygon { points, layer, filled: true } = el {
                    batch.polygon(&painter, &xf, vp.scale, scene, &theme, index, i as usize, points, *layer);
                    continue;
                }
                batch.flush(&painter);
                visitor(el);
            }
            batch.flush(&painter);
        }
        None => scene.visit(&xf.visible_world_bbox(), &mut |el| {
            drawn += 1;
            visitor(el)
        }),
    }
    paint_annotations(&painter, &xf, scene.annotations(), &theme);

    PaintStats { labels_hidden: paint_labels(&painter, labels, rect, &theme), elements: drawn, lod_level }
}

/// Un polígono de menos de estos píxeles se dibuja sin contorno: con el
/// relleno alcanza para verlo, y el contorno es lo que más cuesta teselar.
const OUTLINE_MIN_PX: f64 = 6.0;

/// Rellenos consecutivos de una misma capa acumulados en una sola malla (y
/// sus contornos aparte, encima): egui recibe una o dos formas por tramo de
/// capa en vez de una por polígono.
#[derive(Default)]
struct LayerBatch {
    layer: Option<Layer>,
    mesh: egui::Mesh,
    lines: Vec<Shape>,
}

impl LayerBatch {
    #[allow(clippy::too_many_arguments)]
    fn polygon(
        &mut self,
        painter: &egui::Painter,
        xf: &ScreenXform,
        scale: f64,
        scene: &dyn RenderableScene,
        theme: &CanvasTheme,
        index: &SceneIndex,
        i: usize,
        points: &[(f64, f64)],
        layer: Layer,
    ) {
        if self.layer != Some(layer) {
            self.flush(painter);
            self.layer = Some(layer);
        }
        let (fill, stroke_color) = layer_colors(scene, layer);
        let (fill, stroke_color) = theme.layer_colors(fill, stroke_color);
        let stroke = Stroke::new(1.0_f32, stroke_color);
        let pts: Vec<Pos2> = strip_closing_point(points).iter().map(|(x, y)| xf.to_screen(*x, *y)).collect();
        if pts.len() < 2 {
            return;
        }
        // Por el lado menor: un cable largo de menos de 6 px de ancho no lo lleva.
        let outline = fill.a() == 0 || index.min_size(i) * scale >= OUTLINE_MIN_PX;
        if fill.a() > 0 {
            let base = self.mesh.vertices.len() as u32;
            match index.fill(i) {
                (Fill::Convex, _) => {
                    for p in &pts {
                        self.mesh.colored_vertex(*p, fill);
                    }
                    for k in 1..pts.len() as u32 - 1 {
                        self.mesh.add_triangle(base, base + k, base + k + 1);
                    }
                }
                (Fill::Triangles, Some(tris)) => {
                    for p in &pts {
                        self.mesh.colored_vertex(*p, fill);
                    }
                    for t in tris.chunks_exact(3) {
                        self.mesh.add_triangle(base + t[0], base + t[1], base + t[2]);
                    }
                }
                _ => {}
            }
        }
        if outline {
            self.lines.push(Shape::closed_line(pts, stroke));
        }
    }

    fn flush(&mut self, painter: &egui::Painter) {
        if !self.mesh.is_empty() {
            painter.add(Shape::mesh(std::mem::take(&mut self.mesh)));
        }
        if !self.lines.is_empty() {
            painter.extend(std::mem::take(&mut self.lines));
        }
        self.layer = None;
    }
}

/// Texturas de la pirámide ya compuestas, por escena, nivel, capas ocultas y
/// tema. Viven en la memoria de egui: se arman una vez y cada cuadro solo
/// pinta un rectángulo con la imagen.
#[derive(Clone, Default)]
struct CoverageTextures {
    entries: Vec<(CoverageKey, egui::TextureHandle)>,
}

#[derive(Clone, PartialEq)]
struct CoverageKey {
    scene: usize,
    level: usize,
    span: usize,
    hidden: Vec<Layer>,
    dark: bool,
}

/// Cuántos niveles se guardan (el actual y los vecinos, para acercar y
/// alejar sin recomponer).
const COVERAGE_CACHE: usize = 3;

/// Pinta el nivel `level` de la pirámide: lo que mide menos de unas celdas,
/// como una imagen con el color de cada capa (su relleno, o su contorno
/// en capas de solo contorno, cuyos bordes marca la pirámide), en el orden
/// de pintado de la escena.
#[allow(clippy::too_many_arguments)]
fn paint_coverage(
    painter: &egui::Painter,
    xf: &ScreenXform,
    scene: &dyn RenderableScene,
    index: &SceneIndex,
    level: usize,
    span: usize,
    hidden: &HashSet<Layer>,
    theme: &CanvasTheme,
) {
    let Some(view) = index.coverage(level, span) else { return };
    let mut hidden_sorted: Vec<Layer> = hidden.iter().copied().collect();
    hidden_sorted.sort_unstable();
    let key = CoverageKey { scene: index as *const SceneIndex as usize, level, span, hidden: hidden_sorted, dark: theme.dark };
    let id = egui::Id::new("riku-coverage-textures");
    let ctx = painter.ctx();
    let mut cache: CoverageTextures = ctx.data(|d| d.get_temp(id)).unwrap_or_default();
    let texture = match cache.entries.iter().position(|(k, _)| *k == key) {
        Some(pos) => cache.entries[pos].1.clone(),
        None => {
            let flip = scene.y_axis() == YAxis::Up;
            let image = coverage_image(&view, scene, hidden, theme, flip);
            // Al achicar, promedio (lineal); al agrandar un texel de varios
            // píxeles, bordes nítidos (nearest) en vez de borrosos.
            let options = egui::TextureOptions {
                magnification: egui::TextureFilter::Nearest,
                minification: egui::TextureFilter::Linear,
                ..egui::TextureOptions::LINEAR
            };
            let tex = ctx.load_texture(format!("riku-coverage-{level}-{span}"), image, options);
            cache.entries.push((key, tex.clone()));
            if cache.entries.len() > COVERAGE_CACHE {
                drop(cache.entries.remove(0));
            }
            ctx.data_mut(|d| d.insert_temp(id, cache));
            tex
        }
    };
    let b = view.bbox();
    let rect = Rect::from_two_pos(xf.to_screen(b.min_x, b.min_y), xf.to_screen(b.max_x, b.max_y));
    let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
    painter.image(texture.id(), rect, uv, Color32::WHITE);
}

/// Imagen de `n × n` de un nivel de la pirámide. La fila 0 es la de menor Y
/// del mundo, salvo con `flip` (Y hacia arriba: la fila 0 va arriba).
fn coverage_image(
    view: &viewer_core::index::CoverageView<'_>,
    scene: &dyn RenderableScene,
    hidden: &HashSet<Layer>,
    theme: &CanvasTheme,
    flip: bool,
) -> egui::ColorImage {
    let n = view.n;
    let mut pixels = vec![Color32::TRANSPARENT; n * n];
    for layer in view.layers() {
        if hidden.contains(&layer.layer) {
            continue;
        }
        let (fill, stroke) = layer_colors(scene, layer.layer);
        let (fill, stroke) = theme.layer_colors(fill, stroke);
        let color = if fill.a() > 0 { fill } else { stroke.gamma_multiply(0.7) };
        for (cx, cy) in layer.cells() {
            let row = if flip { n - 1 - cy } else { cy };
            let p = &mut pixels[row * n + cx];
            *p = over(color, *p);
        }
    }
    egui::ColorImage::new([n, n], pixels)
}

/// `src` sobre `dst` (colores premultiplicados, como `Color32`).
fn over(src: Color32, dst: Color32) -> Color32 {
    let k = 255 - u16::from(src.a());
    let mix = |s: u8, d: u8| (u16::from(s) + (u16::from(d) * k + 127) / 255).min(255) as u8;
    Color32::from_rgba_premultiplied(mix(src.r(), dst.r()), mix(src.g(), dst.g()), mix(src.b(), dst.b()), mix(src.a(), dst.a()))
}

/// Coloca y dibuja las etiquetas: punto en el anclaje exacto y pastilla con
/// halo del color del lienzo, borde del color de la capa y texto con
/// contraste garantizado. Retorna cuántas se omitieron por solaparse.
fn paint_labels(
    painter: &egui::Painter,
    labels: Vec<(LabelCandidate, f32)>,
    clip: Rect,
    theme: &CanvasTheme,
) -> usize {
    if labels.is_empty() {
        return 0;
    }
    // Un solo tamaño por frame (todas las etiquetas de una escena comparten
    // tamaño en el mundo): simplifica medir y alinear.
    let px = labels.iter().map(|(_, px)| *px).fold(0.0_f32, f32::max);
    let font = FontId::proportional(px);
    let measure = |t: &str| painter.layout_no_wrap(t.to_string(), font.clone(), Color32::WHITE).size();
    let (placed, hidden) = place(labels.into_iter().map(|(c, _)| c).collect(), measure, clip);

    for l in &placed {
        let dot = Stroke::new(1.5_f32, theme.label_halo);
        painter.circle(l.anchor, 2.5, theme.layer_colors(l.color, l.color).1, dot);
        painter.rect(
            l.rect,
            3.0,
            theme.label_halo,
            Stroke::new(1.0_f32, l.color.gamma_multiply(0.7)),
            StrokeKind::Inside,
        );
        painter.text(
            l.rect.min + PILL_PADDING,
            Align2::LEFT_TOP,
            &l.text,
            font.clone(),
            theme.label_text(l.color),
        );
    }
    hidden
}

fn draw_element(
    painter: &egui::Painter,
    xf: &ScreenXform,
    scale: f64,
    scene: &dyn RenderableScene,
    el: &DrawElement,
    theme: &CanvasTheme,
) {
    let (fill, stroke_color) = layer_colors(scene, el.layer());
    let (fill, stroke_color) = theme.layer_colors(fill, stroke_color);
    let stroke = Stroke::new(1.0_f32, stroke_color);

    match el {
        DrawElement::Line { x1, y1, x2, y2, .. } => {
            painter.line_segment([xf.to_screen(*x1, *y1), xf.to_screen(*x2, *y2)], stroke);
        }
        DrawElement::Rect { x, y, w, h, filled, .. } => {
            let r = Rect::from_two_pos(xf.to_screen(*x, *y), xf.to_screen(*x + *w, *y + *h));
            if *filled {
                painter.rect(r, 0.0, fill, stroke, StrokeKind::Middle);
            } else {
                painter.rect_stroke(r, 0.0, stroke, StrokeKind::Middle);
            }
        }
        DrawElement::Circle { cx, cy, r, filled, .. } => {
            let center = xf.to_screen(*cx, *cy);
            let radius_screen = (*r * scale) as f32;
            if *filled {
                painter.circle(center, radius_screen, fill, stroke);
            } else {
                painter.circle_stroke(center, radius_screen, stroke);
            }
        }
        DrawElement::Polygon { points, filled, .. } => {
            if points.len() < 2 { return; }
            let pts: Vec<Pos2> = points.iter().map(|(x, y)| xf.to_screen(*x, *y)).collect();
            if *filled && pts.len() >= 3 {
                paint_filled_polygon(painter, points, pts, fill, stroke);
            } else {
                painter.add(Shape::line(pts, stroke));
            }
        }
        // Las etiquetas se dibujan aparte (paint_labels): horizontales y
        // legibles siempre, aunque el formato las declare rotadas.
        DrawElement::Text { .. } => {}
    }
}

/// Texto que es parte del dibujo (`TextStyle::Drawn`): escala con el zoom y
/// respeta ángulo y alineación. El ángulo es el visual (pantalla Y-down).
fn draw_text(painter: &egui::Painter, xf: &ScreenXform, scale: f64, el: &DrawElement, color: Color32) {
    let DrawElement::Text { x, y, content, size, angle_deg, h_align, v_align, .. } = el else { return };
    let px = (*size * scale).clamp(4.0, 2000.0) as f32;
    let galley = painter.layout_no_wrap(content.clone(), FontId::monospace(px), color);
    let ax = match h_align {
        HAlign::Start => 0.0,
        HAlign::Middle => 0.5,
        HAlign::End => 1.0,
    };
    let ay = match v_align {
        VAlign::Top => 0.0,
        VAlign::Middle => 0.5,
        VAlign::Bottom => 1.0,
    };
    let angle = angle_deg.to_radians() as f32;
    let (sin, cos) = angle.sin_cos();
    // El galley se ancla por su esquina superior izquierda: se corre para
    // que el punto de alineación caiga en el ancla y se rota alrededor de él.
    let (lx, ly) = (-galley.size().x * ax, -galley.size().y * ay);
    let pos = xf.to_screen(*x, *y) + egui::vec2(lx * cos - ly * sin, lx * sin + ly * cos);
    painter.add(egui::epaint::TextShape::new(pos, galley, color).with_angle(angle));
}

/// Gris de los fantasmas de la versión anterior: visible sin competir con
/// la versión actual.
fn ghost_color(theme: &CanvasTheme) -> Color32 {
    if theme.dark { Color32::from_gray(95) } else { Color32::from_gray(185) }
}

/// Un elemento de la versión anterior, solo en contorno y en un color.
fn draw_tinted(painter: &egui::Painter, xf: &ScreenXform, scale: f64, el: &DrawElement, color: Color32, drawn_text: bool) {
    let stroke = Stroke::new(1.0_f32, color);
    match el {
        DrawElement::Line { x1, y1, x2, y2, .. } => {
            painter.line_segment([xf.to_screen(*x1, *y1), xf.to_screen(*x2, *y2)], stroke);
        }
        DrawElement::Rect { x, y, w, h, .. } => {
            let r = Rect::from_two_pos(xf.to_screen(*x, *y), xf.to_screen(*x + *w, *y + *h));
            painter.rect_stroke(r, 0.0, stroke, StrokeKind::Middle);
        }
        DrawElement::Circle { cx, cy, r, .. } => {
            painter.circle_stroke(xf.to_screen(*cx, *cy), (*r * scale) as f32, stroke);
        }
        DrawElement::Polygon { points, .. } if points.len() >= 2 => {
            painter.add(Shape::line(points.iter().map(|(x, y)| xf.to_screen(*x, *y)).collect(), stroke));
        }
        DrawElement::Text { .. } if drawn_text => draw_text(painter, xf, scale, el, color),
        _ => {}
    }
}

/// Relleno y contorno de la marca de un cambio: añadido verde, eliminado
/// rojo, modificado ámbar, solo movido cian y cosmético gris.
fn annotation_colors(a: &Annotation, theme: &CanvasTheme) -> (Color32, Color32) {
    let (r, g, b) = match (a.kind, a.cosmetic, a.moved) {
        (ChangeKind::Added, _, _) => (0, 200, 0),
        (ChangeKind::Removed, _, _) => (220, 40, 40),
        (ChangeKind::Modified, true, true) => (0, 190, 255),
        (ChangeKind::Modified, true, false) => (150, 150, 150),
        (ChangeKind::Modified, false, _) => (255, 180, 0),
    };
    theme.layer_colors(Color32::from_rgba_unmultiplied(r, g, b, 45), Color32::from_rgb(r, g, b))
}

/// Marcas del diff encima de la geometría: recuadro con el nombre (y un
/// segundo borde cian si además se movió) o trazos gruesos para una net.
fn paint_annotations(painter: &egui::Painter, xf: &ScreenXform, annotations: &[Annotation], theme: &CanvasTheme) {
    let font = FontId::monospace(11.0);
    for a in annotations {
        let (fill, stroke) = annotation_colors(a, theme);
        match &a.shape {
            AnnotationShape::Box(b) => {
                let r = Rect::from_two_pos(xf.to_screen(b.min_x, b.min_y), xf.to_screen(b.max_x, b.max_y)).expand(4.0);
                painter.rect(r, 2.0, fill, Stroke::new(1.5_f32, stroke), StrokeKind::Outside);
                if a.moved && !a.cosmetic && a.kind == ChangeKind::Modified {
                    let cyan = theme.layer_colors(Color32::TRANSPARENT, Color32::from_rgb(0, 190, 255)).1;
                    painter.rect_stroke(r.expand(2.0), 2.0, Stroke::new(1.5_f32, cyan), StrokeKind::Outside);
                }
                painter.text(r.left_top() + egui::vec2(2.0, -3.0), Align2::LEFT_BOTTOM, &a.label, font.clone(), stroke);
            }
            AnnotationShape::Segments(segs) => {
                for (x1, y1, x2, y2) in segs {
                    painter.line_segment([xf.to_screen(*x1, *y1), xf.to_screen(*x2, *y2)], Stroke::new(3.0_f32, stroke));
                }
                if let Some((x1, y1, x2, y2)) = segs.first() {
                    let mid = xf.to_screen((x1 + x2) / 2.0, (y1 + y2) / 2.0);
                    painter.text(mid + egui::vec2(0.0, -8.0), Align2::CENTER_BOTTOM, &a.label, font.clone(), stroke);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use viewer_core::scene::Scene;

    fn panel() -> Rect {
        // Panel desplazado como el CentralPanel real (a la derecha del árbol).
        Rect::from_min_size(Pos2::new(212.0, 60.0), egui::vec2(800.0, 600.0))
    }

    fn square_scene(y_axis: YAxis) -> Scene {
        let mut s = Scene::new();
        s.y_axis = y_axis;
        s.push(DrawElement::Rect { x: 0.0, y: 0.0, w: 10.0, h: 10.0, layer: 1, filled: true });
        s
    }

    #[test]
    fn fit_centers_scene_inside_offset_panel() {
        for axis in [YAxis::Down, YAxis::Up] {
            let scene = square_scene(axis);
            let mut vp = Viewport::default();
            fit_scene(&mut vp, &scene, panel());
            let c = ScreenXform::new(panel(), &vp, axis).to_screen(5.0, 5.0);
            assert!((c - panel().center()).length() < 1e-3, "{axis:?}: {c:?}");
        }
    }

    #[test]
    fn y_up_puts_higher_world_y_higher_on_screen() {
        let scene = square_scene(YAxis::Up);
        let mut vp = Viewport::default();
        fit_scene(&mut vp, &scene, panel());
        let xf = ScreenXform::new(panel(), &vp, YAxis::Up);
        assert!(xf.to_screen(0.0, 10.0).y < xf.to_screen(0.0, 0.0).y);
    }

    #[test]
    fn visible_bbox_covers_fitted_scene() {
        for axis in [YAxis::Down, YAxis::Up] {
            let scene = square_scene(axis);
            let mut vp = Viewport::default();
            fit_scene(&mut vp, &scene, panel());
            let vis = ScreenXform::new(panel(), &vp, axis).visible_world_bbox();
            assert!(vis.contains(0.0, 0.0) && vis.contains(10.0, 10.0), "{axis:?}: {vis:?}");
        }
    }

    #[test]
    fn zoom_keeps_point_under_cursor() {
        let scene = square_scene(YAxis::Up);
        let mut vp = Viewport::default();
        fit_scene(&mut vp, &scene, panel());
        let cursor = ScreenXform::new(panel(), &vp, YAxis::Up).to_screen(2.0, 8.0);
        zoom_at_screen(&mut vp, 2.0, cursor, panel());
        let after = ScreenXform::new(panel(), &vp, YAxis::Up).to_screen(2.0, 8.0);
        assert!((after - cursor).length() < 1e-3);
    }

    /// Dos rects superpuestos en capas 1 (abajo) y 2 (arriba), escena Y-up en µm.
    fn stacked_scene() -> Scene {
        let mut s = Scene::new();
        s.y_axis = YAxis::Up;
        s.world_unit = Some("µm".into());
        s.push(DrawElement::Rect { x: 0.0, y: 0.0, w: 10.0, h: 10.0, layer: 1, filled: true });
        s.push(DrawElement::Rect { x: 0.0, y: 0.0, w: 4.0, h: 2.0, layer: 2, filled: true });
        s.layers.insert(2, viewer_core::paint::LayerPaint {
            name: "met1 68/20".into(),
            fill: Rgba::new(0, 0, 255, 90),
            stroke: Rgba::new(0, 0, 255, 255),
        });
        s
    }

    #[test]
    fn to_world_inverts_to_screen() {
        let scene = stacked_scene();
        let mut vp = Viewport::default();
        fit_scene(&mut vp, &scene, panel());
        let xf = ScreenXform::new(panel(), &vp, YAxis::Up);
        let (x, y) = xf.to_world(xf.to_screen(3.0, 7.0));
        assert!((x - 3.0).abs() < 1e-4 && (y - 7.0).abs() < 1e-4);
    }

    #[test]
    fn pick_prefers_topmost_and_skips_hidden() {
        let scene = stacked_scene();
        let none = HashSet::new();
        assert_eq!(pick_at(&scene, (1.0, 1.0), &none).map(|e| e.layer()), Some(2));
        assert_eq!(pick_at(&scene, (8.0, 8.0), &none).map(|e| e.layer()), Some(1));
        let hide_top: HashSet<Layer> = [2].into();
        assert_eq!(pick_at(&scene, (1.0, 1.0), &hide_top).map(|e| e.layer()), Some(1));
        assert!(pick_at(&scene, (20.0, 20.0), &none).is_none());
    }

    #[test]
    fn pick_skips_outline_layers_unless_nothing_else() {
        // Boundary de solo contorno (relleno transparente) pintado encima de todo.
        let mut scene = stacked_scene();
        scene.push(DrawElement::Rect { x: -1.0, y: -1.0, w: 30.0, h: 30.0, layer: 9, filled: true });
        scene.layers.insert(9, viewer_core::paint::LayerPaint {
            name: "prBoundary".into(),
            fill: Rgba::new(150, 0, 230, 0),
            stroke: Rgba::new(150, 0, 230, 255),
        });
        let none = HashSet::new();
        assert_eq!(pick_at(&scene, (1.0, 1.0), &none).map(|e| e.layer()), Some(2));
        assert_eq!(pick_at(&scene, (20.0, 20.0), &none).map(|e| e.layer()), Some(9), "fuera de todo lo relleno");
    }

    #[test]
    fn hover_info_describes_layer_size_and_area() {
        let scene = stacked_scene();
        let mut vp = Viewport::default();
        fit_scene(&mut vp, &scene, panel());
        let pos = ScreenXform::new(panel(), &vp, YAxis::Up).to_screen(1.0, 1.0);
        let text = hover_info(&scene, &vp, panel(), pos, &HashSet::new()).expect("hit");
        assert_eq!(text, "met1 68/20\n4.000 × 2.000 µm\nárea 8.0000 µm²");
    }

    #[test]
    fn focus_area_adds_context_and_keeps_center() {
        let scene = BoundingBox::from_points((0.0, 0.0), (10.0, 20.0));
        // Cambio diminuto: se agranda al 15 % del lado mayor (3.0).
        let tiny = BoundingBox::from_points((4.9, 9.9), (5.1, 10.1));
        let f = focus_area(&tiny, &scene);
        assert!((f.width() - 3.0).abs() < 1e-9 && (f.height() - 3.0).abs() < 1e-9);
        assert_eq!(f.center(), tiny.center());
        // Cambio grande: 25 % de margen.
        let big = BoundingBox::from_points((0.0, 0.0), (8.0, 4.0));
        let f = focus_area(&big, &scene);
        assert!((f.width() - 10.0).abs() < 1e-9 && (f.height() - 5.0).abs() < 1e-9);
    }

    #[test]
    fn fit_bbox_centers_target_region() {
        let target = BoundingBox::from_points((100.0, 100.0), (110.0, 105.0));
        let mut vp = Viewport::default();
        fit_bbox(&mut vp, &target, YAxis::Up, panel());
        let c = ScreenXform::new(panel(), &vp, YAxis::Up).to_screen(105.0, 102.5);
        assert!((c - panel().center()).length() < 1e-3);
    }

    #[test]
    fn scene_paint_overrides_neutral_palette() {
        let mut scene = square_scene(YAxis::Up);
        scene.layers.insert(1, viewer_core::paint::LayerPaint {
            name: "met1 68/20".into(),
            fill: Rgba::new(60, 130, 240, 90),
            stroke: Rgba::new(60, 130, 240, 255),
        });
        let (fill, stroke) = layer_colors(&scene, 1);
        assert!(fill.a() < 255 && stroke.a() == 255);
        let (nf, _) = layer_colors(&scene, 7);
        assert_eq!(nf, neutral_layer_color(7));
    }
}
