# Arquitectura CLI y Orquestación — Riku

> VCS especializado para diseño de chips sobre Git.  
> Herramientas soportadas: KLayout, Xschem, NGSpice, Magic VLSI.  
> Prioridades: rendimiento, artefactos mínimos, UX práctica.

---

## 1. Diseño de comandos

### `miku diff`

Compara dos revisiones de un archivo de diseño. Detecta automáticamente el tipo y despacha al driver correcto. Es el comando más crítico: cubre la mayoría de la fricción diaria.

```bash
# Diff del working tree contra HEAD
miku diff mycell.mag
miku diff top.sch
miku diff chip.gds

# Diff entre dos commits
miku diff HEAD~2 HEAD mycell.mag
miku diff abc123..def456 amp.spice

# Diff de todos los archivos de diseño modificados
miku diff

# Forzar modo de salida
miku diff --format=text top.sch        # siempre texto plano
miku diff --format=json top.sch        # JSON estructurado (para scripts/CI)
miku diff --format=visual mycell.mag   # abre viewer o genera PNG
miku diff --format=png top.sch         # PNG forzado (útil en CI sin display)

# Controlar qué tipo de diff hacer en GDS
miku diff --mode=structural chip.gds   # LayoutDiff (jerarquía, más rápido)
miku diff --mode=xor chip.gds          # XOR geométrico (verificación física)

# Diff de simulación: comparar resultados .meas entre commits
miku diff --sim abc123..def456 amp.spice
```

**Output por tipo de archivo:**

| Archivo | `--format=text` | `--format=json` | `--format=visual` |
|---|---|---|---|
| `.sch` | git diff estándar (ya legible) | objetos añadidos/eliminados | SVG side-by-side |
| `.mag` | diff sin timestamps | geometría por capa | PNG overlay |
| `.sp/.spice` | diff canonicalizado | diff topológico via Netgen | — |
| `.gds` | salida de `strmcmp` | LayoutDiff JSON | PNG de XOR regions |

---

### `miku merge`

Merge de archivos de diseño. Para formatos texto (`.sch`, `.mag`, `.spice`) se apoya en git merge con drivers personalizados. Para GDS binario el merge automático no es viable; el comando reporta el conflicto y ofrece herramientas de resolución.

```bash
# Merge estándar (delega a git, con pre/post hooks de Riku)
miku merge feature-branch

# Estrategia por tipo
miku merge --strategy=ours feature-branch    # en conflicto, preferir rama actual
miku merge --strategy=theirs feature-branch  # en conflicto, preferir rama incoming

# Para GDS: merge manual asistido
miku merge feature-branch chip.gds
# → "GDS merge automático no soportado. Conflicto en chip.gds."
# → "Ejecuta `miku merge --resolve chip.gds` para abrir KLayout con ambas versiones."

miku merge --resolve chip.gds
# Abre KLayout con A y B como dos layouts separados para merge manual.
# Cuando el usuario guarda el resultado, miku marca el conflicto resuelto.

# Verificar que el merge no rompe LVS (netlists .sch vs .sp)
miku merge feature-branch --check-lvs
```

**Flags:**

| Flag | Efecto |
|---|---|
| `--strategy=ours\|theirs` | Resolución automática en conflictos |
| `--resolve <file>` | Abrir herramienta de resolución visual |
| `--check-lvs` | Correr Netgen LVS post-merge |
| `--no-commit` | Dejar el resultado en staging sin commitear |
| `--dry-run` | Reportar conflictos esperados sin modificar nada |

---

### `miku blame`

Muestra quién modificó cada elemento de diseño. Para texto (`.sch`, `.spice`) es un wrapper de `git blame` con parsing del formato para mostrar información semántica. Para GDS es más limitado.

```bash
# Blame por línea (texto)
miku blame amp.spice
miku blame top.sch

# Blame semántico: mostrar quién tocó cada componente
miku blame --semantic top.sch
# → "C4 (capacitor, value=10u)   — alice  3 días atrás   commit abc123"
# → "M1 (nmos, W=2u L=180n)      — bob    2 semanas atrás commit def456"

# Blame de una celda en un layout
miku blame --cell=nand2 chip.gds
# → Busca en git log cuándo se modificó la celda nand2 y quién hizo el commit

# Blame de parámetro específico
miku blame --param=value amp.spice
# Muestra solo las líneas de parámetros y quién las modificó

miku blame --since="2 months ago" top.sch
```

**Output de `miku blame --semantic top.sch`:**

```
COMMIT   AUTOR    FECHA          OBJETO
abc123   alice    hace 3 días    C4 {name=C4 value=10u device="capacitor"}
def456   bob      hace 2 sem     M1 {name=M1 W=2u L=180n model=nmos}
abc123   alice    hace 3 días    N 890 -130 890 -110 {lab=ANALOG_GND}
```

---

### `miku log`

Historial de cambios con contexto de diseño. Filtra por tipo de cambio, herramienta, celda, o parámetro.

