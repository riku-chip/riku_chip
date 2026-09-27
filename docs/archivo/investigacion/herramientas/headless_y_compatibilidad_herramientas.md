# Entornos Headless y Compatibilidad de Versiones — Herramientas EDA para Riku

## 1. El problema de X11 en CI

KLayout y Xschem asumen entorno gráfico. Al ejecutarlos sin display en CI se obtienen errores como:

```
Error: cannot connect to X server :0
qt.qpa.xcb: could not connect to display
_tkinter.TclError: no display name and no $DISPLAY environment variable
```

Hay tres estrategias para resolverlo, con características distintas:

| Estrategia | Cómo funciona | Overhead | Requiere X11 libs |
|---|---|---|---|
| **Xvfb** | Servidor X virtual en framebuffer en memoria | ~50 MB RAM, mínimo CPU | Sí |
| **Offscreen rendering Qt** | Variable de entorno `QT_QPA_PLATFORM=offscreen` | Ninguno | Sí (Qt) |
| **Modo batch puro** | Flags del propio binario que evitan inicializar Qt/X11 | Ninguno | No |

**Decisión de diseño para Riku:** preferir modo batch puro cuando esté disponible (KLayout `-b`, Magic `-dnull`). Usar `QT_QPA_PLATFORM=offscreen` como fallback para operaciones que necesiten el subsistema gráfico de KLayout sin pantalla real. Xvfb solo cuando no haya alternativa (Xschem para diff visual interactivo, Magic con `-d XR`).

### Patrón Xvfb en CI

```bash
# Instalar: apt-get install -y xvfb
Xvfb :99 -screen 0 1280x1024x24 &
export DISPLAY=:99
# ... ejecutar herramienta
kill %1
```

Con GitHub Actions:

```yaml
- name: Start Xvfb
  run: |
    Xvfb :99 -screen 0 1280x1024x24 &
    echo "DISPLAY=:99" >> $GITHUB_ENV
```

### Patrón offscreen Qt (KLayout)

```bash
export QT_QPA_PLATFORM=offscreen
klayout -zz -r script.py   # no necesita display real
```

Funciona para operaciones que inicializan Qt pero no necesitan una ventana visible. No funciona para `LayoutView.save_image_*` sin display o framebuffer disponible — en ese caso, Xvfb es necesario.

---

## 2. KLayout headless

### Qué funciona sin display

La distinción clave es entre el paquete PyPI (`pip install klayout`) y el paquete del sistema:

| Operación | `pip install klayout` | `apt install klayout` + `-zz` |
|---|---|---|
| Leer/escribir GDS/OASIS | ✅ | ✅ |
| `LayoutDiff` (diff estructural) | ✅ | ✅ |
| XOR geométrico via DRC engine | ✅ | ✅ |
| `strmcmp`, `strmxor` (buddies) | ✅ binarios standalone | ✅ |
| Exportar PNG/SVG (`LayoutView`) | ❌ (`klayout.lay` no incluido) | ✅ con `-zz` o Xvfb |
| DRC scripts Ruby/Python | ✅ | ✅ |
| Macros LVS | ✅ | ✅ |

**Para CI en Riku:** `pip install klayout` cubre diff, XOR y DRC. Para exportar PNG de layout, instalar el paquete del sistema.

### Flags esenciales

```bash
klayout -b -r script.py              # batch mode completo: -zz -nc -rx
klayout -zz -r script.py             # sin GUI, sin display requerido
klayout -b -r xor.drc \
    -rd gds1=a.gds -rd gds2=b.gds \
    -rd out=diff.gds                  # pasar variables al script

# Buddy tools (standalone, sin display):
strmxor a.gds b.gds resultado.gds    # XOR geométrico
strmcmp a.gds b.gds                  # diff estructural → texto + exit code
```

| Flag | Efecto |
|---|---|
| `-b` | Combina `-zz -nc -rx` — batch completo |
| `-zz` | Sin GUI, sin display requerido |
| `-nc` | No cargar configuración del usuario (startup más rápido en CI) |
| `-r <script>` | Ejecutar script y salir |
| `-rd name=val` | Variable disponible en el script como `$name` |

### Limitaciones conocidas

