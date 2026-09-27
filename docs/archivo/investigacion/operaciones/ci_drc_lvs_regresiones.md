# CI y Detección de Regresiones DRC/LVS para Riku

> Investigación para el diseño del subsistema de integración continua de Riku — VCS especializado
> para diseño de chips sobre Git, con soporte para KLayout, Xschem, NGSpice y Magic VLSI.

---

## 1. DRC y LVS en el contexto de CI — el equivalente a "tests que no deben romperse"

### Qué es DRC

**Design Rule Check (DRC)** verifica que el layout físico cumpla las reglas geométricas de la
foundry: distancias mínimas entre metales, anchos mínimos de pistas, enclosures de vías,
densidades por capa. Cada regla existe porque violaciones producen chips que no fabrican
correctamente o tienen baja yield.

Un DRC limpio es un **prerrequisito hard para tapeout** — no hay negociación. Cualquier commit
que introduzca violaciones DRC regresa el proyecto a un estado inválido para fabricación.

### Qué es LVS

**Layout vs. Schematic (LVS)** extrae el netlist del layout físico (qué está conectado a qué
según los polígonos de metal) y lo compara contra el netlist del esquemático (qué debería
estar conectado según el diseño lógico). Un mismatch LVS significa que lo que se va a fabricar
no es lo que se diseñó.

LVS falla cuando:
- Una via fue borrada (net flotante)
- Dos pines con el mismo nombre en el esquemático apuntan a dos redes distintas en el layout
- Se renombró un net en el esquemático sin actualizar el layout
- Se añadió un dispositivo al layout que no existe en el esquemático

### Por qué son "tests que no deben romperse"

La analogía con testing de software es directa:

| Software | Chip design |
|---|---|
| Tests unitarios rotos | DRC con nuevas violaciones |
| Integración fallida | LVS mismatch |
| Build no compila | DRC que impide tapeout |
| Regresión silenciosa | Aumento de violaciones DRC sin alerta |

La diferencia clave con tests de software: **DRC/LVS pueden tardar horas en diseños grandes**.
Esto hace que el diseño del sistema de caché (sección 6) sea tan importante como el check en sí.

### El rol de Riku en este contexto

Riku no reemplaza KLayout DRC, Netgen LVS ni Magic. Los **orquesta como backend**, extrae
resultados, los asocia al commit que los generó, calcula deltas respecto al commit anterior, y
los expone de forma legible en un PR. El valor de Riku es la integración, no la implementación
de los algoritmos de verificación.

---

## 2. KLayout DRC headless — ejecución y parseo de resultados

### Modos de invocación

```bash
# Batch mode completo (sin GUI, sin display requerido)
klayout -b -r drc_script.drc -rd input=layout.gds -rd report=drc_out.lyrdb

# Headless sin display en Linux CI (equivalente a -b para scripts)
klayout -zz -r drc_script.drc
```

El flag `-b` combina `-zz -nc -rx`:
- `-zz`: sin GUI, sin X11 requerido
- `-nc`: no carga configuración de usuario (startup más rápido y determinista)
- `-rx`: sale después de ejecutar el script

Para CI **usar `-b`** — evita que diferencias de configuración local produzcan resultados distintos
entre máquinas.

### Script DRC con output JSON estructurado

KLayout DRC produce por defecto un archivo `.lyrdb` (marker database binario, visualizable en la
GUI). Para CI necesitamos JSON parseable. El patrón es ejecutar el script DRC estándar y luego
convertir el `.lyrdb` a JSON en el mismo script o en un paso separado.

```ruby
# drc_ci.drc — ejecutar con: klayout -b -r drc_ci.drc -rd gds=layout.gds -rd json_out=drc.json
source($gds)
report("DRC CI Report", "drc_report.lyrdb")

# --- Reglas DRC (ejemplo Sky130) ---
poly_width = poly.width(0.15.um)
poly_width.output("poly.W1", "Poly width < 0.15um")

met1_space = metal1.space(0.14.um)
met1_space.output("met1.S1", "Metal1 space < 0.14um")

# ... resto de reglas del PDK ...

# --- Exportar JSON al terminar ---
post_layout do
  require 'json'
  results = []
  RBA::ReportDatabase.new("r").tap do |rdb|
    rdb.load("drc_report.lyrdb")
    rdb.each_category do |cat|
      count = 0
      rdb.each_item_per_category(cat) { count += 1 }
      results << { category: cat.name, description: cat.description, count: count }
    end
  end
  total = results.sum { |r| r[:count] }
  File.write($json_out, JSON.pretty_generate({ total_violations: total, categories: results }))
end
```

**Output JSON:**
```json
{
  "total_violations": 3,
  "categories": [
    { "category": "poly.W1", "description": "Poly width < 0.15um", "count": 2 },
    { "category": "met1.S1", "description": "Metal1 space < 0.14um", "count": 1 }
  ]
}
```

### Parseo del `.lyrdb` desde Python

Si el script DRC ya produce `.lyrdb` (sin modificar), Riku puede parsearlo en Python:

```python
import klayout.db as db
import json

def parse_lyrdb(path: str) -> dict:
    rdb = db.ReportDatabase("drc")
    rdb.load(path)
    categories = []
    for cat in rdb.each_category():
        count = sum(1 for _ in rdb.each_item_per_category(cat))
        categories.append({
            "category": cat.name,
            "description": cat.description,
            "count": count
        })
    total = sum(c["count"] for c in categories)
    return {"total_violations": total, "categories": categories}
```

`klayout.db` está disponible en `pip install klayout` — no requiere KLayout instalado en el
sistema para esta operación. Es headless y funciona en Linux CI sin X11.

### Delta vs. commit anterior

El cálculo del delta es responsabilidad de Riku, no de KLayout. El flujo:

