#![cfg(feature = "xschem")]
use std::fs;
use std::path::Path;

use git2::{Repository, Signature};
use serde_json::json;

use riku::modules::xschem::parse;
use riku::core::domain::git_types::{GitError, LARGE_BLOB_THRESHOLD};
use riku::core::domain::models::FileFormat;
use xschem_viewer::semantic::ChangeKind;
use riku::core::domain::ports::GitRepository;
/// Formato por firma, según los módulos del ejecutable.
fn detect_format(content: &[u8]) -> FileFormat {
    riku::modules::registry().detect_format(content)
}
use riku::core::git::git_service::GitService;
use xschem_viewer::semantic::diff;

fn commit_file(repo: &Repository, rel_path: &str, content: &str, message: &str) -> git2::Oid {
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

    let oid = match repo.head() {
        Ok(head) => {
            let parent = repo.find_commit(head.target().unwrap()).unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[&parent])
                .unwrap()
        }
        Err(_) => repo
            .commit(Some("HEAD"), &sig, &sig, message, &tree, &[])
            .unwrap(),
    };
    oid
}

fn test_tempdir() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("riku-test")
        .tempdir_in(std::env::current_dir().unwrap())
        .unwrap()
}

fn commit_rename(
    repo: &Repository,
    old_rel_path: &str,
    new_rel_path: &str,
    message: &str,
) -> git2::Oid {
    let workdir = repo.workdir().expect("workdir");
    let old_full_path = workdir.join(old_rel_path);
    let new_full_path = workdir.join(new_rel_path);
    if let Some(parent) = new_full_path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::rename(&old_full_path, &new_full_path).unwrap();

    let mut index = repo.index().unwrap();
    index.remove_path(Path::new(old_rel_path)).unwrap();
    index.add_path(Path::new(new_rel_path)).unwrap();
    index.write().unwrap();
    let tree_id = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    let sig = Signature::now("Riku", "riku@example.com").unwrap();

    let head = repo.head().unwrap();
    let parent = repo.find_commit(head.target().unwrap()).unwrap();
    repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[&parent])
        .unwrap()
}

#[test]
fn detects_and_parses_xschem() {
    let content = br#"v {xschem version=3.0.0 file_version=1.2}
C {res.sym} 10 20 0 0 {name=R1 value=10k}
N 10 20 30 40 {lab=NET1}
"#;

    assert_eq!(detect_format(content), FileFormat::Xschem);
    let sch = parse(content);
    assert!(sch.components.contains_key("R1"));
    assert!(sch.nets.contains("NET1"));
    assert_eq!(sch.wires.len(), 1);
}

#[test]
fn parses_real_xschem_fixture() {
    let content = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../examples/SH/op_sim.sch"
    ));

    assert_eq!(detect_format(content), FileFormat::Xschem);

    let sch = parse(content);
    assert!(sch.components.len() >= 15);
    assert!(sch.wires.len() >= 70);
    assert!(sch.nets.contains("Vdd"));
    assert!(sch.nets.contains("Vss"));
    assert!(sch.nets.contains("out"));
    assert!(sch.nets.contains("GND"));
    assert!(sch.components.contains_key("M10"));
    assert!(sch.components.contains_key("C1"));
}

#[test]
fn semantic_diff_marks_move_all() {
    let a = br#"v {xschem version=3.0.0 file_version=1.2}
C {res.sym} 10 20 0 0 {name=R1 value=10k}
C {cap.sym} 30 40 0 0 {name=C1 value=1p}
"#;
    let b = br#"v {xschem version=3.0.0 file_version=1.2}
C {res.sym} 15 25 0 0 {name=R1 value=10k}
C {cap.sym} 35 45 0 0 {name=C1 value=1p}
"#;

    let report = diff(&parse(a), &parse(b));
    assert!(report.is_move_all);
    assert_eq!(report.components.len(), 2);
    assert!(report.components.iter().all(|c| c.cosmetic));
    assert!(report.is_empty());
}

