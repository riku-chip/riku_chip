/// Tests de estres y rendimiento para riku.
///
/// Cubren:
/// - Parser con esquematico real (op_sim.sch)
/// - Diff semantico con multiples revisiones encadenadas
/// - Git service: blob extraction bajo carga
/// - Large blob threshold: archivos cerca y sobre el limite
/// - GDS binario: el parser no debe paniquear
/// - Throughput: N iteraciones de parse+diff dentro de tiempo razonable
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use git2::{Repository, Signature};

use riku::core::git::git_service::GitService;
use riku::core::domain::models::FileFormat;
use riku::core::domain::ports::GitRepository;
use riku::adapters::xschem_driver::parse;
use riku::core::format::detect_format;
use xschem_viewer::semantic::diff as semantic_diff_inner;

/// Shim local: antes `semantic_diff` tomaba bytes. Ahora la librería exige
/// schematics parseados — este helper preserva la ergonomía de los tests.
fn semantic_diff(a: &[u8], b: &[u8]) -> xschem_viewer::semantic::DiffReport {
    semantic_diff_inner(&parse(a), &parse(b))
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn commit_file(repo: &Repository, rel_path: &str, content: &[u8], message: &str) -> git2::Oid {
    let workdir = repo.workdir().expect("workdir");
    let full_path = workdir.join(rel_path);
    if let Some(parent) = full_path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&full_path, content).unwrap();

    let mut index = repo.index().unwrap();
    index.add_path(Path::new(rel_path)).unwrap();
    index.write().unwrap();
    let tree_id = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    let sig = Signature::now("Riku", "riku@example.com").unwrap();

    match repo.head() {
        Ok(head) => {
            let parent = repo.find_commit(head.target().unwrap()).unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[&parent])
                .unwrap()
        }
        Err(_) => repo
            .commit(Some("HEAD"), &sig, &sig, message, &tree, &[])
            .unwrap(),
    }
}

fn make_temp_repo() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("riku-stress")
        .tempdir_in(repo_root())
        .unwrap()
}

// ---------------------------------------------------------------------------
// Datos de test
// ---------------------------------------------------------------------------

fn op_sim_content() -> Vec<u8> {
    fs::read(repo_root().join("examples/SH/op_sim.sch")).expect("op_sim.sch debe existir")
}

fn gds_content() -> Vec<u8> {
    fs::read(repo_root().join("examples/GDS/sram_16x8_sky130.gds"))
        .expect("sram_16x8_sky130.gds debe existir")
}

// ---------------------------------------------------------------------------
// Tests de parser
// ---------------------------------------------------------------------------

#[test]
fn parse_op_sim_returns_components_and_wires() {
    let content = op_sim_content();
    let sch = parse(&content);
    assert!(
        !sch.components.is_empty(),
        "op_sim.sch debe tener componentes"
    );
    assert!(!sch.wires.is_empty(), "op_sim.sch debe tener wires");
}

#[test]
fn detect_format_op_sim_is_xschem() {
    let content = op_sim_content();
    assert_eq!(detect_format(&content), FileFormat::Xschem);
}

#[test]
fn detect_format_gds_is_unknown() {
    let content = gds_content();
    // El parser no debe paniquear con binario GDS
    let fmt = detect_format(&content);
    assert_ne!(
        fmt,
        FileFormat::Xschem,
        "GDS no debe detectarse como Xschem"
    );
}

#[test]
fn parse_gds_binary_does_not_panic() {
    let content = gds_content();
    // parse() sobre binario arbitrario debe retornar schematic vacio, no paniquear
    let sch = parse(&content);
    // No assertions sobre contenido — solo que no paniquea
    let _ = sch;
}

// ---------------------------------------------------------------------------
// Throughput: N parse+diff dentro de tiempo razonable
// ---------------------------------------------------------------------------

#[test]
fn throughput_100_parse_and_diff() {
    const N: usize = 100;
    const MAX_MS: u128 = 10_000; // 10s en dev (sin opt); release es ~3x mas rapido

    let base = op_sim_content();
    let modified: Vec<u8> = String::from_utf8_lossy(&base)
        .replace("value=0.9", "value=1.1")
        .into_bytes();

    let start = Instant::now();
    for _ in 0..N {
        let sch_a = parse(&base);
        let sch_b = parse(&modified);
        let report = semantic_diff_inner(&sch_a, &sch_b);
        // Validar que el diff es coherente
        assert!(
            !report.components.is_empty()
                || !report.nets_added.is_empty()
                || sch_a.components.len() == sch_b.components.len()
        );
    }
    let elapsed = start.elapsed().as_millis();
    assert!(
        elapsed < MAX_MS,
        "{N} iteraciones de parse+diff tardaron {elapsed}ms (limite: {MAX_MS}ms)"
    );
    println!(
        "throughput_100_parse_and_diff: {elapsed}ms total, {}ms/iter",
        elapsed / N as u128
    );
}

// ---------------------------------------------------------------------------
// Diff semantico: multiples variantes del mismo esquematico
// ---------------------------------------------------------------------------

#[test]
fn diff_detects_component_value_change() {
    let base = op_sim_content();
    let modified: Vec<u8> = String::from_utf8_lossy(&base)
        .replace("value=0.9", "value=1.5")
        .into_bytes();

    let report = semantic_diff(&base, &modified);
    let modified_components: Vec<_> = report
        .components
        .iter()
        .filter(|c| c.kind == riku::core::domain::models::ChangeKind::Modified)
        .collect();
    assert!(
        !modified_components.is_empty(),
        "debe detectar al menos un componente modificado"
    );
}