- **`klayout.lay` module** no está disponible en el paquete PyPI — solo en el paquete del sistema o el instalador oficial. Operaciones de rendering requieren esta dependencia.
- **Renderizado PNG con `LayoutView`** falla silenciosamente si no hay display y no se usa `-zz`. Con `-zz` y sin Xvfb puede fallar dependiendo de la versión.
- **LVS scripts** que usan `DRC::LVSData` requieren versión ≥ 0.27.x.
- **Versiones < 0.26** no tienen `LayoutDiff.SmartCellMapping` — el diff de celdas renombradas no funciona.
- **strmcmp en Windows** requiere la instalación completa de KLayout — los buddies no se distribuyen por PyPI.

### Verificación headless en CI

```bash
python -c "import klayout.db as db; l = db.Layout(); print('klayout.db OK', db.Application.version())"
klayout -b -r /dev/null && echo "batch mode OK"
```

---

## 3. Xschem headless

### Flags que funcionan sin display

```bash
# Exportar PNG sin display (X11 no requerido si se usa --no_x):
xschem -q --no_x --png --plotfile output.png schematic.sch

# Exportar SVG sin display:
xschem -q --no_x --svg --plotfile output.svg schematic.sch

# Ejecutar script Tcl y salir:
xschem -q --no_x --tcl "set netlist_type spice; xschem netlist; quit" schematic.sch

# Exportar netlist SPICE:
xschem -q --no_x --netlist --tcl "xschem netlist; quit" schematic.sch
```

| Flag | Efecto |
|---|---|
| `-q` / `--quit` | Salir después de procesar |
| `--no_x` / `-x` | Sin display X11 (headless) |
| `--png` | Exportar como PNG al archivo dado en `--plotfile` |
| `--svg` | Exportar como SVG |
| `--tcl <script>` | Ejecutar script Tcl inline |
| `--diff a.sch b.sch` | Diff visual (requiere display) |

### Qué funciona realmente sin display

Verificado contra el código fuente de Xschem y reportes de usuarios en CI:

| Operación | Sin display (`--no_x`) | Con Xvfb |
|---|---|---|
| Exportar PNG | ✅ | ✅ |
| Exportar SVG | ✅ | ✅ |
| Generar netlist SPICE | ✅ | ✅ |
| Ejecutar scripts Tcl | ✅ | ✅ |
| `--diff` (overlay visual) | ❌ requiere GUI | ✅ |
| Edición interactiva | ❌ | ✅ |

### Limitaciones conocidas

- **`--no_x` requiere Xschem ≥ 3.1.0.** En versiones anteriores el flag no existe y el proceso cuelga esperando conexión X11.
- **El flag es `--no_x` en algunas builds y `-x` en otras** — depende de cómo fue compilado. Probar ambos.
- **Las librerías Tk/Tcl siguen siendo dependencias en tiempo de ejecución** aunque no se use la GUI. Si no están instaladas, el proceso falla con `Error in startup script: can't find package Tk`.
- **Xschem lee `xschemrc`** al arrancar. Si hay comandos que asumen display en ese archivo de config del usuario, fallan en CI. Solución: usar `--rcfile /dev/null` o un xschemrc limpio para CI.
- **El netlist generado puede contener timestamps** en comentarios — aplicar el canonicalizador de Riku antes de hacer commit.
- **En versiones compiladas con soporte Cairo** (la mayoría de los paquetes del sistema), PNG y SVG usan el backend Cairo directamente, sin necesidad de X11 una vez inicializado con `--no_x`. En versiones sin Cairo, la exportación PNG requiere display.

### Comandos Tcl útiles para automatización

```tcl
# Dentro de un script pasado con --tcl:
xschem netlist                          # generar netlist según tipo activo
set netlist_type spice                  # cambiar tipo de netlist
xschem set netlist_dir /ruta/salida/   # cambiar directorio de salida
xschem reload                          # recargar el esquemático del disco
quit                                   # salir (obligatorio)
```

---

## 4. Magic VLSI headless

### Batch mode

```bash
magic -dnull -noconsole script.tcl    # headless estándar (usado por OpenLane)
magic -d NULL -noconsole script.tcl   # equivalente (con espacio)
```

