//! Colores del lienzo según el tema (claro/oscuro) de egui.
//!
//! Los backends entregan colores pensados para fondo oscuro (como KLayout).
//! En tema claro se ajustan aquí, en un solo lugar: contornos más oscuros y
//! rellenos algo más opacos para que no queden lavados sobre blanco, y
//! textos con contraste suficiente sobre su halo.

use eframe::egui::{self, Color32};

/// Escala de espaciado (px). Todo margen o separación de la UI sale de acá:
/// valores deliberados y repetidos, no números sueltos por pantalla.
pub mod space {
    pub const XS: f32 = 4.0;
    pub const S: f32 = 8.0;
    pub const M: f32 = 12.0;
    pub const L: f32 = 16.0;
}

/// Estilo global de la UI, para ambos temas: jerarquía tipográfica
/// (título > cuerpo > secundario), espaciado de la escala, esquinas
/// redondeadas coherentes y paneles con un tono propio que los separa del
/// lienzo sin líneas duras.
pub fn install_style(ctx: &egui::Context) {
    use egui::{CornerRadius, FontFamily::*, FontId, Margin, TextStyle, Theme, Vec2};
    ctx.all_styles_mut(|s| {
        s.text_styles = [
            (TextStyle::Heading, FontId::new(17.0, Proportional)),
            (TextStyle::Body, FontId::new(13.5, Proportional)),
            (TextStyle::Button, FontId::new(13.5, Proportional)),
            (TextStyle::Monospace, FontId::new(12.5, Monospace)),
            (TextStyle::Small, FontId::new(11.0, Proportional)),
        ]
        .into();
        s.spacing.item_spacing = Vec2::new(space::S, 6.0);
        s.spacing.button_padding = Vec2::new(10.0, space::XS);
        s.spacing.interact_size.y = 24.0;
        s.spacing.menu_margin = Margin::same(space::S as i8);
        s.spacing.window_margin = Margin::same(space::M as i8);
        s.spacing.indent = 14.0;
        let r = CornerRadius::same(6);
        for w in [
            &mut s.visuals.widgets.noninteractive,
            &mut s.visuals.widgets.inactive,
            &mut s.visuals.widgets.hovered,
            &mut s.visuals.widgets.active,
            &mut s.visuals.widgets.open,
        ] {
            w.corner_radius = r;
        }
        s.visuals.window_corner_radius = CornerRadius::same(10);
        s.visuals.menu_corner_radius = CornerRadius::same(8);
        s.visuals.indent_has_left_vline = false;
    });
    // Paneles: en oscuro, apenas más claros que el lienzo (el diseño es lo
    // más oscuro, como en KLayout); en claro, apenas más oscuros que el
    // lienzo blanco. La diferencia de tono ya separa las regiones.
    ctx.style_mut_of(Theme::Dark, |s| {
        s.visuals.panel_fill = Color32::from_rgb(27, 27, 31);
        s.visuals.window_fill = Color32::from_rgb(34, 34, 39);
        s.visuals.extreme_bg_color = Color32::from_rgb(17, 17, 20);
        s.visuals.faint_bg_color = Color32::from_rgb(33, 33, 38);
    });
    ctx.style_mut_of(Theme::Light, |s| {
        s.visuals.panel_fill = Color32::from_rgb(242, 242, 238);
        s.visuals.window_fill = Color32::WHITE;
        s.visuals.extreme_bg_color = Color32::WHITE;
        s.visuals.faint_bg_color = Color32::from_rgb(234, 234, 229);
    });
}

/// Paleta del lienzo para el tema activo.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasTheme {
    pub dark: bool,
    /// Fondo del lienzo.
    pub background: Color32,
    /// Fondo de las pastillas de etiqueta (semiopaco, color del lienzo).
    pub label_halo: Color32,
    /// Texto secundario sobre el lienzo ("Escena vacía", avisos).
    pub muted: Color32,
}

impl CanvasTheme {
    pub fn from_visuals(v: &egui::Visuals) -> Self {
        if v.dark_mode {
            Self {
                dark: true,
                background: Color32::from_rgb(20, 20, 24),
                label_halo: Color32::from_rgba_unmultiplied(16, 16, 20, 215),
                muted: Color32::from_gray(160),
            }
        } else {
            Self {
                dark: false,
                background: Color32::from_rgb(250, 250, 247),
                label_halo: Color32::from_rgba_unmultiplied(255, 255, 255, 225),
                muted: Color32::from_gray(90),
            }
        }
    }