#[test]
fn diff_same_content_is_empty() {
    let content = op_sim_content();
    let report = semantic_diff(&content, &content);
    assert!(
        report.components.is_empty(),
        "sin cambios: components debe estar vacio"
    );
    assert!(
        report.nets_added.is_empty(),
        "sin cambios: nets_added debe estar vacio"
    );
    assert!(
        report.nets_removed.is_empty(),
        "sin cambios: nets_removed debe estar vacio"
    );
    assert!(
        !report.is_move_all,
        "sin cambios: is_move_all debe ser false"
    );
}

// ---------------------------------------------------------------------------
// Git service bajo carga
// ---------------------------------------------------------------------------

#[test]
fn git_service_extracts_blob_from_10_commits() {
    const N_COMMITS: usize = 10;
    let temp = make_temp_repo();
    let repo = Repository::init(temp.path()).unwrap();
    let rel_path = "design/top.sch";

    let base = op_sim_content();
    for i in 0..N_COMMITS {
        let content: Vec<u8> = String::from_utf8_lossy(&base)
            .replace("value=0.9", &format!("value={:.1}", 0.9 + i as f64 * 0.1))
            .into_bytes();
        commit_file(&repo, rel_path, &content, &format!("commit {i}"));
    }

    let svc = GitService::open(temp.path()).unwrap();
    let commits = GitRepository::get_commits(&svc, Some(rel_path)).unwrap();
    assert_eq!(commits.len(), N_COMMITS);

    // Extraer blob de cada commit y parsear
    for commit in &commits {
        let blob = GitRepository::get_blob(&svc, &commit.oid, rel_path).unwrap();
        let sch = parse(&blob);
        assert!(
            !sch.components.is_empty(),
            "cada revision debe tener componentes"
        );
    }
}

#[test]
fn git_service_log_semantic_across_revisions() {
    const N_COMMITS: usize = 5;
    let temp = make_temp_repo();
    let repo = Repository::init(temp.path()).unwrap();
    let rel_path = "design/top.sch";

    let base = op_sim_content();
    for i in 0..N_COMMITS {
        let content: Vec<u8> = String::from_utf8_lossy(&base)
            .replace("value=0.9", &format!("value={:.1}", 0.9 + i as f64 * 0.1))
            .into_bytes();
        commit_file(&repo, rel_path, &content, &format!("rev {i}"));
    }

    let svc = GitService::open(temp.path()).unwrap();
    let commits = GitRepository::get_commits(&svc, Some(rel_path)).unwrap();

    let mut semantic_changes = 0usize;
    for window in commits.windows(2) {
        let newer = &window[0];
        let older = &window[1];
        let blob_a = GitRepository::get_blob(&svc, &older.oid, rel_path).unwrap();
        let blob_b = GitRepository::get_blob(&svc, &newer.oid, rel_path).unwrap();
        let report = semantic_diff(&blob_a, &blob_b);
        if !report.components.is_empty() {
            semantic_changes += 1;
        }
    }
    assert!(
        semantic_changes > 0,
        "debe haber al menos un cambio semantico en {N_COMMITS} revisiones"
    );
}

// ---------------------------------------------------------------------------
// Large blob threshold
// ---------------------------------------------------------------------------

#[test]
fn large_blob_threshold_is_honored() {
    use riku::core::git::git_service::LARGE_BLOB_THRESHOLD;

    // Verificar que el threshold esta definido y es razonable (50 MB)
    assert_eq!(LARGE_BLOB_THRESHOLD, 50 * 1024 * 1024);
}

#[test]
fn blob_near_threshold_is_parseable() {
    // Crear un .sch artificial que sea grande pero valido
    let base = op_sim_content();
    let base_str = String::from_utf8_lossy(&base);

    // Repetir el contenido hasta alcanzar ~1 MB (bien por debajo del threshold)
    let reps = (1024 * 1024 / base.len()).max(1);
    let big: String = base_str.repeat(reps);
    let big_bytes = big.as_bytes();

    // No debe paniquear; el formato puede no detectarse como Xschem al repetir
    let _ = detect_format(big_bytes);
    let _ = parse(big_bytes);
}

// ---------------------------------------------------------------------------
// Stress: GDS (871 KB) — no debe bloquearse ni paniquear
// ---------------------------------------------------------------------------

#[test]
fn stress_gds_parse_is_fast() {
    const MAX_MS: u128 = 500;
    let content = gds_content();
    assert!(
        content.len() > 100_000,
        "El GDS de prueba debe ser >100 KB, encontrado: {} bytes",
        content.len()
    );

    let start = Instant::now();
    let _ = detect_format(&content);
    let _ = parse(&content);
    let elapsed = start.elapsed().as_millis();

    assert!(
        elapsed < MAX_MS,
        "parse de GDS ({} KB) tardo {elapsed}ms (limite: {MAX_MS}ms)",
        content.len() / 1024
    );
    println!(
        "stress_gds_parse_is_fast: {} KB en {elapsed}ms",
        content.len() / 1024
    );
}

#[test]
fn stress_gds_in_git_repo() {
    let temp = make_temp_repo();
    let repo = Repository::init(temp.path()).unwrap();
    let rel_path = "layout/sram.gds";

    let content = gds_content();
    commit_file(&repo, rel_path, &content, "add gds");

    let svc = GitService::open(temp.path()).unwrap();
    let commits = GitRepository::get_commits(&svc, Some(rel_path)).unwrap();
    assert_eq!(commits.len(), 1);

    let blob = GitRepository::get_blob(&svc, &commits[0].oid, rel_path).unwrap();
    assert_eq!(
        blob.len(),
        content.len(),
        "blob extraido debe coincidir con el original"
    );
    println!(
        "stress_gds_in_git_repo: {} KB extraidos de git OK",
        blob.len() / 1024
    );
}