`-dnull` indica al display driver que use el driver nulo — no se inicializa ningún subsistema gráfico. **Este es el modo correcto para CI.** El script Tcl **debe terminar con `quit`**; de lo contrario Magic queda esperando input de stdin.

### Extracción de netlist (LVS)

```tcl
# extract_netlist.tcl
load mycell                    ; # cargar celda desde mycell.mag
extract all                    ; # extraer parasíticos a mycell.ext
ext2spice lvs                  ; # convertir a SPICE modo LVS (sin parasíticos RC)
ext2spice                      ; # escribir mycell.spice
quit
```

```bash
magic -dnull -noconsole extract_netlist.tcl
# Resultado: mycell.spice listo para Netgen LVS
```

Para extracción con parasíticos RC (para simulación post-layout):

```tcl
load mycell
extract all
ext2sim labels on
ext2sim
extresist all
exttospice
quit
```

### DRC en batch

```tcl
# drc_check.tcl
drc off                        ; # desactivar DRC interactivo (lento)
load mycell
drc catchup                    ; # correr DRC completo
set drc_count [drc list count total]
puts "DRC errors: $drc_count"
if {$drc_count > 0} {
    drc listall why             ; # imprimir todas las violaciones
}
quit
```

```bash
magic -dnull -noconsole drc_check.tcl 2>&1 | tee drc_report.txt
```

### Exportar GDS desde .mag

```tcl
load mycell
gds write output.gds
quit
```

### Exportar PNG (headless con limitación)

```tcl
load mycell
plot pnm output.pnm 1.0        ; # 1.0 pixel/lambda
quit
```

```bash
magic -dnull -noconsole render.tcl
convert output.pnm output.png  # ImageMagick
```

SVG y rendering de alta calidad requieren `-d XR` (display X11). Para CI usar Xvfb si se necesita SVG.

### Limitaciones conocidas

- **`-dnull` no soporta `plot svg`** — SVG requiere display.
- **La librería de tecnología debe ser accesible.** Magic busca el `.tech` file en `$MAGTYPE` o en el directorio de trabajo. En CI, establecer `MAGTYPE` o copiar el tech file.
- **`ext2spice` escribe en el directorio de trabajo** por defecto — controlar el cwd o redirigir con `ext2spice path /ruta/`.
- **Versiones < 8.3.x** tienen bugs en `ext2spice` con subcircuitos jerárquicos que afectan LVS.
- **Magic 8.3.320+** es la versión mínima recomendada para flujos sky130/gf180 (la que distribuye efabless en sus imágenes Docker).

---

## 5. Estrategia de detección de versiones

### El problema

Cada herramienta tiene un formato distinto para reportar su versión, y las incompatibilidades no son triviales: un flag puede existir en una versión y no en otra, el formato de salida puede cambiar entre versiones menores.

### Detección por herramienta

**KLayout:**

```python
import subprocess, re

def get_klayout_version() -> tuple[int, int, int] | None:
    try:
        out = subprocess.check_output(["klayout", "--version"], stderr=subprocess.STDOUT, text=True)
        # Output: "KLayout 0.28.17"  o  "0.28.17"
        m = re.search(r"(\d+)\.(\d+)\.(\d+)", out)
        if m:
            return tuple(int(x) for x in m.groups())
    except (FileNotFoundError, subprocess.CalledProcessError):
        return None

# También vía Python API (si está disponible):
try:
    import klayout.db as db
    v = db.Application.version()   # "0.28.17"
except ImportError:
    pass
```

**Xschem:**

```python
def get_xschem_version() -> tuple[int, int, int] | None:
    try:
        out = subprocess.check_output(["xschem", "--version"], stderr=subprocess.STDOUT, text=True)
        # Output: "Xschem 3.4.7"
        m = re.search(r"(\d+)\.(\d+)\.(\d+)", out)
        if m:
            return tuple(int(x) for x in m.groups())
    except (FileNotFoundError, subprocess.CalledProcessError):
        return None
```

**NGSpice:**

```python
def get_ngspice_version() -> tuple[int, int] | None:
    try:
        # ngspice --version imprime a stderr en algunas versiones
        out = subprocess.check_output(
            ["ngspice", "--version"], stderr=subprocess.STDOUT, text=True
        )
        # Output: "ngspice-42" o "ngspice 38" o "** ngspice-41 ..."
        m = re.search(r"ngspice[- ](\d+)(?:\.(\d+))?", out, re.IGNORECASE)
        if m:
            major = int(m.group(1))
            minor = int(m.group(2)) if m.group(2) else 0
            return (major, minor)
    except (FileNotFoundError, subprocess.CalledProcessError):
        return None
```