```python
import json, subprocess, pathlib

def drc_delta(commit_a: str, commit_b: str, gds_path: str) -> dict:
    """Calcula delta de violaciones DRC entre dos commits."""
    results = {}
    for commit in (commit_a, commit_b):
        # Extraer GDS del commit
        gds_bytes = subprocess.check_output(["git", "show", f"{commit}:{gds_path}"])
        tmp = pathlib.Path(f"/tmp/miku_{commit[:8]}.gds")
        tmp.write_bytes(gds_bytes)
        # Correr DRC
        json_out = f"/tmp/miku_drc_{commit[:8]}.json"
        subprocess.run([
            "klayout", "-b", "-r", "drc_ci.drc",
            "-rd", f"gds={tmp}",
            "-rd", f"json_out={json_out}"
        ], check=True)
        results[commit] = json.loads(pathlib.Path(json_out).read_text())

    a, b = results[commit_a], results[commit_b]
    delta = b["total_violations"] - a["total_violations"]
    per_category = {}
    cats_a = {c["category"]: c["count"] for c in a["categories"]}
    cats_b = {c["category"]: c["count"] for c in b["categories"]}
    for cat in set(cats_a) | set(cats_b):
        d = cats_b.get(cat, 0) - cats_a.get(cat, 0)
        if d != 0:
            per_category[cat] = {"before": cats_a.get(cat, 0), "after": cats_b.get(cat, 0), "delta": d}
    return {
        "before": a["total_violations"],
        "after": b["total_violations"],
        "delta": delta,
        "regression": delta > 0,
        "categories": per_category
    }
```

**Decisión de diseño:** El threshold de "regresión" es configurable. Por defecto, cualquier
aumento de violaciones es regresión (`delta > 0`). Proyectos con deuda técnica conocida pueden
configurar un baseline y solo alertar cuando se supera ese número.

---

## 3. LVS entre commits con Netgen

### El flujo canónico Sky130 / OpenLane

```
Xschem (.sch)   → netlist esquemático (.spice)   ─┐
                                                   ├→ Netgen LVS → reporte
Magic (.mag)    → magic extract → ext2spice        ─┘
                → netlist de layout (.spice)
```

Netgen compara topología de netlists — no simula, no verifica DRC. Un LVS limpio significa
que el layout físico implementa exactamente el circuito del esquemático.

### Extracción del netlist desde Magic (headless)

```tcl
# extract_netlist.tcl — usar con: magic -dnull -noconsole extract_netlist.tcl
load {CELL}
extract all
ext2spice lvs
ext2spice
quit
```

Donde `{CELL}` es el nombre de la celda sin extensión. Magic genera `{CELL}.spice` en el
directorio de trabajo. El flag `lvs` en `ext2spice` limpia el netlist para comparación
(elimina parásitos RC, normaliza nombres).

```bash
magic -dnull -noconsole -rcfile sky130A.magicrc \
  -T sky130A \
  extract_netlist.tcl
```

El archivo `.magicrc` y el tech file son específicos del PDK — en flujos Sky130 están en
`$PDK_ROOT/sky130A/libs.tech/magic/`.

### Exportar netlist desde Xschem (headless)

```bash
xschem -q --no_x --netlist --netlist_type spice \
  --plotfile /dev/null \
  schematic.sch
# Genera schematic.spice en el mismo directorio
```

O desde el interior de Xschem via Tcl (útil para automatización):
```bash
xschem -q --no_x --tcl "xschem netlist; exit" schematic.sch
```

### Correr Netgen LVS y parsear resultado

```bash
netgen -batch lvs \
  "layout.spice TOP_CELL" \
  "schematic.spice TOP_CELL" \
  $PDK_ROOT/sky130A/libs.tech/netgen/sky130A_setup.tcl \
  lvs_report.json
```

El archivo `sky130A_setup.tcl` contiene las reglas de equivalencia del PDK (qué dispositivos
son equivalentes, qué propiedades comparar). Sin él, Netgen no sabe que `sky130_fd_pr__nfet_01v8`
del layout es el mismo dispositivo que en el esquemático.

**Output JSON de Netgen:**
```json
{
  "circuit1": { "name": "TOP_CELL", "devices": 42, "nets": 38 },
  "circuit2": { "name": "TOP_CELL", "devices": 42, "nets": 38 },
  "match": true,
  "device_mismatches": [],
  "net_mismatches": [],
  "property_mismatches": [
    { "device": "M1", "property": "w", "value1": "2.0", "value2": "1.5" }
  ]
}
```

**Parseo en Python:**
```python
import json, subprocess, pathlib

def run_lvs(layout_spice: str, schematic_spice: str, top_cell: str,
            pdk_setup: str) -> dict:
    report = pathlib.Path("/tmp/miku_lvs.json")
    subprocess.run([
        "netgen", "-batch", "lvs",
        f"{layout_spice} {top_cell}",
        f"{schematic_spice} {top_cell}",
        pdk_setup,
        str(report)
    ], check=True)
    data = json.loads(report.read_text())
    return {
        "pass": data.get("match", False),
        "device_mismatches": len(data.get("device_mismatches", [])),
        "net_mismatches": len(data.get("net_mismatches", [])),
        "property_mismatches": len(data.get("property_mismatches", [])),
        "raw": data
    }
```

### LVS entre commits — el flujo completo

Para comparar LVS entre commit A y commit B, el flujo de Riku es:

```
git show commit_A:schematic.sch → xschem --netlist → sch_A.spice
git show commit_A:layout.mag    → magic extract   → lay_A.spice
git show commit_B:schematic.sch → xschem --netlist → sch_B.spice
git show commit_B:layout.mag    → magic extract   → lay_B.spice

netgen LVS lay_A.spice vs sch_A.spice → lvs_A.json
netgen LVS lay_B.spice vs sch_B.spice → lvs_B.json

delta: ¿pasó de PASS→FAIL? ¿FAIL→PASS? ¿mismatches aumentaron?
```

El caso más importante para CI es detectar **regresiones LVS**: un commit que convierte un LVS
que pasaba en uno que falla. El caso inverso (FAIL→PASS) es una corrección que debe celebrarse
en el reporte.

