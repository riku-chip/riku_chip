import hashlib
import shutil
import subprocess
from pathlib import Path

from riku.core.driver import RikuDriver, DriverInfo, DriverDiffReport, DiffEntry
from riku.parsers.xschem import detect_format, parse
from riku.core.semantic_diff import diff as semantic_diff

CACHE_DIR = Path.home() / ".cache" / "riku" / "ops"


class XschemDriver(RikuDriver):

    _cached_info: DriverInfo | None = None  # cache de version a nivel de clase

    def info(self) -> DriverInfo:
        if XschemDriver._cached_info is not None:
            return XschemDriver._cached_info
        xschem = shutil.which("xschem")
        if not xschem:
            return DriverInfo(
                name="xschem", available=False, version="",
                extensions=[".sch"]
            )
        try:
            result = subprocess.run(
                [xschem, "--version"],
                capture_output=True, text=True, timeout=10
            )
            for line in (result.stdout + result.stderr).splitlines():
                if "XSCHEM V" in line:
                    XschemDriver._cached_info = DriverInfo(
                        name="xschem", available=True, version=line.strip(),
                        extensions=[".sch"]
                    )
                    return XschemDriver._cached_info
        except Exception:
            pass
        XschemDriver._cached_info = DriverInfo(
            name="xschem", available=True, version="unknown", extensions=[".sch"]
        )
        return XschemDriver._cached_info

    def diff(self, content_a: bytes, content_b: bytes, path_hint: str = "") -> DriverDiffReport:
        report = DriverDiffReport(file_type="xschem")

        if detect_format(content_a) != "xschem":
            report.warnings.append(f"{path_hint}: no es formato Xschem, usando diff de texto.")
            return report

        result = semantic_diff(content_a, content_b)

        for cd in result.components:
            report.changes.append(DiffEntry(
                kind=cd.kind,
                element=cd.name,
                before=cd.before,
                after=cd.after,
            ))

        for net in result.nets_added:
            report.changes.append(DiffEntry(kind="added", element=f"net:{net}"))
        for net in result.nets_removed:
            report.changes.append(DiffEntry(kind="removed", element=f"net:{net}"))

        if result.is_move_all:
            report.changes.append(DiffEntry(
                kind="modified", element="layout",
                cosmetic=True,
                after={"note": "reorganizacion cosmetica (Move All)"}
            ))

        return report

    def normalize(self, content: bytes, path_hint: str = "") -> bytes:
        # .sch no tiene timestamps ni ruido que normalizar — retornar tal cual
        return content

    def render(self, content: bytes, path_hint: str = "") -> Path | None:
        xschem = shutil.which("xschem")
        if not xschem:
            return None

        info = self.info()
        key = hashlib.sha256(info.version.encode() + b"::" + content).hexdigest()
        cached = CACHE_DIR / key / "render.svg"

        if cached.exists():
            return cached

        cached.parent.mkdir(parents=True, exist_ok=True)

        import tempfile
        with tempfile.NamedTemporaryFile(suffix=".sch", delete=False) as tmp:
            tmp.write(content)
            tmp_path = Path(tmp.name)

        origins_path = cached.parent / "origins.txt"

        try:
            # La ruta del archivo de origins se pasa via env var para evitar
            # problemas de escape de $ en el comando TCL inline con shell=True.
            env = {**__import__("os").environ, "RIKU_ORIGINS_PATH": str(origins_path)}
            cmd = (
                f"{xschem} --tcl 'wm iconify .' "
                f"--command 'xschem zoom_full;"
                f" set _f [open $env(RIKU_ORIGINS_PATH) w];"
                f" puts $_f [xschem get xorigin];"
                f" puts $_f [xschem get yorigin];"
                f" close $_f;"
                f" xschem print svg {cached}' "
                f"--quit {tmp_path}"
            )
            subprocess.run(cmd, shell=True, capture_output=True, timeout=30, env=env)
            return cached if cached.exists() else None
        except Exception:
            return None
        finally:
            tmp_path.unlink(missing_ok=True)
