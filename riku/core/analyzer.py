from pathlib import Path

from riku.core.git_service import GitService, LargeBlobError
from riku.core.registry import get_driver_for
from riku.core.driver import DriverDiffReport


def analyze_diff(
    repo_path: str | Path,
    commit_a: str,
    commit_b: str,
    file_path: str,
) -> DriverDiffReport:
    """
    Extrae dos revisiones de un archivo desde Git y retorna un diff semantico.

    Si el archivo no existia en commit_a o commit_b, usa b"" para ese lado
    (el driver lo interpreta como todos los elementos added/removed).
    """
    svc = GitService(repo_path)
    driver = get_driver_for(file_path)

    if driver is None:
        report = DriverDiffReport(file_type="unknown")
        report.warnings.append(f"{file_path}: no hay driver disponible para este formato.")
        return report

    content_a = _safe_get_blob(svc, commit_a, file_path)
    content_b = _safe_get_blob(svc, commit_b, file_path)

    report = driver.diff(content_a, content_b, path_hint=file_path)

    if isinstance(content_a, LargeBlobError):
        report.warnings.append(str(content_a))
    if isinstance(content_b, LargeBlobError):
        report.warnings.append(str(content_b))

    return report


def _safe_get_blob(svc: GitService, commit_ish: str, file_path: str) -> bytes:
    """
    Retorna el blob como bytes.
    - Si el archivo no existe en ese commit: retorna b"".
    - Si el blob es demasiado grande: retorna b"" y deja que el caller maneje LargeBlobError.
    """
    try:
        return svc.get_blob(commit_ish, file_path)
    except KeyError:
        return b""
    except LargeBlobError:
        raise