#[test]
fn semantic_diff_marks_added_removed_and_modified() {
    let a = br#"v {xschem version=3.0.0 file_version=1.2}
C {res.sym} 10 20 0 0 {name=R1 value=10k}
C {cap.sym} 30 40 0 0 {name=C1 value=1p}
N 0 0 10 0 {lab=NET1}
"#;
    let b = br#"v {xschem version=3.0.0 file_version=1.2}
C {res.sym} 10 20 0 0 {name=R1 value=22k}
C {ind.sym} 70 80 0 0 {name=L1 value=2u}
N 0 0 10 0 {lab=NET2}
"#;

    let report = diff(&parse(a), &parse(b));
    let added = report
        .components
        .iter()
        .filter(|c| c.kind == ChangeKind::Added && !c.cosmetic)
        .count();
    let removed = report
        .components
        .iter()
        .filter(|c| c.kind == ChangeKind::Removed && !c.cosmetic)
        .count();
    let modified = report
        .components
        .iter()
        .filter(|c| c.kind == ChangeKind::Modified && !c.cosmetic)
        .count();

    assert_eq!(added, 1);
    assert_eq!(removed, 1);
    assert_eq!(modified, 1);
    assert_eq!(report.nets_added, vec!["NET2".to_string()]);
    assert_eq!(report.nets_removed, vec!["NET1".to_string()]);
    assert!(!report.is_move_all);
}

#[test]
fn git_service_reads_commits_and_blobs() {
    let temp = test_tempdir();
    let repo = Repository::init(temp.path()).unwrap();

    let file_path = "design/top.sch";
    let _first = commit_file(
        &repo,
        file_path,
        "v {xschem version=3.0.0 file_version=1.2}\nC {res.sym} 10 20 0 0 {name=R1 value=10k}\n",
        "init",
    );
    let _second = commit_file(
        &repo,
        file_path,
        "v {xschem version=3.0.0 file_version=1.2}\nC {res.sym} 10 20 0 0 {name=R1 value=22k}\n",
        "update",
    );

    let svc = GitService::open(temp.path()).unwrap();
    let blob = svc.get_blob("HEAD", file_path).unwrap();
    assert!(String::from_utf8_lossy(&blob).contains("22k"));

    let commits = svc.get_commits(Some(file_path)).unwrap();
    assert_eq!(commits.len(), 2);
}

#[test]
fn enums_serialize_stably() {
    assert_eq!(
        serde_json::to_value(ChangeKind::Added).unwrap(),
        json!("added")
    );
    assert_eq!(
        serde_json::to_value(FileFormat::Xschem).unwrap(),
        json!("xschem")
    );
}

#[test]
fn git_service_matches_git_port() {
    fn assert_git_port<T: GitRepository>(_svc: &T) {}

    let temp = test_tempdir();
    let repo = Repository::init(temp.path()).unwrap();
    let svc = GitService::open(temp.path()).unwrap();
    assert_git_port(&svc);
    drop(repo);
}

#[test]
fn git_service_reports_renames() {
    let temp = test_tempdir();
    let repo = Repository::init(temp.path()).unwrap();

    let old_path = "design/old.sch";
    let new_path = "design/new.sch";
    commit_file(
        &repo,
        old_path,
        "v {xschem version=3.0.0 file_version=1.2}\nC {res.sym} 10 20 0 0 {name=R1 value=10k}\n",
        "init",
    );
    commit_rename(&repo, old_path, new_path, "rename");

    let svc = GitService::open(temp.path()).unwrap();
    let changes = svc.get_changed_files("HEAD~1", "HEAD").unwrap();

    assert_eq!(changes.len(), 1);
    assert_eq!(
        changes[0].status,
        riku::core::domain::git_types::ChangeStatus::Renamed
    );
    assert_eq!(changes[0].path, new_path);
    assert_eq!(changes[0].old_path.as_deref(), Some(old_path));
}

#[test]
fn git_service_reports_large_blobs() {
    let temp = test_tempdir();
    let repo = Repository::init(temp.path()).unwrap();

    let file_path = "design/huge.bin";
    let workdir = repo.workdir().expect("workdir");
    let full_path = workdir.join(file_path);
    if let Some(parent) = full_path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    let payload = vec![b'x'; LARGE_BLOB_THRESHOLD + 1];
    fs::write(&full_path, &payload).unwrap();

    let mut index = repo.index().unwrap();
    index.add_path(Path::new(file_path)).unwrap();
    index.write().unwrap();
    let tree_id = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    let sig = Signature::now("Riku", "riku@example.com").unwrap();
    repo.commit(Some("HEAD"), &sig, &sig, "large", &tree, &[])
        .unwrap();

    let svc = GitService::open(temp.path()).unwrap();
    let err = svc.get_blob("HEAD", file_path).unwrap_err();

    assert!(matches!(
        err,
        GitError::LargeBlob {
            path,
            size
        } if path == file_path && size == LARGE_BLOB_THRESHOLD + 1
    ));
}

