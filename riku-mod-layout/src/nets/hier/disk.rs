//! La memoria en disco (ver `docs/ronda-5/design.md`, D21): el resumen de
//! una celda en `<caché>/nets/<huella>.json`, para otra corrida. Solo los que
//! tardaron en armarse; con tope de tamaño (se borra lo más viejo) y sin
//! errores para el usuario: una entrada ilegible se borra y se recalcula.

use std::path::{Path, PathBuf};

use gdstk_rs::{OwnedPolygon, Point2D};
use serde::{Deserialize, Serialize};

use super::build::{CellNets, Inst};
use super::key::NetKey;
use super::xf::Xf;
use crate::devices::Device;
use crate::nets::{Net, NetLabel, NetPiece, Netlist, Resistor, Terminals};

/// Tope del directorio.
const MAX_BYTES: u64 = 512 << 20;
/// Versión del formato: otra, y lo guardado no se lee.
const FORMAT: u32 = 1;

#[derive(Serialize, Deserialize)]
struct Poly(u32, u32, Vec<[f64; 2]>);

impl From<&OwnedPolygon> for Poly {
    fn from(p: &OwnedPolygon) -> Self {
        Poly(p.layer, p.datatype, p.points.iter().map(|q| [q.x, q.y]).collect())
    }
}

impl From<Poly> for OwnedPolygon {
    fn from(p: Poly) -> Self {
        OwnedPolygon { layer: p.0, datatype: p.1, points: p.2.into_iter().map(|[x, y]| Point2D { x, y }).collect() }
    }
}

#[derive(Serialize, Deserialize)]
struct DeviceDto {
    model: String,
    magic: String,
    gate: Poly,
    at: (f64, f64),
    w_um: f64,
    l_um: f64,
    sd_at: Vec<(f64, f64)>,
}

#[derive(Serialize, Deserialize)]
struct ResistorDto {
    model: String,
    magic: String,
    body: Poly,
    at: (f64, f64),
    w_um: f64,
    l_um: f64,
    subckt: bool,
}

#[derive(Serialize, Deserialize)]
struct InstDto {
    cell: String,
    key: NetKey,
    xf: Xf,
    bbox: [f64; 4],
}

#[derive(Serialize, Deserialize)]
struct CellDto {
    format: u32,
    key: NetKey,
    name: String,
    pieces: Vec<(String, Poly, usize)>,
    own_nets: Vec<Net>,
    devices: Vec<(DeviceDto, Terminals)>,
    resistors: Vec<(ResistorDto, [usize; 2])>,
    labels: Vec<NetLabel>,
    own_label_nets: Vec<Option<usize>>,
    own_warnings: Vec<String>,
    own_map: Vec<u32>,
    insts: Vec<InstDto>,
    inst_maps: Vec<Vec<u32>>,
    nets: Vec<Net>,
    label_nets: Vec<Option<usize>>,
    bbox: [f64; 4],
    deep_devices: usize,
    open: Vec<(u32, (f64, f64))>,
    sub: Option<u32>,
    sub_points: Vec<(f64, f64)>,
    warnings: Vec<String>,
    build_ms: u64,
}

fn to_dto(key: NetKey, c: &CellNets) -> CellDto {
    CellDto {
        format: FORMAT,
        key,
        name: c.name.clone(),
        pieces: c.own.pieces.iter().map(|p| (p.magic.clone(), Poly::from(&p.poly), p.net)).collect(),
        own_nets: c.own.nets.clone(),
        devices: c
            .own
            .devices
            .iter()
            .map(|(d, t)| {
                let d = DeviceDto {
                    model: d.model.clone(),
                    magic: d.magic.clone(),
                    gate: Poly::from(&d.gate),
                    at: d.at,
                    w_um: d.w_um,
                    l_um: d.l_um,
                    sd_at: d.sd_at.clone(),
                };
                (d, *t)
            })
            .collect(),
        resistors: c
            .own
            .resistors
            .iter()
            .map(|(r, e)| {
                let r = ResistorDto {
                    model: r.model.clone(),
                    magic: r.magic.clone(),
                    body: Poly::from(&r.body),
                    at: r.at,
                    w_um: r.w_um,
                    l_um: r.l_um,
                    subckt: r.subckt,
                };
                (r, *e)
            })
            .collect(),
        labels: c.own.labels.clone(),
        own_label_nets: c.own.label_nets.clone(),
        own_warnings: c.own.warnings.clone(),
        own_map: c.own_map.clone(),
        insts: c.insts.iter().map(|i| InstDto { cell: i.cell.clone(), key: i.key, xf: i.xf, bbox: i.bbox }).collect(),
        inst_maps: c.inst_maps.clone(),
        nets: c.nets.clone(),
        label_nets: c.label_nets.clone(),
        bbox: c.bbox,
        deep_devices: c.deep_devices,
        open: c.open.clone(),
        sub: c.sub,
        sub_points: c.sub_points.clone(),
        warnings: c.warnings.clone(),
        build_ms: c.build_ms,
    }
}

