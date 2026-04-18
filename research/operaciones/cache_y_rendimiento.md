# Caché y Rendimiento para Miku

## 1. Operaciones costosas y tiempos reales

### Benchmarks estimados por tipo de operación

| Operación | Hardware referencia | Tiempo típico | Peor caso |
|---|---|---|---|
| XOR geométrico GDS (100 MB) | 8-core / 32 GB RAM | 30–90 s | 3–5 min |
| XOR geométrico GDS (1 GB) | 8-core / 32 GB RAM | 5–15 min | 30–60 min |
| XOR geométrico GDS (5 GB, full chip) | 16-core / 64 GB RAM | 30–120 min | >4 h |
| `strmcmp` diff estructural (100 MB) | cualquier moderno | 5–30 s | 2 min |
| `strmcmp` diff estructural (1 GB) | cualquier moderno | 30–90 s | 10 min |
| Diff Python `LayoutDiff` (100 MB) | 8-core / 32 GB RAM | 20–60 s | 5 min |
| Render PNG KLayout (100 MB) | cualquier moderno | 5–15 s | 1 min |
| Render PNG KLayout (1 GB) | 8-core / 32 GB RAM | 1–5 min | 20 min |
| DRC KLayout (100 MB) | 8-core / 32 GB RAM | 2–10 min | 30 min |
| DRC Magic (50 MB equiv.) | cualquier moderno | 1–5 min | 15 min |
| Extracción netlist Magic (celda media) | cualquier moderno | 10–60 s | 5 min |
| Extracción netlist KLayout (celda media) | cualquier moderno | 20–120 s | 10 min |
| Simulación NGSpice transient (simple) | cualquier moderno | 1–10 s | 2 min |
| Simulación NGSpice transient (complejo) | cualquier moderno | 1–10 min | 1 h |
| Simulación NGSpice Monte Carlo (100 runs) | 8-core / 32 GB RAM | 5–30 min | 3 h |
| Exportar SVG Xschem (esquemático grande) | cualquier moderno | < 1 s | 5 s |

**Fuentes de los estimados:** benchmarks de OpenLane v2 en GHA (runners ubuntu-latest de 4 cores), reportes de efabless MPW, documentación de KLayout DRC tiling, mediciones propias con gdstk en chips SKY130 de complejidad media (~500k polígonos).

### Qué hace que cada operación sea costosa

**XOR geométrico:** Requiere aplanar (flatten) la jerarquía GDS completa. Una celda top de SKY130 con 10 k instancias puede expandirse a 50–200 M polígonos. El cuello de botella es la memoria y el I/O, no la CPU. Sin tiling, un GDS de 2 GB puede requerir 20–40 GB de RAM para el flat database.

**Simulación NGSpice:** El tiempo escala linealmente con el número de puntos de tiempo y cuadráticamente con el número de nodos (factorización SPARSE LU). Circuitos con memoria (SRAM, PLL) son peores porque requieren pasos muy pequeños para convergencia.

**DRC:** I/O-bound para lecturas repetidas de la misma geometría. Reglas complejas (density checks, antenna) requieren operaciones globales que no se paralelizan fácilmente. KLayout con `tiles(1.mm, 1.mm)` + `threads(N)` reduce el tiempo casi linealmente hasta ~8 threads antes de que el I/O se convierta en cuello de botella.

**Extracción de netlist:** Dominada por la propagación de conectividad (flood fill en el grafo de capas). Escala con el número de vías y contactos, no con el área.

---

## 2. Estrategia de caché por contenido (content-addressable)

### Principio fundamental

Toda operación costosa de Miku se puede modelar como función pura:

```
resultado = f(archivo_fuente, parámetros, versión_herramienta)
```

La clave de caché es el hash de todas las entradas. Si las entradas no cambiaron, el resultado tampoco.

### Esquema de claves

```
cache_key = SHA256(
    content_hash(archivo_fuente),       # SHA256 del GDS/SPICE/mag
    SHA256(parámetros_canonicalizados), # DRC rules file, threads, tolerancia
    version_string(herramienta)         # "klayout 0.29.2", "ngspice 42"
)
```

**Componente por componente:**

| Entrada | Cómo hashear | Por qué |
|---|---|---|
| Archivo GDS | SHA256 del blob git | Exacto, ya lo calcula git |
| Archivo .spice | SHA256 del blob normalizado | Normalizar timestamps antes |
| Archivo .mag | SHA256 sin líneas `timestamp` | El timestamp de Magic es ruido |
| Rules file DRC | SHA256 del archivo de reglas | Una nueva versión del PDK invalida cache |
| Versión KLayout | `klayout --version` | Una actualización puede cambiar resultados DRC |
| Versión NGSpice | `ngspice --version` | Cambios en modelos numéricos cambian resultados |
| Parámetros numéricos | Canonicalizar floats, ordenar dict | Evitar fallos de caché por formato |