**Decisión de diseño:** No bloquear el PR si el LVS ya fallaba antes del commit (no introducir
regresión nueva). Sí bloquear si el commit introduce el fallo.

---

## 4. Waveform regression con NGSpice

### El problema

Las waveforms son flotantes, ruidosas y dependientes del estado del simulador. Comparar dos
archivos `.raw` byte a byte siempre falla aunque el circuito no haya cambiado. La comparación
correcta requiere tolerancias configurables por señal.

### Archivos `.raw` — no se versionan

Los `.raw` son artefactos de build. Van al `.gitignore`. Lo que sí se versiona:
- Los netlists `.spice` (fuente de verdad)
- Los resultados de `.meas` como JSON (métricas extraídas)
- Los archivos de configuración de testbench

### Patrón `.meas` — el estándar actual en open source

El netlist define qué medir:

```spice
* testbench_amp.spice
.include "amp.spice"
Vdd vdd 0 DC 3.3
Vin in 0 AC 1 SIN(0 0.1 1MEG)
.tran 1n 10u
.meas tran gain_3db WHEN v(out)=2.33 CROSS=1
.meas tran vout_max MAX v(out)
.meas tran vout_min MIN v(out)
.meas tran slew_rise DERIV v(out) AT=2u
.end
```

NGSpice batch:
```bash
ngspice -b -o sim.log testbench_amp.spice
```

El log contiene:
```
gain_3db            =  3.342100e+06
vout_max            =  3.287654e+00
vout_min            =  1.234567e-02
slew_rise           =  4.512345e+06
```

### Parseo de `.meas` results desde el log

```python
import re, json, pathlib

def parse_meas_log(log_path: str) -> dict:
    """Extrae todos los resultados .meas del log de NGSpice."""
    pattern = re.compile(r'^(\w+)\s+=\s+([\d.e+\-]+)', re.MULTILINE)
    text = pathlib.Path(log_path).read_text()
    return {m.group(1): float(m.group(2)) for m in pattern.finditer(text)}

# results_a = {"vout_max": 3.287654, "slew_rise": 4512345.0, ...}
```

### Configuración de tolerancias

Riku necesita un archivo de configuración por testbench que defina las tolerancias aceptables:

```yaml
# miku_sim.yaml — en el directorio del testbench o en .miku/
tolerances:
  vout_max:
    nominal: 3.3
    rtol: 0.05      # ±5% relativo
    atol: null
  slew_rise:
    nominal: 4.5e6
    rtol: 0.10      # ±10% — slew es sensible a condiciones de simulación
    atol: null
  gain_3db:
    nominal: null   # no tiene nominal fijo
    rtol: 0.01      # comparar solo contra commit anterior (±1%)
    atol: null
```

Si `nominal` es `null`, la comparación es contra el valor del commit padre (regresión relativa),
no contra un valor absoluto. Esto es más robusto para métricas que varían con el PDK o la
temperatura.

### Motor de comparación de waveforms

```python
import numpy as np
from spicelib import RawRead   # pip install spicelib

def compare_waveforms(raw_a: str, raw_b: str,
                      signal: str, rtol: float = 0.01, atol: float = 0.0) -> dict:
    """
    Compara una señal entre dos archivos .raw.
    Usa interpolación para manejar diferencias de step size.
    """
    ra = RawRead(raw_a)
    rb = RawRead(raw_b)

    time_a = ra.get_trace("time").get_wave()
    time_b = rb.get_trace("time").get_wave()
    wave_a = ra.get_trace(signal).get_wave()
    wave_b = rb.get_trace(signal).get_wave()

    # Interpolar B en los puntos de tiempo de A
    wave_b_interp = np.interp(time_a, time_b, wave_b)

    max_diff = np.max(np.abs(wave_a - wave_b_interp))
    max_val  = np.max(np.abs(wave_a))
    rel_diff = max_diff / max_val if max_val > 0 else 0.0

    passed = bool(np.allclose(wave_a, wave_b_interp, rtol=rtol, atol=atol or 0.0))
    return {
        "signal": signal,
        "passed": passed,
        "max_relative_diff": float(rel_diff),
        "rtol": rtol,
        "atol": atol
    }
```

### Comparación con `spicelib` (más madura)

`spicelib` (PyPI) maneja ASCII y binario `.raw`, es más robusta que `spyci` para uso en CI:

```python
from spicelib import RawRead

r = RawRead("simulation.raw")
print(r.get_trace_names())  # ['time', 'v(out)', 'v(in)', 'i(r1)']
wave = r.get_trace("v(out)").get_wave()   # numpy array
```

### Integración en el flujo Riku

```python
def waveform_regression(commit_base: str, commit_head: str,
                        netlist: str, config: dict) -> dict:
    regressions = []
    for commit, label in [(commit_base, "base"), (commit_head, "head")]:
        spice = checkout_file(commit, netlist)
        subprocess.run(["ngspice", "-b", "-o", f"sim_{label}.log",
                        "-r", f"sim_{label}.raw", spice], check=True)

    meas_base = parse_meas_log("sim_base.log")
    meas_head = parse_meas_log("sim_head.log")

    for metric, tol in config["tolerances"].items():
        val_base = meas_base.get(metric)
        val_head = meas_head.get(metric)
        if val_base is None or val_head is None:
            continue
        nominal = tol.get("nominal") or val_base
        rtol    = tol.get("rtol", 0.01)
        atol    = tol.get("atol") or 0.0
        passed  = abs(val_head - nominal) <= atol + rtol * abs(nominal)
        regressions.append({
            "metric": metric,
            "base": val_base, "head": val_head,
            "nominal": nominal, "rtol": rtol,
            "passed": passed,
            "delta_pct": (val_head - val_base) / val_base * 100 if val_base else None
        })
    return {"regressions": regressions, "any_failed": any(not r["passed"] for r in regressions)}
```

---

## 5. Formato de reporte ideal para el usuario

### Principios de diseño

