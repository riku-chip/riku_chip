//! Diff GDS de punta a punta: repo git real → `GitService` → driver → CLI.
//!
//! Fixtures (de `gds-renderer/tests/fixtures`, generados con gdstk):
//! - `hier_inv_a.gds`: TOP instancia INV en (10, 10); INV = rect (0,0)-(2,1) en 1/0.
//! - `hier_inv_b.gds`: igual, pero INV suma el rect (2,0)-(3,1).
//!
//! El cambio vive en la sub-cell INV y debe verse también en TOP (que la
//! instancia), con bbox en coordenadas absolutas de TOP: (12,10)-(13,11).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use git2::{Repository, Signature};
use serde_json::Value;

use riku::adapters::registry::get_driver_for;
use riku::core::domain::models::{ChangeKind, FileFormat};
use riku::core::domain::ports::GitRepository;
use riku::core::format::detect_format;
use riku::core::git::git_service::GitService;

fn fixture(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../gds-renderer/tests/fixtures")
        .join(name);
    fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn commit_bytes(repo: &Repository, rel_path: &str, content: &[u8], message: &str) -> String {
    let full = repo.workdir().expect("workdir").join(rel_path);
    fs::write(&full, content).unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new(rel_path)).unwrap();
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let sig = Signature::now("Riku", "riku@example.com").unwrap();
    let parents: Vec<git2::Commit> = repo
        .head()
        .ok()
        .and_then(|h| h.target())
        .map(|oid| vec![repo.find_commit(oid).unwrap()])
        .unwrap_or_default();
    let parent_refs: Vec<&git2::Commit> = parents.iter().collect();
    repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parent_refs)
        .unwrap()
        .to_string()
}

/// Repo con tres commits: sin el GDS, con la version A y con la version B.
struct GdsRepo {
    _dir: tempfile::TempDir,
    path: PathBuf,
    file: &'static str,
    empty: String,
    a: String,
    b: String,
}

fn gds_repo() -> GdsRepo {
    layout_repo("gds")
}

/// Repo con `layout.<ext>` en versiones A y B (`hier_inv_{a,b}.<ext>`).
fn layout_repo(ext: &str) -> GdsRepo {
    let dir = tempfile::Builder::new()
        .prefix("riku-gds-e2e")
        .tempdir_in(std::env::current_dir().unwrap())
        .unwrap();
    let file: &'static str = if ext == "oas" { "layout.oas" } else { "layout.gds" };
    let repo = Repository::init(dir.path()).unwrap();
    let empty = commit_bytes(&repo, "README", b"riku", "init sin layout");
    let a = commit_bytes(&repo, file, &fixture(&format!("hier_inv_a.{ext}")), "A");
    let b = commit_bytes(&repo, file, &fixture(&format!("hier_inv_b.{ext}")), "B");
    GdsRepo { path: dir.path().to_path_buf(), _dir: dir, file, empty, a, b }
}

/// `riku diff -f json-v1`: la forma anterior, que se mantiene una versión.
fn riku_json(repo: &GdsRepo, from: &str, to: &str) -> Value {
    riku_json_as(repo, from, to, "json-v1")
}

fn riku_json_as(repo: &GdsRepo, from: &str, to: &str, format: &str) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_riku"))
        .args(["diff", from, to, repo.file, "-f", format, "--repo"])
        .arg(&repo.path)
        .output()
        .expect("ejecutar riku");
    assert!(out.status.success(), "riku diff falló: {}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!("JSON inválido ({e}): {}", String::from_utf8_lossy(&out.stdout))
    })
}

fn component<'a>(json: &'a Value, name: &str) -> &'a Value {
    json["components"]
        .as_array()
        .expect("components")
        .iter()
        .find(|c| c["name"] == name)
        .unwrap_or_else(|| panic!("falta {name} en {json}"))
}