```bash
# Log estándar con tipo de archivo detectado
miku log
miku log -- mycell.mag

# Filtrar por tipo de cambio
miku log --type=schematic         # commits que tocaron .sch
miku log --type=layout            # commits que tocaron .mag o .gds
miku log --type=simulation        # commits que tocaron .spice/.sp

# Filtrar por celda o componente
miku log --cell=inverter          # commits que modificaron la celda "inverter"
miku log --component=R1           # commits donde el componente R1 cambió

# Mostrar estadísticas de cambio
miku log --stat                   # cuántos objetos añadidos/eliminados por commit
miku log --stat --format=json     # JSON para dashboards

# Comparar métricas de simulación a lo largo del tiempo
miku log --sim-metric=vmax amp.spice
# → tabla: commit | fecha | vmax
# abc123 | 2026-04-10 | 3.28V
# def456 | 2026-04-05 | 3.31V

# Integración con git log flags
miku log --since="1 week" --author=alice
miku log -n 10
```

**Output de `miku log --stat`:**

```
abc123  alice  2026-04-18  top.sch, mycell.mag
  .sch:  +2 componentes, -1 net
  .mag:  +4 rectángulos en metal1, -1 instancia

def456  bob    2026-04-15  amp.spice
  .spice: +1 componente (C3 100n), ~1 valor (R1: 1k→2k)
```

---

### `miku ci`

Orquesta el pipeline de CI para un repositorio de diseño. Puede correr localmente o generar configuración para GitHub Actions / GitLab CI.

```bash
# Correr el pipeline completo localmente
miku ci run

# Correr solo una etapa
miku ci run --stage=lint          # verificar formato y sintaxis
miku ci run --stage=lvs           # Netgen LVS
miku ci run --stage=sim           # NGSpice batch
miku ci run --stage=drc           # DRC con KLayout o Magic
miku ci run --stage=diff          # generar diff visual de los cambios

# Correr solo los archivos modificados (modo incremental)
miku ci run --changed-only

# Generar archivos de CI para plataformas
miku ci init --platform=github    # crea .github/workflows/miku.yml
miku ci init --platform=gitlab    # crea .gitlab-ci.yml

# Ver qué stages fallaron en el último run
miku ci status

# Subir artefactos (diffs visuales, reportes) a un PR
miku ci upload --pr=42
```

**Stages del pipeline (configurables en `miku.toml`):**

| Stage | Qué hace | Herramienta |
|---|---|---|
| `lint` | Verifica sintaxis de netlists y formatos | Python parsers |
| `normalize` | Canonicaliza .spice, strip timestamps .mag | Riku hooks |
| `lvs` | Layout vs. Schematic | Netgen |
| `drc` | Design Rule Check | KLayout / Magic |
| `sim` | Simula netlists modificados | NGSpice -b |
| `sim-compare` | Compara resultados vs. baseline | spicelib + numpy |
| `diff-visual` | Genera imágenes de diff para PR | KLayout / Xschem |

**Output de `miku ci run`:**

```
[miku ci] Commit abc123 — 3 archivos modificados

  ✓ lint         amp.spice OK
  ✓ normalize    top.sch (0 cambios), amp.spice (timestamps eliminados)
  ✓ lvs          MATCH — 24 devices, 18 nets
  ✓ sim          vmax=3.28V (baseline: 3.30V, Δ=0.6%, dentro de tolerancia 1%)
  ✗ drc          2 violaciones en metal1 (ver drc_report.html)

  Artefactos: diff_top.png, drc_report.html, sim_results.json
```

---

## 2. Detección automática de tipo de archivo

### Criterios de detección (en orden de prioridad)

1. **Extensión del archivo** — primer filtro, cubre el 95% de los casos.
2. **Contenido del archivo** — fallback para archivos sin extensión o con extensión ambigua.
3. **Configuración explícita en `miku.toml`** — override manual por el usuario.

### Tabla de extensiones → drivers

```python
EXTENSION_MAP = {
    # Layout binario
    ".gds":   "klayout",
    ".gds2":  "klayout",
    ".oas":   "klayout",   # OASIS

    # Layout texto
    ".mag":   "magic",
    ".magicrc": None,      # config, no diff

    # Esquemáticos
    ".sch":   "xschem",    # Xschem nativo
    ".sym":   "xschem",    # Símbolo Xschem

    # Netlists / SPICE
    ".spice": "spice",
    ".sp":    "spice",
    ".cir":   "spice",
    ".net":   "spice",
    ".cdl":   "spice",

    # Modelos
    ".lib":   "spice",     # model library
    ".mod":   "spice",

    # Otros EDA (futuros)
    ".lef":   "lef",
    ".def":   "def",
    ".v":     "verilog",
    ".sv":    "verilog",
}
```

### Detección por contenido (magic bytes / primera línea)

```python
CONTENT_DETECTORS = [
    # GDS: magic bytes 0x0006 0x0002 al inicio
    (lambda h: h[:4] == b'\x00\x06\x00\x02', "klayout"),

    # Magic .mag: primera línea es "magic"
    (lambda h: h[:5] == b'magic', "magic"),

    # Xschem .sch: primera línea contiene "xschem version="
    (lambda h: b'xschem version=' in h[:80], "xschem"),

    # SPICE: primera línea es comentario (* título)
    (lambda h: h[:1] == b'*', "spice"),

    # SPICE alternativo: empieza con .subckt o .title
    (lambda h: h[:7].lower() in (b'.subckt', b'.title '), "spice"),
]
```

### Función de dispatch

