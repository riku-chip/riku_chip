//! La transformación de una instancia en una jerarquía Manhattan: reflejo en
//! X, rotación en múltiplos de 90° y traslación, exacta (sin senos ni
//! cosenos: un 90° con `sin_cos` deja restos de 1e-17 que rompen las
//! comparaciones).

use gdstk_rs::{OwnedPolygon, Point2D, Reference};

/// `p' = R(rot·90°) · M · p + (dx, dy)`, con `M` el reflejo en X (`y → -y`).
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Xf {
    pub rot: u8,
    pub mirror: bool,
    pub dx: f64,
    pub dy: f64,
}

impl Xf {
    pub const IDENTITY: Self = Self { rot: 0, mirror: false, dx: 0.0, dy: 0.0 };

    /// La de una referencia (más el desplazamiento de su repetición), o
    /// `None` si no es Manhattan o tiene magnificación (se aplana).
    pub fn of(r: &Reference<'_>, offset: Point2D) -> Option<Self> {
        if (r.magnification() - 1.0).abs() > 1e-12 {
            return None;
        }
        let q = r.rotation() / std::f64::consts::FRAC_PI_2;
        let k = q.round();
        if (q - k).abs() > 1e-9 {
            return None;
        }
        let o = r.origin();
        Some(Self { rot: (k as i64).rem_euclid(4) as u8, mirror: r.x_reflection(), dx: o.x + offset.x, dy: o.y + offset.y })
    }

    fn lin(&self, x: f64, y: f64) -> (f64, f64) {
        let y = if self.mirror { -y } else { y };
        match self.rot {
            0 => (x, y),
            1 => (-y, x),
            2 => (-x, -y),
            _ => (y, -x),
        }
    }

    pub fn apply(&self, (x, y): (f64, f64)) -> (f64, f64) {
        let (x, y) = self.lin(x, y);
        (x + self.dx, y + self.dy)
    }

    pub fn point(&self, p: Point2D) -> Point2D {
        let (x, y) = self.apply((p.x, p.y));
        Point2D { x, y }
    }

    pub fn bbox(&self, b: &[f64; 4]) -> [f64; 4] {
        let (x0, y0) = self.apply((b[0], b[1]));
        let (x1, y1) = self.apply((b[2], b[3]));
        [x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)]
    }

    pub fn poly(&self, p: &OwnedPolygon) -> OwnedPolygon {
        OwnedPolygon { layer: p.layer, datatype: p.datatype, points: p.points.iter().map(|&q| self.point(q)).collect() }
    }

    /// `self ∘ inner`: primero `inner`, después `self`.
    pub fn then(&self, inner: &Self) -> Self {
        let (dx, dy) = self.apply((inner.dx, inner.dy));
        // R(a) M^m · R(b) M^n: con reflejo, M R(b) = R(-b) M.
        let (rot, mirror) = if self.mirror {
            ((self.rot + 4 - inner.rot) % 4, !inner.mirror)
        } else {
            ((self.rot + inner.rot) % 4, inner.mirror)
        };
        Self { rot, mirror, dx, dy }
    }

    /// La inversa.
    pub fn inverse(&self) -> Self {
        // p = M R⁻¹ (p' - t): sin reflejo, R(-a); con reflejo, M R(-a) = R(a) M.
        let (rot, mirror) = if self.mirror { (self.rot, true) } else { ((4 - self.rot) % 4, false) };
        let lin = Self { rot, mirror, dx: 0.0, dy: 0.0 };
        let (dx, dy) = lin.apply((-self.dx, -self.dy));
        Self { rot, mirror, dx, dy }
    }

    /// Clave entera (para memorizar vecindades): la transformación en la
    /// grilla `grid` (unidades por paso).
    pub fn key(&self, grid: f64) -> (u8, bool, i64, i64) {
        (self.rot, self.mirror, (self.dx / grid).round() as i64, (self.dy / grid).round() as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all() -> Vec<Xf> {
        let mut v = Vec::new();
        for rot in 0..4 {
            for mirror in [false, true] {
                v.push(Xf { rot, mirror, dx: 3.0, dy: -7.0 });
            }
        }
        v
    }

    #[test]
    fn composition_and_inverse_agree_with_applying_in_order() {
        let p = (1.5, 2.25);
        for a in all() {
            for b in all() {
                let ab = a.then(&b);
                assert_eq!(ab.apply(p), a.apply(b.apply(p)), "{a:?} ∘ {b:?}");
            }
            assert_eq!(a.inverse().apply(a.apply(p)), p, "{a:?}");
            assert_eq!(a.apply(a.inverse().apply(p)), p, "{a:?}");
        }
    }

    #[test]
    fn matches_the_gds_order_reflection_then_rotation() {
        // Como `labels::Affine::reference`: reflejo, rotación, traslación.
        let x = Xf { rot: 1, mirror: true, dx: 10.0, dy: 0.0 };
        // (1, 2) → reflejo (1, -2) → 90° (2, 1) → +10.
        assert_eq!(x.apply((1.0, 2.0)), (12.0, 1.0));
    }
}