### Estructura del directorio de caché local

```
~/.cache/miku/
├── ops/
│   ├── <cache_key_64char>/
│   │   ├── result.json        # metadatos: tiempo, exit_code, herramienta
│   │   ├── stdout.log
│   │   ├── artifacts/
│   │   │   ├── diff.gds       # artefactos de la operación
│   │   │   ├── render.png
│   │   │   └── drc_report.xml
│   │   └── MANIFEST           # lista de archivos + sus hashes
│   └── ...
├── blobs/
│   └── <sha256>/              # content-addressable store, igual que git objects
│       └── data
└── index.db                   # SQLite: cache_key → ruta, timestamp, tamaño
```

**`index.db` SQLite** para lookups O(1) sin recorrer el filesystem. Schema mínimo:

```sql
CREATE TABLE cache_entries (
    key TEXT PRIMARY KEY,          -- SHA256 hex 64 chars
    created_at INTEGER,            -- Unix timestamp
    last_accessed INTEGER,
    size_bytes INTEGER,
    op_type TEXT,                  -- "xor", "drc", "sim", "render", "diff_struct"
    source_hash TEXT,              -- hash del archivo fuente (para eviction por fuente)
    artifacts_path TEXT            -- path absoluto al directorio de artefactos
);
CREATE INDEX idx_last_accessed ON cache_entries(last_accessed);
CREATE INDEX idx_source_hash ON cache_entries(source_hash);
```

### Implementación en Python

```python
import hashlib, json, subprocess, sqlite3
from pathlib import Path

CACHE_DIR = Path.home() / ".cache" / "miku"

def compute_cache_key(source_path: Path, op: str, params: dict, tool_version: str) -> str:
    source_hash = hashlib.sha256(source_path.read_bytes()).hexdigest()
    params_hash = hashlib.sha256(
        json.dumps(params, sort_keys=True).encode()
    ).hexdigest()
    combined = f"{source_hash}:{op}:{params_hash}:{tool_version}"
    return hashlib.sha256(combined.encode()).hexdigest()

def cache_lookup(key: str) -> Path | None:
    db = sqlite3.connect(CACHE_DIR / "index.db")
    row = db.execute("SELECT artifacts_path FROM cache_entries WHERE key=?", (key,)).fetchone()
    if row:
        p = Path(row[0])
        if p.exists():
            db.execute("UPDATE cache_entries SET last_accessed=strftime('%s','now') WHERE key=?", (key,))
            db.commit()
            return p
    return None

def cache_store(key: str, artifacts_dir: Path, op_type: str, source_hash: str):
    size = sum(f.stat().st_size for f in artifacts_dir.rglob("*") if f.is_file())
    db = sqlite3.connect(CACHE_DIR / "index.db")
    db.execute("""
        INSERT OR REPLACE INTO cache_entries
        (key, created_at, last_accessed, size_bytes, op_type, source_hash, artifacts_path)
        VALUES (?, strftime('%s','now'), strftime('%s','now'), ?, ?, ?, ?)
    """, (key, size, op_type, source_hash, str(artifacts_dir)))
    db.commit()
```

---

## 3. Caché local vs. caché compartida en equipo

### Modelo de capas (igual que ccache/sccache/Bazel)

```
Request → L1: caché local (disco) → HIT → retornar resultado
                                  → MISS
                                       ↓
                        L2: caché remota (S3 / servidor compartido) → HIT → copiar a L1 + retornar
                                                                     → MISS
                                                                          ↓
                                                           ejecutar operación → guardar en L1 + L2
```

**L1 (local):** `~/.cache/miku/` — acceso instantáneo, sin latencia de red. Límite recomendado: 20–50 GB en workstation, 5–10 GB en laptop.

**L2 (remota compartida):** S3-compatible (AWS S3, MinIO, Cloudflare R2, Backblaze B2). El equipo comparte resultados: si un ingeniero corrió DRC sobre el commit `abc123`, otro no tiene que repetirlo.

### Backend S3 — protocolo de operaciones

```python
import boto3

class RemoteCache:
    def __init__(self, bucket: str, prefix: str = "miku-cache/"):
        self.s3 = boto3.client("s3")
        self.bucket = bucket
        self.prefix = prefix

    def get(self, key: str, dest_dir: Path) -> bool:
        manifest_key = f"{self.prefix}{key}/MANIFEST"
        try:
            manifest = json.loads(
                self.s3.get_object(Bucket=self.bucket, Key=manifest_key)["Body"].read()
            )
        except self.s3.exceptions.NoSuchKey:
            return False
        for filename, _ in manifest["files"]:
            self.s3.download_file(
                self.bucket,
                f"{self.prefix}{key}/{filename}",
                str(dest_dir / filename)
            )
        return True

    def put(self, key: str, artifacts_dir: Path):
        files = [(f.name, hashlib.sha256(f.read_bytes()).hexdigest())
                 for f in artifacts_dir.iterdir() if f.is_file()]
        for filename, _ in files:
            self.s3.upload_file(str(artifacts_dir / filename), self.bucket,
                                f"{self.prefix}{key}/{filename}")
        manifest = {"files": files, "uploaded_at": time.time()}
        self.s3.put_object(
            Bucket=self.bucket,
            Key=f"{self.prefix}{key}/MANIFEST",
            Body=json.dumps(manifest)
        )
```