1. **El delta es lo más importante**, no el número absoluto de violaciones.
2. **Verde/rojo no es suficiente** — mostrar qué cambió y dónde.
3. **Separar bloqueos de advertencias** — DRC que empeora bloquea; DRC que mejora celebra.
4. **Links a artefactos** — el usuario debe poder ver el layout con violaciones marcadas.

### Estructura del reporte de DRC

```markdown
## DRC — KLayout Sky130A

| Estado | Violaciones antes | Violaciones ahora | Delta |
|--------|:-----------------:|:-----------------:|:-----:|
| ⚠️ REGRESIÓN | 0 | 3 | +3 |

### Nuevas violaciones introducidas

| Regla | Descripción | Antes | Ahora | Delta |
|-------|-------------|:-----:|:-----:|:-----:|
| `poly.W1` | Poly width < 0.15um | 0 | 2 | **+2** |
| `met1.S1` | Metal1 space < 0.14um | 0 | 1 | **+1** |

📎 [Ver reporte completo (lyrdb)](artifacts/drc_report.lyrdb) · [Vista GDS con marcadores](artifacts/drc_annotated.png)
```

### Estructura del reporte de LVS

```markdown
## LVS — Netgen Sky130A

| Commit | Layout netlist | Schematic netlist | Resultado |
|--------|---------------|-------------------|-----------|
| HEAD~1 | `amp.spice` (42 devices) | `amp_sch.spice` (42 devices) | ✅ PASS |
| HEAD   | `amp.spice` (42 devices) | `amp_sch.spice` (43 devices) | ❌ FAIL |

### Mismatches en HEAD

**Device count mismatch:** Layout 42, Schematic 43
- `M12` presente en esquemático, no encontrado en layout

> ⛔ Este PR introduce una regresión LVS. El LVS pasaba en el commit anterior.
```

### Estructura del reporte de waveforms

```markdown
## Simulación NGSpice — testbench_amp

| Métrica | Base (HEAD~1) | Head | Delta | Tolerancia | Estado |
|---------|:-------------:|:----:|:-----:|:----------:|:------:|
| `vout_max` | 3.2877 V | 3.1201 V | **-4.8%** | ±5% | ⚠️ LÍMITE |
| `slew_rise` | 4.51 MV/s | 4.49 MV/s | -0.4% | ±10% | ✅ OK |
| `gain_3db` | 3.34 MHz | 3.35 MHz | +0.3% | ±1% | ✅ OK |

📎 [Log de simulación HEAD](artifacts/sim_head.log)
```

### Integración con GitHub Actions — PR comment via API

```python
import os, requests

def post_pr_comment(report_markdown: str):
    token   = os.environ["GITHUB_TOKEN"]
    repo    = os.environ["GITHUB_REPOSITORY"]
    pr_num  = os.environ["PR_NUMBER"]
    headers = {"Authorization": f"Bearer {token}", "Accept": "application/vnd.github+json"}

    # Buscar comment existente de Riku para actualizar en vez de crear nuevo
    comments = requests.get(
        f"https://api.github.com/repos/{repo}/issues/{pr_num}/comments",
        headers=headers
    ).json()
    miku_comment = next((c for c in comments if "<!-- miku-ci -->" in c["body"]), None)

    body = f"<!-- miku-ci -->\n{report_markdown}"
    if miku_comment:
        requests.patch(
            f"https://api.github.com/repos/{repo}/issues/comments/{miku_comment['id']}",
            headers=headers, json={"body": body}
        )
    else:
        requests.post(
            f"https://api.github.com/repos/{repo}/issues/{pr_num}/comments",
            headers=headers, json={"body": body}
        )
```

**Decisión de diseño clave:** Actualizar el mismo comment en vez de crear uno nuevo por cada
push al PR. Sin esto, un PR con 10 force-pushes genera 10 comentarios de Riku.

### Integración con GitLab CI — MR notes

```python
import os, requests

def post_mr_note(report_markdown: str):
    token   = os.environ["GITLAB_TOKEN"]
    api     = os.environ["CI_API_V4_URL"]
    proj_id = os.environ["CI_PROJECT_ID"]
    mr_iid  = os.environ["CI_MERGE_REQUEST_IID"]
    headers = {"PRIVATE-TOKEN": token}

    notes = requests.get(
        f"{api}/projects/{proj_id}/merge_requests/{mr_iid}/notes",
        headers=headers
    ).json()
    miku_note = next((n for n in notes if "<!-- miku-ci -->" in n["body"]), None)

    body = f"<!-- miku-ci -->\n{report_markdown}"
    if miku_note:
        requests.put(
            f"{api}/projects/{proj_id}/merge_requests/{mr_iid}/notes/{miku_note['id']}",
            headers=headers, json={"body": body}
        )
    else:
        requests.post(
            f"{api}/projects/{proj_id}/merge_requests/{mr_iid}/notes",
            headers=headers, json={"body": body}
        )
```

### Estados de check — cuándo bloquear el merge

| Condición | Acción sugerida |
|---|---|
| DRC delta > 0 (nuevas violaciones) | Bloquear (check failure) |
| DRC delta < 0 (menos violaciones) | Pasar con nota de mejora |
| DRC igual que antes | Pasar sin comentario de DRC |
| LVS PASS → FAIL | Bloquear |
| LVS FAIL → PASS | Pasar con nota de corrección |
| LVS FAIL → FAIL (mismo error) | Warning, no bloquear |
| Waveform fuera de tolerancia | Configurable — default warning |
| Waveform falla check nominal | Bloquear si está configurado |

---

## 6. Estrategia de caché para checks costosos

### El problema

KLayout DRC en un chip completo Sky130 puede tardar 20-60 minutos. Netgen LVS también es
costoso para circuitos grandes. Re-ejecutar estos checks en cada push hace CI impracticable.

La solución es una caché basada en **hash de los archivos de entrada**, no en timestamps.
Si los archivos relevantes no cambiaron, el resultado del check anterior sigue siendo válido.

