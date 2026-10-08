//! La memoria del proceso (ver `docs/dev/design-notes.md`, D21): el resumen de
//! cada celda por su [`NetKey`], compartido entre los dos lados de un diff,
//! los commits de un `log` y las celdas que abre el visor; y las vecindades
//! entre dos hijas. Con un tope en bytes estimados: al pasarlo se saca lo
//! que se usó hace más tiempo.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use super::build::CellNets;
use super::disk::Disk;
use super::key::NetKey;

/// Una celda que tardó menos que esto en armarse no se guarda en disco:
/// leerla costaría lo mismo.
const DISK_MIN_MS: u64 = 20;

/// Cuánto espera un hilo a que otro termine de armar la misma celda antes
/// de armarla él (así nunca se traba: ver [`Memo::cell`]).
const WAIT: std::time::Duration = std::time::Duration::from_secs(5);

/// Lo propio de una celda extraído y sus candidatos al sustrato.
pub(crate) type OwnPart = (Arc<crate::nets::Netlist>, Vec<(usize, (f64, f64))>);

/// Clave de una vecindad: las dos hijas y la segunda vista desde la primera.
pub(crate) type NeighbourKey = (NetKey, NetKey, (u8, bool, i64, i64));

struct Slot {
    cell: Arc<CellNets>,
    used: u64,
    bytes: u64,
}

pub(crate) struct Memo {
    cells: Mutex<HashMap<NetKey, Slot>>,
    /// Las que algún hilo está armando, y cuál.
    building: Mutex<HashMap<NetKey, std::thread::ThreadId>>,
    neighbours: Mutex<HashMap<NeighbourKey, Arc<Vec<(u32, u32)>>>>,
    /// Lo propio de cada celda extraído (ver `key::own_key`).
    own: Mutex<HashMap<NetKey, Arc<OwnPart>>>,
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
            building: Mutex::new(HashMap::new()),
            neighbours: Mutex::new(HashMap::new()),
            own: Mutex::new(HashMap::new()),
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

    /// El resumen de `key`, o el que arma `build`. El `bool` dice si ya
    /// estaba (en memoria o en disco).
    ///
    /// Si otro hilo la está armando, se espera sin bloquear el pool de
    /// `rayon`: mientras, se hacen otras tareas (`yield_now`). Un bloqueo de
    /// verdad podría trabarse: el hilo que espera puede robar una tarea que
    /// pide lo que él mismo arma. Por eso, si el que la arma es este mismo
    /// hilo, o la espera pasa de [`WAIT`], se arma de nuevo (y queda la
    /// primera).
    pub(crate) fn cell(&self, key: NetKey, disk: Option<&Disk>, build: impl FnOnce() -> CellNets) -> (Arc<CellNets>, bool) {
        // Del disco, o armada (y guardada si tardó).
        let make = || -> (CellNets, bool) {
            if let Some(c) = disk.and_then(|d| d.load(key)) {
                return (c, true);
            }
            let c = build();
            if let Some(d) = disk.filter(|_| c.build_ms >= DISK_MIN_MS) {
                d.store(key, &c);
            }
            (c, false)
        };
        if !self.enabled() {
            let (c, known) = make();
            return (Arc::new(c), known);
        }
        let now = self.tick.fetch_add(1, Ordering::Relaxed);
        let me = std::thread::current().id();
        let started = std::time::Instant::now();
        loop {
            if let Some(s) = self.cells.lock().unwrap().get_mut(&key) {
                s.used = now;
                return (s.cell.clone(), true);
            }
            let mut building = self.building.lock().unwrap();
            match building.get(&key) {
                Some(&who) if who != me && started.elapsed() < WAIT => {
                    drop(building);
                    if !matches!(rayon::yield_now(), Some(rayon::Yield::Executed)) {
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                }
                _ => {
                    building.insert(key, me);
                    break;
                }
            }
        }
        let (built, from_disk) = make();
        let built = Arc::new(built);
        let b = built.bytes();
        let (cell, fresh) = {
            let mut cells = self.cells.lock().unwrap();
            match cells.get(&key) {
                Some(s) => (s.cell.clone(), false),
                None => {
                    cells.insert(key, Slot { cell: built.clone(), used: now, bytes: b });
                    (built, true)
                }
            }
        };
        // Recién ahora (ya está en `cells`): quien espere la encuentra.
        {
            let mut building = self.building.lock().unwrap();
            if building.get(&key) == Some(&me) {
                building.remove(&key);
            }
        }
        if fresh && self.bytes.fetch_add(b, Ordering::Relaxed) + b > self.cap {
            self.evict();
        }
        (cell, from_disk)
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
        self.own.lock().unwrap().clear();
    }

    /// Si ya está el resumen de esa huella.
    pub(crate) fn has(&self, key: NetKey) -> bool {
        self.cells.lock().unwrap().contains_key(&key)
    }

    /// Lo propio de una celda con esa huella, o lo que arma `make`. Se guarda
    /// solo mientras haya lugar (es lo primero que se tira al pasar el tope).
    pub(crate) fn own(&self, key: NetKey, make: impl FnOnce() -> Arc<OwnPart>) -> Arc<OwnPart> {
        if let Some(p) = self.own.lock().unwrap().get(&key) {
            return p.clone();
        }
        let p = make();
        if self.enabled() {
            self.own.lock().unwrap().insert(key, p.clone());
        }
        p
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