```python
def detect_driver(filepath: Path) -> str:
    """Detecta el driver correcto para un archivo de diseño."""
    # 1. Extensión
    ext = filepath.suffix.lower()
    if ext in EXTENSION_MAP:
        driver = EXTENSION_MAP[ext]
        if driver is not None:
            return driver

    # 2. Contenido (primeros 256 bytes)
    try:
        with open(filepath, 'rb') as f:
            header = f.read(256)
        for detector, driver in CONTENT_DETECTORS:
            if detector(header):
                return driver
    except OSError:
        pass

    # 3. Fallback a git diff estándar
    return "text"

def dispatch_diff(filepath: Path, rev_a: str, rev_b: str, opts: DiffOptions):
    driver_name = detect_driver(filepath)
    driver = DRIVERS[driver_name]
    return driver.diff(filepath, rev_a, rev_b, opts)
```

### Resolución de ambigüedades

- `.net` puede ser SPICE (Xschem export) o KiCad netlist → usar detección por contenido.
- `.lib` puede ser SPICE library o Liberty timing → los primeros 256 bytes lo aclaran (`* SPICE` vs `/* Liberty`).
- `.sch` puede ser KiCad o Xschem → primera línea `v {xschem version=` vs `EESchema Schematic`.

---

## 3. Arquitectura interna

### Comparativa: plugin system vs. monolito vs. subprocess delegation

#### Monolito

Todo el código de cada herramienta vive en el mismo módulo. Sin interfaces formales entre drivers.

**Pros:** simple de arrancar, sin overhead de IPC.  
**Contras:** inescalable. Agregar soporte para una herramienta nueva rompe otras. Imposible de testear en aislamiento. Conflictos de dependencias (el módulo KLayout vs. el módulo Magic comparten namespaces).

**Veredicto: no aplica para Riku.** El número de herramientas y la disparidad de sus interfaces lo hacen inviable desde el MVP.

---

#### Plugin System (carga dinámica)

Cada driver es un módulo Python independiente que implementa una interfaz definida. Riku los descubre y carga en runtime vía `importlib` o entry points de `setuptools`.

```
miku/
├── core/
│   ├── cli.py           ← parseo de argumentos, routing
│   ├── git.py           ← operaciones git via pygit2
│   ├── dispatcher.py    ← detect_driver() + dispatch
│   └── errors.py        ← jerarquía de errores
├── drivers/
│   ├── base.py          ← Protocol/ABC que todos los drivers implementan
│   ├── klayout.py       ← driver KLayout (GDS/OASIS)
│   ├── magic.py         ← driver Magic (.mag)
│   ├── xschem.py        ← driver Xschem (.sch)
│   └── spice.py         ← driver SPICE/NGSpice
└── miku.toml            ← config del repo
```

**Interface del driver (Protocol):**

```python
from typing import Protocol, Optional
from pathlib import Path
from dataclasses import dataclass

@dataclass
class DiffResult:
    identical: bool
    format: str            # "text" | "json" | "png" | "svg"
    content: bytes         # el diff en sí
    summary: dict          # resumen estructurado siempre presente
    artifacts: list[Path]  # archivos generados (PNGs, GDS de XOR, etc.)

class RikuDriver(Protocol):
    name: str
    supported_extensions: list[str]

    def available(self) -> bool:
        """¿Está la herramienta instalada y en PATH?"""
        ...

    def version(self) -> Optional[str]:
        """Versión de la herramienta detectada."""
        ...

    def diff(self, path: Path, content_a: bytes, content_b: bytes,
             opts: DiffOptions) -> DiffResult:
        """Compara dos versiones del archivo."""
        ...

    def normalize(self, content: bytes) -> bytes:
        """Canonicaliza el archivo para reducir ruido en diff."""
        ...

    def blame_info(self, path: Path, git_repo) -> list[BlameEntry]:
        """Información de blame semántico."""
        ...
```

**Pros:**
- Cada driver se puede testear, actualizar y reemplazar independientemente.
- La herramienta sigue funcionando aunque un driver falle al cargar (graceful degradation).
- Usuarios avanzados pueden escribir drivers para herramientas propietarias.
- Las dependencias de cada driver son opcionales: si no instalaste KLayout, el driver klayout simplemente reporta `available() → False`.

**Contras:**
- Más boilerplate inicial.
- Descubrimiento dinámico agrega complejidad si se hace con entry points.

**Veredicto: esta es la arquitectura correcta para Riku.** No requiere plugin discovery dinámico en el MVP — los drivers están hardcodeados en `DRIVERS = {"klayout": KLayoutDriver(), ...}`. La extensibilidad por entry points se agrega cuando haya usuarios externos.

---

#### Subprocess Delegation

Cada operación se implementa como una llamada a un proceso externo (shell script, binario separado). Riku solo orquesta las llamadas.

```python
# En vez de importar klayout.db:
result = subprocess.run(
    ["klayout", "-b", "-r", "miku_diff.py", "-rd", f"gds1={a}", "-rd", f"gds2={b}"],
    capture_output=True, timeout=120
)
```

**Pros:**
- Isolación perfecta: si KLayout crashea, no lleva a Riku consigo.
- No hay conflicto de dependencias Python.
- Los scripts KLayout pueden ser en Ruby (su lenguaje nativo).
- Facilita testear los scripts de forma independiente.

**Contras:**
- Latencia de arranque por cada llamada (KLayout tarda ~1s en iniciar en modo batch).
- Marshaling de datos entre procesos (archivos temporales o stdin/stdout JSON).
- Más difícil de hacer streaming de output.

**Veredicto: usar subprocess para KLayout y Magic, API Python directa para SPICE.**