**Magic:**

```python
def get_magic_version() -> str | None:
    try:
        # magic --version no existe en todas las builds; usar -dnull y leer startup
        out = subprocess.check_output(
            ["magic", "--version"], stderr=subprocess.STDOUT, text=True
        )
        # Output: "8.3.459"
        m = re.search(r"(\d+\.\d+\.\d+)", out)
        return m.group(1) if m else None
    except (FileNotFoundError, subprocess.CalledProcessError):
        # Fallback: correr magic -dnull con script que imprime version
        try:
            script = "puts [magic version]\nquit\n"
            proc = subprocess.run(
                ["magic", "-dnull", "-noconsole"],
                input=script, capture_output=True, text=True, timeout=10
            )
            m = re.search(r"(\d+\.\d+\.\d+)", proc.stdout + proc.stderr)
            return m.group(1) if m else None
        except Exception:
            return None
```

### Tabla de versiones mínimas recomendadas

| Herramienta | Mínima soportada | Recomendada | Incompatibilidades críticas |
|---|---|---|---|
| KLayout | 0.26.x | 0.28.x | `SmartCellMapping` ausente en < 0.26; LVS via `DRC::LVSData` requiere ≥ 0.27 |
| Xschem | 3.1.0 | 3.4.x | `--no_x` ausente en < 3.1; `--diff` ausente en < 3.4.0 |
| NGSpice | 36 | 42 | Cambios en formato de salida `.meas` entre versiones principales |
| Magic | 8.3.100 | 8.3.320+ | Bugs en `ext2spice` jerárquico en < 8.3.x |

### Implementación del checker en Riku

```python
# miku/version_check.py

REQUIREMENTS = {
    "klayout": {
        "min": (0, 26, 0),
        "recommended": (0, 28, 0),
        "get_version": get_klayout_version,
        "features": {
            (0, 26, 0): ["LayoutDiff", "batch mode"],
            (0, 27, 0): ["LVS via DRC engine"],
            (0, 28, 0): ["SmartCellMapping estable"],
        }
    },
    "xschem": {
        "min": (3, 1, 0),
        "recommended": (3, 4, 0),
        "get_version": get_xschem_version,
        "features": {
            (3, 1, 0): ["--no_x headless"],
            (3, 4, 0): ["--diff visual"],
        }
    },
    # ... ngspice, magic
}

def check_tools(warn_only: bool = False) -> dict:
    results = {}
    for tool, spec in REQUIREMENTS.items():
        version = spec["get_version"]()
        status = "missing" if version is None else (
            "ok" if version >= spec["recommended"] else
            "outdated" if version >= spec["min"] else
            "incompatible"
        )
        results[tool] = {"version": version, "status": status}
        if status in ("missing", "incompatible"):
            msg = f"[miku] {tool}: {status} (instalada: {version}, mínima: {spec['min']})"
            if warn_only:
                print(f"WARNING: {msg}", file=sys.stderr)
            else:
                raise RuntimeError(msg)
    return results
```

Riku ejecuta `check_tools()` al inicio de cualquier operación que involucre una herramienta específica, no globalmente al arrancar — esto permite que Riku sea útil incluso si solo está instalada una parte del stack.

---

## 6. Containerización

### Imagen base y estrategia de capas

**Base recomendada:** `ubuntu:22.04` (no Alpine — las herramientas EDA dependen de glibc y tienen dependencias C++ complejas que Alpine resuelve mal con musl).

```dockerfile
FROM ubuntu:22.04
```

**No usar** `ubuntu:latest` — el tag cambia y rompe reproducibilidad. Fijar siempre a una versión concreta.

### Dependencias mínimas por herramienta