#[test]
fn show_compares_a_commit_against_its_parent() {
    use riku::core::analysis::show::analyze_show;
    use riku::core::domain::git_types::ChangeStatus;
    use riku::core::domain::models::Element;

    let temp = test_tempdir();
    let repo = Repository::init(temp.path()).unwrap();
    let sch = |v: &str| format!("v {{xschem version=3.0.0 file_version=1.2}}\nC {{res.sym}} 10 20 0 0 {{name=R1 value={v}}}\n");
    commit_file(&repo, "amp.sch", &sch("10k"), "init");
    commit_file(&repo, "amp.sch", &sch("22k"), "valor");
    commit_file(&repo, "notas.txt", "hola", "notas");

    let svc = GitService::open(temp.path()).unwrap();
    let modules = riku::modules::registry();
    let opts = riku_kernel::DiffOptions::default();

    // Commit con padre: el valor de R1 cambió.
    let r = analyze_show(&svc, "HEAD~1", None, &modules, &opts).unwrap();
    assert_eq!(r.commit.info.message, "valor");
    assert!(r.parent().is_some());
    assert_eq!(r.files.len(), 1);
    assert_eq!(r.files[0].status, Some(ChangeStatus::Modified));
    let change = r.files[0].change.as_ref().unwrap();
    assert!(matches!(&change.changes[0].element, Element::Component { name } if name == "R1"));
    assert!(r.has_functional_changes());

    // Commit inicial: se compara contra vacío.
    let root = analyze_show(&svc, "HEAD~2", None, &modules, &opts).unwrap();
    assert!(root.parent().is_none());
    assert_eq!(root.files[0].status, Some(ChangeStatus::Added));

    // Archivo sin módulo: se lista, sin diff, y no cuenta como cambio funcional.
    let txt = analyze_show(&svc, "HEAD", None, &modules, &opts).unwrap();
    assert_eq!(txt.files[0].path, "notas.txt");
    assert!(txt.files[0].change.is_none());
    assert!(!txt.has_functional_changes());

    // Un archivo que el commit no tocó: sin cambios.
    let same = analyze_show(&svc, "HEAD", Some("amp.sch"), &modules, &opts).unwrap();
    assert_eq!(same.files[0].status, None);
    assert!(same.files[0].change.as_ref().unwrap().is_empty());

    assert!(analyze_show(&svc, "no-existe", None, &modules, &opts).is_err());
}

/// Commitea varios archivos binarios o de texto de una vez.
fn commit_files(repo: &Repository, files: &[(&str, Vec<u8>)], message: &str) {
    let workdir = repo.workdir().expect("workdir");
    let mut index = repo.index().unwrap();
    for (rel, bytes) in files {
        fs::write(workdir.join(rel), bytes).unwrap();
        index.add_path(Path::new(rel)).unwrap();
    }
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let sig = Signature::now("Riku", "riku@example.com").unwrap();
    let parents: Vec<git2::Commit<'_>> = repo.head().ok().and_then(|h| h.target()).map(|t| repo.find_commit(t).unwrap()).into_iter().collect();
    let parents: Vec<&git2::Commit<'_>> = parents.iter().collect();
    repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parents).unwrap();
}