El patrón híbrido tiene sentido técnico:
- **KLayout:** subprocess (`klayout -b -r script.py`) o Python API (`import klayout.db`) según disponibilidad. La API Python es más rápida para diffs pequeños; subprocess es más segura para GDS grandes.
- **Magic:** siempre subprocess (`magic -dnull -noconsole script.tcl`) — no hay Python API.
- **NGSpice:** subprocess (`ngspice -b`) — el binario es la interfaz canónica.
- **SPICE parsing:** Python directo (`spicelib`, `pygit2`) — no requieren proceso externo.

---

### Arquitectura recomendada para Riku (síntesis)

```
┌─────────────────────────────────────────────────────────────┐
│                     CLI (Click / Typer)                      │
│          miku diff | merge | blame | log | ci                │
└────────────────────────┬────────────────────────────────────┘
                         │
┌────────────────────────▼────────────────────────────────────┐
│                  Core Dispatcher                             │
│   detect_driver() → dispatch(driver, op, args)              │
│   git operations via pygit2 (extraer contenido de commits)  │
└──┬──────────────┬──────────────┬──────────────┬─────────────┘
   │              │              │              │
┌──▼───┐     ┌───▼──┐      ┌────▼──┐      ┌────▼──┐
│KLayout│    │Magic │      │Xschem │      │SPICE  │
│Driver │    │Driver│      │Driver │      │Driver │
│       │    │      │      │       │      │       │
│.db API│    │subp. │      │subp.  │      │Python │
│ o     │    │TCL   │      │headls.│      │direct │
│subp.  │    │      │      │       │      │       │
└───────┘    └──────┘      └───────┘      └───────┘
```

**Regla de decisión para cada driver:**
- Si existe Python API estable y no requiere proceso externo → usarla directamente.
- Si la herramienta solo tiene interfaz CLI/TCL → subprocess con timeout y stderr capture.
- Si el proceso puede tardar > 10s (GDS grandes) → subprocess en thread separado con progress reporting.

---

## 4. Manejo de errores de herramientas externas

### Jerarquía de errores

```python
class RikuError(Exception):
    """Base para todos los errores de Riku."""
    exit_code: int = 1

class ToolNotFoundError(RikuError):
    """La herramienta requerida no está instalada."""
    exit_code = 127

class ToolVersionError(RikuError):
    """La versión de la herramienta es incompatible."""
    exit_code = 126

class ToolTimeoutError(RikuError):
    """La herramienta tardó más del timeout configurado."""
    exit_code = 124

class ToolRuntimeError(RikuError):
    """La herramienta falló durante la ejecución."""
    exit_code = 1

class FileFormatError(RikuError):
    """El archivo no tiene el formato esperado."""
    exit_code = 1

class DriverUnavailableError(RikuError):
    """El driver necesario no está disponible (herramienta ausente o versión incompatible)."""
    exit_code = 127
```

---

### Detección de herramientas: check vs. assume

No asumir que la herramienta está disponible. Verificar explícitamente antes de intentar usarla, y fallar con mensaje accionable.

```python
class KLayoutDriver:
    MIN_VERSION = (0, 28, 0)

    def available(self) -> bool:
        return shutil.which("klayout") is not None or self._pip_klayout_available()

    def _pip_klayout_available(self) -> bool:
        try:
            import klayout.db
            return True
        except ImportError:
            return False

    def version(self) -> Optional[tuple[int, ...]]:
        # Intentar vía Python API primero (más confiable)
        try:
            import klayout.db as db
            v = db.Application.version()   # "0.29.2"
            return tuple(int(x) for x in v.split("."))
        except Exception:
            pass

        # Fallback a subprocess
        try:
            r = subprocess.run(["klayout", "--version"],
                               capture_output=True, text=True, timeout=5)
            # Output: "KLayout 0.29.2"
            match = re.search(r'(\d+)\.(\d+)\.(\d+)', r.stdout)
            if match:
                return tuple(int(x) for x in match.groups())
        except (subprocess.TimeoutExpired, FileNotFoundError):
            pass
        return None

    def require(self):
        """Lanza error útil si el driver no está disponible."""
        if not self.available():
            raise ToolNotFoundError(
                "KLayout no encontrado.\n"
                "  Para instalar:\n"
                "    pip install klayout          # headless, para diff/CI\n"
                "    apt install klayout          # con GUI y render PNG\n"
                "    https://www.klayout.de/build.html\n"
                "  O configura en miku.toml:\n"
                "    [tools.klayout]\n"
                "    path = \"/opt/klayout/bin/klayout\""
            )
        v = self.version()
        if v and v < self.MIN_VERSION:
            min_str = ".".join(str(x) for x in self.MIN_VERSION)
            cur_str = ".".join(str(x) for x in v)
            raise ToolVersionError(
                f"KLayout {cur_str} instalado, se requiere >= {min_str}.\n"
                f"  Actualizar: https://www.klayout.de/build.html"
            )
```

---

### Mensajes de error por herramienta

**KLayout ausente:**
```
Error: KLayout no encontrado.
  Este archivo requiere KLayout para diff: chip.gds

  Opciones:
    pip install klayout          ← headless, suficiente para miku diff
    apt install klayout          ← incluye renderizado PNG

  Alternativa sin instalar KLayout:
    miku diff --format=text chip.gds   ← usa strmcmp si está disponible

  Más info: miku doctor
```