```dockerfile
RUN apt-get update && apt-get install -y --no-install-recommends \
    # KLayout
    klayout \
    # Xschem
    xschem \
    # Magic
    magic \
    # NGSpice
    ngspice \
    # Netgen (para LVS)
    netgen-lvs \
    # Runtime X11 headless (para Xvfb si se necesita)
    xvfb \
    # Python + pip (para klayout PyPI y scripts de Riku)
    python3 python3-pip \
    # Conversión de imágenes (PNM → PNG, overlay de diffs)
    imagemagick \
    # Dependencias de compilación Magic/Xschem si se buildan desde fuente
    # (no necesarias con paquetes del sistema)
    && rm -rf /var/lib/apt/lists/*
```

**Excluir explícitamente:**
- Documentación de paquetes (`--no-install-recommends` ya ayuda)
- Fuentes de sistema extra (`fonts-*` excepto la mínima para rendering)
- `texlive`, `octave`, `gnuplot` — algunas herramientas los sugieren como recommends
- Locales extras: solo generar `en_US.UTF-8` y `C.UTF-8`
- Screensavers, gestores de ventanas, librerías de audio

### Tamaño estimado

| Capa | Tamaño aproximado |
|---|---|
| `ubuntu:22.04` base | ~77 MB |
| KLayout (apt) | ~250 MB (incluye Qt, librerías) |
| Xschem (apt) | ~15 MB |
| Magic (apt) | ~20 MB |
| NGSpice (apt) | ~25 MB |
| Netgen (apt) | ~8 MB |
| Python 3 + pip | ~30 MB |
| Xvfb + X11 runtime | ~20 MB |
| **Total estimado (comprimida)** | **~350-400 MB** |

Sin KLayout (solo Xschem + Magic + NGSpice): ~150 MB.

La imagen descomprimida en disco es ~700-900 MB dependiendo del release de Ubuntu y los paquetes.

### Dockerfile completo para CI de Riku

```dockerfile
FROM ubuntu:22.04

ENV DEBIAN_FRONTEND=noninteractive
ENV LANG=C.UTF-8
ENV LC_ALL=C.UTF-8

RUN apt-get update && apt-get install -y --no-install-recommends \
    klayout \
    xschem \
    magic \
    ngspice \
    netgen-lvs \
    xvfb \
    python3 \
    python3-pip \
    imagemagick \
    git \
    && rm -rf /var/lib/apt/lists/*

# klayout Python API (para scripts de diff en Python)
RUN pip3 install --no-cache-dir klayout

# Crear usuario no-root para seguridad
RUN useradd -m -u 1000 miku
USER miku
WORKDIR /workspace

# Verificar que las herramientas arrancan en headless
RUN klayout -b -r /dev/null \
    && ngspice --version \
    && magic --version \
    && xschem --version
```

### Alternativa: imagen con versiones fijadas (máxima reproducibilidad)

Para CI donde se necesita reproducibilidad exacta, usar la imagen de OpenLane como base:

```dockerfile
FROM efabless/openlane:latest
# Ya incluye KLayout, Magic, Xschem, NGSpice, Netgen con versiones validadas
# Tamaño: ~2 GB descomprimida — útil si ya se usa el flujo OpenLane
```

Para Riku, que apunta a ser liviano, la imagen propia basada en ubuntu:22.04 es preferible.

### GitHub Actions — uso de la imagen

```yaml
jobs:
  miku-ci:
    runs-on: ubuntu-latest
    container:
      image: ghcr.io/tu-org/miku-eda:latest
    steps:
      - uses: actions/checkout@v4
      - name: Run Riku checks
        run: |
          miku diff HEAD~1 HEAD
          miku drc --tool klayout
```

### PDK en el contenedor

Los PDKs (sky130, gf180mcu) son grandes (~5 GB sky130 completo) y no deben estar en la imagen base. Opciones:

1. **Volume mount en CI:** montar el PDK desde el runner host
2. **PDK slim:** instalar solo la variante necesaria (`sky130A` en lugar del PDK completo)
3. **Variable de entorno:** `PDK_ROOT`, `PDK`, `STD_CELL_LIBRARY` — Magic y KLayout los leen

```dockerfile
ENV PDK_ROOT=/pdks
ENV PDK=sky130A
```

---

## 7. Instalación en macOS, Linux y WSL2

### Linux (Ubuntu/Debian) — instalación de referencia

