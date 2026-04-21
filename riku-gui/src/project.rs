use std::fs;
use std::path::{Path, PathBuf};

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
    pub fn build(root: &Path) -> Self {
        Self::Directory {
            path: root.to_path_buf(),
            name: root
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.to_string())
                .unwrap_or_else(|| root.display().to_string()),
            children: read_children(root),
        }
    }
}

fn read_children(path: &Path) -> Vec<ProjectEntry> {
    let mut dirs = Vec::new();
    let mut files = Vec::new();

    let Ok(entries) = fs::read_dir(path) else {
        return Vec::new();
    };

    for entry in entries.flatten() {
        let entry_path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if entry_path.is_dir() {
            dirs.push(ProjectEntry::Directory {
                path: entry_path.clone(),
                name,
                children: read_children(&entry_path),
            });
        } else {
            files.push(ProjectEntry::File {
                path: entry_path,
                name,
            });
        }
    }

    dirs.sort_by(|a, b| entry_name(a).cmp(&entry_name(b)));
    files.sort_by(|a, b| entry_name(a).cmp(&entry_name(b)));

    dirs.into_iter().chain(files).collect()
}

fn entry_name(entry: &ProjectEntry) -> &str {
    match entry {
        ProjectEntry::Directory { name, .. } | ProjectEntry::File { name, .. } => name,
    }
}

pub fn is_gds_renderable(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase()),
        Some(ext) if ext == "gds" || ext == "oas" || ext == "oasisc" || ext == "gdsii"
    )
}