**Magic ausente:**
```
Error: Magic VLSI no encontrado.
  Este archivo requiere Magic para diff visual: mycell.mag

  Para diff de texto (siempre disponible):
    miku diff --format=text mycell.mag   ← funciona sin Magic

  Para instalar Magic:
    apt install magic
    https://opencircuitdesign.com/magic/

  Más info: miku doctor
```

**NGSpice ausente:**
```
Error: NGSpice no encontrado.
  Se necesita NGSpice para: miku ci run --stage=sim

  Para instalar:
    apt install ngspice
    brew install ngspice

  El stage de simulación se puede deshabilitar:
    miku ci run --skip=sim
```

**Versión incompatible de Xschem:**
```
Warning: Xschem 3.3.1 detectado; se requiere >= 3.4.0 para diff visual.
  El diff de texto funcionará normalmente.
  Para diff visual: actualizar Xschem a >= 3.4.0
    https://xschem.sourceforge.io/stefan/index.html

  Continuando con diff de texto...
```

---

### `miku doctor`

Comando de diagnóstico que verifica todas las dependencias y reporta su estado.

```bash
miku doctor
```

```
miku doctor — verificando entorno

  Core:
    ✓ git        2.43.0
    ✓ pygit2     1.14.1

  Drivers:
    ✓ klayout    0.29.2  (Python API)     → diff GDS, XOR, render PNG
    ✓ magic      8.3.414                  → diff .mag, extracción
    ✗ xschem     no encontrado            → diff visual .sch no disponible
                                            pip: n/a, instalar desde fuente
    ✓ ngspice    40                       → simulación batch

  Opcionales:
    ✓ netgen     1.5.263                  → diff estructural netlists (LVS)
    ✗ strmcmp    no encontrado            → parte de KLayout (ya cubierto)
    ✓ imagemagick 7.1.1                   → compositing PNG

  Repositorio:
    ✓ miku.toml  encontrado
    ✓ .gitattributes configurado
    ⚠ .gitignore  falta entrada para *.raw (artefactos NGSpice)
      Sugerencia: miku init --fix-gitignore

  Resumen: 4/5 herramientas disponibles. 1 advertencia.
```

---

### Estrategia de degradación graceful

Cuando una herramienta no está disponible, Riku no falla completamente — ofrece el mejor diff posible con las herramientas presentes:

```
miku diff chip.gds
 ↓ KLayout disponible?
   Sí → LayoutDiff JSON + XOR visual
   No → strmcmp disponible?
          Sí → diff texto estructural
          No → "binario, sin diff disponible. Instala KLayout: pip install klayout"

miku diff top.sch
 ↓ Xschem disponible?
   Sí → SVG side-by-side
   No → diff texto plano (siempre funciona, .sch es texto)
        + "Para diff visual: instalar Xschem >= 3.4.0"
```

Esta cadena se configura en `miku.toml`:

```toml
[fallback]
gds = ["klayout", "strmcmp", "none"]    # orden de preferencia
mag = ["magic+klayout", "text"]
sch = ["xschem", "text"]
spice = ["netgen+spicelib", "text"]
```

---

## 5. Comparación con herramientas similares

### git-lfs

**Qué hace:** Almacena archivos binarios grandes fuera del repositorio git (en un servidor LFS). El repo guarda un puntero; `git lfs pull` descarga el binario cuando se necesita.

**Qué hace bien:**
- Resuelve el problema de tamaño: un GDS de 500MB no infla el repo.
- Transparente para el usuario: `git push/pull` funciona igual.
- Ampliamente soportado (GitHub, GitLab, Gitea).
- Retrocompatible: repos existentes se migran con `git lfs migrate`.

**Qué hace mal:**
- `git diff` sigue mostrando "binary file" — no hay diff semántico.
- No sabe nada de diseño de chips: no distingue entre GDS y un MP4.
- `git log` no muestra qué celdas cambiaron.
- Sin historial de simulaciones, sin integración con herramientas EDA.
- Almacenamiento remoto requerido; sin servidor LFS, no funciona.
- No canonicaliza .mag ni .spice — el ruido de timestamps persiste.

**Relación con Riku:** complementario, no competidor. Riku puede y debe recomendar git-lfs para GDS grandes. Lo que Riku agrega es la capa semántica encima.

```toml
# miku.toml — Riku puede configurar .gitattributes automáticamente
[lfs]
track = ["*.gds", "*.oas"]   # miku init --with-lfs configura esto
```

---

### DVC (Data Version Control)

**Qué hace:** Versionado de datasets y modelos ML sobre git. Almacena archivos grandes en S3/GCS/local, guarda metadatos en git. Pipeline de stages con caché de resultados.

**Qué hace bien:**
- Pipeline caching: si los inputs no cambiaron, no re-corre el stage.
- Soporte multi-backend (S3, GCS, Azure, SSH, local).
- Reproducibilidad de experimentos.
- Diff de métricas: `dvc metrics diff` compara JSON/YAML de métricas entre commits.

**Qué hace mal:**
- No sabe nada de EDA: trata GDS como "un archivo grande", netlists como "texto".
- Sin diff semántico de ningún tipo de archivo de diseño.
- Sin integración con KLayout, Xschem, NGSpice, Magic.
- Orientado a ML: su vocabulario (experimentos, modelos, datasets) no encaja con diseño de chips.
- Overhead de configuración alto para repos pequeños.