```bash
# KLayout — paquete del sistema (incluye buddies: strmcmp, strmxor)
sudo apt-get install klayout

# O instalador oficial para versión más reciente:
# https://www.klayout.de/build.html → descargar .deb

# Xschem
sudo apt-get install xschem

# NGSpice
sudo apt-get install ngspice

# Magic
sudo apt-get install magic

# Netgen
sudo apt-get install netgen-lvs
```

**Versiones en Ubuntu 22.04 LTS (jammy):**
- KLayout: 0.27.x (desactualizado — el paquete del sistema está 1-2 versiones mayor detrás)
- Magic: 8.3.x
- Xschem: 3.x (varía)

Para versiones más recientes: compilar desde fuente o usar los instaladores oficiales.

### macOS — instalación y workarounds

**KLayout:**

```bash
brew install klayout
# O instalador oficial .dmg desde klayout.de
```

- Los buddies (`strmcmp`, `strmxor`) están incluidos en el `.app` bundle: `/Applications/KLayout.app/Contents/MacOS/strmcmp`
- Para uso en scripts: `export PATH="/Applications/KLayout.app/Contents/MacOS:$PATH"`
- `klayout.lay` (rendering de PNG) requiere la instalación del `.app` — no funciona con `brew install klayout` para la parte gráfica

**Xschem:**

```bash
brew install xschem
```

- Requiere XQuartz para operaciones con display: `brew install --cask xquartz`
- Las operaciones headless (`--no_x`) funcionan sin XQuartz
- Ruta de instalación de tech files: diferente a Linux — puede requerir ajuste de `XSCHEM_LIBRARY_PATH`

**NGSpice:**

```bash
brew install ngspice
```

Funciona correctamente en macOS. Sin diferencias funcionales con Linux para batch mode.

**Magic:**

```bash
brew install magic
# O compilar desde fuente (recomendado para sky130):
git clone https://github.com/RTimothyEdwards/magic
cd magic && ./configure && make && sudo make install
```

- Magic en macOS requiere XQuartz incluso con `-dnull` en algunas versiones — testear con `magic --version` y una ejecución batch simple
- **Workaround conocido:** instalar `xquartz` y exportar `DISPLAY=:0` antes de correr Magic, incluso en batch

```bash
brew install --cask xquartz
export DISPLAY=:0
magic -dnull -noconsole script.tcl
```

### WSL2 (Windows) — instalación y workarounds

WSL2 con Ubuntu 22.04 es la plataforma más predecible en Windows. Las herramientas se instalan igual que en Linux nativo.

```bash
# Dentro de WSL2 Ubuntu:
sudo apt-get install klayout xschem ngspice magic netgen-lvs
```

**Display en WSL2:**

WSL2 ≥ kernel 5.10 + Windows 11 incluye **WSLg** (servidor X/Wayland embebido). Las aplicaciones gráficas funcionan directamente:

```bash
echo $DISPLAY   # debería mostrar :0 o similar si WSLg está activo
klayout         # abre ventana en Windows
```

Para CI en WSL2 o si WSLg no está disponible:

```bash
# Opción 1: usar Xvfb (igual que Linux CI)
Xvfb :99 -screen 0 1280x1024x24 &
export DISPLAY=:99

# Opción 2: usar herramientas en modo batch puro (sin display)
klayout -b -r script.py           # no necesita display
magic -dnull -noconsole script.tcl
xschem -q --no_x --png --plotfile out.png schematic.sch
```

**Problemas conocidos en WSL2:**

| Problema | Causa | Solución |
|---|---|---|
| `cannot connect to X server` con WSLg | WSLg no iniciado | Abrir una app gráfica de Windows primero, o usar `-b`/`-dnull`/`--no_x` |
| KLayout muy lento con archivos grandes | I/O del sistema de archivos WSL2 ↔ Windows lento | Mantener archivos en `/home/` (ext4 de WSL2), no en `/mnt/c/` |
| Magic no encuentra tech file | `MAGTYPE` no establecido | `export MAGTYPE=/usr/share/magic/sys` |
| Xschem no encuentra symbols | `XSCHEM_LIBRARY_PATH` incorrecto | Establecer la variable o usar `--rcfile` con paths absolutos |
| `strmcmp` no encontrado | Buddies de KLayout en ruta distinta | Agregar `/usr/lib/klayout/` o el directorio de instalación al PATH |

