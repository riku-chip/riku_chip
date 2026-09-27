#![cfg(feature = "layout")]
//! Magic (`.mag`) de punta a punta: un repo Git real con una jerarquía en
//! varios archivos. Las sub-celdas salen del mismo commit (o del disco en
//! `status`), no del disco de hoy.
//!
//! - `chip/top.mag` usa `inv` (en `chip/inv.mag`) y `buf` (en `lib/buf.mag`,
//!   por el directorio del `use`).
//! - c1: la jerarquía; c2: `inv` gana un rect de metal1 (top.mag no cambia);
//!   c3: `inv` con la misma geometría escrita en otras tiras; c4: `top` usa
//!   una celda que no existe.

use std::fs;
use std::path::Path;
use std::process::Command;

use git2::{Repository, Signature};
use serde_json::Value;

use riku::core::analysis::log::{walk_with_summary, EnvelopedLogReport, LogOptions};
use riku::core::analysis::status::{analyze_with_options, EnvelopedStatusReport, StatusOptions};
use riku::core::analysis::summary::DetailLevel;
use riku::core::git::git_service::GitService;
use riku_kernel::{ChangeKind, Element};

const TOP: &str = "magic
tech sky130A
magscale 1 2
timestamp 1
<< metal1 >>
rect 0 0 100 20
use inv  inv_0
timestamp 1
transform 1 0 200 0 1 0
box 0 0 40 40
use buf  buf_0 ../lib
timestamp 1
transform 1 0 400 0 1 0
box 0 0 40 40
<< end >>
";

const INV: &str = "magic
tech sky130A
magscale 1 2
timestamp 1
<< poly >>
rect 10 0 20 40
<< metal1 >>
rect 0 0 40 10
<< labels >>
rlabel metal1 s 0 0 40 10 0 A
port 1 nsew signal input
<< end >>
";

// c2: un rect más de metal1.
const INV_2: &str = "magic
tech sky130A
magscale 1 2
timestamp 2
<< poly >>
rect 10 0 20 40
<< metal1 >>
rect 0 0 40 10
rect 0 30 80 70
<< labels >>
rlabel metal1 s 0 0 40 10 0 A
port 1 nsew signal input
<< end >>
";

// c3: la misma geometría que c2, con el rect de abajo partido en dos tiras
// (como Magic reescribe las tiras después de editar).
const INV_3: &str = "magic
tech sky130A
magscale 1 2
timestamp 3
<< poly >>
rect 10 0 20 40
<< metal1 >>
rect 0 0 25 10
rect 25 0 40 10
rect 0 30 80 70
<< labels >>
rlabel metal1 s 0 0 40 10 0 A
port 1 nsew signal input
<< end >>
";

const BUF: &str = "magic
tech sky130A
magscale 1 2
<< metal2 >>
rect 0 0 30 30
<< end >>
";

fn commit(repo: &Repository, files: &[(&str, &str)], message: &str) -> String {
    let workdir = repo.workdir().unwrap();
    let mut index = repo.index().unwrap();
    for (rel, text) in files {
        let full = workdir.join(rel);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(&full, text).unwrap();
        index.add_path(Path::new(rel)).unwrap();
    }
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let sig = Signature::now("Riku", "riku@example.com").unwrap();
    let parents: Vec<git2::Commit<'_>> =
        repo.head().ok().and_then(|h| h.target()).map(|t| repo.find_commit(t).unwrap()).into_iter().collect();
    let parents: Vec<&git2::Commit<'_>> = parents.iter().collect();
    repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parents).unwrap().to_string()
}

struct MagRepo {
    dir: tempfile::TempDir,
    c: Vec<String>,
}

fn mag_repo() -> MagRepo {
    let dir = tempfile::Builder::new().prefix("riku-mag-e2e").tempdir_in(std::env::current_dir().unwrap()).unwrap();
    let repo = Repository::init(dir.path()).unwrap();
    let c1 = commit(&repo, &[("chip/top.mag", TOP), ("chip/inv.mag", INV), ("lib/buf.mag", BUF)], "c1");
    let c2 = commit(&repo, &[("chip/inv.mag", INV_2)], "c2");
    let c3 = commit(&repo, &[("chip/inv.mag", INV_3)], "c3");
    let ghost = TOP.replace("<< end >>", "use ghost  g0\ntransform 1 0 0 0 1 0\nbox 0 0 1 1\n<< end >>");
    let c4 = commit(&repo, &[("chip/top.mag", &ghost)], "c4");
    MagRepo { dir, c: vec![c1, c2, c3, c4] }
}

fn riku(repo: &MagRepo, args: &[&str]) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_riku"))
        .args(args)
        .arg("--repo")
        .arg(repo.dir.path())
        .env("RIKU_NO_CACHE", "1")
        .output()
        .expect("ejecutar riku");
    assert!(out.status.success(), "riku {args:?} falló: {}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("JSON inválido ({e}): {}", String::from_utf8_lossy(&out.stdout)))
}