### Costos reales de caché remota (referencia AWS S3)

| Operación | Costo |
|---|---|
| Almacenamiento | $0.023/GB/mes (S3 Standard) |
| PUT (subida) | $0.005 por 1k requests |
| GET (descarga, mismo region) | Gratis entre EC2 y S3; $0.09/GB a internet |
| Alternativa R2 | $0.015/GB/mes, sin cargo por egress |

Para un equipo de 5 personas trabajando en chips SKY130, la caché remota de un proyecto (~50 GB de artefactos DRC/sim) cuesta ~$1–3/mes en Cloudflare R2. Vale absolutamente la pena.

### Política de escritura: write-through vs. write-back

**Decisión recomendada: write-through** — al completar cualquier operación costosa, subir a L2 inmediatamente. El overhead de subida (~5–30 s para artefactos de varios MB) es irrelevante comparado con horas de recomputo. No hay riesgo de pérdida de caché por crash.

### Autenticación para caché compartida en CI

Usar presigned URLs de S3 para CI sin exponer credenciales permanentes:

```python
# Miku genera un presigned URL válido 1 hora para descarga
url = s3.generate_presigned_url("get_object",
    Params={"Bucket": bucket, "Key": key}, ExpiresIn=3600)
```

---

## 4. Invalidación de caché

### Cuándo invalidar — árbol de dependencias

```
.mag / .gds (fuente)
    └── XOR diff         → invalidar si cambia el GDS fuente o el GDS comparado
    └── Render PNG       → invalidar si cambia el GDS fuente o los layer properties
    └── DRC              → invalidar si cambia el GDS fuente O el rules file del PDK
    └── Extracción       → invalidar si cambia el GDS fuente O el tech file

.spice / .sch (fuente)
    └── Simulación       → invalidar si cambia el netlist, los modelos del PDK, o los comandos .meas
    └── Diff estructural → invalidar si cambia cualquiera de los dos netlists comparados

PDK (sky130A/libs.tech/)
    └── Cualquier operación que use el PDK → invalidar TODO lo que dependa de ese PDK
```

### Invalidación por dependencia transitiva

El problema crítico: si el PDK se actualiza (nueva versión de sky130A), todos los resultados de DRC y extracción son potencialmente inválidos. Miku necesita rastrear esto.

**Solución:** incluir el hash del PDK en la clave de caché.

```python
def pdk_hash(pdk_dir: Path) -> str:
    """Hash reproducible de un PDK: hash de los archivos de tech relevantes."""
    relevant = sorted(pdk_dir.rglob("*.tech")) + sorted(pdk_dir.rglob("*.lyp")) + \
               sorted(pdk_dir.rglob("*.lyt")) + sorted(pdk_dir.rglob("*.tcl"))
    h = hashlib.sha256()
    for f in relevant:
        h.update(f.read_bytes())
    return h.hexdigest()[:16]  # 16 chars es suficiente para colisión negligible
```

### Política de invalidación explícita

Comandos que el usuario puede ejecutar:

```
miku cache invalidate --source <path>      # invalida todas las entradas para este archivo
miku cache invalidate --op xor             # invalida todos los XOR (ej: después de actualizar KLayout)
miku cache invalidate --pdk sky130A        # invalida todo lo que use este PDK
miku cache invalidate --all                # limpieza total
miku cache evict --max-size 20GB           # LRU eviction hasta quedar bajo el límite
miku cache evict --older-than 30d          # eliminar entradas > 30 días
```

### Eviction automática — LRU con límite de tamaño

```python
def evict_lru(max_size_bytes: int):
    db = sqlite3.connect(CACHE_DIR / "index.db")
    total = db.execute("SELECT SUM(size_bytes) FROM cache_entries").fetchone()[0] or 0
    if total <= max_size_bytes:
        return
    # Ordenar por last_accessed ASC (más viejo primero)
    old_entries = db.execute(
        "SELECT key, artifacts_path, size_bytes FROM cache_entries ORDER BY last_accessed ASC"
    ).fetchall()
    freed = 0
    for key, path, size in old_entries:
        if total - freed <= max_size_bytes:
            break
        shutil.rmtree(path, ignore_errors=True)
        db.execute("DELETE FROM cache_entries WHERE key=?", (key,))
        freed += size
    db.commit()
```

---

## 5. Streaming y procesamiento incremental de GDS multi-GB

