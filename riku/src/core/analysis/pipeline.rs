//! El paso final de [`diff_pair`](super::diff_pair): dos versiones ya leídas
//! de un archivo → `FileChange`.

use riku_kernel::{DiffFiles, DiffOptions, FileChange, FormatModule};

use crate::core::analysis::blob_io::Blob;

/// El diff del módulo. Si una versión existe pero no se pudo leer (muy
/// grande, ilegible), el archivo queda con error sin llamar al módulo: como
/// vacío, el diff diría que se borró o se agregó todo.
///
/// `files`: los otros archivos de cada versión, para los formatos que los
/// necesitan (Magic).
pub fn diff_blobs(
    module: &dyn FormatModule,
    before: &Blob,
    after: &Blob,
    path: &str,
    opts: &DiffOptions,
    files: &DiffFiles,
) -> FileChange {
    if let Some(why) = before.skipped().or(after.skipped()) {
        return FileChange::failed(module.info().format, why);
    }
    module.diff_with(before.bytes(), after.bytes(), path, opts, files)
}

#[cfg(test)]
mod tests {
    use riku_kernel::{FileFormat, ModuleInfo};

    use super::*;
    use crate::core::analysis::summary::{DetailLevel, FileSummary, SummaryCategory};

    fn summarize(before: &Blob, after: &Blob) -> FileSummary {
        let r = diff_blobs(&Warns, before, after, "a.gds", &DiffOptions::default(), &DiffFiles::default());
        FileSummary::from_report_with(&r, "a.gds", DetailLevel::Resumen)
    }

    /// Un módulo que solo avisa (como Magic con una celda que falta).
    struct Warns;

    impl FormatModule for Warns {
        fn info(&self) -> ModuleInfo {
            ModuleInfo { name: "w".into(), version: "test".into(), format: FileFormat::Gds, extensions: vec![".w".into()], available: true }
        }
        fn detect(&self, _: &[u8]) -> bool {
            true
        }
        fn diff(&self, before: &[u8], after: &[u8], _: &str, _: &DiffOptions) -> FileChange {
            assert!(!before.is_empty() || !after.is_empty(), "un lado omitido no debe llegar como vacío");
            let mut f = FileChange::new(FileFormat::Gds);
            f.warnings.push("falta la celda inv".into());
            f
        }
    }

    #[test]
    fn un_lado_omitido_es_error_y_no_llega_al_modulo() {
        let big = Blob::Skipped("a.gds: 80 MB, más que el límite de 50 MB; no se compara".into());
        let r = diff_blobs(&Warns, &Blob::Missing, &big, "a.gds", &DiffOptions::default(), &DiffFiles::default());
        assert!(r.error.as_deref().is_some_and(|e| e.contains("80 MB")), "{r:?}");
        assert_eq!(r.format, FileFormat::Gds);
        let s = summarize(&big, &Blob::Bytes(b"x".to_vec()));
        assert_eq!(s.category, SummaryCategory::Error);
    }

    #[test]
    fn los_avisos_llegan_al_resumen() {
        let x = Blob::Bytes(b"x".to_vec());
        let s = summarize(&x, &x);
        assert_eq!(s.category, SummaryCategory::Unchanged);
        assert_eq!(s.warnings, vec!["falta la celda inv".to_string()]);
    }
}