### Qué entra al hash de caché

```
cache_key = hash(
    contenido de layout.gds (o .mag),
    contenido del script DRC,
    versión de KLayout,
    PDK_COMMIT o PDK_VERSION
)
```

**Crítico:** el PDK debe entrar al hash. Un cambio de PDK con el mismo GDS puede producir
resultados DRC distintos. Si el PDK se especifica como variable de entorno con la versión,
eso resuelve el problema.

### Implementación con caché de archivos

```python
import hashlib, json, pathlib, os

CACHE_DIR = pathlib.Path(os.environ.get("MIKU_CACHE_DIR", ".miku_cache"))

def compute_cache_key(inputs: list[pathlib.Path], metadata: dict) -> str:
    h = hashlib.sha256()
    for path in sorted(inputs):
        h.update(path.read_bytes())
    h.update(json.dumps(metadata, sort_keys=True).encode())
    return h.hexdigest()

def get_cached_result(key: str) -> dict | None:
    cache_file = CACHE_DIR / f"{key}.json"
    if cache_file.exists():
        return json.loads(cache_file.read_text())
    return None

def save_cached_result(key: str, result: dict):
    CACHE_DIR.mkdir(parents=True, exist_ok=True)
    (CACHE_DIR / f"{key}.json").write_text(json.dumps(result, indent=2))

def run_drc_cached(gds_path: str, drc_script: str, pdk_version: str) -> dict:
    inputs = [pathlib.Path(gds_path), pathlib.Path(drc_script)]
    key = compute_cache_key(inputs, {"pdk": pdk_version, "tool": "klayout_drc"})
    cached = get_cached_result(key)
    if cached:
        cached["from_cache"] = True
        return cached
    result = run_klayout_drc(gds_path, drc_script)   # función real
    result["from_cache"] = False
    save_cached_result(key, result)
    return result
```

### Granularidad de caché por celda

Para diseños jerárquicos, el DRC se puede cachear por celda. Si solo cambió una celda hija,
solo se re-ejecuta el DRC de esa celda y sus padres, no del chip completo.

```python
def compute_cell_cache_key(cell_name: str, layout: db.Layout,
                           pdk_version: str) -> str:
    cell = layout.cell(cell_name)
    # Hash de shapes y propiedades de la celda
    h = hashlib.sha256()
    h.update(cell_name.encode())
    for layer_idx in layout.layer_indices():
        shapes = cell.shapes(layer_idx)
        for shape in shapes.each():
            h.update(str(shape).encode())
    h.update(pdk_version.encode())
    return h.hexdigest()
```

### Integración con GitHub Actions cache

```yaml
- name: Cache Riku DRC/LVS results
  uses: actions/cache@v4
  with:
    path: .miku_cache/
    key: miku-checks-${{ hashFiles('**/*.gds', '**/*.mag', '**/*.spice', 'drc/*.drc') }}
    restore-keys: |
      miku-checks-
```

**Limitación:** `hashFiles` de GitHub Actions es por patrón de archivos en el workspace.
La caché local de Riku (basada en contenido exacto) es más granular — la caché de Actions
es el segundo nivel para evitar re-transferir resultados entre runners.

### Qué evitar en la estrategia de caché

- **No cachear basado en timestamp de archivo.** Magic actualiza timestamps sin cambiar contenido.
  Usar hash de contenido siempre.
- **No compartir caché entre PDK versions.** Un resultado DRC correcto para Sky130A PDK 0.0.2
  puede ser incorrecto en 0.0.3 si cambiaron reglas.
- **No cachear resultados de LVS sin incluir el setup.tcl del PDK en el hash.** El setup file
  define las reglas de equivalencia — si cambia, el resultado LVS puede cambiar.

---

## 7. Configuración YAML para CI

### GitHub Actions — flujo completo