fn changes(json: &Value) -> &Vec<Value> {
    json["changes"].as_array().expect("changes")
}

#[test]
fn a_change_in_a_sub_cell_file_shows_in_the_top_of_the_same_commit() {
    let r = mag_repo();
    // top.mag no cambió entre c1 y c2: el cambio viene de inv.mag, leído del
    // mismo commit que top.
    let json = riku(&r, &["diff", &r.c[0], &r.c[1], "chip/top.mag", "-f", "json"]);
    assert_eq!(json["format"], "gds");
    let geo: Vec<&Value> = changes(&json).iter().filter(|c| c["element"]["type"] == "geometry").collect();
    let top = geo.iter().find(|c| c["element"]["cell"] == "top").unwrap_or_else(|| panic!("{json}"));
    assert_eq!(top["kind"], "added");
    assert_eq!(top["element"]["layer_name"], "metal1");
    assert_eq!(top["element"]["via"]["path"], serde_json::json!(["inv"]));
    // 80×40 unidades de 0,005 µm = 0,4 × 0,2 µm.
    let area = top["details"].as_array().unwrap().iter().find(|d| d["key"] == "added_area_um2").unwrap();
    assert!((area["after"].as_f64().unwrap() - 0.08).abs() < 1e-9, "{area}");
    // Instancia en x = 200 unidades = 1 µm: el rect nuevo va de 1,0 a 1,4 µm.
    assert!((top["location"]["min_x"].as_f64().unwrap() - 1.0).abs() < 1e-9);
    assert!(json["warnings"].as_array().is_none_or(|w| w.is_empty()), "{json}");

    // El archivo de la sub-celda también, por sí solo.
    let json = riku(&r, &["diff", &r.c[0], &r.c[1], "chip/inv.mag", "-f", "json"]);
    let inv = changes(&json).iter().find(|c| c["element"]["cell"] == "inv").unwrap_or_else(|| panic!("{json}"));
    assert_eq!(inv["element"]["layer_name"], "metal1");
}

#[test]
fn the_same_geometry_in_other_strips_is_not_a_change() {
    let r = mag_repo();
    let json = riku(&r, &["diff", &r.c[1], &r.c[2], "chip/inv.mag", "-f", "json"]);
    assert!(changes(&json).iter().all(|c| c["element"]["type"] != "geometry"), "{json}");
    let json = riku(&r, &["diff", &r.c[1], &r.c[2], "chip/top.mag", "-f", "json"]);
    assert!(changes(&json).iter().all(|c| c["element"]["type"] != "geometry"), "{json}");
}

#[test]
fn a_missing_cell_is_a_warning_and_the_rest_is_compared() {
    let r = mag_repo();
    let json = riku(&r, &["diff", &r.c[2], &r.c[3], "chip/top.mag", "-f", "json"]);
    let warnings = json["warnings"].as_array().expect("warnings").iter().map(|w| w.as_str().unwrap()).collect::<Vec<_>>();
    assert!(warnings.iter().any(|w| w.contains("ghost")), "{warnings:?}");
    // La celda nueva aparece (vacía) como añadida.
    assert!(changes(&json).iter().any(|c| c["element"]["type"] == "cell" && c["element"]["name"] == "ghost"), "{json}");
}

#[test]
fn text_output_names_the_layer() {
    let r = mag_repo();
    let out = Command::new(env!("CARGO_BIN_EXE_riku"))
        .args(["diff", &r.c[0], &r.c[1], "chip/top.mag", "--repo"])
        .arg(r.dir.path())
        .env("RIKU_NO_CACHE", "1")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("top:metal1:inv"), "{text}");
}

