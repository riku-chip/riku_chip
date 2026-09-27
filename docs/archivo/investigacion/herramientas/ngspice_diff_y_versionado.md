# NGSpice — Diff y Versionado para Riku

## 1. Formatos de archivo

### Archivos de entrada — todos son texto plano

| Extensión | Uso |
|---|---|
| `.spice`, `.cir`, `.sp` | Netlists del circuito |
| `.net` | Netlists exportados desde Xschem/KiCad |
| `.lib`, `.mod` | Librerías de modelos de dispositivos |
| `.spiceinit` | Comandos de configuración de sesión |

Ejemplo de netlist mínimo:

```spice
* RC low-pass filter
Vin in 0 AC 1
R1  in out 1k
C1  out 0 100n
.ac dec 10 1 1Meg
.print ac v(out)
.end
```

**Git-diffability: excelente.** Un componente por línea. Cambiar `1k` → `2k` es un diff de una línea. Agregar un capacitor es una línea nueva.

**Problema principal:** Las herramientas EDA (especialmente Xschem al exportar netlists) pueden inyectar comentarios con timestamps o numerar nodos de forma no determinista. Esto genera ruido en el diff aunque el circuito no haya cambiado.

### Archivos de salida

| Archivo | Formato | ¿Versionable? |
|---|---|---|
| `.raw` (ASCII) | Texto — headers + datos de simulación | No directamente (timestamps + float noise) |
| `.raw` (binario) | IEEE 754 binario | No |
| `.log` / stdout | Texto | Sí, útil como artefacto |
| `.meas` output | Texto tabular | ✅ Sí — este es el archivo a versionar |

**Los `.raw` van en `.gitignore`.** Son artefactos de build, no fuente.  
**Los resultados de `.meas` sí se versionan** — son texto compacto con los valores medidos.

---

## 2. Headless / Batch mode

NGSpice tiene excelente soporte headless:

```bash
# Modo batch: corre todas las simulaciones del netlist y sale
ngspice -b -r output.raw -o simulation.log mycircuit.spice

# Modo pipe (backend para otro programa)
ngspice -p mycircuit.spice
```

| Flag | Efecto |
|---|---|
| `-b` | Batch mode — ejecuta y sale |
| `-r rawfile` | Escribe output a este archivo `.raw` |
| `-o logfile` | Escribe log a este archivo |
| `-p` | Pipe mode (para usar como backend) |

En batch mode:
- `.print` → stdout en formato tabular ASCII
- `.plot` → silenciosamente ignorado
- `.meas` → imprime valores medidos al log

**Usado en producción por:** TinyTapeout (GitHub Actions), Efabless MPW CI, el propio test suite de NGSpice (`make check` corre cientos de netlists en batch).

---

## 3. Comparación de netlists SPICE

### El problema

SPICE no tiene orden canónico. El mismo circuito puede representarse de muchas formas distintas: componentes en cualquier orden, nodos como números o strings, `.subckt` antes o después de su uso. El `git diff` muestra qué líneas cambiaron, pero no "R3 fue reemplazado por dos resistores en serie".

### Herramientas existentes

**No existe una herramienta open-source dedicada a diff de netlists SPICE.** Lo más cercano:

| Herramienta | Qué hace | Relevancia |
|---|---|---|
| **Netgen** | LVS: compara dos netlists topológicamente | Indirecta — ver sección 5 |
| **spicelib** (`SpiceEditor`) | Lee y edita netlists SPICE en Python | Base para construir un diff |
| **PySpice** (parser) | Parsea SPICE a objetos Python | Base para construir un diff |
| **dan-fritchman/Netlist** | AST de netlists multi-dialecto en Python | Base para diff estructural |

### La solución pragmática: canonicalización

El enfoque más efectivo a corto plazo para Riku es un **hook de pre-commit que canonicalice el netlist** antes de hacer commit:
- Ordenar líneas de componentes alfabéticamente
- Normalizar listas de parámetros de `.subckt`
- Eliminar o normalizar timestamps en comentarios generados

Esto es análogo a `gofmt` o `prettier` para código — nadie lo ha hecho para SPICE todavía. **Sería un aporte concreto de Riku.**

---

## 4. Comparación de resultados de simulación

El equivalente analógico del XOR de KLayout para layouts.

### Enfoque con `.meas` (el más común en open source)

NGSpice tiene un sistema de medición integrado. Dentro del netlist:

```spice
.meas tran rise_time TRIG v(out) VAL=0.5 RISE=1 TARG v(out) VAL=2.5 RISE=1
.meas tran vmax MAX v(out)
.meas tran vmin MIN v(out)
```