**Rendimiento:**

Las herramientas corren a velocidad nativa en WSL2 (no emuladas). La única penalización significativa es I/O entre sistemas de archivos: operar siempre dentro del sistema de archivos WSL2 (`/home`, `/tmp`), nunca en `/mnt/c/` o `/mnt/d/`.

### Tabla comparativa de plataformas

| Aspecto | Linux | macOS | WSL2 |
|---|---|---|---|
| KLayout headless | ✅ nativo | ✅ con workaround PATH | ✅ igual a Linux |
| Xschem `--no_x` | ✅ | ✅ (sin XQuartz) | ✅ |
| Magic `-dnull` | ✅ | ⚠️ puede requerir XQuartz | ✅ |
| NGSpice batch | ✅ | ✅ | ✅ |
| Renderizado PNG de layout | ✅ Xvfb | ✅ XQuartz o offscreen | ✅ Xvfb o WSLg |
| I/O performance | ✅ | ✅ | ⚠️ lento en /mnt/c/ |
| Uso en GitHub Actions | ✅ ubuntu-latest | ✅ macos-latest | ❌ no soportado directamente |
| PDK accesible | ✅ | ✅ | ✅ (dentro de WSL2 fs) |

### Detección de plataforma en Riku

```python
import platform, shutil, os

def detect_platform() -> dict:
    is_wsl = "microsoft" in platform.uname().release.lower()
    return {
        "os": platform.system(),          # "Linux", "Darwin", "Windows"
        "wsl": is_wsl,
        "display": bool(os.environ.get("DISPLAY") or os.environ.get("WAYLAND_DISPLAY")),
        "wslg": is_wsl and bool(os.environ.get("DISPLAY")),
    }

def get_klayout_buddies_path() -> str | None:
    """Encuentra strmcmp/strmxor según la plataforma."""
    # Intentar en PATH primero
    if shutil.which("strmcmp"):
        return shutil.which("strmcmp")
    # macOS: dentro del .app bundle
    macos_path = "/Applications/KLayout.app/Contents/MacOS/strmcmp"
    if os.path.exists(macos_path):
        return macos_path
    # Linux: puede estar en /usr/lib/klayout/
    linux_alt = "/usr/lib/klayout/strmcmp"
    if os.path.exists(linux_alt):
        return linux_alt
    return None
```

---

## ¿Cuándo refutar estas decisiones?

**"Batch puro primero, Xvfb como fallback"** falla si:
- Alguna operación crítica de Riku (ej. render de PNG de alta calidad para el PR) no tiene equivalente en batch puro en KLayout o Xschem. En ese caso Xvfb pasa de fallback a requerimiento y hay que documentarlo como tal, no esconderlo.

**"pip install klayout para el 80% de operaciones"** deja de ser verdad si:
- Una operación core de Riku (ej. DRC con reglas complejas de density) requiere el paquete del sistema porque el PyPI package tiene la funcionalidad incompleta. Verificar con un caso de uso real antes de asumir que PyPI es suficiente.

**"Imagen Docker propia de ~400 MB"** no es mantenible si:
- Las herramientas EDA cambian versiones frecuentemente y mantener la imagen actualizada consume tiempo desproporcionado. En ese caso, depender de IIC-OSIC-TOOLS o efabless y aceptar el tamaño mayor (~2GB) a cambio de no mantener la imagen.

**"WSL2 como único soporte Windows"** es insuficiente si:
- Hay usuarios reales en Windows que no pueden o no quieren usar WSL2 (restricciones corporativas, etc.). KLayout tiene binarios nativos Windows que podrían funcionar para operaciones básicas. No implementar soporte nativo Windows sin evidencia de demanda real.

**"`DISPLAY` como variable central"** es frágil si:
- Hay entornos donde `$DISPLAY` está seteado pero no hay display real (ej. algunas configuraciones de SSH con X11 forwarding mal configurado). En ese caso necesitamos un probe activo (`xdpyinfo`) en vez de confiar en la variable.

## Decisiones de diseño para Riku

1. **Modo batch puro como primer intento.** Antes de levantar Xvfb, intentar siempre el modo sin display de cada herramienta. Xvfb es el fallback, no el default.

