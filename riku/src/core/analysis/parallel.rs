//! Reparto de `log`, `show` y `status` entre los hilos del pool de `rayon`.
//!
//! Cada unidad (un commit, un archivo) se analiza con su propia conexión a
//! Git: `git2::Repository` se puede mover entre hilos pero no compartir (ver
//! [`GitRepository::reopener`]). El orden de la salida es el de entrada.
//!
//! **Memoria sin bloquear hilos.** Varios diffs de layouts a la vez son
//! varias librerías cargadas (~12 veces el tamaño del archivo cada una). En
//! vez de un semáforo, que con `rayon` anidado (el diff de un layout reparte
//! sus propias tareas) puede trabar a un hilo que espera cupo detrás de otro
//! que lo tiene, el trabajo se planifica antes: cada unidad trae su costo
//! estimado (por el tamaño de sus blobs, leído sin cargarlos) y se arman
//! tandas consecutivas que caben en la mitad de la memoria disponible. Cada
//! tanda corre en paralelo; las tandas, una detrás de otra. Una unidad más
//! grande que todo el cupo va sola en su tanda.

use std::ops::Range;

use rayon::prelude::*;

use crate::core::domain::git_types::GitError;
use crate::core::domain::ports::{GitRepository, Reopener};

/// Memoria estimada de un diff por byte de archivo (A + B). Medido: un
/// layout de 42 MB usa ~0,5 GB por diff.
pub(crate) const BYTES_PER_INPUT_BYTE: u64 = 12;

/// Costo estimado de diffear dos versiones que pesan `a` y `b` bytes.
pub(crate) fn diff_cost(a: Option<u64>, b: Option<u64>) -> u64 {
    (a.unwrap_or(0) + b.unwrap_or(0)) * BYTES_PER_INPUT_BYTE
}

/// `MemAvailable` de `/proc/meminfo`; 4 GiB si no se puede leer.
fn mem_available() -> u64 {
    std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|s| {
            let line = s.lines().find(|l| l.starts_with("MemAvailable:"))?;
            line.split_whitespace().nth(1)?.parse::<u64>().ok()
        })
        .map_or(4 << 30, |kb| kb * 1024)
}

/// Parte `costs` en tandas consecutivas cuya suma no pasa de `budget`. Una
/// unidad que sola ya lo pasa va en su propia tanda.
pub(crate) fn waves(costs: &[u64], budget: u64) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let (mut start, mut sum) = (0, 0u64);
    for (i, &c) in costs.iter().enumerate() {
        if i > start && sum.saturating_add(c) > budget {
            out.push(start..i);
            start = i;
            sum = 0;
        }
        sum = sum.saturating_add(c);
    }
    if start < costs.len() {
        out.push(start..costs.len());
    }
    out
}

/// Aplica `each` a cada unidad con una conexión propia por hilo, en tandas
/// que caben en memoria, y devuelve los resultados en el orden de `items`.
///
/// Sin forma de abrir otra conexión (mocks), con un solo hilo o una sola
/// unidad, usa `sequential` con la conexión de quien llama. Si abrir una
/// conexión falla en un hilo, esa unidad pasa por `failed`.
pub(crate) fn map_in_waves<T, O>(
    reopener: Option<Reopener>,
    items: Vec<T>,
    costs: &[u64],
    mut sequential: impl FnMut(T) -> O,
    each: impl Fn(&dyn GitRepository, T) -> O + Sync,
    failed: impl Fn(T, &GitError) -> O + Sync,
) -> Vec<O>
where
    T: Send,
    O: Send,
{
    let open = match reopener {
        Some(open) if items.len() > 1 && rayon::current_num_threads() > 1 => open,
        _ => return items.into_iter().map(&mut sequential).collect(),
    };
    let budget = (mem_available() / 2).max(1);
    let plan = waves(costs, budget);
    let mut items: Vec<Option<T>> = items.into_iter().map(Some).collect();
    let mut out = Vec::with_capacity(items.len());
    for range in plan {
        let wave: Vec<T> = items[range].iter_mut().filter_map(Option::take).collect();
        let done: Vec<O> = wave
            .into_par_iter()
            .map_init(
                || open(),
                |conn, item| match conn {
                    Ok(repo) => each(repo.as_ref(), item),
                    Err(e) => failed(item, e),
                },
            )
            .collect();
        out.extend(done);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waves_fit_the_budget_and_keep_order() {
        assert_eq!(waves(&[3, 3, 3, 3], 7), vec![0..2, 2..4]);
        // Una unidad más grande que el cupo va sola.
        assert_eq!(waves(&[1, 10, 1], 5), vec![0..1, 1..2, 2..3]);
        // Lo chico (costo 0) entra todo junto.
        assert_eq!(waves(&[0, 0, 0], 5), vec![0..3]);
        assert!(waves(&[], 5).is_empty());
    }
}