#[test]
fn log_show_and_status_are_the_same_with_one_thread_and_with_several() {
    use riku::core::analysis::log::{walk_with_summary, EnvelopedLogReport, LogOptions};
    use riku::core::analysis::show::analyze_show;
    use riku::core::analysis::status::{analyze_with_options, EnvelopedStatusReport, StatusOptions};

    // Varios commits que tocan varios archivos a la vez (esquemáticos y, si
    // el módulo de layouts está, GDS), y cambios sin commitear.
    let temp = test_tempdir();
    let repo = Repository::init(temp.path()).unwrap();
    let sch = |name: &str, v: u32| {
        format!("v {{xschem version=3.0.0 file_version=1.2}}\nC {{res.sym}} {v} 20 0 0 {{name={name} value={v}k}}\nN 0 0 {v} 0 {{lab=n{v}}}\n").into_bytes()
    };
    let fixture = |name: &str| -> Vec<u8> {
        fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../riku-mod-layout/tests/fixtures").join(name)).unwrap()
    };
    let with_layout = cfg!(feature = "layout");
    for i in 0..8u32 {
        let mut files: Vec<(&str, Vec<u8>)> = vec![("a.sch", sch("R1", i)), ("b.sch", sch("R2", i * 2)), ("c.sch", sch("R3", 7))];
        if with_layout {
            let (h, m) = if i % 2 == 0 { ("hier_inv_a.gds", "multi_inst_a.gds") } else { ("hier_inv_b.gds", "multi_inst_b.gds") };
            files.push(("h.gds", fixture(h)));
            files.push(("m.gds", fixture(m)));
        }
        commit_files(&repo, &files, &format!("c{i}"));
    }
    fs::write(temp.path().join("a.sch"), sch("R1", 99)).unwrap();
    fs::write(temp.path().join("b.sch"), sch("R2", 98)).unwrap();
    if with_layout {
        fs::write(temp.path().join("h.gds"), fixture("hier_inv_b.gds")).unwrap();
    }

    let path = temp.path().to_path_buf();
    let run = |threads: usize| -> (serde_json::Value, serde_json::Value, String) {
        let path = path.clone();
        rayon::ThreadPoolBuilder::new().num_threads(threads).build().unwrap().install(move || {
            let svc = GitService::open(&path).unwrap();
            let modules = riku::modules::registry();
            let log = walk_with_summary(&svc, &LogOptions::default(), &modules).unwrap();
            let status = analyze_with_options(&svc, Some(&path), &StatusOptions::default(), &modules).unwrap();
            let show = analyze_show(&svc, "HEAD~3", None, &modules, &riku_kernel::DiffOptions::default()).unwrap();
            (
                serde_json::to_value(EnvelopedLogReport::from(&log)).unwrap(),
                serde_json::to_value(EnvelopedStatusReport::from(&status)).unwrap(),
                format!("{:?}", show.files),
            )
        })
    };
    let (one, many) = (run(1), run(4));
    assert_eq!(one.0["commits"].as_array().unwrap().len(), 8);
    assert!(one.1["files"].as_array().unwrap().len() >= 2);
    assert!(one.2.contains("a.sch"));
    assert_eq!(one.0, many.0, "log");
    assert_eq!(one.1, many.1, "status");
    assert_eq!(one.2, many.2, "show");
}

/// Un archivo renombrado y modificado se compara contra su versión con la
/// ruta vieja: sale solo lo que cambió, no "todo añadido". Vale para `log`,
/// `status` y `diff` de todos los archivos (`show` ya lo hacía).
#[test]
fn renamed_and_modified_files_are_compared_with_their_old_path() {
    use riku::core::analysis::diff_set::{analyze_all, Side};
    use riku::core::analysis::log::{walk_with_summary, LogOptions};
    use riku::core::analysis::status::{analyze_with_options, StatusOptions};
    use riku::core::analysis::summary::labels;

    // Muchos componentes para que git vea el renombre (similitud > 50 %).
    let sch = |extra: &str| {
        let mut s = String::from("v {xschem version=3.0.0 file_version=1.2}\n");
        for i in 0..20 {
            s += &format!("C {{res.sym}} {} 20 0 0 {{name=R{i} value=1k}}\n", i * 40);
        }
        s + extra
    };
    let temp = test_tempdir();
    let repo = Repository::init(temp.path()).unwrap();
    commit_files(&repo, &[("old.sch", sch("").into_bytes())], "v1");
    let v1 = repo.head().unwrap().target().unwrap().to_string();
    let workdir = temp.path();
    let rename_to = |from: &str, to: &str, content: String| {
        let mut index = repo.index().unwrap();
        fs::remove_file(workdir.join(from)).unwrap();
        index.remove_path(Path::new(from)).unwrap();
        fs::write(workdir.join(to), content).unwrap();
        index.add_path(Path::new(to)).unwrap();
        index.write().unwrap();
    };
    rename_to("old.sch", "new.sch", sch("C {res.sym} 900 20 0 0 {name=R99 value=1k}\n"));
    let tree = repo.find_tree(repo.index().unwrap().write_tree().unwrap()).unwrap();
    let sig = Signature::now("Riku", "riku@example.com").unwrap();
    let parent = repo.head().unwrap().peel_to_commit().unwrap();
    repo.commit(Some("HEAD"), &sig, &sig, "renombre", &tree, &[&parent]).unwrap();

    let svc = GitService::open(temp.path()).unwrap();
    let modules = riku::modules::registry();
    let one_added = |counts: &std::collections::BTreeMap<String, i64>, what: &str| {
        assert_eq!(counts.get(labels::COMPONENTS_ADDED), Some(&1), "{what}: {counts:?}");
        assert_eq!(counts.get(labels::COMPONENTS_REMOVED), None, "{what}: {counts:?}");
    };

    let log = walk_with_summary(&svc, &LogOptions::default(), &modules).unwrap();
    let renamed = log.commits.iter().find(|c| c.info.message.trim() == "renombre").expect("commit del renombre");
    let f = &renamed.files[0];
    assert_eq!(f.path, "new.sch");
    one_added(&f.counts, "log");

    let set = analyze_all(&svc, Some(workdir), &Side::Rev(v1), &Side::Rev("HEAD".into()), &modules, &Default::default()).unwrap();
    assert_eq!(set.files.len(), 1, "{:?}", set.files);
    assert_eq!(set.files[0].old_path.as_deref(), Some("old.sch"));
    let added = set.files[0].change.as_ref().unwrap().functional().count();
    assert_eq!(added, 1, "diff A B: {:?}", set.files[0].change);

    // status: renombre en el índice (git mv) y otro cambio en disco.
    rename_to("new.sch", "otro.sch", sch("C {res.sym} 900 20 0 0 {name=R99 value=1k}\nC {res.sym} 950 20 0 0 {name=R98 value=1k}\n"));
    let status = analyze_with_options(&svc, Some(workdir), &StatusOptions::default(), &modules).unwrap();
    let f = status.files.iter().find(|f| f.path == "otro.sch").unwrap_or_else(|| panic!("{:?}", status.files));
    one_added(&f.counts, "status");
}

