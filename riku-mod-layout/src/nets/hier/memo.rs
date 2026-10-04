//! La memoria del proceso (ver `docs/ronda-5/design.md`, D21): el resumen de
//! cada celda por su [`NetKey`], compartido entre los dos lados de un diff,
//! los commits de un `log` y las celdas que abre el visor; y las vecindades
//! entre dos hijas. Con un tope en bytes estimados: al pasarlo se saca lo
//! que se usó hace más tiempo.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use super::build::CellNets;
use super::key::NetKey;

/// Clave de una vecindad: las dos hijas y la segunda vista desde la primera.
pub(crate) type NeighbourKey = (NetKey, NetKey, (u8, bool, i64, i64));

struct Slot {
    cell: Arc<OnceLock<Arc<CellNets>>>,
    used: u64,
    bytes: u64,
}

pub(crate) struct Memo {
    cells: Mutex<HashMap<NetKey, Slot>>,
    neighbours: Mutex<HashMap<NeighbourKey, Arc<Vec<(u32, u32)>>>>,
    bytes: AtomicU64,
    tick: AtomicU64,
    cap: u64,
}

/// La memoria del proceso. `RIKU_NETS_MEM_MB` cambia el tope (512 MB por
/// omisión; 0 la apaga).
pub(crate) fn memo() -> &'static Memo {
    static MEMO: OnceLock<Memo> = OnceLock::new();
    MEMO.get_or_init(|| {
        let mb: u64 = std::env::var("RIKU_NETS_MEM_MB").ok().and_then(|v| v.parse().ok()).unwrap_or(512);
        Memo {
            cells: Mutex::new(HashMap::new()),
            neighbours: Mutex::new(HashMap::new()),
            bytes: AtomicU64::new(0),
            tick: AtomicU64::new(0),
            cap: mb << 20,
        }
    })
}

impl Memo {
    pub(crate) fn enabled(&self) -> bool {
        self.cap > 0
    }

    /// El resumen de `key`, o el que arma `build` (una sola vez aunque lo
    /// pidan dos hilos a la vez). El `bool` dice si ya estaba.
    pub(crate) fn cell(&self, key: NetKey, build: impl FnOnce() -> CellNets) -> (Arc<CellNets>, bool) {
        if !self.enabled() {
            return (Arc::new(build()), false);
        }
        let now = self.tick.fetch_add(1, Ordering::Relaxed);
        let slot = {
            let mut cells = self.cells.lock().unwrap();
            let s = cells.entry(key).or_insert_with(|| Slot { cell: Arc::new(OnceLock::new()), used: now, bytes: 0 });
            s.used = now;
            s.cell.clone()
        };
        let mut fresh = false;
        let cell = slot
            .get_or_init(|| {
                fresh = true;
                Arc::new(build())
            })
            .clone();
        if fresh {
            let b = cell.bytes();
            if let Some(s) = self.cells.lock().unwrap().get_mut(&key) {
                s.bytes = b;
            }
            if self.bytes.fetch_add(b, Ordering::Relaxed) + b > self.cap {
                self.evict();
            }
        }
        (cell, !fresh)
    }

    /// Saca lo usado hace más tiempo hasta quedar en tres cuartos del tope.
    fn evict(&self) {
        let mut cells = self.cells.lock().unwrap();
        let mut by_age: Vec<(u64, NetKey, u64)> = cells.iter().map(|(k, s)| (s.used, *k, s.bytes)).collect();
        by_age.sort_unstable();
        let mut total = self.bytes.load(Ordering::Relaxed);
        for (_, k, b) in by_age {
            if total <= self.cap / 4 * 3 {
                break;
            }
            cells.remove(&k);
            total = total.saturating_sub(b);
        }
        self.bytes.store(total, Ordering::Relaxed);
        self.neighbours.lock().unwrap().clear();
    }

    pub(crate) fn neighbour(&self, key: &NeighbourKey) -> Option<Arc<Vec<(u32, u32)>>> {
        self.neighbours.lock().unwrap().get(key).cloned()
    }

    pub(crate) fn put_neighbour(&self, key: NeighbourKey, v: Arc<Vec<(u32, u32)>>) {
        if self.enabled() {
            self.neighbours.lock().unwrap().insert(key, v);
        }
    }
}