### Por qué es un problema

`klayout.db.Layout.read(path)` carga todo el GDS en memoria. Un archivo de 2 GB puede requerir 15–25 GB de RAM para el modelo en memoria (el overhead de objetos Python/C++ es ~8–12x el tamaño en disco). Un chip completo de SKY130 puede producir GDS de 5–15 GB.

### Estrategia 1: Tiling en KLayout (disponible hoy)

Para XOR y DRC, KLayout soporta procesamiento por tiles sin modificar el API:

```ruby
# xor_tiled.drc
source($gds1, $cell)
target($out)
l1 = layout($gds1)
l2 = layout($gds2)

tiles(1.mm, 1.mm)          # tiles de 1mm × 1mm
tile_border(0.002.mm)      # overlap de 2 µm para evitar artefactos en bordes
threads(8)                 # un thread por tile en paralelo

[...capas...].each do |l, d|
  (l1.input(l, d) ^ l2.input(l, d)).output(l, d)
end
```

Con tiles de 1 mm², KLayout procesa cada tile de forma independiente y libera la memoria. El pico de RAM es proporcional al tamaño de un tile, no al GDS completo. Para chips de 3 mm × 3 mm → 9 tiles, procesados secuencialmente o en paralelo.

**Tradeoff:** Tiles muy pequeños aumentan el overhead de I/O y el número de polígonos duplicados en los borders. Para XOR, 0.5–1 mm² es el rango óptimo para chips SKY130 típicos.

### Estrategia 2: gdstk para diff por celda (sin cargar todo)

Para diff estructural sin XOR, es posible comparar celda por celda sin cargar el GDS completo:

```python
import gdstk

def diff_by_cell(gds_a: Path, gds_b: Path):
    """Compara los layouts celda por celda. Carga solo una celda a la vez."""
    lib_a = gdstk.read_gds(str(gds_a))   # gdstk sí carga todo — ver nota
    lib_b = gdstk.read_gds(str(gds_b))

    cells_a = {c.name: c for c in lib_a.cells}
    cells_b = {c.name: c for c in lib_b.cells}

    for name in set(cells_a) | set(cells_b):
        if name not in cells_a:
            yield {"op": "added", "cell": name}
        elif name not in cells_b:
            yield {"op": "removed", "cell": name}
        else:
            yield from compare_cell(cells_a[name], cells_b[name])
```

**Nota:** gdstk también carga todo en memoria actualmente. El streaming real requiere Rust (`gds21`) con un parser SAX-style o la implementación del streaming GDS propuesto en el stack híbrido Python+Rust.

### Estrategia 3: Streaming GDS en Rust via PyO3 (fase posterior)

El formato GDS es un stream de records con estructura simple:

```
[2 bytes: record_length] [1 byte: record_type] [1 byte: data_type] [datos]
```

Un parser SAX-style en Rust puede procesar el archivo sin cargar todo en memoria:

```rust
// Pseudo-código del parser streaming
pub fn stream_cells<F>(path: &Path, mut callback: F) -> Result<()>
where F: FnMut(GdsRecord)
{
    let mut reader = BufReader::with_capacity(64 * 1024, File::open(path)?);
    loop {
        let record = read_next_record(&mut reader)?;
        match record {
            GdsRecord::Eof => break,
            _ => callback(record),
        }
    }
    Ok(())
}
```

Esto reduce el uso de RAM de O(tamaño GDS) a O(celda más grande), permitiendo procesar GDS de cualquier tamaño. **Esta es la razón técnica principal del stack híbrido Python+Rust** documentado en `lenguajes_y_stack.md`.

### Estrategia 4: Procesar solo celdas modificadas

Si Miku conoce qué celdas cambiaron entre commits (via diff estructural rápido de texto o `strmcmp`), puede evitar el XOR completo y hacer XOR solo sobre esas celdas:

```python
def targeted_xor(gds_a: Path, gds_b: Path, changed_cells: list[str]) -> Path:
    """XOR solo sobre las celdas que cambiaron. Mucho más rápido que XOR completo."""
    for cell in changed_cells:
        run_klayout_xor(gds_a, gds_b, cell=cell, out=f"xor_{cell}.gds")
```

**Benchmark estimado:** Para un chip con 500 celdas donde solo 3 cambiaron, el XOR por celda es ~150x más rápido que el XOR flat completo.

---

## 6. Paralelización

### Qué se puede paralelizar y cómo

| Operación | Paralelizable | Granularidad | Mecanismo |
|---|---|---|---|
| XOR geométrico | Sí | Por tile | KLayout `threads(N)` nativo |
| DRC | Sí | Por tile | KLayout `threads(N)` nativo |
| Render PNG por capa | Sí | Por capa o celda | `multiprocessing` Python |
| Simulaciones NGSpice | Sí | Por netlist o corner | `subprocess` paralelo |
| Monte Carlo NGSpice | Parcialmente | Por batch de runs | NGSpice `+compat` modo batch |
| Diff estructural multi-celda | Sí | Por celda | `multiprocessing` Python |
| Extracción Magic por celda | Sí | Por celda | `subprocess` paralelo |
| Carga de caché remota | Sí | Por artefacto | `asyncio` + S3 parallel multipart |

