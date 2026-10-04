//! Grilla uniforme sobre bboxes: responde rápido qué bboxes tocan una zona
//! o contienen un punto. Con n bboxes, √n × √n celdas (hasta 1024²); cada
//! bbox se anota en las celdas que cubre.

/// Grilla uniforme sobre un conjunto de bboxes para preguntar rápido si otro
/// bbox toca alguno (bordes incluidos).
pub(crate) struct BoxGrid {
    bounds: [f64; 4],
    n: usize,
    cells: Vec<Vec<usize>>,
    boxes: Vec<[f64; 4]>,
}

impl BoxGrid {
    /// Un bbox vacío (mínimo > máximo, el de una celda sin geometría) o no
    /// finito no toca nada y no entra a la grilla: si no, los límites serían
    /// infinitos y todo caería en una sola celda.
    pub(crate) fn new(boxes: &[[f64; 4]]) -> Self {
        let valid = |b: &[f64; 4]| b.iter().all(|v| v.is_finite()) && b[0] <= b[2] && b[1] <= b[3];
        let bounds = boxes
            .iter()
            .filter(|b| valid(b))
            .fold([f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY], |b, x| {
                [b[0].min(x[0]), b[1].min(x[1]), b[2].max(x[2]), b[3].max(x[3])]
            });
        let n = ((boxes.len() as f64).sqrt().ceil() as usize).clamp(1, 1024);
        let mut grid = Self { bounds, n, cells: vec![Vec::new(); n * n], boxes: boxes.to_vec() };
        for (k, b) in boxes.iter().enumerate().filter(|(_, b)| valid(b)) {
            let (x0, y0, x1, y1) = grid.span(b);
            for gy in y0..=y1 {
                for gx in x0..=x1 {
                    grid.cells[gy * n + gx].push(k);
                }
            }
        }
        grid
    }

    /// Rango de celdas de la grilla que cubre `b` (recortado a la grilla).
    fn span(&self, b: &[f64; 4]) -> (usize, usize, usize, usize) {
        let [mx, my, xx, xy] = self.bounds;
        let w = (xx - mx).max(f64::MIN_POSITIVE);
        let h = (xy - my).max(f64::MIN_POSITIVE);
        let cell = |v: f64, lo: f64, len: f64| (((v - lo) / len * self.n as f64).floor().max(0.0) as usize).min(self.n - 1);
        (cell(b[0], mx, w), cell(b[1], my, h), cell(b[2], mx, w), cell(b[3], my, h))
    }

    pub(crate) fn touches(&self, b: [f64; 4]) -> bool {
        let [mx, my, xx, xy] = self.bounds;
        if b[2] < mx || b[0] > xx || b[3] < my || b[1] > xy {
            return false;
        }
        let (x0, y0, x1, y1) = self.span(&b);
        (y0..=y1).any(|gy| {
            (x0..=x1).any(|gx| {
                self.cells[gy * self.n + gx].iter().any(|&k| {
                    let t = &self.boxes[k];
                    b[0] <= t[2] && b[2] >= t[0] && b[1] <= t[3] && b[3] >= t[1]
                })
            })
        })
    }

    /// Índices de los bboxes que contienen el punto (bordes incluidos): solo
    /// se miran los de su celda de la grilla.
    pub(crate) fn containing(&self, x: f64, y: f64) -> impl Iterator<Item = usize> + '_ {
        let [mx, my, xx, xy] = self.bounds;
        let inside = x >= mx && x <= xx && y >= my && y <= xy;
        let list: &[usize] = if inside {
            let (gx, gy, ..) = self.span(&[x, y, x, y]);
            &self.cells[gy * self.n + gx]
        } else {
            &[]
        };
        list.iter().copied().filter(move |&k| {
            let t = &self.boxes[k];
            x >= t[0] && x <= t[2] && y >= t[1] && y <= t[3]
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn containing_matches_brute_force() {
        // Bboxes pseudoaleatorios (LCG): chicos, grandes, anidados.
        let mut seed = 7u64;
        let mut rnd = move || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) as f64 / (1u64 << 31) as f64
        };
        let mut boxes: Vec<[f64; 4]> = (0..500)
            .map(|_| {
                let (x, y, w, h) = (rnd() * 100.0, rnd() * 100.0, rnd() * 20.0, rnd() * 20.0);
                [x, y, x + w, y + h]
            })
            .collect();
        boxes.push([0.0, 0.0, 100.0, 100.0]);
        boxes.push([f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY]);
        let grid = BoxGrid::new(&boxes);
        for _ in 0..2000 {
            let (x, y) = (rnd() * 130.0 - 10.0, rnd() * 130.0 - 10.0);
            let mut got: Vec<usize> = grid.containing(x, y).collect();
            got.sort_unstable();
            let want: Vec<usize> = (0..boxes.len())
                .filter(|&k| {
                    let t = &boxes[k];
                    x >= t[0] && x <= t[2] && y >= t[1] && y <= t[3]
                })
                .collect();
            assert_eq!(got, want, "({x}, {y})");
        }
    }
}