#[test]
fn log_and_status_read_sub_cells_from_their_own_version() {
    let r = mag_repo();
    // Sin commitear: top gana metal2 y inv pierde el rect de arriba. En
    // status, top se compara contra HEAD con inv leído del disco.
    let top = TOP.replace("<< end >>", "<< metal2 >>\nrect 0 0 10 10\n<< end >>");
    fs::write(r.dir.path().join("chip/top.mag"), top).unwrap();
    fs::write(r.dir.path().join("chip/inv.mag"), INV).unwrap();

    let svc = GitService::open(r.dir.path()).unwrap();
    let modules = riku::modules::registry();
    let opts = StatusOptions { level: DetailLevel::Completo, paths: Vec::new(), ..Default::default() };
    let status = analyze_with_options(&svc, Some(r.dir.path()), &opts, &modules).unwrap();
    let top = status.files.iter().find(|f| f.path == "chip/top.mag").expect("top en status");
    let report = top.full_report.as_ref().expect("reporte completo");
    let removed_via_inv = report.changes.iter().any(|c| {
        c.kind == ChangeKind::Removed
            && matches!(&c.element, Element::Geometry { cell, via: Some(v), layer_name: Some(l), .. }
                if cell == "top" && v.path == ["inv"] && l == "metal1")
    });
    assert!(removed_via_inv, "{:#?}", report.changes);

    // log: c2 y c3 tocan inv.mag; c3 no cambia la geometría.
    let log = walk_with_summary(&svc, &LogOptions::default(), &modules).unwrap();
    let json = serde_json::to_value(EnvelopedLogReport::from(&log)).unwrap();
    let commits = json["commits"].as_array().unwrap();
    assert_eq!(commits.len(), 4);
    let files_of = |i: usize| commits[3 - i]["files"].as_array().unwrap().iter().map(|f| f["path"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    assert_eq!(files_of(1), ["chip/inv.mag"]);

    // Lo mismo con 1 y 4 hilos.
    let run = |threads: usize| {
        let path = r.dir.path().to_path_buf();
        rayon::ThreadPoolBuilder::new().num_threads(threads).build().unwrap().install(move || {
            let svc = GitService::open(&path).unwrap();
            let modules = riku::modules::registry();
            let log = walk_with_summary(&svc, &LogOptions::default(), &modules).unwrap();
            let status = analyze_with_options(&svc, Some(&path), &StatusOptions::default(), &modules).unwrap();
            (
                serde_json::to_value(EnvelopedLogReport::from(&log)).unwrap(),
                serde_json::to_value(EnvelopedStatusReport::from(&status)).unwrap(),
            )
        })
    };
    assert_eq!(run(1), run(4));
}

#[test]
fn port_changes_are_reported() {
    let dir = tempfile::Builder::new().prefix("riku-mag-ports").tempdir_in(std::env::current_dir().unwrap()).unwrap();
    let repo = Repository::init(dir.path()).unwrap();
    let a = commit(&repo, &[("inv.mag", INV)], "a");
    let changed = INV.replace("port 1 nsew signal input", "port 1 nsew signal inout");
    let b = commit(&repo, &[("inv.mag", &changed)], "b");
    let r = MagRepo { dir, c: vec![a, b] };
    let json = riku(&r, &["diff", &r.c[0], &r.c[1], "inv.mag", "-f", "json"]);
    let port = changes(&json).iter().find(|c| c["element"]["type"] == "port").unwrap_or_else(|| panic!("{json}"));
    assert_eq!((port["element"]["cell"].as_str(), port["element"]["name"].as_str()), (Some("inv"), Some("A")));
    assert_eq!(port["kind"], "modified");
    let class = port["details"].as_array().unwrap().iter().find(|d| d["key"] == "class").unwrap();
    assert_eq!((class["before"].as_str(), class["after"].as_str()), (Some("input"), Some("inout")));
    // Solo cambió el puerto: nada de geometría.
    assert!(changes(&json).iter().all(|c| c["element"]["type"] != "geometry"), "{json}");

    let out = Command::new(env!("CARGO_BIN_EXE_riku"))
        .args(["diff", &r.c[0], &r.c[1], "inv.mag", "--repo"])
        .arg(r.dir.path())
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("inv:port:A") && text.contains("class: input → inout"), "{text}");
}

#[test]
fn pdk_cells_are_found_when_the_pdk_is_installed() {
    let root = std::env::var("PDK_ROOT").unwrap_or_else(|_| "/foss/pdks".into());
    let inv = Path::new(&root).join("sky130A/libs.ref/sky130_fd_sc_hd/mag/sky130_fd_sc_hd__inv_1.mag");
    if !inv.exists() {
        eprintln!("sin PDK SKY130 en {root}: se omite");
        return;
    }
    let dir = tempfile::Builder::new().prefix("riku-mag-pdk").tempdir_in(std::env::current_dir().unwrap()).unwrap();
    let repo = Repository::init(dir.path()).unwrap();
    let top = |x: i32| {
        format!(
            "magic\ntech sky130A\nmagscale 1 2\nuse sky130_fd_sc_hd__inv_1  x0\ntransform 1 0 {x} 0 1 0\nbox 0 0 1 1\n<< end >>\n"
        )
    };
    let a = commit(&repo, &[("top.mag", &top(0))], "a");
    let b = commit(&repo, &[("top.mag", &top(1000))], "b");
    let r = MagRepo { dir, c: vec![a, b] };
    let json = riku(&r, &["diff", &r.c[0], &r.c[1], "top.mag", "-f", "json"]);
    assert!(json["warnings"].as_array().is_none_or(|w| w.is_empty()), "{json}");
    // La celda del PDK es igual en los dos lados; lo que cambia es dónde está.
    let names: Vec<String> =
        changes(&json).iter().filter_map(|c| c["element"]["layer_name"].as_str().map(str::to_string)).collect();
    assert!(names.contains(&"locali".to_string()) && names.contains(&"metal1".to_string()), "{names:?}");
}