En batch mode, estos valores se imprimen al log. Se pueden extraer y comparar con tolerancia:

```bash
ngspice -b -o sim.log circuit.spice
vmax=$(grep "vmax" sim.log | awk '{print $3}')
python3 -c "assert abs($vmax - 3.3) < 0.1, f'Vmax fuera de tolerancia: {$vmax}'"
```

**Este es el patrón de CI más usado en el ecosistema open source hoy.** Riku podría estandarizarlo.

### Parseo de archivos `.raw` con Python

Para comparaciones más sofisticadas entre dos runs:

```python
# spyci — parsea .raw ASCII a numpy
from spyci import spyci
data_a = spyci.load("sim_a.raw")
data_b = spyci.load("sim_b.raw")

import numpy as np
# Comparar con 1% de tolerancia
assert np.allclose(data_a['v(out)'], data_b['v(out)'], rtol=0.01), "Waveform fuera de tolerancia"
```

Librerías para parsear `.raw`:
- **spyci** — ASCII `.raw` → numpy. Simple, liviana.
- **spicelib `RawRead`** — ASCII y binario → numpy. Más completa.
- **PySpice WaveForm** — similar, parte de un ecosistema más grande.

### Herramientas existentes de comparación de waveforms

| Herramienta | Tipo | Estado |
|---|---|---|
| **NGSpice `.meas` + shell** | Manual, por umbral | El estándar actual en open source |
| **Analog Flavor `bspwave_compare`** | CLI con tolerancia configurable | Propietario (evaluación limitada) |
| **spyci + numpy** | Python custom | Parseo resuelto, lógica de comparación manual |
| **spicelib RawRead + numpy** | Python custom | Igual |

**No existe un "waveform XOR" open source.** Este es el gap más grande de Riku en el espacio NGSpice.

---

## 5. Netgen — Rol en el flujo y para Riku

### El flujo canónico

```
Xschem (.sch) → exportar → netlist esquemático (.spice)
Magic (.mag) → extract → netlist de layout (.spice)
Netgen → comparar ambos → reporte LVS
NGSpice → simular el netlist esquemático → waveforms
```

Netgen **no simula** — solo compara topología de netlists.

### Output de Netgen

```bash
netgen -batch lvs "v1/amp.spice amp" "v2/amp.spice amp" \
       sky130A/libs.tech/netgen/sky130A_setup.tcl comp.json
```

Produce tres formatos:
- `comp.out` — texto legible (matched/unmatched devices y nets)
- `comp.json` — JSON estructurado (ideal para scripting)
- Lista Tcl (con `-list`)

### Uso para diff entre commits

Aunque Netgen fue diseñado para LVS (layout vs. esquemático), se puede usar para **comparar dos versiones del mismo netlist**:

```bash
# ¿Cambió la topología del circuito entre commits?
netgen -batch lvs "commit_a/amp.spice amp" "commit_b/amp.spice amp" \
       sky130A_setup.tcl diff_report.json
```

Si la topología es la misma pero cambiaron valores de componentes, Netgen lo reporta como match con property mismatches — exactamente la señal útil para un diff de versiones.

**El JSON de Netgen podría ser el backend del diff estructural de Riku para netlists.**

---

## 6. Prácticas actuales de la comunidad con Git

### Lo que hacen hoy

- Commitear `.spice`/`.cir` a git (texto, diff razonablemente útil)
- Commitear `.sch` de Xschem (también texto)
- `.gitignore` para archivos `.raw`
- Sin diff drivers ni hooks especializados

### Lo que casi nadie hace

- Git diff drivers para SPICE
- Diff semántico de netlists
- Comparación automática de waveforms en CI
- PR checks con resultados de simulación

**AllSpice.io** es la plataforma más avanzada (Gitea + diff visual de hardware), pero apunta a PCB/KiCad/Altium, no a SPICE ni diseño IC analógico.

---

## 7. Ecosystem tools relevantes para Riku

| Herramienta | Qué hace | Relevancia para Riku |
|---|---|---|
| **spicelib** | SpiceEditor + SimRunner + RawRead | La base más madura para integración NGSpice |
| **spyci** | Parser `.raw` ASCII → numpy | Parseo de resultados |
| **PySpice** | Circuitos en Python, driver NGSpice | Referencia para "netlist como código" |
| **Hdl21 + VLSIR** | Circuitos en Python + interchange format JSON/ProtoBuf | Referencia para formato canónico |
| **Netgen** | LVS — comparación estructural de netlists | Backend para diff estructural |