```yaml
# .github/workflows/miku_checks.yml
name: Riku DRC/LVS/Sim Checks

on:
  pull_request:
    paths:
      - '**/*.gds'
      - '**/*.mag'
      - '**/*.sch'
      - '**/*.spice'
      - '**/*.cir'

jobs:
  miku-checks:
    runs-on: ubuntu-22.04
    env:
      PDK_ROOT: /opt/pdk
      PDK_VERSION: sky130A-0.0.2
      MIKU_CACHE_DIR: .miku_cache

    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0    # necesario para acceder al commit base del PR

      - name: Setup Python
        uses: actions/setup-python@v5
        with:
          python-version: '3.11'

      - name: Install Riku and dependencies
        run: |
          pip install miku klayout spicelib
          # KLayout para DRC completo (incluye klayout.lay)
          sudo apt-get install -y klayout netgen-lvs magic

      - name: Install PDK
        run: |
          pip install volare
          volare enable --pdk sky130 ${{ env.PDK_VERSION }}

      - name: Restore Riku cache
        uses: actions/cache@v4
        with:
          path: .miku_cache/
          key: miku-${{ env.PDK_VERSION }}-${{ hashFiles('**/*.gds', '**/*.mag', '**/*.spice', 'drc/*.drc') }}
          restore-keys: |
            miku-${{ env.PDK_VERSION }}-

      - name: Run DRC check
        id: drc
        run: |
          miku drc \
            --gds layout/top.gds \
            --script $PDK_ROOT/sky130A/libs.tech/klayout/sky130A.drc \
            --base ${{ github.event.pull_request.base.sha }} \
            --head ${{ github.sha }} \
            --output drc_report.json
        continue-on-error: true

      - name: Run LVS check
        id: lvs
        run: |
          miku lvs \
            --layout layout/top.mag \
            --schematic schematics/top.sch \
            --cell TOP \
            --pdk $PDK_ROOT/sky130A \
            --base ${{ github.event.pull_request.base.sha }} \
            --head ${{ github.sha }} \
            --output lvs_report.json
        continue-on-error: true

      - name: Run simulation regression
        id: sim
        run: |
          miku sim \
            --testbench testbench/tb_amp.spice \
            --config testbench/miku_sim.yaml \
            --base ${{ github.event.pull_request.base.sha }} \
            --head ${{ github.sha }} \
            --output sim_report.json
        continue-on-error: true

      - name: Upload artifacts
        uses: actions/upload-artifact@v4
        if: always()
        with:
          name: miku-reports-${{ github.sha }}
          path: |
            drc_report.json
            lvs_report.json
            sim_report.json
            .miku_cache/*.lyrdb

      - name: Post PR comment
        uses: actions/github-script@v7
        if: github.event_name == 'pull_request'
        with:
          script: |
            const fs = require('fs');
            const drc = JSON.parse(fs.readFileSync('drc_report.json'));
            const lvs = JSON.parse(fs.readFileSync('lvs_report.json'));
            const sim = JSON.parse(fs.readFileSync('sim_report.json'));
            // Riku genera el markdown del reporte
            const body = require('./scripts/miku_format_report.js')({drc, lvs, sim});
            // Buscar comment existente para actualizar
            const comments = await github.rest.issues.listComments({
              owner: context.repo.owner, repo: context.repo.repo,
              issue_number: context.issue.number
            });
            const existing = comments.data.find(c => c.body.includes('<!-- miku-ci -->'));
            const fullBody = `<!-- miku-ci -->\n${body}`;
            if (existing) {
              await github.rest.issues.updateComment({
                owner: context.repo.owner, repo: context.repo.repo,
                comment_id: existing.id, body: fullBody
              });
            } else {
              await github.rest.issues.createComment({
                owner: context.repo.owner, repo: context.repo.repo,
                issue_number: context.issue.number, body: fullBody
              });
            }

      - name: Set check status
        run: |
          DRC_REGRESSION=$(python -c "import json; d=json.load(open('drc_report.json')); exit(1 if d['delta'] > 0 else 0)")
          LVS_REGRESSION=$(python -c "import json; d=json.load(open('lvs_report.json')); exit(1 if d['regression'] else 0)")
          if [ $DRC_REGRESSION -ne 0 ] || [ $LVS_REGRESSION -ne 0 ]; then
            echo "CI FAILED: DRC or LVS regression detected"
            exit 1
          fi
```

### GitLab CI — flujo equivalente

```yaml
# .gitlab-ci.yml
stages:
  - miku-checks

variables:
  PDK_ROOT: /opt/pdk
  PDK_VERSION: sky130A-0.0.2
  MIKU_CACHE_DIR: .miku_cache

miku-drc-lvs:
  stage: miku-checks
  image: python:3.11
  rules:
    - if: $CI_MERGE_REQUEST_ID
      changes:
        - "**/*.gds"
        - "**/*.mag"
        - "**/*.sch"
        - "**/*.spice"

  cache:
    key:
      files:
        - "**/*.gds"
        - "**/*.mag"
        - "drc/*.drc"
      prefix: miku-$PDK_VERSION
    paths:
      - .miku_cache/

  before_script:
    - pip install miku klayout spicelib
    - apt-get install -y klayout netgen-lvs magic
    - pip install volare && volare enable --pdk sky130 $PDK_VERSION

  script:
    - git fetch origin $CI_MERGE_REQUEST_TARGET_BRANCH_NAME
    - BASE=$(git merge-base HEAD FETCH_HEAD)
    - |
      miku drc \
        --gds layout/top.gds \
        --script $PDK_ROOT/sky130A/libs.tech/klayout/sky130A.drc \
        --base $BASE --head $CI_COMMIT_SHA \
        --output drc_report.json
    - |
      miku lvs \
        --layout layout/top.mag \
        --schematic schematics/top.sch \
        --cell TOP --pdk $PDK_ROOT/sky130A \
        --base $BASE --head $CI_COMMIT_SHA \
        --output lvs_report.json
    - |
      miku sim \
        --testbench testbench/tb_amp.spice \
        --config testbench/miku_sim.yaml \
        --base $BASE --head $CI_COMMIT_SHA \
        --output sim_report.json
    - miku post-report --mr --drc drc_report.json --lvs lvs_report.json --sim sim_report.json

  artifacts:
    when: always
    paths:
      - drc_report.json
      - lvs_report.json
      - sim_report.json
    expire_in: 30 days

  allow_failure: false
```

### Configuración del proyecto Riku (`.miku/config.yaml`)

```yaml
# .miku/config.yaml — en el repositorio del proyecto de chip
version: 1

pdk:
  name: sky130A
  version: "0.0.2"    # Riku verifica que el PDK instalado coincide

drc:
  enabled: true
  tool: klayout
  script: ${PDK_ROOT}/sky130A/libs.tech/klayout/sky130A.drc
  layout: layout/top.gds
  block_on_regression: true    # bloquear PR si delta > 0

lvs:
  enabled: true
  tool: netgen
  cell: TOP
  layout: layout/top.mag
  schematic: schematics/top.sch
  pdk_setup: ${PDK_ROOT}/sky130A/libs.tech/netgen/sky130A_setup.tcl
  block_on_regression: true

simulation:
  enabled: true
  testbenches:
    - path: testbench/tb_amp.spice
      config: testbench/miku_sim.yaml
      block_on_regression: false   # warning, no bloquea
    - path: testbench/tb_bias.spice
      config: testbench/miku_sim_bias.yaml
      block_on_regression: true

cache:
  enabled: true
  dir: .miku_cache
  max_size_gb: 5
  ttl_days: 30
```

---

## Notas de retroalimentación — flujos reales

> Estas notas provienen del research de experiencia de usuario. Requieren revisión de secciones
> específicas de este documento.

### Nota 1: El flujo LVS asume Xschem + Magic. Hay casos donde uno o ambos están ausentes.

La Sección 3 ("LVS entre commits") modela:
```
Xschem (.sch) → netlist esquemático
Magic (.mag) → extract → netlist de layout
```

