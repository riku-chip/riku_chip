"""
Test basico de GitService contra el repo de Riku.
Uso: python tests/test_git_service.py
"""
import sys
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="replace")

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from riku.core.git_service import GitService


def main():
    svc = GitService(".")

    # --- commits del repo ---
    commits = svc.get_commits()
    print(f"Total commits: {len(commits)}")
    for c in commits[:5]:
        print(f"  {c.short_id}  {c.author:<20}  {c.message[:60]}")

    # --- commits que tocaron xschem_driver.py ---
    print()
    touched = svc.get_commits("riku/adapters/xschem_driver.py")
    print(f"Commits que tocaron xschem_driver.py: {len(touched)}")
    for c in touched:
        print(f"  {c.short_id}  {c.message[:60]}")

    # --- archivos cambiados entre los dos ultimos commits ---
    if len(commits) >= 2:
        print()
        changed = svc.get_changed_files(commits[1].oid, commits[0].oid)
        print(f"Archivos cambiados en el ultimo commit ({commits[0].short_id}):")
        for f in changed:
            print(f"  [{f.status:<8}] {f.path}")

    # --- extraer blob del ultimo commit ---
    print()
    blob = svc.get_blob(commits[0].oid, "riku/adapters/xschem_driver.py")
    print(f"Blob extraido: riku/adapters/xschem_driver.py — {len(blob)} bytes")
    print("  primera linea:", blob.splitlines()[0].decode())


if __name__ == "__main__":
    main()
