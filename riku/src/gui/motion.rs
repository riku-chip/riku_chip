//! Movimiento de la vista: springs interrumpibles e inercia al soltar.
//!
//! Criterios (guía *Designing Fluid Interfaces*, WWDC 2018):
//! - **Springs críticamente amortiguados** (sin rebote) para los cambios de
//!   vista que pide la UI (Encuadrar, ir a un cambio): parten del valor en
//!   pantalla, así que cualquier gesto del usuario los interrumpe sin saltos.
//! - **Inercia al soltar un arrastre**: la vista sigue a la velocidad del
//!   puntero y desacelera con decaimiento exponencial (tasa 0.998 por ms,
//!   la del scroll de iOS), de modo que el recorrido total es el que da la
//!   fórmula de proyección de Apple.
//!
//! La vista se anima en (centro en el mundo, ln de la escala): interpolar
//! `pan` y `scale` por separado haría que el zoom pivote en un punto errado;
//! la escala en log hace que acercar ×2 y alejar ×2 se sientan simétricos.

use viewer_core::viewport::Viewport;

/// Tiempo de respuesta del spring de vista (s): rápido pero legible.
pub const VIEW_RESPONSE: f64 = 0.3;
/// Tasa de desaceleración por milisegundo (scroll "normal" de iOS).
pub const DECELERATION: f64 = 0.998;
/// Por debajo de esta velocidad (px/s) la inercia se detiene.
const INERTIA_MIN_SPEED: f64 = 8.0;
/// Velocidad mínima al soltar para que haya inercia (un arrastre lento y
/// soltado quieto no debe "patinar").
pub const INERTIA_START_SPEED: f64 = 60.0;

/// Un paso de un spring críticamente amortiguado (solución exacta, estable
/// con cualquier `dt`). Retorna la nueva posición y velocidad.
pub fn spring_step(x: f64, v: f64, target: f64, dt: f64, response: f64) -> (f64, f64) {
    let omega = std::f64::consts::TAU / response;
    let delta = x - target;
    let c = v + omega * delta;
    let e = (-omega * dt).exp();
    let x_new = target + (delta + c * dt) * e;
    let v_new = (c - omega * (delta + c * dt)) * e;
    (x_new, v_new)
}

/// Recorrido total de una inercia que arranca a `v` px/s (proyección de Apple).
/// `Inertia` lo reproduce integrando el decaimiento; los tests lo comparan.
#[cfg(test)]
pub fn projected_travel(v: f64) -> f64 {
    v / 1000.0 * DECELERATION / (1.0 - DECELERATION)
}

/// Estado de la vista en coordenadas "de cámara": centro en el espacio de
/// vista del `Viewport` (sin offset de panel) y logaritmo de la escala.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Camera {
    cx: f64,
    cy: f64,
    ln_scale: f64,
}

impl Camera {
    fn of(vp: &Viewport, w: f64, h: f64) -> Self {
        Self { cx: (w * 0.5 - vp.pan_x) / vp.scale, cy: (h * 0.5 - vp.pan_y) / vp.scale, ln_scale: vp.scale.ln() }
    }

    fn viewport(&self, w: f64, h: f64) -> Viewport {
        let scale = self.ln_scale.exp();
        Viewport { pan_x: w * 0.5 - self.cx * scale, pan_y: h * 0.5 - self.cy * scale, scale }
    }
}

/// Animación de la vista hacia `target` (Encuadrar, ir a un cambio).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewAnimation {
    target: Viewport,
    /// Velocidad de (cx, cy, ln_scale).
    vel: [f64; 3],
}

impl ViewAnimation {
    pub fn to(target: Viewport) -> Self {
        Self { target, vel: [0.0; 3] }
    }

    /// Destino de la animación (para encadenar pedidos, p.ej. varios "+").
    pub fn target(&self) -> Viewport {
        self.target
    }

    /// Avanza `dt` segundos sobre `vp` (que es el valor en pantalla: si el
    /// usuario movió la vista, la animación sigue desde ahí). Retorna `true`
    /// al llegar; entonces `vp` queda exactamente en el destino.
    pub fn step(&mut self, vp: &mut Viewport, dt: f64, w: f64, h: f64) -> bool {
        let cur = Camera::of(vp, w, h);
        let dst = Camera::of(&self.target, w, h);
        let (cx, vx) = spring_step(cur.cx, self.vel[0], dst.cx, dt, VIEW_RESPONSE);
        let (cy, vy) = spring_step(cur.cy, self.vel[1], dst.cy, dt, VIEW_RESPONSE);
        let (ls, vs) = spring_step(cur.ln_scale, self.vel[2], dst.ln_scale, dt, VIEW_RESPONSE);
        self.vel = [vx, vy, vs];

        // Llegó cuando el error en pantalla es < 0.5 px y la escala < 0.1 %.
        let scale = ls.exp();
        let px_err = ((cx - dst.cx).abs() + (cy - dst.cy).abs()) * scale;
        let done = px_err < 0.5 && (ls - dst.ln_scale).abs() < 1e-3 && vs.abs() < 1e-2;
        *vp = if done { self.target } else { Camera { cx, cy, ln_scale: ls }.viewport(w, h) };
        done
    }
}

/// Duración del fundido al cambiar de tema (s).
pub const THEME_FADE: f64 = 0.25;

/// Opacidad del velo del tema anterior `t` segundos después del cambio:
/// 1 → 0 con salida suave (ease-out cúbico), para que el salto de brillo
/// claro↔oscuro no sea brusco. `None` cuando terminó.
pub fn theme_fade_alpha(t: f64) -> Option<f32> {
    if !(0.0..THEME_FADE).contains(&t) {
        return None;
    }
    let p = t / THEME_FADE;
    Some((1.0 - (1.0 - (1.0 - p).powi(3))) as f32)
}

