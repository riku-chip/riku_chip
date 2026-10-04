//! Una escena del visor (`viewer_core`) como SVG, sin ventana ni GPU.
//!
//! Mismo orden y colores que el lienzo de la GUI (`gui::scene_painter`):
//! fondo, versión anterior atenuada (diff), geometría con el estilo de cada
//! capa, marcas del diff y textos. Sirve para cualquier formato con visor
//! (esquemáticos, layouts GDS/OASIS/Magic).

use std::fmt::Write;

use crate::i18n::tr;
use viewer_core::{
    Annotation, AnnotationShape, BoundingBox, ChangeKind, DrawElement, HAlign, Layer, RenderableScene, TextStyle, VAlign, YAxis,
};

/// Tamaño y tema de la imagen.
#[derive(Clone, Debug)]
pub struct Style {
    pub width: u32,
    pub height: u32,
    pub dark: bool,
    /// Texto de la franja superior (archivo, versiones); vacío = sin franja.
    pub caption: String,
}

/// Alto de la franja del título, en píxeles.
const CAPTION_H: f64 = 30.0;
const MARGIN: f64 = 24.0;
/// Etiquetas de layout: tamaño en pantalla y cuántas como mucho.
const LABEL_PX: (f64, f64) = (9.0, 13.0);
const MAX_LABELS: usize = 400;

#[derive(Clone, Copy)]
struct Rgb(u8, u8, u8, f64);

impl Rgb {
    fn css(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }
}

fn mix(c: Rgb, to: (u8, u8, u8), t: f64) -> Rgb {
    let l = |a: u8, b: u8| (a as f64 + (b as f64 - a as f64) * t).round() as u8;
    Rgb(l(c.0, to.0), l(c.1, to.1), l(c.2, to.2), c.3)
}

/// Color de una capa sin estilo propio (como el visor).
fn neutral(layer: Layer) -> Rgb {
    let (r, g, b) = match layer {
        1 => (180, 180, 200),
        2 => (120, 120, 140),
        3 => (220, 220, 160),
        4 => (120, 200, 255),
        5 => (180, 200, 255),
        6 => (220, 180, 120),
        _ => (
            (layer.wrapping_mul(131) & 0xFF) as u8 | 0x40,
            (layer.wrapping_mul(197) & 0xFF) as u8 | 0x40,
            (layer.wrapping_mul(263) & 0xFF) as u8 | 0x40,
        ),
    };
    Rgb(r, g, b, 1.0)
}

/// Relleno y contorno de una capa, ajustados al tema como en el visor: sobre
/// fondo claro, contorno más oscuro y relleno más opaco.
fn layer_colors(scene: &dyn RenderableScene, layer: Layer, dark: bool) -> (Rgb, Rgb) {
    let (fill, stroke) = match scene.layer_paint(layer) {
        Some(p) => (
            Rgb(p.fill.r, p.fill.g, p.fill.b, p.fill.a as f64 / 255.0),
            Rgb(p.stroke.r, p.stroke.g, p.stroke.b, p.stroke.a as f64 / 255.0),
        ),
        None => (neutral(layer), neutral(layer)),
    };
    if dark {
        return (fill, stroke);
    }
    let alpha = if fill.3 == 0.0 { 0.0 } else { (fill.3 * 1.6).min(200.0 / 255.0) };
    (Rgb(fill.0, fill.1, fill.2, alpha), mix(stroke, (0, 0, 0), 0.35))
}