#[test]
fn git_blob_to_driver_detects_hierarchical_change() {
    let r = gds_repo();
    let svc = GitService::open(&r.path).unwrap();
    let before = svc.get_blob(&r.a, "layout.gds").unwrap();
    let after = svc.get_blob(&r.b, "layout.gds").unwrap();
    assert_eq!(detect_format(&after), FileFormat::Gds);

    let driver = get_driver_for("layout.gds").expect("driver GDS registrado");
    let report = driver.diff(&before, &after, "layout.gds");
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);

    let names: Vec<String> = report.changes.iter().map(|c| c.element.name()).collect();
    // INV cambia directamente; TOP lo ve a traves de la reference (origen INV).
    assert!(names.iter().any(|n| n == "INV:L1/0"), "{names:?}");
    assert!(names.iter().any(|n| n == "TOP:L1/0:INV"), "{names:?}");
    assert!(report.changes.iter().all(|c| c.kind == ChangeKind::Added && !c.cosmetic));
}

#[test]
fn cli_json_reports_areas_and_absolute_bbox() {
    let r = gds_repo();
    let json = riku_json(&r, &r.a, &r.b);
    assert_eq!(json["file"], "layout.gds");
    assert!(json["warnings"].as_array().unwrap().is_empty());

    let top = component(&json, "TOP:L1/0:INV");
    assert_eq!(top["kind"], "added");
    assert_eq!(top["cosmetic"], false);
    let after = &top["after"];
    assert_eq!(after["added_area_um2"], "1.000");
    // +0.000, nunca "-0.000" (Sum de f64 vacio da -0.0).
    assert_eq!(after["removed_area_um2"], "0.000");
    assert_eq!(after["bbox_um"], "12.000,10.000,13.000,11.000");
    assert_eq!(after["flattened"], "true");

    let inv = component(&json, "INV:L1/0");
    assert_eq!(inv["after"]["bbox_um"], "2.000,0.000,3.000,1.000");
}

#[test]
fn cli_new_file_lists_all_cells_as_added() {
    // El archivo no existe en el primer commit: no es "GDSII invalido".
    let r = gds_repo();
    let json = riku_json(&r, &r.empty, &r.a);
    assert!(json["warnings"].as_array().unwrap().is_empty(), "{json}");
    for cell in ["INV", "TOP"] {
        assert_eq!(component(&json, &format!("cell:{cell}"))["kind"], "added");
    }
}

#[test]
fn cli_identical_versions_report_nothing() {
    let r = gds_repo();
    let json = riku_json(&r, &r.b, &r.b);
    assert!(json["components"].as_array().unwrap().is_empty(), "{json}");
}

#[test]
fn cli_oasis_diff_matches_gds_diff() {
    // Misma geometria en .oas y .gds: mismo reporte, salvo el nombre del archivo.
    let (gds, oas) = (gds_repo(), layout_repo("oas"));
    let (jg, jo) = (riku_json(&gds, &gds.a, &gds.b), riku_json(&oas, &oas.a, &oas.b));
    assert_eq!(jo["file"], "layout.oas");
    assert!(jo["warnings"].as_array().unwrap().is_empty(), "{jo}");
    assert_eq!(jg["components"], jo["components"]);
    assert_eq!(component(&jo, "TOP:L1/0:INV")["after"]["bbox_um"], "12.000,10.000,13.000,11.000");
}

#[test]
fn cli_json_v2_has_typed_changes() {
    let r = gds_repo();
    let json = riku_json_as(&r, &r.a, &r.b, "json");
    assert_eq!(json["schema"], "riku-diff/v2");
    assert_eq!(json["format"], "gds");
    let changes = json["changes"].as_array().expect("changes");
    let top = changes
        .iter()
        .find(|c| c["element"]["cell"] == "TOP")
        .unwrap_or_else(|| panic!("sin TOP: {json}"));
    assert_eq!(top["kind"], "added");
    assert_eq!(top["element"]["type"], "geometry");
    assert_eq!((top["element"]["layer"].as_u64(), top["element"]["datatype"].as_u64()), (Some(1), Some(0)));
    assert_eq!(top["element"]["via"]["path"], serde_json::json!(["INV"]));
    // Números de verdad, no strings.
    let area = top["details"].as_array().unwrap().iter().find(|d| d["key"] == "added_area_um2").unwrap();
    assert_eq!(area["after"].as_f64(), Some(1.0));
    assert_eq!(top["location"]["min_x"].as_f64(), Some(12.0));
}
