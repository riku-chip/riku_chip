import sys
sys.stdout.reconfigure(encoding="utf-8", errors="replace")

import json
import typer
from enum import Enum
from typing import Optional
from pathlib import Path

app = typer.Typer(help="Riku - VCS semantico para diseno de chips.")


class Format(str, Enum):
    text = "text"
    json = "json"
    visual = "visual"


@app.command()
def diff(
    commit_a: str = typer.Argument(..., help="Commit base (mas antiguo)"),
    commit_b: str = typer.Argument(..., help="Commit destino (mas nuevo)"),
    file_path: str = typer.Argument(..., help="Ruta al archivo dentro del repo"),
    repo: str = typer.Option(".", "--repo", "-r", help="Ruta al repositorio Git"),
    fmt: Format = typer.Option(Format.text, "--format", "-f", help="Formato de salida: text, json, visual"),
):
    """Muestra los cambios semanticos de un archivo entre dos commits."""
    from riku.core.analyzer import analyze_diff
    from riku.core.git_service import GitService
    from riku.core.registry import get_driver_for

    report = analyze_diff(repo, commit_a, commit_b, file_path)

    for w in report.warnings:
        typer.echo(f"[!] {w}", err=True)

    if fmt == Format.json:
        _output_json(report)
        return

    if fmt == Format.visual:
        _output_visual(repo, commit_a, commit_b, file_path, report)
        return

    # text (default)
    if report.is_empty():
        typer.echo("Sin cambios semanticos.")
        return

    typer.echo(f"Archivo: {file_path}  ({report.file_type})")
    typer.echo(f"Cambios: {len(report.changes)}")
    typer.echo("")

    for change in report.changes:
        cosmetic = "  [cosmetico]" if change.cosmetic else ""
        typer.echo(f"  {change.kind:<10} {change.element}{cosmetic}")


def _output_json(report):
    from riku.core.driver import DriverDiffReport
    data = {
        "file_type": report.file_type,
        "warnings": report.warnings,
        "changes": [
            {
                "kind": c.kind,
                "element": c.element,
                "cosmetic": c.cosmetic,
                "before": c.before,
                "after": c.after,
            }
            for c in report.changes
        ],
    }
    typer.echo(json.dumps(data, indent=2, ensure_ascii=False))


def _output_visual(repo: str, commit_a: str, commit_b: str, file_path: str, report):
    import tempfile, subprocess, os
    from riku.core.git_service import GitService
    from riku.core.registry import get_driver_for
    from riku.core.models import DiffReport, ComponentDiff
    from riku.core.svg_annotator import annotate
    from riku.parsers.xschem import parse

    svc = GitService(repo)
    driver = get_driver_for(file_path)

    if driver is None:
        typer.echo("[!] No hay driver visual para este formato.", err=True)
        raise typer.Exit(code=1)

    try:
        content_b = svc.get_blob(commit_b, file_path)
    except KeyError:
        typer.echo(f"[!] {file_path} no existe en {commit_b[:7]}.", err=True)
        raise typer.Exit(code=1)

    svg_path = driver.render(content_b, file_path)
    if svg_path is None:
        typer.echo("[!] Render no disponible (herramienta EDA no instalada).", err=True)
        raise typer.Exit(code=1)

    sch_b = parse(content_b)

    try:
        content_a = svc.get_blob(commit_a, file_path)
        sch_a = parse(content_a)
    except KeyError:
        sch_a = None

    diff_report = DiffReport(
        components=[
            ComponentDiff(name=c.element, kind=c.kind)
            for c in report.changes
            if not c.element.startswith("net:")
        ],
        nets_added=[c.element[4:] for c in report.changes if c.kind == "added"   and c.element.startswith("net:")],
        nets_removed=[c.element[4:] for c in report.changes if c.kind == "removed" and c.element.startswith("net:")],
    )

    svg_content = svg_path.read_text(encoding="utf-8", errors="replace")
    annotated = annotate(svg_content, sch_b, diff_report, sch_a=sch_a, svg_path=svg_path)

    with tempfile.NamedTemporaryFile(suffix=".svg", delete=False) as f:
        f.write(annotated.encode("utf-8"))
        out = f.name

    typer.echo(f"SVG anotado: {out}")

    # Abrir en el visor del sistema
    if sys.platform == "win32":
        os.startfile(out)
    elif sys.platform == "darwin":
        subprocess.run(["open", out])
    else:
        subprocess.run(["xdg-open", out])


@app.command()
def log(
    file_path: Optional[str] = typer.Argument(None, help="Filtrar por archivo (opcional)"),
    repo: str = typer.Option(".", "--repo", "-r", help="Ruta al repositorio Git"),
    limit: int = typer.Option(20, "--limit", "-n", help="Maximo de commits a mostrar"),
    semantic: bool = typer.Option(False, "--semantic", "-s", help="Mostrar resumen semantico por commit"),
):
    """Lista el historial de commits, opcionalmente filtrado por archivo."""
    from riku.core.git_service import GitService
    from riku.core.analyzer import analyze_diff

    svc = GitService(repo)
    commits = svc.get_commits(file_path)[:limit]

    if not commits:
        typer.echo("Sin commits encontrados.")
        return

    for i, c in enumerate(commits):
        typer.echo(f"{c.short_id}  {c.author:<20}  {c.message[:60]}")

        if semantic and file_path and i + 1 < len(commits):
            try:
                report = analyze_diff(repo, commits[i + 1].oid, c.oid, file_path)
                if not report.is_empty():
                    added = sum(1 for ch in report.changes if ch.kind == "added" and not ch.cosmetic)
                    removed = sum(1 for ch in report.changes if ch.kind == "removed" and not ch.cosmetic)
                    modified = sum(1 for ch in report.changes if ch.kind == "modified" and not ch.cosmetic)
                    parts = []
                    if added:   parts.append(f"+{added}")
                    if removed: parts.append(f"-{removed}")
                    if modified: parts.append(f"~{modified}")
                    typer.echo(f"           {'  '.join(parts)}")
            except Exception:
                pass


@app.command()
def doctor(
    repo: str = typer.Option(".", "--repo", "-r", help="Ruta al repositorio Git"),
):
    """Verifica que herramientas EDA estan disponibles."""
    from riku.core.registry import get_drivers

    drivers = get_drivers()
    any_missing = False

    for driver in drivers:
        info = driver.info()
        status = "[ok]" if info.available else "[x]"
        version = f"  {info.version}" if info.available else "  no encontrado"
        typer.echo(f"  {status}  {info.name:<12}{version}")
        if not info.available:
            any_missing = True

    if any_missing:
        raise typer.Exit(code=1)


def main():
    app()