**Qué puede aprender Riku de DVC:**
- El modelo de **pipeline stages con caché** es exactamente lo que `miku ci` necesita. Si `amp.spice` no cambió, no re-simular.
- El formato de **métricas versionadas** (`dvc metrics diff`) es el modelo para `miku log --sim-metric`.
- La idea de separar **código fuente** (git) de **artefactos** (almacenamiento externo) es el mismo razonamiento que "GDS como artefacto de build".

---

### plotgitsch / KiRI (KiCad)

**plotgitsch:**
- Git difftool driver para KiCad `.sch`.
- Dos modos: bitmap overlay (ImageMagick) y SVG vectorial.
- SVG: adiciones en verde, eliminaciones en rojo — vectorial, zoomable.
- Funciona como `git difftool -t plotgitsch`.

**Qué hace bien:**
- UX excelente para el caso de uso específico (KiCad schematic diff visual).
- Integración limpia con `git difftool`.
- SVG vectorial permite zoom sin pérdida de calidad.

**Qué hace mal:**
- Solo KiCad `.sch` — no soporta Xschem, SPICE, GDS, Magic.
- Sin diff semántico: compara imágenes, no objetos.
- Sin integración con CI.
- Sin soporte para `.lib` ni footprints.

**KiRI:**
- Exporta ambas revisiones como SVG con `kicad-cli`.
- Interface web local (servidor HTTP) con slider side-by-side.
- Soporte para esquemáticos multi-hoja.
- Genera reporte HTML de diferencias visuales.

**Qué hace bien:**
- UI más rica que plotgitsch: slider interactivo, zoom, navegación.
- Multi-sheet: compara esquemáticos con varios niveles.
- Genera HTML standalone — ideal para adjuntar a PRs.

**Qué hace mal:**
- Requiere `kicad-cli` instalado (solo KiCad 7+).
- Solo KiCad — sin soporte Xschem, EDA IC.
- Diff visual de imágenes, no semántico.
- Sin pipeline CI, sin checks de LVS/DRC.

**Qué puede aprender Riku de KiRI:**
- El **HTML standalone** (un archivo autocontenido con las dos imágenes y el JS del slider) es la mejor UX para adjuntar diffs a PRs sin requerir infraestructura.
- El **patrón de exportar SVG headless** (`kicad-cli export svgsch`) es directamente replicable con `xschem -q --no_x --svg`.
- El **side-by-side con slider** es más informativo que un overlay para cambios de posición de componentes.

---

### AllSpice.io

**Qué hace:** Gitea fork con diff visual de hardware. Soporta KiCad, Altium, Eagle. Plataforma SaaS + self-hosted.

**Qué hace bien:**
- Diff visual integrado en el PR, sin configuración del usuario.
- Soporte multi-formato (KiCad, Altium, Eagle).
- UX de nivel producción.

**Qué hace mal:**
- No soporta Xschem, SPICE, GDS, Magic — el espacio IC analógico.
- SaaS con costos; self-hosted requiere infraestructura.
- Sin diff semántico (compara imágenes renderizadas).
- Sin pipeline CI integrado para DRC/LVS/simulación.

---

### Resumen comparativo

| Herramienta | Diff GDS | Diff .sch (Xschem) | Diff .mag | Diff SPICE | CI/DRC/LVS/Sim | Semántico |
|---|---|---|---|---|---|---|
| **git-lfs** | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ |
| **DVC** | ❌ | ❌ | ❌ | ❌ | ⚠️ (genérico) | ❌ |
| **plotgitsch** | ❌ | KiCad only | ❌ | ❌ | ❌ | ❌ |
| **KiRI** | ❌ | KiCad only | ❌ | ❌ | ❌ | ❌ |
| **AllSpice.io** | ❌ | KiCad only | ❌ | ❌ | ❌ | ❌ |
| **Riku** | ✅ KLayout | ✅ Xschem | ✅ Magic | ✅ NGSpice | ✅ | ✅ |

---

## 6. Decisiones de diseño justificadas

### Decisión 1: Python como lenguaje del MVP

KLayout tiene Python API en PyPI (`pip install klayout`). `spicelib` cubre NGSpice. `pygit2` cubre Git. El 80% de los requisitos están cubiertos con librerías existentes. Rust se agrega como extensión en fases posteriores para parseo de GDS en streaming (ver `research/lenguajes_y_stack.md`).

### Decisión 2: Driver system sobre monolito desde el MVP

No esperar a tener "suficiente código" para refactorizar. Definir el protocolo `RikuDriver` desde el principio y que cada herramienta sea un módulo separado. Costo: ~1 día de boilerplate. Beneficio: cada driver es testeable en aislamiento, las dependencias son opcionales, y el sistema funciona aunque falte una herramienta.

### Decisión 3: subprocess para Magic y NGSpice, API directa para KLayout y SPICE

Magic y NGSpice solo tienen interfaz TCL/CLI. KLayout tiene Python API usable sin proceso externo (para diffs pequeños). SPICE es texto y `spicelib` lo maneja directamente. Mezclar ambos enfoques según la herramienta es más pragmático que una política uniforme.

### Decisión 4: GDS como artefacto de build, no como fuente versionada

Si el flujo es Magic → GDS, la fuente es `.mag`. Si es GDSFactory/GLayout → GDS, la fuente es el Python. Versionar GDS en git es análogo a versionar binarios compilados. Se versiona en git-lfs o CI artifacts. Riku aplica diff de GDS cuando está disponible (para verificación), pero no lo trata como el objeto principal de versionado.