### Simulaciones NGSpice en paralelo

NGSpice no tiene paralelismo interno para un solo netlist en el modo estándar. El paralelismo debe ser a nivel de proceso:

```python
import subprocess
from concurrent.futures import ProcessPoolExecutor

def run_sim(args):
    netlist, rawfile, logfile = args
    return subprocess.run(
        ["ngspice", "-b", "-r", rawfile, "-o", logfile, netlist],
        capture_output=True
    )

# Correr todos los corners en paralelo
corners = [("tt.spice", "tt.raw", "tt.log"),
           ("ff.spice", "ff.raw", "ff.log"),
           ("ss.spice", "ss.raw", "ss.log")]

with ProcessPoolExecutor(max_workers=len(corners)) as pool:
    results = list(pool.map(run_sim, corners))
```

**Para Monte Carlo:** Dividir N runs en batches de N/threads, correr en paralelo, luego consolidar los `.raw` resultantes.

### KLayout XOR con tiling y threads

```ruby
# Combinación óptima: tiles + threads
tiles(0.5.mm, 0.5.mm)   # para chips < 5mm²
tile_border(0.002.mm)
threads(8)               # detectar cores: RBA::Application.instance.num_threads
```

**Scaling observado (estimado):** En un chip de 3×3 mm² con 8 threads: ~6–7x speedup sobre single-thread (eficiencia ~75–87%, limitada por I/O compartido de lectura del GDS original).

### Detectar número de cores disponibles

```python
import os
# Preferir MIKU_JOBS si está seteado, sino detectar automáticamente
n_jobs = int(os.environ.get("MIKU_JOBS", os.cpu_count() or 4))
```

---

## 7. Tamaño de artefactos — qué va en git y qué no

### Política fundamental: git solo guarda fuentes

```
FUENTES (en git):
  .sch       esquemáticos Xschem
  .mag       layouts Magic
  .spice     netlists (sin timestamps)
  .py        código GDSFactory/GLayout
  .tcl       scripts de Magic/KLayout
  Makefile   flujo de build

ARTEFACTOS DE BUILD (NO en git):
  .gds       GDS generado desde .mag o .py
  .raw       resultados de simulación NGSpice
  .log       logs de herramientas
  .ext       archivos intermedios de extracción Magic
  *.drc      resultados DRC (regenerables)
  *.lyrdb    marker databases de KLayout
```

**`.gitignore` recomendado para proyectos Miku:**

```gitignore
# Artefactos de build EDA
*.gds
*.oas
*.raw
*.bin
*.ext
*.sim
*.nodes
*.res
*.cap
*.al
*.log
*.lyrdb
*.lvsdb

# Renderizados (van en caché Miku, no en git)
*.png
*.svg
*.pnm

# Directorios de build
build/
ngspice_output/
drc_output/
extraction/
```

### Excepción justificada: GDS de PDK y GDS externos

El GDS del PDK (sky130A_fd_sc_hd*.gds, etc.) no se regenera — es un artefacto externo. Opciones:

1. **Git LFS:** Para GDS del PDK en el repo. LFS guarda el binario fuera del packfile pero mantiene la referencia en git. `git lfs track "*.gds"` antes del primer commit del GDS.
2. **No en el repo:** Que el PDK sea una dependencia externa declarada (como pip install). Miku puede tener un `miku.toml` con `pdk = "sky130A@1.0.136"` y descargarlo en el setup.

**Recomendación:** Opción 2 para PDKs públicos (sky130A, gf180). Opción 1 (Git LFS) para GDS propietarios o cerrados que el equipo necesita versionar.

### Cuantificación del ahorro

| Escenario | Con GDS en git | Sin GDS en git |
|---|---|---|
| Repo de chip pequeño (1 celda) | ~50 MB/commit | ~500 KB/commit |
| Repo de chip mediano (50 celdas) | ~2 GB total repo | ~10 MB total repo |
| Clone inicial (red lenta) | 2–10 min | < 5 s |
| `git log` / `git status` | Lento (pack scan) | Instantáneo |

La diferencia es de 2–3 órdenes de magnitud. **Un repo que incluye GDS generados se vuelve inutilizable en semanas.**

---

## 8. Almacenamiento de artefactos de diff/render

### Dónde NO guardar

- En el repo git (aumenta el tamaño del repo permanentemente)
- En `/tmp` (se pierde al reiniciar, no compartible entre máquinas)
- En el worktree del proyecto (contamina el workspace del diseñador)

### Dónde SÍ guardar

**Estructura de almacenamiento de Miku:**

