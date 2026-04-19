"""
Debug del overhead de cache en XschemDriver.render().
Mide cada paso por separado para identificar el cuello de botella.
"""
import sys
import time
import hashlib
import shutil
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from riku.adapters.xschem_driver import CACHE_DIR, XschemDriver

DESIGNS = Path("/foss/designs")
TARGET = DESIGNS / "gilbert/multiplicador_bueno_elmejor_18_11_2023.sch"
RUNS = 10


def ms(t0): return (time.perf_counter() - t0) * 1000


def avg(fn, runs=RUNS):
    times = []
    for _ in range(runs):
        t0 = time.perf_counter()
        fn()
        times.append(ms(t0))
    return sum(times)/len(times), min(times), max(times)


def section(title):
    print(f"\n{'='*55}")
    print(f"  {title}")
    print(f"{'='*55}")


def main():
    print("\nDebug de overhead de cache — XschemDriver.render()")

    driver = XschemDriver()
    info = driver.info()
    print(f"Xschem: {info.version}")
    print(f"Archivo: {TARGET.name} ({TARGET.stat().st_size / 1024:.1f} KB)")

    # -------------------------------------------------------
    section("Paso 1 — leer bytes del archivo")
    a, mn, mx = avg(lambda: TARGET.read_bytes())
    content = TARGET.read_bytes()
    print(f"  promedio={a:.3f}ms  min={mn:.3f}ms  max={mx:.3f}ms")

    # -------------------------------------------------------
    section("Paso 2 — shutil.which('xschem')")
    a, mn, mx = avg(lambda: shutil.which("xschem"))
    print(f"  promedio={a:.3f}ms  min={mn:.3f}ms  max={mx:.3f}ms")

    # -------------------------------------------------------
    section("Paso 3 — calcular SHA256 del contenido")
    a, mn, mx = avg(lambda: hashlib.sha256(content).hexdigest())
    print(f"  promedio={a:.3f}ms  min={mn:.3f}ms  max={mx:.3f}ms")
    print(f"  (archivo: {len(content)} bytes)")

    # -------------------------------------------------------
    section("Paso 4 — construir clave de cache completa")
    version = info.version
    def make_key():
        return hashlib.sha256(f"{version}::{content.hex()}".encode()).hexdigest()

    a, mn, mx = avg(make_key)
    key = make_key()
    print(f"  promedio={a:.3f}ms  min={mn:.3f}ms  max={mx:.3f}ms")
    print(f"  NOTA: content.hex() convierte {len(content)} bytes a string hex")
    print(f"  String hex resultante: {len(content.hex())} chars")

    # -------------------------------------------------------
    section("Paso 4b — clave con SHA256 directo (sin .hex())")
    def make_key_fast():
        h = hashlib.sha256(version.encode() + b"::" + content).hexdigest()
        return h

    a, mn, mx = avg(make_key_fast)
    print(f"  promedio={a:.3f}ms  min={mn:.3f}ms  max={mx:.3f}ms")
    print(f"  (evita convertir {len(content)} bytes a hex string)")

    # -------------------------------------------------------
    section("Paso 5 — verificar si cached.exists()")
    cached = CACHE_DIR / key / "render.svg"
    a, mn, mx = avg(lambda: cached.exists())
    print(f"  promedio={a:.3f}ms  min={mn:.3f}ms  max={mx:.3f}ms")
    print(f"  existe: {cached.exists()}")

    # -------------------------------------------------------
    section("Paso 6 — driver.render() completo (hit de cache)")
    if not cached.exists():
        print("  [SKIP] No hay cache — corre el benchmark primero.")
    else:
        a, mn, mx = avg(lambda: driver.render(content, TARGET.name))
        print(f"  promedio={a:.3f}ms  min={mn:.3f}ms  max={mx:.3f}ms")

    # -------------------------------------------------------
    section("Resumen — donde esta el tiempo")
    print()
    steps = {}

    t0 = time.perf_counter(); content2 = TARGET.read_bytes(); steps["read_bytes"] = ms(t0)
    t0 = time.perf_counter(); shutil.which("xschem"); steps["which"] = ms(t0)
    t0 = time.perf_counter(); content2.hex(); steps["content.hex()"] = ms(t0)
    t0 = time.perf_counter(); hashlib.sha256(version.encode() + b"::" + content2).hexdigest(); steps["sha256_fast"] = ms(t0)
    t0 = time.perf_counter(); cached.exists(); steps["path.exists()"] = ms(t0)

    total = sum(steps.values())
    for step, t in steps.items():
        bar = "#" * int(t / total * 30)
        print(f"  {step:<20} {t:>8.3f}ms  |{bar}")
    print(f"  {'TOTAL':<20} {total:>8.3f}ms")


if __name__ == "__main__":
    main()
