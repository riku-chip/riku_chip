"""
Benchmark de velocidad del parser y diff semantico de Riku.
Uso: python tests/benchmark_speed.py

Mide:
  1. Deteccion de formato
  2. Parseo de .sch (por tamano de archivo)
  3. Diff semantico entre dos revisiones
  4. Render SVG — primera llamada vs hit de cache
"""
import sys
import time
from pathlib import Path
from dataclasses import asdict

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from riku.parsers.xschem import detect_format, parse
from riku.core.semantic_diff import diff
from riku.adapters.xschem_driver import XschemDriver

DESIGNS = Path("/foss/designs")

FIXTURES = [
    ("pequeño (~100 líneas)",   "gilbert/multiplicador_bueno_elmejor_18_11_2023.sch"),
    ("mediano (~240 líneas)",   "prueba/memristor/schematics/prueba14M.sch"),
    ("grande (~9200 líneas)",   "OpenRAM/ciel/sky130/versions/e8294524e5f67c533c5d0c3afa0bcc5b2a5fa066/sky130A/libs.tech/xschem/xschem_verilog_import/audiodac.sch"),
    ("muy grande (~10500 líneas)", "OpenRAM/ciel/sky130/versions/e8294524e5f67c533c5d0c3afa0bcc5b2a5fa066/sky130A/libs.tech/xschem/sky130_tests/test_nmos.sch"),
    ("masivo (~214500 líneas)", "OpenRAM/ciel/sky130/versions/e8294524e5f67c533c5d0c3afa0bcc5b2a5fa066/sky130A/libs.tech/xschem/decred_hash_macro/decred_hash_macro.sch"),
]

DIFF_PAIR = (
    "gilbert/multiplicador_bueno_elmejor_18_11_2023.sch",
    "gilbert/bode_multiplicador_bueno_2023.sch",
)

RENDER_TARGET = "gilbert/multiplicador_bueno_elmejor_18_11_2023.sch"

RUNS = 5  # repeticiones por medicion para promediar


def measure(fn, runs=RUNS):
    """Ejecuta fn N veces y retorna (promedio_ms, min_ms, max_ms)."""
    times = []
    for _ in range(runs):
        t0 = time.perf_counter()
        fn()
        times.append((time.perf_counter() - t0) * 1000)
    return sum(times) / len(times), min(times), max(times)


def bar(ms, scale=200):
    filled = int(ms / scale * 30)
    return "[" + "#" * filled + "-" * (30 - filled) + f"] {ms:.2f} ms"


def section(title):
    print(f"\n{'='*60}")
    print(f"  {title}")
    print(f"{'='*60}")


def main():
    print("\nRiku — Benchmark de Velocidad")
    print(f"Runs por medicion: {RUNS}")

    # ------------------------------------------------------------------
    # 1. Deteccion de formato
    # ------------------------------------------------------------------
    section("1. Deteccion de formato (detect_format)")
    path = DESIGNS / FIXTURES[0][1]
    if path.exists():
        content = path.read_bytes()
        avg, mn, mx = measure(lambda: detect_format(content))
        print(f"  {bar(avg, scale=5)}")
        print(f"  promedio={avg:.3f}ms  min={mn:.3f}ms  max={mx:.3f}ms")
    else:
        print(f"  [SKIP] archivo no encontrado: {path}")

    # ------------------------------------------------------------------
    # 2. Parseo por tamano
    # ------------------------------------------------------------------
    section("2. Parseo de .sch por tamano (parse)")
    print(f"  {'Tamano':<30} {'Componentes':>12} {'Promedio':>12} {'Min':>10} {'Max':>10}")
    print(f"  {'-'*78}")

    for label, rel_path in FIXTURES:
        path = DESIGNS / rel_path
        if not path.exists():
            print(f"  {label:<30} {'[no encontrado]':>12}")
            continue
        content = path.read_bytes()
        size_kb = len(content) / 1024

        result = parse(content)
        n_components = len(result.components)

        avg, mn, mx = measure(lambda c=content: parse(c))
        print(f"  {label:<30} {n_components:>12} {avg:>10.2f}ms {mn:>10.2f}ms {mx:>10.2f}ms")
        print(f"  {'':30} {size_kb:>10.1f}KB")

    # ------------------------------------------------------------------
    # 3. Diff semantico
    # ------------------------------------------------------------------
    section("3. Diff semantico (semantic_diff)")
    path_a = DESIGNS / DIFF_PAIR[0]
    path_b = DESIGNS / DIFF_PAIR[1]

    if path_a.exists() and path_b.exists():
        content_a = path_a.read_bytes()
        content_b = path_b.read_bytes()
        result = diff(content_a, content_b)

        avg, mn, mx = measure(lambda: diff(content_a, content_b))
        print(f"  Cambios detectados: {len(result.components)} componentes, "
              f"{len(result.nets_added)} nets+, {len(result.nets_removed)} nets-")
        print(f"  Move All detectado: {result.is_move_all}")
        print(f"  {bar(avg, scale=10)}")
        print(f"  promedio={avg:.3f}ms  min={mn:.3f}ms  max={mx:.3f}ms")
    else:
        print("  [SKIP] archivos de diff no encontrados")

    # ------------------------------------------------------------------
    # 4. Render SVG — primera llamada vs cache
    # ------------------------------------------------------------------
    section("4. Render SVG — primera llamada vs cache")
    driver = XschemDriver()
    info = driver.info()

    if not info.available:
        print("  [SKIP] Xschem no disponible en este entorno.")
    else:
        print(f"  Xschem: {info.version}")
        path = DESIGNS / RENDER_TARGET
        content = path.read_bytes()

        # Forzar cache miss borrando entrada si existe
        import hashlib
        from riku.adapters.xschem_driver import CACHE_DIR
        key = hashlib.sha256(f"{info.version}::{content.hex()}".encode()).hexdigest()
        cached = CACHE_DIR / key / "render.svg"

        if cached.exists():
            cached.unlink()
            print("  Cache limpiada para medir primera llamada real.")

        # Primera llamada (sin cache)
        t0 = time.perf_counter()
        svg = driver.render(content, RENDER_TARGET)
        first_call_ms = (time.perf_counter() - t0) * 1000

        if svg:
            print(f"  Primera llamada (sin cache): {first_call_ms:.0f} ms  →  {svg}")
        else:
            print(f"  Primera llamada: FALLO ({first_call_ms:.0f} ms)")

        # Segunda llamada (hit de cache)
        if svg:
            avg, mn, mx = measure(lambda: driver.render(content, RENDER_TARGET))
            print(f"  Hit de cache:    {bar(avg, scale=1000)}")
            print(f"  promedio={avg:.3f}ms  min={mn:.3f}ms  max={mx:.3f}ms")
            if first_call_ms > 0:
                speedup = first_call_ms / avg
                print(f"  Speedup cache: {speedup:.0f}x mas rapido")

    print(f"\n{'='*60}")
    print("  Benchmark completado.")
    print(f"{'='*60}\n")


if __name__ == "__main__":
    main()
