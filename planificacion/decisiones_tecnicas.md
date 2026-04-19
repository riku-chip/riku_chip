# Decisiones Técnicas — Riku

Registro de decisiones de implementación, fixes y su justificación.
Se actualiza a medida que avanza el desarrollo.

---

## Git integration — pygit2 vs subprocess

**Decisión:** usar `pygit2` (bindings a libgit2) en lugar de `subprocess git` o `gitpython`.

**Por qué:**
- `subprocess git` tiene ~50-200ms de overhead fijo por llamada (fork de proceso). Inaceptable para archivos GDS grandes donde ya hay latencia de I/O.
- `gitpython` es un wrapper de subprocess — mismo problema.
- `pygit2` accede directamente a la base de datos de objetos Git via libgit2 (~0.5ms por blob).
- Permite manejar blobs >50MB sin cargar todo en RAM de Python (escribir a `.riku/tmp/`).

**Trade-offs aceptados:**
- Requiere wheel precompilado de libgit2 en Windows — ya declarado en `pyproject.toml`.
- La API de pygit2 es menos legible que un comando git, pero el rendimiento lo justifica.

---

## Cache de versión de Xschem — class-level vs instance-level

**Decisión:** `XschemDriver._cached_info` es un atributo de clase, no de instancia.

**Fix aplicado:** commit `1634271`

**Problema:** `render()` llamaba `self.info()` en cada invocación, incluso en cache hits. `info()` ejecutaba `subprocess.run(["xschem", "--version"])` cada vez — 335ms de overhead por llamada.

**Por qué class-level:** xschem es una herramienta del sistema, su versión no cambia entre instancias del driver ni durante la vida del proceso. Un cache de instancia requeriría que el mismo objeto sobreviva entre llamadas, lo cual no está garantizado.

**Resultado:** cache hit bajó de 335ms → 0.24ms (speedup ~1400x).

---

## Clave de cache SHA256 — sin `.hex()`

**Decisión:** `hashlib.sha256(version.encode() + b"::" + content)` en lugar de `hashlib.sha256(f"{version}::{content.hex()}".encode())`.

**Fix aplicado:** commit `1634271`

**Por qué:** `content.hex()` convierte cada byte en 2 caracteres ASCII, duplicando el tamaño del dato antes de hashearlo. Para un archivo de 200KB esto genera 400KB de string intermedio en heap. La concatenación de bytes es directa y no genera copia.

---

## Adapter genérico — sin paths hardcodeados de iic-osic-tools

**Decisión:** `XschemDriver` depende solo de que `xschem` esté en el PATH. No ejecuta `sak-pdk sky130A` ni asume rutas de Docker.

**Por qué:** `sak-pdk` es configuración del entorno del usuario, no responsabilidad de Riku. Hardcodear rutas de iic-osic-tools rompería el adapter en cualquier otra instalación de xschem (Arch Linux, Nix, conda-forge).

**Consecuencia:** el usuario es responsable de configurar su entorno antes de invocar Riku.

---

## Orquestador de diff — función libre en `analyzer.py`

**Decisión:** la capa que conecta `GitService` + `registry` + `DriverDiffReport` será una función libre en `riku/core/analyzer.py`, no un método de `GitService`.

**Por qué:** `GitService` no debe conocer drivers EDA — su responsabilidad es acceso a objetos Git. Mezclar ambas responsabilidades violaría separación de concerns y dificultaría testear cada capa por separado.

**Interfaz planeada:**
```python
def analyze_diff(repo_path, commit_a, commit_b, file_path) -> DriverDiffReport
```

---

## Encoding UTF-8 en scripts de terminal (Windows)

**Fix aplicado:** `sys.stdout.reconfigure(encoding="utf-8", errors="replace")` al inicio de cada script de test/benchmark.

**Por qué:** la consola de Windows usa cp1252 por defecto. Los mensajes de commit con tildes (é, ó, ñ) lanzaban `UnicodeEncodeError` o se mostraban como `?`. El reconfigure fuerza UTF-8 sin romper el output en ningún entorno.

---

## Blobs grandes (>50MB) — escritura a `.riku/tmp/`

**Decisión:** si un blob supera 50MB, `GitService.get_blob()` lo escribe a `.riku/tmp/<short_id>_<filename>` y lanza `LargeBlobError` con la ruta.

**Por qué:** cargar un GDS de 200MB en RAM de Python para luego pasárselo a KLayout (que también lo cargará) duplica el uso de memoria innecesariamente. El caller puede decidir si usar la ruta del archivo temporal o ignorar el error.

**Threshold:** 50MB — valor conservador que cubre todos los .sch y la mayoría de .gds pequeños, pero protege contra GDS de chips completos.
