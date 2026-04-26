use std::fs;
use std::path::Path;

use git2::{Repository, Signature};
use serde_json::json;

use riku::core::git::git_service::{GitError, GitService, LARGE_BLOB_THRESHOLD};
use riku::core::domain::models::{ChangeKind, FileFormat};
use riku::core::domain::ports::GitRepository;
use riku::adapters::xschem_driver::parse;
use riku::core::format::detect_format;
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
        riku::core::git::git_service::ChangeStatus::Renamed
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