---

## 8. Flujo propuesto para Riku + NGSpice

```
.spice commit A ─→ canonicalizar ─→ git diff legible
                                          │
                                          ▼
                               netgen LVS A vs B → JSON estructurado
                                          │
                                          ▼
             ngspice -b commit A → .meas results → results_a.json ─┐
             ngspice -b commit B → .meas results → results_b.json ─┘→ comparar con tolerancia
```

### Git diff driver para .spice

```ini
# .gitattributes
*.spice  diff=spice
*.cir    diff=spice
*.sp     diff=spice

# .git/config
[diff "spice"]
    textconv = miku-spice-canonicalize   # hook que normaliza el netlist
```

---

## 9. Conclusiones para Riku

1. **Los netlists SPICE ya son texto plano** — `git diff` funciona sin configuración. El problema es el ruido (timestamps, ordenamiento no determinista). Un hook de canonicalización lo resuelve.

2. **Los `.raw` no se versionan** — son artefactos de build. Lo que sí se versiona son los resultados de `.meas` (texto compacto) o métricas extraídas.

3. **No existe diff semántico de netlists en open source.** Riku puede construirlo sobre spicelib (parseo) + Netgen (comparación topológica). **Es el gap más claro del ecosistema NGSpice.**

4. **No existe waveform comparison open source con tolerancia.** El patrón actual es `.meas` + shell scripting. Riku puede formalizarlo con un CLI estándar y un formato JSON para los resultados.

5. **`spicelib`** es la librería más madura para la capa de integración de Riku con NGSpice: maneja lectura/escritura de netlists, ejecución batch, y parseo de `.raw`.

6. **El JSON de Netgen** es la clave para diff estructural de netlists — Riku puede invocar Netgen entre dos commits y exponer su output de forma legible en un PR.

---

## 10. Notas de retroalimentación — flujos reales

> Estas notas provienen del research de experiencia de usuario en foros y comunidades reales.
> Identifican casos que el modelo de este documento no contempla actualmente.

### 10a. SPICE como fuente primaria (no derivado de Xschem)

La Sección 3 ("Comparación de netlists SPICE") y la Sección 8 ("Flujo propuesto") asumen implícitamente que el `.spice` es exportado desde Xschem. Hay tres situaciones donde esto no se cumple:

1. **Exploración topológica:** Diseñadores escriben netlists a mano para explorar topologías antes de formalizarlas en un esquemático. El `.spice` es la fuente; no existe `.sch`.
2. **Celdas estándar del PDK:** En SKY130, los símbolos de dispositivos en Xschem no tienen esquemático propio — solo usan `spice_sym_def` para apuntar a un netlist externo. El `.spice` del PDK es la fuente; Xschem lo incluye por referencia.
3. **Proyectos Cadence/Virtuoso + PDK open:** El esquemático vive en Virtuoso y Riku nunca lo ve. El `.spice` exportado es lo único disponible — y es fuente, no artefacto.

**Implicación:** La política "SPICE siempre va en `.gitignore`" (mencionada en cache_y_rendimiento y estrategia_merge) no puede ser una regla global. El `miku.toml` o `deps.toml` debe poder declarar si un `.spice` es derivado o fuente.

