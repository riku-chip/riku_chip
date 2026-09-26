//! Helper de pipeline "dos blobs → FileSummary".
//!
//! Centraliza la cola del flujo que comparten `status` y `log`: dado un
//! driver ya resuelto y los dos contenidos, computa el `FileChange`
//! y lo agrega como `FileSummary`. Los callers deciden cómo obtener los
//! bytes y qué hacer si el formato no tiene driver — la pre-resolución
//! del driver se mantiene fuera para no leer blobs innecesarios.

use crate::core::analysis::summary::{DetailLevel, FileSummary};
use crate::core::domain::driver::RikuDriver;

pub fn summarize(
    driver: &dyn RikuDriver,
    before: &[u8],
    after: &[u8],
    path: &str,
    level: DetailLevel,
) -> FileSummary {
    let report = driver.diff(before, after, path);
    FileSummary::from_report_with(&report, path, level)
}
