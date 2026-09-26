//! Mensajes temporales ("toasts") en la esquina inferior derecha.
//!
//! Cuatro tipos de feedback, cada uno con su duración:
//! - **Info** (estado) y **Éxito** (completado): se van solos en 2.5 s.
//! - **Aviso**: 5 s, hay algo que conviene saber pero no bloquea.
//! - **Error**: queda hasta que se cierra, para no perder el mensaje.
//!
//! Se desvanecen al final en vez de desaparecer de golpe. Mensajes iguales
//! seguidos no se apilan: se renueva el existente.

use eframe::egui::{self, Align2, Color32, RichText, Stroke};

use crate::gui::theme::{mix, space};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Warning,
    Error,
}

impl ToastKind {
    /// Segundos visibles; `None` = hasta que el usuario lo cierre.
    fn ttl(self) -> Option<f64> {
        match self {
            Self::Info | Self::Success => Some(2.5),
            Self::Warning => Some(5.0),
            Self::Error => None,
        }
    }

    fn accent(self, dark: bool) -> Color32 {
        let c = match self {
            Self::Info => Color32::from_rgb(90, 160, 240),
            Self::Success => Color32::from_rgb(80, 200, 120),
            Self::Warning => Color32::from_rgb(235, 180, 60),
            Self::Error => Color32::from_rgb(235, 90, 90),
        };
        if dark { c } else { mix(c, Color32::BLACK, 0.25) }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Toast {
    kind: ToastKind,
    text: String,
    born: f64,
}

/// Duración del desvanecimiento final (s).
const FADE: f64 = 0.4;
/// Como máximo se ven los últimos N (los más viejos ceden su lugar).
const MAX_VISIBLE: usize = 4;

#[derive(Default)]
pub struct Toasts {
    items: Vec<Toast>,
}

impl Toasts {
    pub fn push(&mut self, kind: ToastKind, text: impl Into<String>, now: f64) {
        let text = text.into();
        if let Some(t) = self.items.iter_mut().find(|t| t.kind == kind && t.text == text) {
            t.born = now;
            return;
        }
        self.items.push(Toast { kind, text, born: now });
        if self.items.len() > MAX_VISIBLE {
            self.items.remove(0);
        }
    }

    /// Quita los vencidos. Separado de `show` para poder testearlo.
    fn expire(&mut self, now: f64) {
        self.items.retain(|t| t.kind.ttl().is_none_or(|ttl| now - t.born < ttl));
    }

    /// Opacidad de un toast (1 → 0 durante el último `FADE`).
    fn opacity(t: &Toast, now: f64) -> f32 {
        match t.kind.ttl() {
            Some(ttl) => (((ttl - (now - t.born)) / FADE).clamp(0.0, 1.0)) as f32,
            None => 1.0,
        }
    }

    /// Dibuja los mensajes en la esquina inferior derecha de `area` (el
    /// lienzo): así no tapan los paneles laterales.
    pub fn show(&mut self, ctx: &egui::Context, area: egui::Rect) {
        let now = ctx.input(|i| i.time);
        self.expire(now);
        if self.items.is_empty() {
            return;
        }
        let dark = ctx.global_style().visuals.dark_mode;
        let mut close = None;
        egui::Area::new(egui::Id::new("riku_toasts"))
            .pivot(Align2::RIGHT_BOTTOM)
            .fixed_pos(area.right_bottom() - egui::vec2(space::L, space::L))
            .order(egui::Order::Foreground)
            .interactable(true)
            .show(ctx, |ui| {
                for (i, t) in self.items.iter().enumerate() {
                    ui.set_opacity(Self::opacity(t, now));
                    let accent = t.kind.accent(dark);
                    egui::Frame::popup(ui.style())
                        .stroke(Stroke::new(1.0_f32, accent.gamma_multiply(0.6)))
                        .inner_margin(egui::Margin::symmetric(space::M as i8, space::S as i8))
                        .show(ui, |ui| {
                            ui.set_max_width(360.0);
                            ui.horizontal(|ui| {
                                let (bar, _) = ui.allocate_exact_size(egui::vec2(3.0, 18.0), egui::Sense::hover());
                                ui.painter().rect_filled(bar, 2.0, accent);
                                ui.add(egui::Label::new(RichText::new(&t.text)).wrap());
                                if t.kind == ToastKind::Error && ui.small_button("✕").on_hover_text("Cerrar").clicked() {
                                    close = Some(i);
                                }
                            });
                        });
                    ui.add_space(space::XS);
                }
            });
        if let Some(i) = close {
            self.items.remove(i);
        }
        // Mantener vivo el desvanecimiento y el vencimiento.
        if self.items.iter().any(|t| t.kind.ttl().is_some()) {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transient_toasts_expire_but_errors_stay() {
        let mut t = Toasts::default();
        t.push(ToastKind::Success, "Celda cargada", 0.0);
        t.push(ToastKind::Error, "No se pudo abrir", 0.0);
        t.expire(3.0);
        let kinds: Vec<_> = t.items.iter().map(|x| x.kind).collect();
        assert_eq!(kinds, vec![ToastKind::Error]);
    }

    #[test]
    fn repeated_message_refreshes_instead_of_stacking() {
        let mut t = Toasts::default();
        t.push(ToastKind::Info, "Etiquetas ocultas", 0.0);
        t.push(ToastKind::Info, "Etiquetas ocultas", 2.0);
        assert_eq!(t.items.len(), 1);
        t.expire(3.0);
        assert_eq!(t.items.len(), 1, "se renovó en t=2");
    }

    #[test]
    fn fades_out_at_the_end() {
        let toast = Toast { kind: ToastKind::Info, text: String::new(), born: 0.0 };
        assert_eq!(Toasts::opacity(&toast, 1.0), 1.0);
        assert!(Toasts::opacity(&toast, 2.3) < 1.0 && Toasts::opacity(&toast, 2.3) > 0.0);
    }

    #[test]
    fn keeps_only_the_latest_few() {
        let mut t = Toasts::default();
        for i in 0..6 {
            t.push(ToastKind::Error, format!("e{i}"), 0.0);
        }
        let texts: Vec<_> = t.items.iter().map(|x| x.text.as_str()).collect();
        assert_eq!(texts, vec!["e2", "e3", "e4", "e5"]);
    }
}