```
~/.cache/miku/artifacts/
├── by-commit/
│   └── <repo-id>/<commit-sha>/
│       ├── render/
│       │   ├── top_cell.png          # render del layout completo
│       │   ├── top_cell_metal1.png   # render por capa
│       │   └── schematic.svg         # SVG del esquemático Xschem
│       ├── drc/
│       │   ├── drc_report.xml
│       │   └── drc_report.html
│       └── sim/
│           ├── results.json          # métricas extraídas de .meas
│           └── waveforms.npz         # numpy compressed (no el .raw completo)
├── by-diff/
│   └── <repo-id>/<commit-a>-<commit-b>/
│       ├── xor.gds                   # polígonos de diferencia XOR
│       ├── xor_render.png            # render del XOR coloreado
│       ├── diff_struct.json          # diff estructural de celdas
│       └── sim_diff.json             # comparación de métricas de simulación
└── thumbnails/
    └── <content-hash>.png            # thumbnails 256x256 para UI
```

### Waveforms: no guardar el .raw completo

Un archivo `.raw` de NGSpice de una simulación transient de 1 µs con 1000 nodos = ~50–200 MB. Es irreproducible desde el netlist + condiciones, pero guardar el `.raw` completo en caché es costoso.

**Estrategia recomendada:**

```python
import numpy as np
from spicelib import RawRead

def compress_waveform(raw_path: Path, out_path: Path, signals_of_interest: list[str]):
    """
    Guarda solo las señales relevantes como numpy compressed.
    Reducción típica: 200 MB → 500 KB para 5 señales.
    """
    raw = RawRead(str(raw_path))
    data = {}
    for sig in signals_of_interest:
        try:
            data[sig] = raw.get_trace(sig).data
        except KeyError:
            pass
    data["time"] = raw.get_trace("time").data
    np.savez_compressed(str(out_path), **data)
```

**Política de retención:**
- Render PNGs de cada commit reciente: 90 días o 100 commits, lo que sea menor
- XOR GDS: solo el diff entre commits adyacentes del branch principal
- Waveforms comprimidos: 30 días o hasta que se supere límite de caché
- Thumbnails: permanentes (son pequeños, ~10–50 KB cada uno)

### Integración con interfaces web/PR