Fuente: [xschem tutorial SKY130 — spice_sym_def](https://xschem.sourceforge.io/stefan/xschem_man/tutorial_xschem_sky130.html), [xschem issue #35 — SPICE import request](https://github.com/StefanSchippers/xschem/issues/35)

### 10b. Dos fases de simulación con netlists distintos

El documento describe una sola fase de simulación (pre-layout). En flujos reales hay dos fases con netlists completamente distintos:

**Fase 1 — Pre-layout (esquemático):**
```
Xschem (.sch) → netlist.spice → NGSpice
```

**Fase 2 — Post-layout (extracción de parásitos):**
```
Magic (.mag) → extract all → .ext → ext2spice → netlist_extracted.spice → NGSpice
```

El netlist extraído puede tener decenas de miles de MOSFETs y capacitores parásitos. Los resultados de simulación son sistemáticamente distintos entre fases — no es una regresión, es una diferencia esperada por diseño.

El problema: si Riku compara `.meas` results entre commits sin saber en qué fase está cada run, el diff es ambiguo o produce falsos positivos.

**Propuesta de extensión:** El `miku_sim.yaml` debería tener un campo `phase: pre_layout | post_layout` para que Riku compare solo runs de la misma fase.

Fuente: [ngspice sourceforge discussion on real tapeouts](https://sourceforge.net/p/ngspice/discussion/120972/thread/69a4488f56/), [unic-cass analog design flow](https://unic-cass.github.io/training/1.4-analog-design-flow-intro.html)

### 10c. Modelos Verilog-A para PDKs BiCMOS/RF (IHP SG13G2)

El IHP SG13G2 PDK incluye dispositivos HBT con modelos compact en Verilog-A que NGSpice no puede usar directamente. El flujo requiere un paso adicional de compilación:

```
modelo.va → OpenVAF → modelo.osdi → cargado en NGSpice via OSDI interface
```

- `.va` son archivos de modelo — texto versionable, no deben ir en `.gitignore`
- `.osdi` son binarios compilados — sí deben ir en `.gitignore` (artefacto de build)

Si Riku genera `.gitignore` automáticamente con `miku init`, debe conocer el PDK para decidir correctamente. Un `.gitignore` genérico que incluya `*.va` destruiría los modelos Verilog-A de IHP.

Fuente: [ngspice.sourceforge.io/osdi.html](https://ngspice.sourceforge.io/osdi.html), [OpenVAF discussions](https://github.com/pascalkuthe/OpenVAF/discussions/22), [IHP-Open-PDK](https://github.com/IHP-GmbH/IHP-Open-PDK)

### 10d. Simulación estadística (Monte Carlo) con modelos SKY130

SKY130 incluye parámetros estadísticos en formato Spectre. NGSpice no puede leerlos directamente — requiere un script de conversión externo que fue contribuido por la comunidad (Brad Minch), no por Google/SkyWater. Sin este paso, las simulaciones corner/Monte Carlo no tienen los parámetros estadísticos del PDK real.

**Implicación para el CI de simulación:** Un testbench de Monte Carlo en SKY130 requiere el script de conversión como prerequisito. `miku doctor` debería verificar su presencia si el PDK es sky130 y hay simulaciones MC configuradas.

Fuente: [google/skywater-pdk issue #309](https://github.com/google/skywater-pdk/issues/309)

### 10e. Extensiones de archivo SPICE adicionales en proyectos reales

La tabla de extensiones del documento incluye `.spice`, `.cir`, `.sp`, `.net`. En proyectos reales también aparecen:

- `.cdl` — Circuit Description Language, usado por Virtuoso para exportar netlists LVS-clean
- `.spi` — variante menor usada por algunos scripts de OpenLane

El driver SPICE de Riku debería reconocer también `.cdl` y `.spi` por detección de contenido (misma sintaxis).

---

## Referencias

### NGSpice
- **Repo oficial**: https://sourceforge.net/projects/ngspice/ (mirror en GitHub: https://github.com/ngspice/ngspice)
- **Documentación**: http://ngspice.sourceforge.net/docs/ngspice-manual.pdf
- **Modo batch y `.meas`**: sección 17 del manual oficial
- **Formato `.raw`**: sección de análisis de salida en el manual

### Librerías de parse y análisis
- **spicelib**: https://github.com/nunobrum/spicelib — parse de netlists y `.raw`, reemplaza a ltspice_utils
- **spyci**: https://github.com/gmagno/spyci — parse de `.raw` de NGSpice en Python
- **PySpice**: https://github.com/FabriceSalvaire/PySpice — generación de netlists y simulación programática

### Netgen (LVS)
- **Repo oficial**: https://github.com/RTimothyEdwards/netgen
- **Documentación**: http://opencircuitdesign.com/netgen/
- **Setup SKY130 para Netgen**: incluido en open_pdks — `sky130A_setup.tcl`
- **open_pdks**: https://github.com/RTimothyEdwards/open_pdks

### Comparación de netlists (herramientas alternativas)
- No existe un equivalente open-source maduro a `klayout --diff` para SPICE. Gap documentado.

### Ver también
- [../operaciones/ci_drc_lvs_regresiones.md](../operaciones/ci_drc_lvs_regresiones.md) — waveform regression y LVS en CI
- [../operaciones/estrategia_merge_archivos_mixtos.md](../operaciones/estrategia_merge_archivos_mixtos.md) — manejo de .spice derivados en merge
- [headless_y_compatibilidad_herramientas.md](headless_y_compatibilidad_herramientas.md) — NGSpice en modo batch
- [../operaciones/cache_y_rendimiento.md](../operaciones/cache_y_rendimiento.md) — benchmarks de simulación y estrategia de caché para runs costosos
- [../arquitectura/arquitectura_cli_y_orquestacion.md](../arquitectura/arquitectura_cli_y_orquestacion.md) — detección de .spice/.cir/.net y registro de diff drivers
