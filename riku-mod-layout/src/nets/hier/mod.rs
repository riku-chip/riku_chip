//! Extracción jerárquica de redes y transistores, con memoria por huella
//! (ver `docs/dev/design-notes.md`).

mod build;
mod check;
mod compare;
mod disk;
mod key;
mod memo;
mod touch;
pub mod xf;

pub use build::{extract, flatten, locate, CellNets, Cells, Extractor, HierNets, Inst, Options, Stats};
pub use check::same_netlist;
pub use compare::cell_changes;
pub use disk::Disk;
pub use key::NetKey;

use crate::devices::{DeviceChange, DeviceRules};
use crate::nets::NetChange;

/// Las dos versiones de una librería, listas para extraer por celdas.
pub struct Pair<'a> {
    xa: Extractor<'a>,
    xb: Extractor<'a>,
    unit_um: f64,
}

impl<'a> Pair<'a> {
    pub fn new(
        (la, lb): (&'a gdstk_rs::Library, &'a gdstk_rs::Library),
        (ia, ib): (Option<&'a gdstk_rs::magic::MagInfo>, Option<&'a gdstk_rs::magic::MagInfo>),
        rules: &'a DeviceRules,
        disk: Option<Disk>,
    ) -> Self {
        let opts = Options::default();
        let (xa, xb) = rayon::join(
            || Extractor::new(la, rules, ia, opts).with_disk(disk.clone()),
            || Extractor::new(lb, rules, ib, opts).with_disk(disk.clone()),
        );
        Pair { xa, xb, unit_um: lb.unit() / 1e-6 }
    }

    /// Si la extracción de `name` ya está hecha en los dos lados (en la
    /// memoria del proceso o en disco): compararla es barato.
    pub fn cached(&self, name: &str) -> bool {
        self.xa.cached(name) && self.xb.cached(name)
    }

    /// Lo que cambió en cada celda pedida, por celdas: sus abiertos, cortos
    /// y renombres, y sus transistores propios. `cells`: el nombre y las
    /// cajas (µm) de sus cambios de geometría. Las celdas con la misma
    /// huella en los dos lados no se comparan; las que faltan de un lado,
    /// tampoco.
    pub fn changes(&self, cells: &[(String, Vec<[f64; 4]>)]) -> Vec<(String, Vec<NetChange>, Vec<DeviceChange>)> {
        let profile = std::env::var_os("RIKU_PROFILE").is_some();
        let mut out = Vec::new();
        for (name, boxes) in cells {
            let (Some(ka), Some(kb)) = (self.xa.key(name), self.xb.key(name)) else { continue };
            if ka == kb {
                continue;
            }
            let t = std::time::Instant::now();
            let ha = self.xa.get(name);
            let hb = self.xb.get(name);
            let (Some(ha), Some(hb)) = (ha, hb) else { continue };
            let t1 = t.elapsed();
            let (nets, devices) = cell_changes(name, (&ha.root, &ha.cells), (&hb.root, &hb.cells), self.unit_um, boxes);
            if profile {
                eprintln!(
                    "[redes] {name}: extraer {t1:?} ({}+{} y {}+{} celdas), comparar {:?}: {} redes, {} transistores",
                    ha.stats.cells,
                    ha.stats.remembered,
                    hb.stats.cells,
                    hb.stats.remembered,
                    t.elapsed() - t1,
                    nets.len(),
                    devices.len()
                );
            }
            out.push((name.clone(), nets, devices));
        }
        out
    }
}

/// Lo que cambió en la celda abierta en el visor: hasta `max_polygons`
/// aplanados, comparando las dos versiones aplanadas, como siempre (con lo
/// de todas sus sub-celdas, en sus coordenadas), pero con la extracción por
/// celdas y su memoria; más grande, por celdas ([`cell_changes`]).
pub fn opened_cell_changes(
    (la, lb): (&gdstk_rs::Library, &gdstk_rs::Library),
    (ia, ib): (Option<&gdstk_rs::magic::MagInfo>, Option<&gdstk_rs::magic::MagInfo>),
    rules: &DeviceRules,
    name: &str,
    changed: &[[f64; 4]],
    max_polygons: u64,
) -> (Vec<NetChange>, Vec<DeviceChange>) {
    let (Some(ca), Some(cb)) = (la.find_cell(name), lb.find_cell(name)) else { return Default::default() };
    let opts = Options::default();
    let (xa, xb) = (Extractor::new(la, rules, ia, opts), Extractor::new(lb, rules, ib, opts));
    let (Some(ha), Some(hb)) = (xa.get(name), xb.get(name)) else { return Default::default() };
    let unit_um = lb.unit() / 1e-6;
    let big = crate::devices::flat_polygon_estimate(la, &ca).max(crate::devices::flat_polygon_estimate(lb, &cb)) > max_polygons;
    if big {
        return cell_changes(name, (&ha.root, &ha.cells), (&hb.root, &hb.cells), unit_um, changed);
    }
    let (na, nb) = (ha.flatten(false), hb.flatten(false));
    let devices = crate::devices::device_changes(
        name,
        &na.devices.iter().map(|(d, _)| d.clone()).collect::<Vec<_>>(),
        &nb.devices.iter().map(|(d, _)| d.clone()).collect::<Vec<_>>(),
        unit_um,
    );
    (crate::nets::net_changes(name, &na, &nb, unit_um, changed), devices)
}