En proyectos reales:
- **PDK IHP (KLayout primario):** La extracción de layout para LVS puede hacerse desde KLayout, no desde Magic. KLayout tiene su propio engine LVS (desde v0.27). El flujo `magic extract → ext2spice` no aplica si el diseñador nunca usó Magic.
- **Cadence Virtuoso + open PDK:** El netlist esquemático viene de Virtuoso (`.cdl`), no de Xschem. Riku no puede generar el netlist esquemático invocando `xschem --netlist`.
- **KLayout LVS como alternativa a Netgen:** KLayout incorpora un engine de LVS propio que puede reemplazar a Netgen para algunos PDKs. Está documentado como alternativa en la nota "¿Cuándo refutar estas decisiones?" — pero no hay investigación sobre cuándo KLayout LVS tiene paridad real con Netgen para SKY130/IHP.

**Investigación pendiente:** Verificar si el KLayout LVS engine (disponible vía `klayout -b -r lvs_script.lylvs`) produce resultados equivalentes a Netgen para SKY130A y IHP SG13G2. Si tiene paridad, reduce la dependencia de Netgen para proyectos KLayout-primary.

Fuente: [KLayout LVS reference](https://www.klayout.de/doc/manual/lvs_ref.html)

### Nota 2: PDK version pinning es crítico para reproducibilidad de DRC/LVS

La Sección 7 del YAML de CI tiene `PDK_VERSION: sky130A-0.0.2` como variable de entorno, pero no hay un mecanismo que verifique que todos los miembros del equipo usan exactamente esa versión.

Casos documentados donde la versión del PDK importa:
- Un cambio de PDK puede introducir nuevas reglas DRC o cambiar umbrales existentes → resultados DRC distintos sobre el mismo GDS.
- El `sky130A_setup.tcl` de Netgen (que define equivalencias de dispositivos para LVS) cambia entre versiones del PDK → LVS puede pasar en una versión y fallar en otra.
- Los bugs documentados en KLayout+SKY130 (XML parsing, referencias de archivos) son específicos de versiones concretas del PDK.

**Herramientas para PDK version pinning:**
- `volare` (anteriormente de Efabless, ahora bajo chipfoundry): gestiona versiones pinneadas de SKY130/GF180 con hash verificable. Ya aparece en el YAML de CI como `pip install volare && volare enable --pdk sky130 $PDK_VERSION`.
- `ciel` (FOSSi Foundation): alternativa a volare post-cierre de Efabless.

**Propuesta:** El campo `pdk.version` en `.miku/config.yaml` debería requerir el commit hash del PDK (no solo el tag de versión), y `miku doctor` debería verificar que el PDK local coincide con ese hash.

Fuente: [github.com/chipfoundry/volare](https://github.com/chipfoundry/volare), [github.com/fossi-foundation/ciel](https://github.com/fossi-foundation/ciel)

### Nota 3: Bugs de integración PDK-herramienta que rompen CI silenciosamente

El tutorial de unic-cass (2024) documenta bugs en la distribución oficial de SKY130 que hacen fallar KLayout sin mensaje de error claro. Si el CI de Riku no aplica los patches necesarios, el DRC falla silenciosamente.

El `miku doctor` y el step de setup del workflow de GitHub Actions deben incluir detección y aplicación automática de estos patches, o al menos verificación explícita de que el PDK fue instalado correctamente.

Fuente: [unic-cass KLayout Sky130 tutorial (2024)](https://unic-cass.github.io/training/sky130/3.3-layout-klayout.html)

### Nota 4: El CI de referencia (efabless/mpw_precheck) ya no está mantenido

El documento referencia `efabless/caravel_user_project` como ejemplo de CI completo con DRC/LVS. Efabless cerró en febrero 2025 — este repositorio y su pipeline ya no reciben mantenimiento activo.

La plataforma de sustitución emergente es LibreLane (FOSSi Foundation) + acceso directo a IHP. El CI de referencia para proyectos nuevos debería ser:
- [github.com/iic-jku/IIC-OSIC-TOOLS](https://github.com/iic-jku/IIC-OSIC-TOOLS) — Docker con todas las herramientas, mantenido activamente
- Los workflows de OpenLane v2 / LibreLane bajo FOSSi Foundation

Fuente: [semiwiki.com Efabless shutdown](https://semiwiki.com/forum/threads/efabless-just-shut-down.22217/), [wiki.f-si.org IHP integration](https://wiki.f-si.org/index.php?title=IHP_Open_PDK_integration_with_Magic,_Netgen,_and_LibreLane)

### Nota 5: Waveform regression en dos fases — pre-layout y post-layout

La Sección 4 asume una sola fase de simulación. En flujos reales existen dos fases con resultados sistemáticamente distintos (ver ngspice_diff_y_versionado.md Nota 10b).

El `miku_sim.yaml` debería tener un campo `phase: pre_layout | post_layout` para que Riku no compare resultados de fases distintas y genere falsos positivos de regresión.

---

## ¿Cuándo refutar estas decisiones?

**"Delta de violaciones en vez de conteo absoluto"** falla si:
- El proyecto empieza desde cero sin deuda técnica — en ese caso el delta y el absoluto son equivalentes, y el absoluto es más simple de implementar y comunicar. Solo el delta vale la complejidad en proyectos con historial.

**"Waveform regression como warning, no error"** debe cambiar a error si:
- El equipo llega a un acuerdo sobre qué señales son críticas y con qué tolerancias. El warning como default es conservador porque no conocemos el contexto del equipo. Una vez que hay casos de uso reales y se calibran las tolerancias, bloquear en simulación puede ser lo correcto.

**"Un comment único actualizable en el PR"** no escala si:
- Un PR toca 10+ archivos con checks independientes y el comment se vuelve tan largo que es ilegible. En ese caso puede ser mejor un comment por tipo de check (uno para DRC, uno para LVS, uno para sim) o un link a un reporte externo.

**"Netgen para LVS"** deja de ser la mejor opción si:
- KLayout LVS (disponible desde v0.27 via el engine DRC) madura al punto de reemplazar a Netgen para SKY130 — ya hay soporte parcial y el ecosistema se puede consolidar en una sola herramienta. Revisar cuando KLayout LVS tenga soporte verificado para el PDK objetivo.

## 8. Decisiones de diseño justificadas

### Por qué calcular el delta en vez de solo el conteo absoluto

Un proyecto heredado puede tener 500 violaciones DRC conocidas. Bloquear todos los PRs hasta
que estén resueltas es impracticable. Lo que importa es no introducir nuevas. El delta permite
trabajar con deuda técnica existente mientras se previene que empeore.

### Por qué hashear contenido para la caché en vez de timestamps

Magic actualiza timestamps al abrir y guardar, aunque el contenido no cambie. Xschem también
puede inyectar timestamps en comentarios. Una caché basada en timestamps invalida en cada
operación normal. La caché basada en hash del contenido real resuelve esto de raíz — es el
mismo principio que usa `ccache` o Nix.

### Por qué separar DRC, LVS y simulación en checks independientes

Tienen dependencias distintas, tiempos de ejecución distintos y pueden cachearse de forma
independiente. Si el `.spice` cambia pero el layout no, el DRC puede usar caché mientras LVS
y simulación re-ejecutan. Combinarlos en un solo job obliga a re-ejecutar todo o nada.

### Por qué exponer el JSON estructurado y no solo pass/fail

El JSON de cada check es la interfaz pública de Riku hacia otras herramientas. Permite que
otros sistemas lean los resultados (dashboards, alertas Slack, métricas históricas). Un
pass/fail es suficiente para el CI pero insuficiente para el flujo de trabajo del diseñador.

### Por qué un único comment actualizable en vez de múltiples

Reduce el ruido en el PR. Un PR activo puede tener 20+ pushes. 20 comentarios de CI hacen
el PR ilegible. La solución del comment actualizable con `<!-- miku-ci -->` como marcador es
el patrón estándar de herramientas de CI bien diseñadas (Codecov, dependabot, etc.).

### Por qué tolerar waveform regression como warning y no error por defecto

Las simulaciones analógicas son sensibles a condiciones de simulación (temperatura, esquinas
de proceso, ruido numérico del integrador de NGSpice). Un cambio de `rtol=0.01` puede producir
falsos positivos frecuentes que entrenan al equipo a ignorar los alerts. El default conservador
(warning) requiere opt-in explícito para bloquear, reduciendo alarmas falsas.

---

## Referencias y dependencias clave

| Herramienta | Versión mínima | Instalación en CI |
|---|---|---|
| `klayout` | 0.28+ | `pip install klayout` (headless DRC/diff) |
| `klayout` (sistema) | 0.28+ | `apt install klayout` (renderizado PNG) |
| `netgen` | 1.5.240+ | `apt install netgen-lvs` |
| `magic` | 8.3.400+ | `apt install magic` |
| `ngspice` | 40+ | `apt install ngspice` |
| `spicelib` | 1.0+ | `pip install spicelib` |
| `volare` | cualquiera | `pip install volare` (gestión de PDK) |

**PDK recomendado para CI:** `volare` (de efabless/Tim Edwards) gestiona versiones pinneadas
de Sky130A/GF180MCU con hash verificable — garantiza reproducibilidad entre runners.

---

## Referencias

### KLayout DRC en CI
- **Repo KLayout**: https://github.com/KLayout/klayout
- **DRC scripting reference**: https://www.klayout.de/doc/manual/drc_ref.html
- **Marker database (`.lyrdb`) format**: https://www.klayout.de/doc/manual/lvs_ref.html
- **Ejemplo DRC SKY130**: https://github.com/google/skywater-pdk — scripts `.lydrc` incluidos

### Netgen LVS
- **Repo Netgen**: https://github.com/RTimothyEdwards/netgen
- **Documentación LVS**: http://opencircuitdesign.com/netgen/
- **open_pdks (incluye `sky130A_setup.tcl`)**: https://github.com/RTimothyEdwards/open_pdks
- **Ejemplo de flujo LVS completo**: https://github.com/efabless/caravel_user_project/blob/main/Makefile

### NGSpice waveform regression
- **spicelib (parse `.raw` y `.meas`)**: https://github.com/nunobrum/spicelib
- **spyci**: https://github.com/gmagno/spyci
- **Referencia `.meas` syntax**: sección 15 del NGSpice manual

### CI de referencia en proyectos EDA reales
- **caravel_user_project (efabless)**: https://github.com/efabless/caravel_user_project — CI completo con DRC/LVS/GL-SIM
- **OpenLane GHA workflows**: https://github.com/The-OpenROAD-Project/OpenLane/tree/master/.github/workflows
- **IIC-OSIC-TOOLS**: https://github.com/iic-jku/iic-osic-tools — entorno Docker usado en CI académico

### Integración con GitHub/GitLab
- **GitHub Actions `actions/cache`**: https://github.com/actions/cache
- **GitHub PR comments API**: https://docs.github.com/en/rest/issues/comments
- **GitLab MR notes API**: https://docs.gitlab.com/ee/api/notes.html

### Ver también
- [../herramientas/gds_klayout_magic_diff.md](../herramientas/gds_klayout_magic_diff.md) — KLayout DRC y extracción con Magic
- [../herramientas/ngspice_diff_y_versionado.md](../herramientas/ngspice_diff_y_versionado.md) — formatos de salida de NGSpice
- [../herramientas/headless_y_compatibilidad_herramientas.md](../herramientas/headless_y_compatibilidad_herramientas.md) — ejecutar herramientas sin display
- [cache_y_rendimiento.md](cache_y_rendimiento.md) — estrategia de caché para checks costosos
- [../arquitectura/arquitectura_cli_y_orquestacion.md](../arquitectura/arquitectura_cli_y_orquestacion.md) — diseño de comandos `miku ci` y formato de reporte de resultados