### Decisión 5: El diff de texto siempre funciona

Cualquier comando `miku diff` en un archivo de texto (`.sch`, `.mag`, `.spice`) produce output útil aunque ninguna herramienta EDA esté instalada. La cadena de degradación graceful garantiza que el sistema no falla completamente por una dependencia ausente. Esto es crítico para CI en entornos mínimos.

### Decisión 6: `miku.toml` en el repositorio, no `~/.mikuconfig` global

La configuración de drivers, fallbacks, stages de CI y tolerancias de simulación vive en el repositorio. Esto garantiza que el CI reproduce exactamente lo que corre el diseñador localmente. Las preferencias de usuario (path de KLayout en una instalación no estándar) van en `~/.config/miku/config.toml` y son mergeadas al runtime.

### Decisión 7: Artefactos de diff como outputs de primera clase

`miku diff` puede generar archivos (PNGs, GDS de XOR, HTML con slider). Estos son ciudadanos de primera clase: tienen paths predecibles, se listan en `miku ci status`, y `miku ci upload` los sube al PR automáticamente. Esto es lo que convierte un diff técnico en UX para revisores que no leen código.

---

## Notas de retroalimentación — flujos reales

> Estas notas provienen del research de experiencia de usuario. Afectan directamente el diseño
> del dispatcher, la configuración `miku.toml`, y el comando `miku init`.

### Nota 1: `miku.toml` necesita declarar la fuente de verdad del layout

La Sección 7 ("Configuración de referencia") tiene `pdk = "sky130A"` como único campo de proyecto relevante para el flujo. Falta un campo crítico:

```toml
[project]
name = "my_chip"
pdk = "sky130A"
pdk_version = "abc1234def5678"   # commit hash del PDK — para reproducibilidad
layout_source = "magic"          # "magic" | "klayout" | "python"
```

Sin `layout_source`:
- El dispatcher no sabe si debe buscar `.mag` o `.gds` como fuente
- La generación del `.gitignore` en `miku init` no sabe si excluir `*.gds`
- Los mensajes de resolución de conflictos en merge pueden recomendar "resolver en `.mag`" cuando el proyecto no tiene `.mag`

Para proyectos KLayout-primary (IHP SG13G2, GF180MCU): `layout_source = "klayout"`.
Para proyectos generados por código: `layout_source = "python"`.

