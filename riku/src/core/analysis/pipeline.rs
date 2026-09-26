//! Helper de pipeline "dos blobs → FileSummary".
//!
//! Centraliza la cola del flujo que comparten `status` y `log`: dado un
//! módulo ya resuelto y los dos contenidos, computa el `FileChange` y lo
//! agrega como `FileSummary`. Los callers deciden cómo obtener los bytes y
//! qué hacer si el formato no tiene módulo — la pre-resolución se mantiene
//! fuera para no leer blobs innecesarios.

use riku_kernel::{DiffOptions, FormatModule};

use crate::core::analysis::summary::{DetailLevel, FileSummary};

pub fn summarize(
    module: &dyn FormatModule,
    before: &[u8],
    after: &[u8],
    path: &str,
    level: DetailLevel,
) -> FileSummary {
    let report = module.diff(before, after, path, &DiffOptions::default());
    FileSummary::from_report_with(&report, path, level)
}