fn change_rgb(kind: ChangeKind, cosmetic: bool, moved: bool, dark: bool) -> Rgb {
    let (r, g, b) = match (kind, cosmetic, moved) {
        (ChangeKind::Added, _, _) => (0, 200, 0),
        (ChangeKind::Removed, _, _) => (220, 40, 40),
        (ChangeKind::Modified, true, true) => (0, 190, 255),
        (ChangeKind::Modified, true, false) => (150, 150, 150),
        (ChangeKind::Modified, false, _) => (255, 180, 0),
    };
    let c = Rgb(r, g, b, 1.0);
    if dark {
        c
    } else {
        mix(c, (0, 0, 0), 0.35)
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Mundo → píxeles: encuadra `bbox` en la imagen, con el eje Y de la escena.
struct Xform {
    s: f64,
    ox: f64,
    oy: f64,
    y_up: bool,
    bbox: BoundingBox,
}

impl Xform {
    fn new(bbox: BoundingBox, style: &Style, top: f64, y_up: bool) -> Self {
        let (w, h) = (style.width as f64 - 2.0 * MARGIN, style.height as f64 - top - 2.0 * MARGIN);
        let (bw, bh) = (bbox.width().max(1e-9), bbox.height().max(1e-9));
        let s = (w / bw).min(h / bh);
        let ox = MARGIN + (w - bw * s) / 2.0;
        let oy = top + MARGIN + (h - bh * s) / 2.0;
        Self { s, ox, oy, y_up, bbox }
    }

    fn p(&self, x: f64, y: f64) -> (f64, f64) {
        let px = self.ox + (x - self.bbox.min_x) * self.s;
        let py = if self.y_up { self.oy + (self.bbox.max_y - y) * self.s } else { self.oy + (y - self.bbox.min_y) * self.s };
        (px, py)
    }
}

fn scene_bbox(scene: &dyn RenderableScene) -> BoundingBox {
    let mut b = scene.bbox();
    for el in scene.ghost() {
        b.expand(&el.bounding_box());
    }
    for a in scene.annotations() {
        if let AnnotationShape::Box(bb) = &a.shape {
            b.expand(bb);
        }
    }
    b
}

/// La escena como SVG.
pub fn scene_svg(scene: &dyn RenderableScene, style: &Style) -> String {
    let (w, h) = (style.width, style.height);
    let bg = if style.dark { "#141418" } else { "#fafaf7" };
    let fg = if style.dark { "#e6e6e6" } else { "#1e1e1e" };
    let mut out = String::new();
    let _ = write!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}" font-family="DejaVu Sans Mono, monospace">"#
    );
    let _ = write!(out, r#"<rect width="{w}" height="{h}" fill="{bg}"/>"#);
    let top = if style.caption.is_empty() { 0.0 } else { CAPTION_H };
    if !style.caption.is_empty() {
        let _ = write!(out, r#"<text x="{MARGIN}" y="20" font-size="14" fill="{fg}">{}</text>"#, esc(&style.caption));
    }
    if scene.is_empty() {
        out.push_str("</svg>");
        return out;
    }
    // Aire alrededor: la caja de la escena no incluye el ancho de los textos.
    let raw = scene_bbox(scene);
    let bbox = raw.inflate(raw.width().max(raw.height()) * 0.06);
    let xf = Xform::new(bbox, style, top, scene.y_axis() == YAxis::Up);
    let drawn_text = scene.text_style() == TextStyle::Drawn;

    // Versión anterior (diff), atenuada y solo en contorno.
    let ghost = if style.dark { Rgb(95, 95, 95, 1.0) } else { Rgb(185, 185, 185, 1.0) };
    let mut batch = Batch::default();
    for el in scene.ghost() {
        if !batch.add(&mut out, &xf, el, None, ghost) {
            element(&mut out, &xf, el, None, ghost, drawn_text);
        }
    }
    batch.flush(&mut out);

    let mut labels: Vec<(f64, f64, String, Rgb)> = Vec::new();
    scene.visit(&bbox.inflate(bbox.width().max(bbox.height())), &mut |el| {
        let (fill, stroke) = layer_colors(scene, el.layer(), style.dark);
        match el {
            DrawElement::Text { x, y, content, size, .. } if !drawn_text => {
                let natural = size * xf.s;
                if natural >= 4.0 && labels.len() < MAX_LABELS {
                    let (px, py) = xf.p(*x, *y);
                    labels.push((px, py, content.clone(), stroke));
                }
            }
            _ => {
                if !batch.add(&mut out, &xf, el, Some(fill), stroke) {
                    batch.flush(&mut out);
                    element(&mut out, &xf, el, Some(fill), stroke, drawn_text);
                }
            }
        }
        true
    });
    batch.flush(&mut out);

    for a in scene.annotations() {
        annotation(&mut out, &xf, a, style.dark);
    }
    for (x, y, text, color) in labels {
        let size = 11.0f64.clamp(LABEL_PX.0, LABEL_PX.1);
        let _ = write!(
            out,
            r#"<text x="{x:.1}" y="{y:.1}" font-size="{size}" fill="{}" text-anchor="middle" dominant-baseline="central">{}</text>"#,
            color.css(),
            esc(&text)
        );
    }
    out.push_str("</svg>");
    out
}

fn stroke_attrs(c: Rgb, width: f64) -> String {
    format!(r#"stroke="{}" stroke-opacity="{:.3}" stroke-width="{width}""#, c.css(), c.3)
}

fn fill_attrs(filled: bool, fill: Option<Rgb>) -> String {
    match (filled, fill) {
        (true, Some(f)) if f.3 > 0.0 => format!(r#"fill="{}" fill-opacity="{:.3}""#, f.css(), f.3),
        _ => r#"fill="none""#.to_string(),
    }
}

/// Rectángulos y polígonos seguidos con el mismo estilo (los de una capa)
/// van en un solo `<path>`: un layout grande son cientos de miles, y como
/// elementos sueltos el SVG pesa decenas de MB y tarda en rasterizarse.
#[derive(Default)]
struct Batch {
    attrs: String,
    d: String,
}

impl Batch {
    /// Suma `el` si es un rectángulo o polígono; `false` si no lo es.
    fn add(&mut self, out: &mut String, xf: &Xform, el: &DrawElement, fill: Option<Rgb>, stroke: Rgb) -> bool {
        let (pts, filled): (Vec<(f64, f64)>, bool) = match el {
            DrawElement::Rect { x, y, w, h, filled, .. } => {
                (vec![xf.p(*x, *y), xf.p(x + w, *y), xf.p(x + w, y + h), xf.p(*x, y + h)], *filled)
            }
            DrawElement::Polygon { points, filled, .. } if points.len() >= 2 => {
                (points.iter().map(|(x, y)| xf.p(*x, *y)).collect(), *filled)
            }
            _ => return false,
        };
        let (mut lo, mut hi) = ((f64::MAX, f64::MAX), (f64::MIN, f64::MIN));
        for &(x, y) in &pts {
            lo = (lo.0.min(x), lo.1.min(y));
            hi = (hi.0.max(x), hi.1.max(y));
        }
        // Menos de un píxel: no se vería (layouts grandes vistos enteros).
        if hi.0 - lo.0 < 0.3 && hi.1 - lo.1 < 0.3 {
            return true;
        }
        let attrs = format!("{} {}", fill_attrs(filled, fill), stroke_attrs(stroke, 1.0));
        if attrs != self.attrs {
            self.flush(out);
            self.attrs = attrs;
        }
        for (i, (x, y)) in pts.iter().enumerate() {
            let _ = write!(self.d, "{}{x:.1} {y:.1}", if i == 0 { "M" } else { "L" });
        }
        if filled {
            self.d.push('Z');
        }
        true
    }

    fn flush(&mut self, out: &mut String) {
        if !self.d.is_empty() {
            let _ = write!(out, r#"<path d="{}" {}/>"#, self.d, self.attrs);
            self.d.clear();
        }
    }
}

/// Un elemento que no es rectángulo ni polígono (esos van en [`Batch`]).
/// `fill = None`: solo contorno (versión anterior).
fn element(out: &mut String, xf: &Xform, el: &DrawElement, fill: Option<Rgb>, stroke: Rgb, drawn_text: bool) {
    let st = stroke_attrs(stroke, 1.0);
    let fill_attrs = |filled: bool| fill_attrs(filled, fill);
    match el {
        DrawElement::Line { x1, y1, x2, y2, .. } => {
            let ((a, b), (c, d)) = (xf.p(*x1, *y1), xf.p(*x2, *y2));
            let _ = write!(out, r#"<line x1="{a:.2}" y1="{b:.2}" x2="{c:.2}" y2="{d:.2}" {st}/>"#);
        }
        DrawElement::Circle { cx, cy, r, filled, .. } => {
            let (a, b) = xf.p(*cx, *cy);
            let _ = write!(out, r#"<circle cx="{a:.2}" cy="{b:.2}" r="{:.2}" {} {st}/>"#, r * xf.s, fill_attrs(*filled));
        }
        DrawElement::Text { x, y, content, size, angle_deg, h_align, v_align, .. } if drawn_text => {
            let (a, b) = xf.p(*x, *y);
            let px = (size * xf.s).clamp(3.0, 400.0);
            let anchor = match h_align {
                HAlign::Start => "start",
                HAlign::Middle => "middle",
                HAlign::End => "end",
            };
            let baseline = match v_align {
                VAlign::Top => "hanging",
                VAlign::Middle => "central",
                VAlign::Bottom => "text-after-edge",
            };
            let rot =
                if *angle_deg == 0.0 { String::new() } else { format!(r#" transform="rotate({angle_deg:.1} {a:.2} {b:.2})""#) };
            let _ = write!(
                out,
                r#"<text x="{a:.2}" y="{b:.2}" font-size="{px:.2}" fill="{}" text-anchor="{anchor}" dominant-baseline="{baseline}"{rot}>{}</text>"#,
                stroke.css(),
                esc(content)
            );
        }
        _ => {}
    }
}

fn annotation(out: &mut String, xf: &Xform, a: &Annotation, dark: bool) {
    let c = change_rgb(a.kind, a.cosmetic, a.moved, dark);
    match &a.shape {
        AnnotationShape::Box(b) => {
            let ((x1, y1), (x2, y2)) = (xf.p(b.min_x, b.min_y), xf.p(b.max_x, b.max_y));
            let (x, y, w, h) = (x1.min(x2) - 4.0, y1.min(y2) - 4.0, (x2 - x1).abs() + 8.0, (y2 - y1).abs() + 8.0);
            let _ = write!(
                out,
                r#"<rect x="{x:.2}" y="{y:.2}" width="{w:.2}" height="{h:.2}" rx="2" fill="{}" fill-opacity="0.18" {}/>"#,
                c.css(),
                stroke_attrs(c, 1.5)
            );
            let _ = write!(
                out,
                r#"<text x="{:.2}" y="{:.2}" font-size="11" fill="{}">{}</text>"#,
                x + 2.0,
                y - 3.0,
                c.css(),
                esc(&a.label)
            );
        }
        AnnotationShape::Segments(segs) => {
            for (x1, y1, x2, y2) in segs {
                let ((a1, b1), (a2, b2)) = (xf.p(*x1, *y1), xf.p(*x2, *y2));
                let _ = write!(
                    out,
                    r#"<line x1="{a1:.2}" y1="{b1:.2}" x2="{a2:.2}" y2="{b2:.2}" {} stroke-linecap="round"/>"#,
                    stroke_attrs(c, 3.0)
                );
            }
            if let Some((x1, y1, _, _)) = segs.first() {
                let (x, y) = xf.p(*x1, *y1);
                let _ = write!(
                    out,
                    r#"<text x="{:.2}" y="{:.2}" font-size="11" fill="{}">{}</text>"#,
                    x + 4.0,
                    y - 4.0,
                    c.css(),
                    esc(&a.label)
                );
            }
        }
    }
}

/// SVG → PNG con `resvg` (sin GPU). Las fuentes salen del sistema; sin
/// ninguna, la imagen sale igual pero sin textos.
pub fn to_png(svg: &str) -> Result<Vec<u8>, String> {
    use resvg::{tiny_skia, usvg};
    let mut opt = usvg::Options::default();
    opt.fontdb_mut().load_system_fonts();
    let tree = usvg::Tree::from_str(svg, &opt).map_err(|e| e.to_string())?;
    let size = tree.size().to_int_size();
    let mut pixmap = tiny_skia::Pixmap::new(size.width(), size.height()).ok_or_else(|| tr!("image.zero_size"))?;
    resvg::render(&tree, tiny_skia::Transform::default(), &mut pixmap.as_mut());
    pixmap.encode_png().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use viewer_core::{LayerPaint, Rgba, Scene};

    fn style() -> Style {
        Style { width: 400, height: 300, dark: false, caption: "a.sch".into() }
    }

    #[test]
    fn draws_elements_annotations_and_escapes_text() {
        let mut s = Scene::new();
        s.push(DrawElement::Line { x1: 0.0, y1: 0.0, x2: 10.0, y2: 0.0, layer: 1 });
        s.push(DrawElement::Rect { x: 0.0, y: 0.0, w: 10.0, h: 5.0, layer: 7, filled: true });
        s.layers.insert(
            7,
            LayerPaint {
                name: "met1".into(),
                fill: Rgba::new(10, 20, 30, 90),
                stroke: Rgba::new(10, 20, 30, 255),
                hidden: false,
            },
        );
        s.annotations.push(Annotation {
            kind: ChangeKind::Added,
            cosmetic: false,
            moved: false,
            label: "R1 <new>".into(),
            shape: AnnotationShape::Box(BoundingBox::from_points((0.0, 0.0), (10.0, 5.0))),
        });
        let svg = scene_svg(&s, &style());
        assert!(svg.starts_with("<svg") && svg.ends_with("</svg>"));
        assert!(svg.contains("<line") && svg.contains(r##"fill="#0a141e""##));
        assert!(svg.contains("R1 &lt;new&gt;"), "{svg}");
        assert!(svg.contains(">a.sch</text>"));
    }

    #[test]
    fn y_up_scenes_are_flipped() {
        let mut s = Scene::new();
        s.y_axis = YAxis::Up;
        s.push(DrawElement::Line { x1: 0.0, y1: 0.0, x2: 0.0, y2: 10.0, layer: 1 });
        let b = s.bbox;
        let xf = Xform::new(b, &style(), CAPTION_H, true);
        // y mayor en el mundo = más arriba en la imagen.
        assert!(xf.p(0.0, 10.0).1 < xf.p(0.0, 0.0).1);
    }

    #[test]
    fn png_has_the_requested_size() {
        let mut s = Scene::new();
        s.push(DrawElement::Line { x1: 0.0, y1: 0.0, x2: 10.0, y2: 10.0, layer: 1 });
        let png = to_png(&scene_svg(&s, &style())).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        // Ancho y alto en la cabecera IHDR.
        assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 400);
        assert_eq!(u32::from_be_bytes(png[20..24].try_into().unwrap()), 300);
    }
}
