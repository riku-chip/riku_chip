from dataclasses import dataclass, field
from pathlib import Path

import pygit2


@dataclass
class CommitInfo:
    oid: str
    short_id: str
    message: str
    author: str
    timestamp: int  # Unix epoch


@dataclass
class ChangedFile:
    path: str
    status: str  # "added" | "removed" | "modified" | "renamed"
    old_path: str = ""  # solo para renamed


LARGE_BLOB_THRESHOLD = 50 * 1024 * 1024  # 50 MB


class GitService:
    """
    Acceso a objetos Git via pygit2 (libgit2 — sin subprocess).
    Todos los métodos reciben y devuelven datos — nunca modifican el working tree.
    """

    def __init__(self, repo_path: str | Path = "."):
        path = pygit2.discover_repository(str(repo_path))
        self._repo = pygit2.Repository(path)

    # ------------------------------------------------------------------
    # Blobs
    # ------------------------------------------------------------------

    def get_blob(self, commit_ish: str, file_path: str) -> bytes:
        """
        Extrae el contenido de un archivo en un commit dado.
        Para blobs >50MB escribe a .riku/tmp/ y lanza LargeBlobError
        con la ruta, evitando cargar el archivo entero en RAM de Python.
        """
        commit = self._resolve_commit(commit_ish)
        entry = self._tree_entry(commit.tree, file_path)
        blob: pygit2.Blob = self._repo.get(entry.id)

        if blob.size > LARGE_BLOB_THRESHOLD:
            tmp_path = self._dump_large_blob(commit_ish, file_path, blob)
            raise LargeBlobError(file_path, blob.size, tmp_path)

        return blob.data

    def _dump_large_blob(self, commit_ish: str, file_path: str, blob: pygit2.Blob) -> Path:
        short = commit_ish[:7]
        filename = Path(file_path).name
        tmp_dir = Path(self._repo.workdir) / ".riku" / "tmp"
        tmp_dir.mkdir(parents=True, exist_ok=True)
        out = tmp_dir / f"{short}_{filename}"
        out.write_bytes(blob.data)
        return out

    # ------------------------------------------------------------------
    # Historial
    # ------------------------------------------------------------------

    def get_commits(self, file_path: str | None = None) -> list[CommitInfo]:
        """
        Lista commits del branch actual.
        Si file_path se especifica, filtra solo commits que tocaron ese archivo.
        """
        head = self._repo.head.target
        walker = self._repo.walk(head, pygit2.GIT_SORT_TIME)

        results = []
        for commit in walker:
            if file_path and not self._commit_touches(commit, file_path):
                continue
            results.append(CommitInfo(
                oid=str(commit.id),
                short_id=str(commit.id)[:7],
                message=commit.message.strip(),
                author=commit.author.name,
                timestamp=commit.author.time,
            ))

        return results

    def _commit_touches(self, commit: pygit2.Commit, file_path: str) -> bool:
        if not commit.parents:
            try:
                self._tree_entry(commit.tree, file_path)
                return True
            except KeyError:
                return False

        parent = commit.parents[0]
        diff = parent.tree.diff_to_tree(commit.tree)
        for delta in diff.deltas:
            if delta.new_file.path == file_path or delta.old_file.path == file_path:
                return True
        return False

    # ------------------------------------------------------------------
    # Diff de archivos entre dos commits
    # ------------------------------------------------------------------

    def get_changed_files(self, commit_a: str, commit_b: str) -> list[ChangedFile]:
        """Retorna los archivos que cambiaron entre dos commits."""
        tree_a = self._resolve_commit(commit_a).tree
        tree_b = self._resolve_commit(commit_b).tree
        diff = tree_a.diff_to_tree(tree_b)

        results = []
        for delta in diff.deltas:
            status = _DELTA_STATUS.get(delta.status, "modified")
            results.append(ChangedFile(
                path=delta.new_file.path,
                status=status,
                old_path=delta.old_file.path if status == "renamed" else "",
            ))
        return results

    # ------------------------------------------------------------------
    # Helpers internos
    # ------------------------------------------------------------------

    def _resolve_commit(self, commit_ish: str) -> pygit2.Commit:
        obj = self._repo.revparse_single(commit_ish)
        if isinstance(obj, pygit2.Tag):
            obj = obj.peel(pygit2.Commit)
        if not isinstance(obj, pygit2.Commit):
            raise ValueError(f"'{commit_ish}' no resuelve a un commit")
        return obj

    def _tree_entry(self, tree: pygit2.Tree, file_path: str) -> pygit2.TreeEntry:
        parts = Path(file_path).parts
        node = tree
        for part in parts[:-1]:
            node = self._repo.get(node[part].id)
        entry = node[parts[-1]]
        return entry


# ------------------------------------------------------------------
# Excepciones
# ------------------------------------------------------------------

class LargeBlobError(Exception):
    def __init__(self, path: str, size: int, tmp_path: Path):
        self.path = path
        self.size = size
        self.tmp_path = tmp_path
        super().__init__(
            f"{path} ({size / 1024 / 1024:.1f} MB) — escrito a {tmp_path}"
        )


_DELTA_STATUS = {
    pygit2.GIT_DELTA_ADDED: "added",
    pygit2.GIT_DELTA_DELETED: "removed",
    pygit2.GIT_DELTA_MODIFIED: "modified",
    pygit2.GIT_DELTA_RENAMED: "renamed",
}