    /// Relleno y contorno de una capa ajustados al tema.
    pub fn layer_colors(&self, fill: Color32, stroke: Color32) -> (Color32, Color32) {
        if self.dark {
            return (fill, stroke);
        }
        // Sobre blanco: contorno ~35 % más oscuro y relleno hasta 1.6× más
        // opaco (sin tocar las capas de solo contorno, que tienen alfa 0).
        let alpha = if fill.a() == 0 { 0 } else { (fill.a() as f32 * 1.6).min(200.0) as u8 };
        let [r, g, b, _] = fill.to_srgba_unmultiplied();
        (Color32::from_rgba_unmultiplied(r, g, b, alpha), mix(stroke, Color32::BLACK, 0.35))
    }

    /// Color de texto legible derivado del color de su capa: más claro en
    /// tema oscuro, más oscuro en tema claro (siempre sobre `label_halo`).
    pub fn label_text(&self, layer_color: Color32) -> Color32 {
        let [r, g, b, _] = layer_color.to_srgba_unmultiplied();
        let opaque = Color32::from_rgb(r, g, b);
        if self.dark {
            mix(opaque, Color32::WHITE, 0.45)
        } else {
            mix(opaque, Color32::BLACK, 0.55)
        }
    }
}

/// Color de un tipo de cambio (texto de listas y marcadores): los mismos
/// matices que el overlay del diff, oscurecidos en tema claro para leerse
/// sobre blanco.
pub fn change_color(kind: viewer_core::diff::ChangeKind, dark: bool) -> Color32 {
    use viewer_core::diff::ChangeKind as K;
    let c = match kind {
        K::Added => Color32::from_rgb(90, 220, 120),
        K::Removed => Color32::from_rgb(240, 100, 100),
        K::Modified => Color32::from_rgb(230, 190, 80),
    };
    if dark {
        c
    } else {
        mix(c, Color32::BLACK, 0.45)
    }
}

/// Interpolación lineal por canal en sRGB (suficiente para ajustes de UI).
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let [ar, ag, ab, aa] = a.to_srgba_unmultiplied();
    let [br, bg, bb, _] = b.to_srgba_unmultiplied();
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_unmultiplied(l(ar, br), l(ag, bg), l(ab, bb), aa)
}

/// Luminancia relativa (WCAG) de un color opaco. Solo la usan los tests de
/// contraste (el tema se calibra con ellos, no en runtime).
#[cfg(test)]
pub fn luminance(c: Color32) -> f32 {
    let [r, g, b, _] = c.to_srgba_unmultiplied();
    let lin = |v: u8| {
        let s = v as f32 / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
}

/// Contraste WCAG entre dos colores opacos (1 = nulo, 21 = máximo).
#[cfg(test)]
pub fn contrast(a: Color32, b: Color32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn themes() -> [CanvasTheme; 2] {
        [CanvasTheme::from_visuals(&egui::Visuals::dark()), CanvasTheme::from_visuals(&egui::Visuals::light())]
    }

    /// Los colores de capa de los PDKs soportados, incluidos los más
    /// problemáticos (amarillo de GF180 en claro, azul oscuro en oscuro).
    const LAYER_COLORS: [(u8, u8, u8); 8] = [
        (160, 110, 230),    // sky130 li1
        (60, 130, 240),     // sky130 met1
        (230, 50, 50),      // sky130 poly
        (0xed, 0xdd, 0x07), // gf180 Metal1
        (0x2e, 0x95, 0x21), // gf180 Poly2
        (0x39, 0xbf, 0xff), // ihp Metal1
        (0x80, 0x31, 0x7c), // gf180 Metal4
        (220, 220, 160),    // texto
    ];

    #[test]
    fn label_text_meets_wcag_aa_on_its_halo() {
        for t in themes() {
            // El halo es casi opaco: se evalúa contra el fondo del lienzo.
            for (r, g, b) in LAYER_COLORS {
                let text = t.label_text(Color32::from_rgb(r, g, b));
                let c = contrast(text, t.background);
                assert!(c >= 4.5, "tema dark={} capa ({r},{g},{b}): contraste {c:.2}", t.dark);
            }
        }
    }

    #[test]
    fn light_theme_darkens_strokes_and_keeps_outline_layers_transparent() {
        let [dark, light] = themes();
        let stroke = Color32::from_rgb(0xed, 0xdd, 0x07);
        let fill = Color32::from_rgba_unmultiplied(0xed, 0xdd, 0x07, 90);
        assert_eq!(dark.layer_colors(fill, stroke), (fill, stroke));
        let (f, s) = light.layer_colors(fill, stroke);
        assert!(luminance(s) < luminance(stroke));
        assert!(f.a() > fill.a());
        let (f0, _) = light.layer_colors(Color32::TRANSPARENT, stroke);
        assert_eq!(f0.a(), 0);
    }
}