/// El visor con `B = :worktree` (`riku diff A top.mag -f visual`) busca las
/// sub-celdas en el disco, no en un commit llamado `:worktree`.
#[test]
fn viewer_sides_read_other_files_from_the_commit_or_the_disk() {
    use riku::core::analysis::diff_set::{token_files, WORKTREE};

    let temp = test_tempdir();
    let repo = Repository::init(temp.path()).unwrap();
    commit_files(&repo, &[("sub.mag", b"en HEAD".to_vec())], "v1");
    fs::write(temp.path().join("sub.mag"), b"en disco").unwrap();
    let svc = GitService::open(temp.path()).unwrap();

    let read = |token: &str| token_files(&svc, token).and_then(|f| f.read("sub.mag"));
    assert_eq!(read("HEAD").as_deref(), Some(&b"en HEAD"[..]));
    assert_eq!(read(WORKTREE).as_deref(), Some(&b"en disco"[..]));
    assert!(token_files(&svc, "").is_none(), "el commit inicial no tiene versión anterior");
}

/// `log -n 5 a.sch`: los 5 commits más recientes que tocan `a.sch`, aunque
/// estén más atrás que los últimos 5 del repo (antes cortaba primero y
/// filtraba después: salía vacío).
#[test]
fn log_limit_counts_only_the_commits_that_touch_the_paths() {
    use riku::core::analysis::log::{walk_with_summary, LogOptions};

    let sch = |v: u32| format!("v {{xschem version=3.0.0 file_version=1.2}}\nC {{res.sym}} 0 0 0 0 {{name=R1 value={v}k}}\n").into_bytes();
    let temp = test_tempdir();
    let repo = Repository::init(temp.path()).unwrap();
    commit_files(&repo, &[("a.sch", sch(1))], "a1");
    commit_files(&repo, &[("a.sch", sch(2))], "a2");
    for i in 0..10 {
        commit_files(&repo, &[("b.sch", sch(i))], &format!("b{i}"));
    }
    let svc = GitService::open(temp.path()).unwrap();
    let modules = riku::modules::registry();
    // Los commits del test caen en el mismo segundo: el orden por fecha
    // entre ellos no está definido, así que se comparan ordenados.
    let messages = |opts: &LogOptions| -> Vec<String> {
        let mut m: Vec<String> =
            walk_with_summary(&svc, opts, &modules).unwrap().commits.iter().map(|c| c.info.message.trim().to_string()).collect();
        m.sort();
        m
    };
    let opts = LogOptions { paths: vec!["a.sch".into()], limit: Some(5), ..Default::default() };
    assert_eq!(messages(&opts), ["a1", "a2"]);
    let opts = LogOptions { paths: vec!["*.sch".into()], limit: Some(3), ..Default::default() };
    assert_eq!(messages(&opts).len(), 3);
    // Sin resúmenes (el panel History) filtra igual, sin leer blobs.
    let opts = LogOptions { paths: vec!["a.sch".into()], limit: Some(5), skip_summaries: true, ..Default::default() };
    assert_eq!(messages(&opts), ["a1", "a2"]);
    let opts = LogOptions { paths: vec!["a.sch".into()], limit: Some(1), skip_summaries: true, ..Default::default() };
    assert_eq!(messages(&opts).len(), 1);
}