/// Inercia del pan tras soltar un arrastre (velocidad en px/s de pantalla).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Inertia {
    pub vx: f64,
    pub vy: f64,
}

impl Inertia {
    /// `None` si el arrastre se soltó casi quieto.
    pub fn from_release(vx: f64, vy: f64) -> Option<Self> {
        (vx.hypot(vy) >= INERTIA_START_SPEED).then_some(Self { vx, vy })
    }

    /// Desplazamiento de este frame (px) y si la inercia sigue viva.
    pub fn step(&mut self, dt: f64) -> ((f64, f64), bool) {
        // Integral exacta del decaimiento v·d^(t_ms) en el intervalo: el
        // recorrido total coincide con `projected_travel` sin importar dt.
        let k = DECELERATION.ln() * 1000.0; // decaimiento por segundo (negativo)
        let decay = (k * dt).exp();
        let factor = (decay - 1.0) / k;
        let d = (self.vx * factor, self.vy * factor);
        self.vx *= decay;
        self.vy *= decay;
        (d, self.vx.hypot(self.vy) >= INERTIA_MIN_SPEED)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn critically_damped_spring_reaches_target_without_overshoot() {
        let (mut x, mut v) = (0.0, 0.0);
        let mut max = f64::MIN;
        for _ in 0..120 {
            (x, v) = spring_step(x, v, 100.0, 1.0 / 60.0, VIEW_RESPONSE);
            max = max.max(x);
        }
        assert!((x - 100.0).abs() < 1e-3, "llega: {x}");
        assert!(max <= 100.0 + 1e-9, "sin rebote: máximo {max}");
    }

    #[test]
    fn spring_is_frame_rate_independent() {
        // Solución exacta: 1 paso de 0.1 s == 6 pasos de 1/60 s.
        let one = spring_step(0.0, 0.0, 1.0, 0.1, VIEW_RESPONSE);
        let mut many = (0.0, 0.0);
        for _ in 0..6 {
            many = spring_step(many.0, many.1, 1.0, 0.1 / 6.0, VIEW_RESPONSE);
        }
        assert!((one.0 - many.0).abs() < 1e-9 && (one.1 - many.1).abs() < 1e-9);
    }

    fn vp(pan_x: f64, pan_y: f64, scale: f64) -> Viewport {
        Viewport { pan_x, pan_y, scale }
    }

    #[test]
    fn view_animation_lands_exactly_on_target() {
        let target = vp(-500.0, 200.0, 8.0);
        let mut cur = vp(100.0, 50.0, 1.0);
        let mut anim = ViewAnimation::to(target);
        let mut frames = 0;
        while !anim.step(&mut cur, 1.0 / 60.0, 800.0, 600.0) {
            frames += 1;
            assert!(frames < 300, "debe terminar");
        }
        assert_eq!(cur, target);
        // ~0.3 s de respuesta: termina en menos de un segundo.
        assert!(frames < 60, "frames {frames}");
    }

    #[test]
    fn view_animation_continues_from_user_moved_view() {
        // Interrupción: si el usuario movió la vista, el siguiente paso parte
        // del valor en pantalla (no salta a donde "iba" la animación).
        let mut cur = vp(0.0, 0.0, 1.0);
        let mut anim = ViewAnimation::to(vp(-1000.0, 0.0, 1.0));
        anim.step(&mut cur, 1.0 / 60.0, 800.0, 600.0);
        cur.pan_x += 300.0; // arrastre del usuario
        let before = cur;
        let mut uninterrupted = before;
        uninterrupted.pan_x -= 300.0;
        anim.step(&mut cur, 1.0 / 60.0, 800.0, 600.0);
        // Avanza desde donde lo dejó el usuario hacia el destino, sin pasarse
        // ni volver a la posición previa al arrastre.
        assert!(cur.pan_x < before.pan_x && cur.pan_x > -1000.0, "{} → {}", before.pan_x, cur.pan_x);
        assert!(cur.pan_x > uninterrupted.pan_x, "no retoma la trayectoria anterior");
    }

    #[test]
    fn inertia_travel_matches_apple_projection() {
        let mut it = Inertia::from_release(1200.0, 0.0).expect("rápido");
        let mut travel = 0.0;
        loop {
            let ((dx, _), alive) = it.step(1.0 / 60.0);
            travel += dx;
            if !alive {
                break;
            }
        }
        let expected = projected_travel(1200.0);
        // Se corta al bajar de 8 px/s: queda un residuo mínimo sin recorrer.
        assert!((travel - expected).abs() < projected_travel(8.0) + 1e-6, "{travel} vs {expected}");
    }

    #[test]
    fn slow_release_has_no_inertia() {
        assert!(Inertia::from_release(20.0, 20.0).is_none());
    }

    #[test]
    fn theme_fade_goes_from_opaque_to_done_monotonically() {
        assert_eq!(theme_fade_alpha(0.0), Some(1.0));
        let mut prev = 1.0;
        for i in 1..25 {
            let a = theme_fade_alpha(i as f64 * 0.01).expect("dentro del fundido");
            assert!(a <= prev, "monótono");
            prev = a;
        }
        assert!(prev < 0.05, "casi transparente al final: {prev}");
        assert_eq!(theme_fade_alpha(THEME_FADE), None);
        assert_eq!(theme_fade_alpha(-0.1), None);
    }
}