2. **`pip install klayout` para diff y XOR, paquete del sistema para PNG.** Documentar esta distinción claramente. El PyPI package es suficiente para el 80% de las operaciones de Riku.

3. **Verificación de versiones lazy.** Detectar versiones al primer uso de cada herramienta, no al arrancar. Imprimir warnings accionables: qué versión está instalada, qué versión se necesita, cómo actualizar.

4. **Imagen Docker propia pequeña (~400 MB comprimida).** No depender de la imagen de OpenLane (2+ GB). Publicarla en `ghcr.io` para que los usuarios de Riku puedan usarla directamente en GitHub Actions.

5. **WSL2 como ciudadano de primera clase.** Las instrucciones de instalación para Windows apuntan a WSL2, no a los binarios nativos Windows de KLayout. Todos los scripts de Riku usan rutas Unix. La única excepción es el I/O warning: documentar que los archivos deben estar en el filesystem WSL2.

6. **`DISPLAY` como variable central de decisión.** Si `$DISPLAY` no está seteado, Riku elige automáticamente el modo sin display de cada herramienta. Si está seteado, permite operaciones que requieren display (rendering PNG de alta calidad, diff visual de Xschem).

---

## Referencias

### KLayout headless
- **Repo**: https://github.com/KLayout/klayout
- **klayout PyPI** (sin GUI): https://pypi.org/project/klayout/
- **Flags de línea de comandos**: https://www.klayout.de/doc/manual/klayout_ref.html
- **Python API (`klayout.db`)**: https://www.klayout.de/doc/code/index.html
- **DRC scripting**: https://www.klayout.de/doc/manual/drc_ref.html
- **Distinción `klayout.db` vs `klayout.lay`**: https://www.klayout.de/doc/manual/python.html

### Xschem headless
- **Repo**: https://github.com/StefanSchippers/xschem
- **CHANGELOG con `--no_x`**: https://github.com/StefanSchippers/xschem/blob/master/CHANGELOG
- **Documentación oficial**: https://xschem.sourceforge.io/stefan/pg_Installation.html
- **Problema de xschemrc en CI**: issue tracker del repo de Xschem

### NGSpice headless
- **Repo** (mirror GitHub): https://github.com/ngspice/ngspice
- **Manual oficial (PDF)**: http://ngspice.sourceforge.net/docs/ngspice-manual.pdf — batch mode en sección 17, `.meas` en sección de análisis de salida
- **Sourceforge (releases oficiales)**: https://sourceforge.net/projects/ngspice/

### Magic headless
- **Repo**: https://github.com/RTimothyEdwards/magic
- **Documentación general**: http://opencircuitdesign.com/magic/
- **Modo batch `-dnull`**: documentado en `magic(1)` man page y tutoriales
- **Tutorial extracción batch**: http://opencircuitdesign.com/magic/tutorials/tut8.html

### Entornos CI de referencia con herramientas EDA
- **IIC-OSIC-TOOLS (Docker)**: https://github.com/iic-jku/iic-osic-tools — imagen Docker con todas las herramientas (~2GB)
- **OpenLane Docker**: https://github.com/The-OpenROAD-Project/OpenLane — referencia de tamaño y contenido
- **efabless CI**: https://github.com/efabless/caravel_user_project — ejemplo real de CI con DRC/LVS

### Xvfb y rendering offscreen
- **Xvfb man page**: https://www.x.org/releases/current/doc/man/man1/Xvfb.1.xhtml
- **`xvfb-run`**: wrapper conveniente disponible en `x11-utils` — `man xvfb-run`
- **Qt offscreen platform plugin**: https://doc.qt.io/qt-6/qpa.html — `QT_QPA_PLATFORM=offscreen`

### Ver también
- [gds_klayout_magic_diff.md](gds_klayout_magic_diff.md) — operaciones específicas de KLayout y Magic
- [xschem_diff_y_ecosistema_eda.md](xschem_diff_y_ecosistema_eda.md) — operaciones específicas de Xschem
- [ngspice_diff_y_versionado.md](ngspice_diff_y_versionado.md) — NGSpice batch mode
- [../operaciones/ci_drc_lvs_regresiones.md](../operaciones/ci_drc_lvs_regresiones.md) — uso en pipelines CI