fn from_dto(d: CellDto) -> CellNets {
    let own = Netlist {
        pieces: d.pieces.into_iter().map(|(magic, poly, net)| NetPiece { magic, poly: poly.into(), net }).collect(),
        nets: d.own_nets,
        devices: d
            .devices
            .into_iter()
            .map(|(x, t)| {
                let dev = Device {
                    model: x.model,
                    magic: x.magic,
                    gate: x.gate.into(),
                    at: x.at,
                    w_um: x.w_um,
                    l_um: x.l_um,
                    sd_at: x.sd_at,
                };
                (dev, t)
            })
            .collect(),
        resistors: d
            .resistors
            .into_iter()
            .map(|(x, e)| {
                let r = Resistor {
                    model: x.model,
                    magic: x.magic,
                    body: x.body.into(),
                    at: x.at,
                    w_um: x.w_um,
                    l_um: x.l_um,
                    subckt: x.subckt,
                };
                (r, e)
            })
            .collect(),
        labels: d.labels,
        label_nets: d.own_label_nets,
        warnings: d.own_warnings,
    };
    let insts = d.insts.into_iter().map(|i| Inst { cell: i.cell, key: i.key, xf: i.xf, bbox: i.bbox }).collect();
    CellNets::assemble(super::build::Parts {
        name: d.name,
        own: std::sync::Arc::new(own),
        own_map: d.own_map,
        insts,
        inst_maps: d.inst_maps,
        nets: d.nets,
        label_nets: d.label_nets,
        bbox: d.bbox,
        deep_devices: d.deep_devices,
        open: d.open,
        sub: d.sub,
        sub_points: d.sub_points,
        warnings: d.warnings,
        build_ms: d.build_ms,
    })
}

/// El directorio de la memoria en disco.
#[derive(Clone, Debug)]
pub struct Disk {
    dir: PathBuf,
}

impl Disk {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn path(&self, key: NetKey) -> PathBuf {
        self.dir.join(format!("{:032x}.json", key.0))
    }

    /// Si hay algo guardado con esa huella (sin leerlo).
    pub(crate) fn has(&self, key: NetKey) -> bool {
        self.path(key).is_file()
    }

    /// El resumen guardado con esa huella, si hay y se puede leer.
    pub(crate) fn load(&self, key: NetKey) -> Option<CellNets> {
        let path = self.path(key);
        let bytes = std::fs::read(&path).ok()?;
        match serde_json::from_slice::<CellDto>(&bytes) {
            Ok(d) if d.format == FORMAT && d.key == key => {
                crate::diff_cache::touch(&path);
                Some(from_dto(d))
            }
            _ => {
                let _ = std::fs::remove_file(&path);
                None
            }
        }
    }

    /// Lo guarda (escribe aparte y renombra: otro proceso nunca lee una
    /// entrada a medias) y poda el directorio.
    pub(crate) fn store(&self, key: NetKey, c: &CellNets) {
        let Ok(json) = serde_json::to_vec(&to_dto(key, c)) else { return };
        if std::fs::create_dir_all(&self.dir).is_err() {
            return;
        }
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = self.path(key);
        let tmp = path.with_extension(format!("tmp{}-{seq}", std::process::id()));
        if std::fs::write(&tmp, json).is_ok() && std::fs::rename(&tmp, &path).is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        crate::diff_cache::prune(&self.dir, MAX_BYTES);
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }
}