Para mostrar diffs en PRs (similar a GitHub's rendering de STL/imágenes), Miku puede generar artefactos estandarizados y hospedarlos:

```
PR #42 → trigger CI → miku diff HEAD~1..HEAD
    → genera xor_render.png, diff_struct.json, sim_diff.json
    → sube a S3 presigned URLs (válidos 30 días)
    → comenta en el PR con los URLs y un resumen JSON
```

El comentario de PR incluye:
- Thumbnail del XOR coloreado (cambios en rojo sobre fondo gris)
- Lista de celdas modificadas/añadidas/eliminadas
- Comparación de métricas de simulación (si las hay)
- Link al reporte DRC si hay violaciones nuevas

### Politica de tamaño total de caché

| Tipo de cache | Límite recomendado (workstation) | Límite recomendado (CI) |
|---|---|---|
| L1 local total | 30 GB | 5 GB (efímero por runner) |
| Artefactos de render | 5 GB | 1 GB |
| XOR GDS | 10 GB | 2 GB |
| Waveforms comprimidos | 5 GB | 500 MB |
| Thumbnails | 500 MB | 100 MB |
| Caché remota S3 (equipo) | Sin límite (pagar) | — |

Eviction automática: cuando L1 supera el límite, eliminar por LRU empezando por XOR GDS (los más grandes y más fáciles de regenerar).

---

## Notas de retroalimentación — flujos reales

> Estas notas provienen del research de experiencia de usuario. Identifican casos donde la
> estrategia de caché o la política de artefactos necesita revisión.

### Nota 1: La política "GDS siempre en .gitignore" no es universal

La Sección 7 ("Tamaño de artefactos") clasifica `.gds` como artefacto de build que nunca va en git. Esta clasificación es correcta para flujos Magic→GDS, pero no para:

- **KLayout-primary (IHP/GF180):** El `.gds` o `.oas` puede ser la fuente editada directamente. Ponerlo en `.gitignore` destruiría la fuente del layout.
- **GDSFactory:** El `.gds` es output de Python, correcto que vaya en `.gitignore`. Pero el `.oas` puede ser el formato de entrega — no va en `.gitignore` si es el artefacto de tapeout que se versiona.
- **GDS de PDK:** Los GDS de celdas estándar del PDK no se regeneran — son dependencias externas. Si el flujo los incluye en el repo (por reproducibilidad), van en LFS, no en `.gitignore`.

**Extensión propuesta:** El `.gitignore` que genera `miku init` debe ser condicional al valor de `layout.source` en `miku.toml`. Si `layout.source = "magic"` → `*.gds` a `.gitignore`. Si `layout.source = "klayout"` → `*.gds` NO va a `.gitignore`.

### Nota 2: OASIS como alternativa de storage para reducir tamaño

La Sección 7 menciona `.oas` como formato gitignoreado junto a `.gds`. Hay un caso donde OASIS como formato de storage en caché tiene ventaja enorme sobre GDS:

- OASIS puede ser **1000x más compacto** que GDS equivalente (ejemplo: 150 MB → 25 kB).
- Para la caché L1/L2, almacenar artefactos en OASIS en lugar de GDS reduciría dramáticamente el costo de storage y la latencia de transferencia S3.
- KLayout convierte GDS↔OASIS de forma transparente y sin pérdida.

**Oportunidad:** Al almacenar XOR results y renders en caché, convertir automáticamente de GDS a OASIS. El ahorro de espacio y egress en S3 justifica el paso de conversión.

Fuente: [KLayout forum — OASIS as GDS successor](https://www.klayout.de/forum/discussion/2152/oasis-the-successor-to-gds)

### Nota 3: El hash del PDK en la clave de caché requiere PDK version pinning

La Sección 4 ("Invalidación por PDK") propone incluir el hash del PDK en la clave de caché con la función `pdk_hash()`. Esto solo funciona si el PDK está instalado de forma reproducible y versionada.

Sin PDK version pinning (con `volare` o `ciel`), dos máquinas del mismo equipo pueden tener versiones diferentes del PDK sin saberlo, produciendo cache misses constantemente o, peor, hits incorrectos si el hash no incluye el PDK correctamente.

**Dependencia:** La estrategia de caché de este documento requiere que el equipo adopte PDK version pinning. Estos son complementarios — sin el pinning, la caché de PDK no es confiable.

Fuente: [github.com/chipfoundry/volare](https://github.com/chipfoundry/volare)

### Nota 4: Artefactos `.ext` de Magic son intermedios, no finales

La tabla de `.gitignore` ya incluye `*.ext`. Confirmación de práctica real: los archivos `.ext` son intermedios generados por `magic extract all` — uno por celda — y son siempre regenerables. En proyectos con muchas celdas pueden generar cientos de archivos que contaminan el working tree. `miku init` debe asegurarse de que `*.ext` está en `.gitignore` independientemente del PDK.

---

## ¿Cuándo refutar estas decisiones?

**"SHA256 como clave de caché"** cambia si:
- El hashing de archivos GDS grandes (>1 GB) se convierte en un cuello de botella medible. SHA256 de 1 GB tarda ~1-2s — si ese tiempo aparece en el perfil de `miku diff`, migrar a Blake3 o a un hash incremental basado en chunks.

**"SQLite como índice de caché"** no es necesario si:
- El volumen de entradas de caché en uso real es menor de ~5k entradas. Con eso, un directorio con nombres de archivo como clave es suficiente y más simple. Solo mover a SQLite cuando haya evidencia de que el filesystem es el cuello de botella.

**"S3 como caché L2"** no tiene sentido si:
- El usuario inicial de Miku es un solo diseñador trabajando en local — el caché L2 compartido no le aporta nada y agrega complejidad de configuración. Solo implementar L2 cuando haya un equipo real que lo necesite.

**"XOR selectivo por celdas"** puede ser insuficiente si:
- Los cambios en un diseño real tienden a tocar la celda top (por ejemplo, al mover instancias). Si el 80% de los diffs en la práctica tocan la celda top, el XOR selectivo no aporta el speedup esperado y habría que pensar en otro enfoque (ej. diff de bounding boxes, diff de capas individuales).

**"Invalidación conservadora (ante duda, recomputar)"** tiene costo real si:
- Los falsos positivos de invalidación son frecuentes y el tiempo de recomputo es alto. Si el equipo se queja de que el caché "nunca funciona", investigar si la clave de caché es demasiado sensible (ej. incluye versión de herramienta en el minor version cuando no es necesario).

## 9. Decisiones de diseño y justificaciones

### Por qué SHA256 y no MD5/Blake3

SHA256 ya está en git. Usar el mismo algoritmo permite aprovechar el object store de git como caché de contenido para blobs de archivos fuente: `git cat-file blob <sha>` retorna el contenido sin necesitar rehashear. Blake3 es 3–5x más rápido pero no está en git; SHA256 es suficiente rápido para archivos EDA (un SHA256 de un GDS de 1 GB tarda ~1–2 s).

### Por qué SQLite y no sistema de archivos plano

El sistema de archivos plano (directorios = keys) escala bien hasta ~10k entradas. Con operaciones diarias en un chip completo, se pueden acumular 50–200k entradas de caché en meses. SQLite con índice en `last_accessed` permite eviction O(log n) y queries complejas (invalidar por PDK, por tipo de operación) sin recorrer el filesystem. SQLite es un único archivo portable, fácil de inspeccionar con herramientas estándar.

### Por qué S3 y no un servidor propio

S3 (o compatible: R2, MinIO) elimina la necesidad de mantener un servidor de caché. El protocolo es un estándar de facto. En entornos CI (GitHub Actions), hay acceso nativo a AWS/GCP sin VPN. Para equipos pequeños (2–10 personas), el costo es < $5/mes. Para CI, la caché remota elimina el problema de los "cold runners" donde cada job rebuild todo.

### Por qué no usar git-lfs para los artefactos de caché

Git LFS está diseñado para artefactos que se versionan (tienen historia). Los artefactos de caché de Miku son derivados — no tienen historia propia, son funciones de sus inputs. Usar LFS los trataría como primeros ciudadanos del repo cuando son ciudadanos de segunda. Además, LFS no tiene eviction ni invalidación por contenido.

### Por qué comprimir waveforms en lugar de guardar el .raw

El formato `.raw` de NGSpice incluye todos los nodos internos (incluyendo nodos de dispositivos BSIM que no interesan al diseñador). Para un opamp con 50 transistores, puede haber 300–500 señales de las cuales solo 5–10 son de interés. La compresión selectiva reduce el tamaño 100–400x con pérdida cero en las señales de interés. El `.raw` completo se puede regenerar en minutos si se necesita un análisis más profundo.

### Trade-off: trabajo extra por mejor UX

Siguiendo la prioridad del proyecto (invertir más trabajo si mejora la UX), las decisiones tomadas priorizan:

1. **Transparencia del caché:** `miku status` muestra qué operaciones están cacheadas y cuánto tiempo ahorran. El usuario nunca necesita limpiar manualmente la caché.
2. **Caché compartida sin configuración:** Si el equipo usa el mismo S3 bucket (configurado en `miku.toml`), el sharing es automático sin acción extra por parte del usuario.
3. **Invalidación conservadora:** Ante duda, recomputar. Un falso negativo de caché (resultado incorrecto servido) es catastrófico en diseño de chips. Un falso positivo (recomputar cuando no era necesario) solo es más lento.
4. **Artefactos siempre disponibles offline:** El L1 local guarda copias de todos los artefactos usados recientemente. Trabajar sin red es la experiencia normal esperada.

---

## Referencias

### Sistemas de caché content-addressable de referencia
- **ccache**: https://github.com/ccache/ccache — caché para compiladores C/C++, modelo L1/L2
- **sccache**: https://github.com/mozilla/sccache — ccache compatible con S3, referencia para caché remoto
- **Bazel remote cache**: https://bazel.build/remote/caching — protocolo de caché remoto estándar
- **cas-server**: https://github.com/buildbarn/bb-storage — implementación del protocolo Bazel remote

### Almacenamiento S3-compatible (opciones económicas)
- **Cloudflare R2**: https://developers.cloudflare.com/r2/ — sin costo de egress, ~$0.015/GB/mes
- **Backblaze B2**: https://www.backblaze.com/b2/cloud-storage.html — barato, compatible con S3
- **MinIO** (self-hosted): https://github.com/minio/minio — S3-compatible para on-premise

### Streaming y procesamiento GDS
- **gds21 (Rust)**: https://github.com/dan-fritchman/Layout21/tree/main/gds21 — parser GDS en Rust puro
- **gdstk (C++)**: https://github.com/heitzmann/gdstk — procesamiento eficiente de GDS en C++
- **KLayout tiling API**: https://www.klayout.de/doc/manual/tiling.html — procesamiento por tiles

### Benchmarks de referencia
- **efabless MPW timing data**: https://platform.efabless.com — tiempos reales de DRC/LVS en chips MPW
- **OpenLane GHA runs**: https://github.com/The-OpenROAD-Project/OpenLane/actions — tiempos observables en CI público

### Paralelización Python
- **`concurrent.futures`**: https://docs.python.org/3/library/concurrent.futures.html
- **`multiprocessing`**: https://docs.python.org/3/library/multiprocessing.html
- **Ray** (para workloads distribuidos): https://github.com/ray-project/ray

### Ver también
- [../arquitectura/lenguajes_y_stack.md](../arquitectura/lenguajes_y_stack.md) — justificación de Rust para streaming
- [ci_drc_lvs_regresiones.md](ci_drc_lvs_regresiones.md) — integración de caché en pipelines CI
- [../herramientas/gds_klayout_magic_diff.md](../herramientas/gds_klayout_magic_diff.md) — operaciones que se cachean (XOR, DRC)
- [../arquitectura/arquitectura_cli_y_orquestacion.md](../arquitectura/arquitectura_cli_y_orquestacion.md) — diseño de `miku ci` y cómo los stages invocan el sistema de caché
- [estrategia_merge_archivos_mixtos.md](estrategia_merge_archivos_mixtos.md) — deps.toml como fuente de invalidación por dependencias cruzadas