Fuente: [open-source-silicon.dev/t/16219913](https://web.open-source-silicon.dev/t/16219913), [IHP Certificate Course](https://www.ihp-microelectronics.com/events-1/detail/title-certificate-course-in-person-analog-design-with-ihp-sg13g2-open-source-pdk)

### Nota 2: Detección de `.sch` por contenido es más importante de lo estimado

La Sección 2 ("Resolución de ambigüedades") menciona `".sch" puede ser KiCad o Xschem`. En práctica, la extensión `.sch` también la usa Qucs-S (herramienta alternativa para simulación NGSpice con mejor UI). Los tres formatos son completamente distintos.

El CONTENT_DETECTOR para Xschem ya está correcto: `b'xschem version=' in h[:80]`. Confirmar que este check se ejecuta siempre antes de despachar al driver Xschem — nunca asumir que `.sch` = Xschem solo por extensión.

Fuente: [github.com/ra3xdh/qucs_s](https://github.com/ra3xdh/qucs_s)

### Nota 3: `miku init` debe generar `.gitignore` condicional al PDK y flujo

El `miku.toml` de referencia tiene un bloque `[ignore]` con `build_artifacts = ["*.gds"]`. Esto solo es correcto cuando `layout_source = "magic"`. El generador de `.gitignore` de `miku init` debe ser condicional:

```
si layout_source = "magic"  → ignorar *.gds
si layout_source = "klayout" → NO ignorar *.gds (es la fuente)
si layout_source = "python"  → ignorar *.gds (es build artifact)
```

Además, si `pdk` es IHP SG13G2, el `.gitignore` debe NO incluir `*.va` — los archivos Verilog-A son modelos de dispositivos que deben versionarse. El `.osdi` (compilado de Verilog-A) sí debe ignorarse.

Fuente: [ngspice.sourceforge.io/osdi.html](https://ngspice.sourceforge.io/osdi.html), [IHP-Open-PDK](https://github.com/IHP-GmbH/IHP-Open-PDK)

### Nota 4: `miku doctor` necesita verificar PDK version y bugs de integración conocidos

La salida ejemplo de `miku doctor` verifica herramientas instaladas pero no verifica el PDK. Agregar:

```
  PDK:
    ✓ sky130A      commit abc1234  (coincide con miku.toml)
    ⚠ KLayout+SKY130 XML bug detectado
      Aplicar patch: miku doctor --fix-pdk
      (sed en sky130.lym y sky130A.lyt)
```

El bug de XML parsing en KLayout+SKY130 afecta a cualquier entorno con PDK fresh install y hace fallar el DRC silenciosamente. Es el primer problema que encuentra un usuario nuevo.

Fuente: [unic-cass KLayout Sky130 tutorial (2024)](https://unic-cass.github.io/training/sky130/3.3-layout-klayout.html)

### Nota 5: Tipos de archivo nuevos que el dispatcher debe reconocer

Actualizar `EXTENSION_MAP` para incluir tipos encontrados en proyectos reales:

```python
EXTENSION_MAP = {
    # ... entradas existentes ...
    ".oas":    "klayout",   # ya incluido — confirmar
    ".oasis":  "klayout",   # variante de extensión
    ".va":     "text",      # Verilog-A — diff como texto, no driver especial
    ".osdi":   None,        # binario compilado, no diffear
    ".cdl":    "spice",     # Circuit Description Language (Cadence/Virtuoso export)
    ".spi":    "spice",     # variante de extensión SPICE en algunos scripts OpenLane
    ".kicad_sch": "text",   # KiCad nuevo formato — fallback a texto
}
```

---

## ¿Cuándo refutar estas decisiones?

**"Driver system desde el MVP" (Decisión 2)** es prematuro si:
- El primer usuario real de Riku es una sola persona con un solo flujo (ej. solo Magic+NGSpice, sin KLayout). El boilerplate del protocolo `RikuDriver` agrega fricción sin beneficio real hasta que haya al menos dos drivers siendo usados simultáneamente.

**"subprocess para Magic y NGSpice" (Decisión 3)** cambia si:
- Aparece una librería Python con bindings nativos para Magic (no existe hoy, pero el proyecto está activo). Subprocess tiene overhead y hace el manejo de errores más frágil.
- NGSpice mejora su API embebida (`libngspice`) al punto de ser usable sin proceso externo — hay trabajo en curso en el proyecto NGSpice.

**"GDS como artefacto de build" (Decisión 4)** no aplica si:
- El flujo del usuario es diseño custom en KLayout directamente (sin Magic como fuente). En ese caso el GDS es la fuente, no el derivado. La decisión asume un flujo Magic→GDS que no es universal.

**"miku.toml en el repo" (Decisión 6)** crea fricción si:
- Riku se usa en repos que no son de chip (ej. un repo de firmware que incluye algunos esquemáticos). En ese caso un archivo de configuración en el repo puede molestar a colaboradores que no usan Riku.

---

## 7. Configuración de referencia (`miku.toml`)

```toml
[project]
name = "my_chip"
pdk = "sky130A"

[tools]
# Paths opcionales — si no se especifican, Riku busca en PATH
# klayout = "/opt/klayout/bin/klayout"
# ngspice = "/usr/local/bin/ngspice"

[tools.klayout]
min_version = "0.28.0"
prefer_python_api = true        # usar klayout.db si está disponible
xor_tile_size = "1.mm"          # tileado para GDS grandes
xor_threads = 4

[tools.ngspice]
timeout = 300                   # segundos máximo por simulación

[diff]
mag_strip_timestamps = true     # eliminar timestamps de .mag antes de diff
spice_canonicalize = true       # ordenar componentes alfabéticamente

[ci]
stages = ["lint", "normalize", "lvs", "sim", "drc", "diff-visual"]

[ci.sim]
baseline_commit = "main"        # comparar resultados contra esta rama
tolerance = 0.01                # 1% de tolerancia en métricas .meas

[fallback]
gds   = ["klayout", "strmcmp", "none"]
mag   = ["magic+klayout", "text"]
sch   = ["xschem", "text"]
spice = ["netgen+spicelib", "text"]

[lfs]
track = ["*.gds", "*.oas"]

[ignore]
# Riku puede poblar .gitignore automáticamente
simulation_outputs = ["*.raw", "*.log", "*.err"]
build_artifacts = ["*.gds"]     # si .mag es la fuente
```

---

## Referencias

### Herramientas comparadas
- **git-lfs**: https://github.com/git-lfs/git-lfs — almacenamiento de archivos grandes fuera del repo
- **DVC (Data Version Control)**: https://github.com/iterative/dvc — versionado de datos/modelos ML, cacheable
- **plotgitsch**: https://github.com/jnavila/plotkicadsch — diff visual de esquemáticos KiCad sobre git
- **KiRI (KiCad Review Interface)**: https://github.com/leoheck/kiri — diff visual web para KiCad
- **AllSpice.io**: https://allspice.io — plataforma Git para hardware (propietaria), similar en visión a Riku

### Diseño de CLIs de referencia
- **git** source: https://github.com/git/git — el modelo de subcomandos y extensibilidad
- **cargo** (Rust): https://github.com/rust-lang/cargo — referencia de UX para CLIs de herramientas técnicas
- **click** (Python): https://github.com/pallets/click — librería para construir CLIs en Python
- **typer** (Python): https://github.com/tiangolo/typer — CLI con tipos Python, basado en click

### Configuración como código (referencia para `miku.toml`)
- **pyproject.toml** spec: https://packaging.python.org/en/latest/specifications/pyproject-toml/
- **TOML spec**: https://toml.io/en/

### Ver también
- [lenguajes_y_stack.md](lenguajes_y_stack.md) — stack tecnológico subyacente
- [../herramientas/headless_y_compatibilidad_herramientas.md](../herramientas/headless_y_compatibilidad_herramientas.md) — cómo los drivers detectan herramientas instaladas
- [../operaciones/estrategia_merge_archivos_mixtos.md](../operaciones/estrategia_merge_archivos_mixtos.md) — subsistema de merge
- [../operaciones/ci_drc_lvs_regresiones.md](../operaciones/ci_drc_lvs_regresiones.md) — subsistema de CI
- [../operaciones/cache_y_rendimiento.md](../operaciones/cache_y_rendimiento.md) — estrategia de caché para stages costosos de `miku ci`