/// `RIKU_FULL_NETS=1`: comparar las redes también de las celdas de más de
/// [`crate::devices::MAX_POLYGONS`] polígonos aplanados aunque no estén
/// extraídas de antes (la primera vez, un chip entero cuesta segundos).
pub fn full_requested() -> bool {
    std::env::var("RIKU_FULL_NETS").is_ok_and(|v| !v.is_empty() && v != "0")
}

/// `RIKU_FLAT_NETS=1`: el diff de redes y transistores aplanando cada celda,
/// como antes de la ronda 5 (para comparar, mientras la jerárquica se asienta).
pub fn flat_requested() -> bool {
    std::env::var("RIKU_FLAT_NETS").is_ok_and(|v| !v.is_empty() && v != "0")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices;
    use crate::style::Pdk;
    use gdstk_rs::Library;

    fn sram() -> Library {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/GDS/sram_16x8_sky130.gds");
        Library::from_bytes(&std::fs::read(&path).expect("sram")).expect("gds")
    }

    /// La SRAM de OpenRAM del repo (varios niveles, celdas de contacto, el
    /// `dnwell` arriba y el cierre del pozo P): por celdas da la misma
    /// netlist que aplanada.
    #[test]
    fn the_sram_example_gives_the_same_netlist_by_cells() {
        let lib = sram();
        let rules = devices::compiled(Pdk::Sky130).expect("sky130");
        let top = crate::select_top_cell(&lib).expect("top");
        let flat = crate::nets::cell_nets(&lib, &top, rules, None);
        let h = extract(&lib, &top, rules, None, Options { inline: 256 });
        let diffs = same_netlist(&flat, &h.flatten(false), lib.unit() / 1e-6, 20);
        assert!(diffs.is_empty(), "{diffs:#?}");
        assert!(h.stats.cells + h.stats.remembered > 10, "{:?}", h.stats);
        // Otra vez: todo sale de la memoria, con el mismo resultado.
        let again = extract(&lib, &top, rules, None, Options { inline: 256 });
        assert_eq!(again.stats.cells, 0, "{:?}", again.stats);
        assert!(same_netlist(&flat, &again.flatten(false), lib.unit() / 1e-6, 5).is_empty());
    }

    /// Sin meter nada en el padre, las celdas de contacto quedan solas y la
    /// netlist cambia: el umbral importa (ver `docs/dev/design-notes.md`).
    #[test]
    fn without_inlining_contact_cells_the_netlist_differs() {
        let lib = sram();
        let rules = devices::compiled(Pdk::Sky130).expect("sky130");
        let top = crate::select_top_cell(&lib).expect("top");
        let flat = crate::nets::cell_nets(&lib, &top, rules, None);
        let h = extract(&lib, &top, rules, None, Options { inline: 0 });
        assert!(!same_netlist(&flat, &h.flatten(false), lib.unit() / 1e-6, 1).is_empty());
    }

    /// Lo que sale del disco es lo mismo que se guardó; una entrada ilegible
    /// no es un error: se borra y no se usa.
    #[test]
    fn the_disk_gives_back_the_same_extraction_and_drops_a_corrupt_entry() {
        let lib = sram();
        let rules = devices::compiled(Pdk::Sky130).expect("sky130");
        let top = crate::select_top_cell(&lib).expect("top");
        let h = extract(&lib, &top, rules, None, Options { inline: 256 });
        let dir = std::env::temp_dir().join(format!("riku-nets-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let disk = Disk::new(&dir);
        for (k, c) in &h.cells {
            disk.store(*k, c);
        }
        let back: Cells = h.cells.keys().map(|k| (*k, std::sync::Arc::new(disk.load(*k).expect("guardada")))).collect();
        let root = back[&h.keys[top.name()]].clone();
        let diffs = same_netlist(&h.flatten(false), &flatten(&root, &back, false), lib.unit() / 1e-6, 5);
        assert!(diffs.is_empty(), "{diffs:#?}");
        // Ilegible: no se usa y se borra.
        let k = h.keys[top.name()];
        let path = dir.join(format!("{:032x}.json", k.0));
        std::fs::write(&path, b"{ roto").unwrap();
        assert!(disk.load(k).is_none());
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Las huellas no dependen de la corrida; otro umbral para meter
    /// sub-celdas (o otras reglas) da otras.
    #[test]
    fn keys_are_stable_and_depend_on_what_interprets_the_geometry() {
        let lib = sram();
        let rules = devices::compiled(Pdk::Sky130).expect("sky130");
        let k = |inline, print| key::net_keys(&lib, None, key::salt(&lib, print, inline)).0;
        let a = k(256, rules.fingerprint);
        assert_eq!(a, k(256, rules.fingerprint));
        let top = crate::select_top_cell(&lib).expect("top");
        let name = top.name();
        assert_ne!(a[name], k(128, rules.fingerprint)[name]);
        assert_ne!(a[name], k(256, rules.fingerprint ^ 1)[name]);
    }
}
