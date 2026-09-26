use std::fs;
use std::path::{Path, PathBuf};

/// Extensiones que la GUI sabe abrir. Con el filtro activo, el árbol solo
/// muestra estas (y las carpetas que las contienen): en un proyecto real la
/// carpeta suele estar llena de scripts, logs e imágenes que no se pueden
/// visualizar y esconden lo importante.
const OPENABLE: &[&str] = &["sch", "sym", "gds"];

/// Carpetas que nunca contienen diseño y cuestan recorrer.
const SKIPPED_DIRS: &[&str] = &["target", "node_modules", "__pycache__"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProjectEntry {
    Directory {
        path: PathBuf,
        name: String,
        children: Vec<ProjectEntry>,
    },
    File {
        path: PathBuf,
        name: String,
    },
}

impl ProjectEntry {
    /// Árbol de `root`. Con `show_all = false` solo quedan archivos
    /// abribles, sin carpetas ocultas ni de build, y sin carpetas vacías.
    pub fn build(root: &Path, show_all: bool) -> Self {
        Self::Directory {
            path: root.to_path_buf(),
            name: root
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.to_string())
                .unwrap_or_else(|| root.display().to_string()),
            children: read_children(root, show_all),
        }
    }
}

pub fn is_openable(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| OPENABLE.iter().any(|o| e.eq_ignore_ascii_case(o)))
}

fn read_children(path: &Path, show_all: bool) -> Vec<ProjectEntry> {
    let mut dirs = Vec::new();
    let mut files = Vec::new();

    let Ok(entries) = fs::read_dir(path) else {
        return Vec::new();
    };

    for entry in entries.flatten() {
        let entry_path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if entry_path.is_dir() {
            if !show_all && (name.starts_with('.') || SKIPPED_DIRS.contains(&name.as_str())) {
                continue;
            }
            let children = read_children(&entry_path, show_all);
            if !show_all && children.is_empty() {
                continue;
            }
            dirs.push(ProjectEntry::Directory { path: entry_path.clone(), name, children });
        } else if show_all || is_openable(&entry_path) {
            files.push(ProjectEntry::File { path: entry_path, name });
        }
    }

    dirs.sort_by(|a, b| entry_name(a).cmp(entry_name(b)));
    files.sort_by(|a, b| entry_name(a).cmp(entry_name(b)));

    dirs.into_iter().chain(files).collect()
}

fn entry_name(entry: &ProjectEntry) -> &str {
    match entry {
        ProjectEntry::Directory { name, .. } | ProjectEntry::File { name, .. } => name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(e: &ProjectEntry) -> Vec<String> {
        match e {
            ProjectEntry::Directory { name, children, .. } => {
                let mut v = vec![format!("{name}/")];
                for c in children {
                    v.extend(names(c).into_iter().map(|n| format!("  {n}")));
                }
                v
            }
            ProjectEntry::File { name, .. } => vec![name.clone()],
        }
    }

    #[test]
    fn filter_keeps_openable_files_and_prunes_noise() {
        let root = std::env::temp_dir().join(format!("riku-project-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for dir in ["lay", "scripts", ".git", "target", "vacia"] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        for f in ["top.sch", "lay/inv.GDS", "lay/shot.png", "scripts/gen.py", ".git/HEAD", "target/x.gds", "notas.txt"] {
            fs::write(root.join(f), b"").unwrap();
        }

        let filtered = names(&ProjectEntry::build(&root, false));
        assert_eq!(&filtered[1..], ["  lay/", "    inv.GDS", "  top.sch"]);

        let all = names(&ProjectEntry::build(&root, true));
        assert!(all.iter().any(|n| n.trim() == "notas.txt"));
        assert!(all.iter().any(|n| n.trim() == "vacia/"));
        fs::remove_dir_all(&root).unwrap();
    }
}
