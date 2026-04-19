"""
Test de analyze_diff contra commits reales del repo.
Uso: python tests/test_analyzer.py
"""
import sys
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from riku.core.git_service import GitService
from riku.core.analyzer import analyze_diff


def main():
    svc = GitService(".")
    commits = svc.get_commits("riku/adapters/xschem_driver.py")

    if len(commits) < 2:
        print("No hay suficientes commits para hacer diff.")
        return

    commit_new = commits[0].oid
    commit_old = commits[1].oid
    file_path = "riku/adapters/xschem_driver.py"

    print(f"Diffing {file_path}")
    print(f"  A: {commits[1].short_id} — {commits[1].message[:60]}")
    print(f"  B: {commits[0].short_id} — {commits[0].message[:60]}")
    print()

    report = analyze_diff(".", commit_old, commit_new, file_path)

    print(f"Tipo de archivo: {report.file_type}")
    print(f"Cambios: {len(report.changes)}")
    print(f"Warnings: {report.warnings or 'ninguno'}")
    print(f"is_empty: {report.is_empty()}")

    for change in report.changes:
        cosmetic = " [cosmético]" if change.cosmetic else ""
        print(f"  [{change.kind:<8}] {change.element}{cosmetic}")


if __name__ == "__main__":
    main()
