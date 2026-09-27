# GDS / KLayout / Magic — Diff y Git Integration para Riku

## 1. El problema raíz: GDS es binario

**GDSII (.gds)** es un formato binario — `git diff` solo muestra "binary file changed". No hay texto que comparar. Esta es la diferencia fundamental con Xschem (.sch) y Magic (.mag), que son texto plano.

**Estrategia de Riku para GDS:** tratar GDS como artefacto de build (como un ejecutable compilado), no como fuente. La fuente versionable es .mag o código Python (GDSFactory/GLayout).

---

## 2. KLayout — Motor de diff para GDS

KLayout tiene dos herramientas de comparación distintas y complementarias:

### 2a. Diff Tool (estructural / jerárquico)

Compara celda por celda, objeto por objeto, preservando la jerarquía.

| Qué detecta | Qué no detecta |
|---|---|
| Celdas renombradas (por bbox + conteos) | Cambios físicos enmascarados |
| Instancias añadidas/eliminadas | — |
| Shapes añadidas/eliminadas | — |
| Diferencias de texto/labels | — |
| Propiedades de objetos | — |

**Output:** Marker database (`.lyrdb`) — visualizable en KLayout. No produce JSON/XML nativo; la salida estructurada requiere la API Python.

**Cuándo usar:** ECOs pequeños, cambios incrementales, verificar que dos versiones son lógicamente equivalentes, detectar celdas renombradas.

### 2b. XOR Tool (geométrico / físico)

Compara los layouts como si fueran máscaras de fabricación, completamente aplanados.

| Característica | Valor |
|---|---|
| Aplanamiento | Sí (flat) |
| Tolerancia configurable | Sí (filtra ruido numérico) |
| Tiling para layouts grandes | Sí (esencial para GDS > 1GB) |
| Multi-core | Sí |
| Texto/labels | Ignorados |
| Output | GDS con polígonos de diferencia |

**Cuándo usar:** Verificación física pre-tapeout, cuando la jerarquía cambió significativamente, cuando se quiere saber "¿producirían el mismo chip estas dos versiones?".

### 2c. Cuadro comparativo

| Aspecto | XOR | Diff |
|---|---|---|
| Tipo | Geométrico | Estructural |
| Velocidad | Más lento | Más rápido |
| Jerarquía | Ignora | Preserva |
| Tolerancia | Sí | No |
| Output | GDS con polígonos | Marker database |
| Uso ideal | Tapeout verification | ECO / CI checks |

---

## 3. KLayout — Uso headless y CLI

### Flags esenciales

```bash
klayout -b -r script.py -rd var=value   # batch mode completo
klayout -zz -r script.py                # headless sin display (Linux CI)
klayout -b -r xor.drc -rd gds1=a.gds -rd gds2=b.gds -rd out=diff.gds
```

| Flag | Efecto |
|---|---|
| `-b` | Batch mode: `-zz -nc -rx` combinados |
| `-zz` | Sin GUI, sin display requerido |
| `-r <script>` | Ejecutar script y salir |
| `-rd name=val` | Pasar variable al script |
| `-nc` | No cargar config (startup más rápido) |

### Buddy tools (binarios standalone)

```bash
# XOR geométrico
strmxor a.gds b.gds resultado.gds

# Diff estructural — salida texto a stdout
strmcmp a.gds b.gds
# exit code: 0 = idénticos, != 0 = diferencias
```

`strmcmp` es el más directo para un git diff driver — produce texto legible y exit code semántico.

### Script XOR con DRC (el patrón canónico)

```ruby
# xor.drc — ejecutar con: klayout -b -r xor.drc -rd gds1=a.gds -rd gds2=b.gds -rd out=xor.gds
l1 = layout($gds1)
l2 = layout($gds2)
target($out)

layers = []
[l1, l2].each do |l|
  l.layout.layer_indices.each do |index|
    info = l.layout.get_info(index)
    layers << [info.layer, info.datatype]
  end
end

layers.sort.uniq.each do |l, d|
  log "XOR layer #{l}/#{d}"
  (l1.input(l, d) ^ l2.input(l, d)).output(l, d)
end
```

Para layouts grandes, agregar:
```ruby
tiles(1.mm, 1.mm)
threads(4)
```

---

## 4. KLayout Python API — Diff programático con JSON

La clase `LayoutDiff` permite extraer diferencias como datos estructurados vía callbacks.

```python
# klayout -b -r miku_diff.py -rd gds1=old.gds -rd gds2=new.gds
import json, sys
import klayout.db as db

la = db.Layout(); la.read(gds1)
lb = db.Layout(); lb.read(gds2)

results = {"cells_a_only": [], "cells_b_only": [], "changes": []}
current_cell = [None]

diff = db.LayoutDiff()

diff.on_cell_in_a_only = lambda c: results["cells_a_only"].append(c.name)
diff.on_cell_in_b_only = lambda c: results["cells_b_only"].append(c.name)
diff.on_begin_cell = lambda ca, cb: current_cell.__setitem__(0, ca.name if ca else cb.name)

def record(kind, side, obj, pid):
    results["changes"].append({
        "cell": current_cell[0],
        "layer": str(diff.layer_info_a if side == "a" else diff.layer_info_b),
        "type": kind, "side": side, "shape": str(obj)
    })

diff.on_polygon_in_a_only = lambda p, pid: record("polygon", "a", p, pid)
diff.on_polygon_in_b_only = lambda p, pid: record("polygon", "b", p, pid)
diff.on_box_in_a_only     = lambda b, pid: record("box",     "a", b, pid)
diff.on_box_in_b_only     = lambda b, pid: record("box",     "b", b, pid)
diff.on_text_in_a_only    = lambda t, pid: record("text",    "a", t, pid)
diff.on_text_in_b_only    = lambda t, pid: record("text",    "b", t, pid)

flags = (db.LayoutDiff.Verbose +
         db.LayoutDiff.SmartCellMapping +
         db.LayoutDiff.IgnoreDuplicates)   # ← crítico para GDS vs OASIS

identical = diff.compare(la, lb, flags)
results["identical"] = identical
print(json.dumps(results, indent=2))
sys.exit(0 if identical else 1)
```

**Nota crítica:** Siempre usar `IgnoreDuplicates` al comparar GDS — el formato permite shapes duplicadas que OASIS elimina, causando falsos positivos sin este flag.

### Callbacks disponibles en LayoutDiff

```
Cell-level:  on_cell_in_a_only, on_cell_in_b_only, on_cell_name_differs, on_begin_cell, on_end_cell
Layer-level: on_layer_in_a_only, on_layer_in_b_only, on_layer_name_differs, on_begin_layer
Shape-level: on_polygon_in_a/b_only, on_box_in_a/b_only, on_path_in_a/b_only, on_text_in_a/b_only
Global:      on_dbu_differs, on_bbox_differs
```

---

## 5. KLayout — Exportar PNG para visualización

### Con el paquete completo de KLayout (sistema, no PyPI)

```python
# klayout -zz -r render.py -rd gds=layout.gds -rd out=layout.png
import klayout.lay as lay
import klayout.db as db

lv = lay.LayoutView()
layout = db.Layout()
layout.read(gds)
lv.show_layout(layout, False)
lv.max_hier()
lv.active_cellview().cell = layout.top_cell()
box = layout.top_cell().bbox().to_dtype(layout.dbu)
lv.save_image_with_options(out, 2048, 2048, 0, 0, 0, db.DBox(box), False)
```

### Limitaciones por paquete

| Paquete | `klayout.db` (diff/XOR) | `klayout.lay` (PNG) |
|---|---|---|
| `pip install klayout` | ✅ Sí, headless | ❌ No disponible |
| `apt install klayout` | ✅ Sí | ✅ Sí (con `-zz`) |
| KLayout app instalada | ✅ Sí | ✅ Sí (con `-zz`) |

**Para CI Linux:** `pip install klayout` es suficiente para diff/XOR. Para PNG necesitás el paquete del sistema + `-zz`.

---

## 6. Magic VLSI — Formato .mag

### Estructura del archivo

Los `.mag` son **texto plano ASCII**, un objeto por sección — excelente para git.

```
magic
tech sky130A
timestamp 1693000000
<< metal1 >>
rect 10 10 90 40
<< labels >>
rlabel metal1 10 10 90 40 0 VDD
use nand2 nand2_0
timestamp 1692000000
transform 1 0 200 0 1 0
box 0 0 100 100
<< end >>
```

| Sección | Contenido |
|---|---|
| `<< layername >>` + `rect` | Rectángulos por capa (coordenadas en lambda) |
| `<< labels >>` + `rlabel` | Etiquetas de red |
| `use filename id` | Referencia a subcelda (jerarquía) |
| `transform a b c d e f` | Matriz de transformación 3×3 |
| `timestamp` | Unix timestamp — **el problema principal** |

### El problema del timestamp

Magic actualiza el campo `timestamp` en cada save de la celda y de sus parents. Resultado: abrir y guardar sin cambios produce un diff con líneas `timestamp` modificadas — ruido puro.

**Solución para Riku:** git textconv/clean filter que normalice o elimine timestamps:

```bash
# .gitattributes
*.mag diff=magic

# .git/config
[diff "magic"]
    textconv = strip-mag-timestamps

# strip-mag-timestamps (script)
grep -v "^timestamp" "$1"
```

---

## 7. Magic — Headless y batch

```bash
magic -dnull -noconsole script.tcl    # headless estándar (usado por OpenLane)
magic -d NULL -noconsole script.tcl   # equivalente con espacio
```

El script **debe terminar con `quit`** o Magic queda esperando input interactivo.

### Exportar GDS desde Magic (headless)

```tcl
load mycell
gds write output.gds
quit
```

### Exportar PNG desde Magic (headless)

```tcl
load mycell
plot pnm output.pnm 1.0    # 1.0 pixel/lambda
quit
```
Luego convertir: `convert output.pnm output.png`

**SVG requiere display** (`magic -d XR`) — no funciona con `-dnull`. Para SVG en CI usar Xvfb.

---

## 8. Magic en flujos open source

### Rol de Magic vs. KLayout

| Dimensión | Magic | KLayout |
|---|---|---|
| Formato nativo | .mag (texto) | GDS/OASIS (binario) |
| Edición de layouts | ✅ Principal | ✅ También |
| DRC | ✅ Reglas en TCL | ✅ Scripts Ruby |
| LVS | ✅ extract → Netgen | ✅ LVS en Ruby |
| Diff/comparación | ❌ No tiene | ✅ XOR + Diff |
| Scripting CI | TCL | Python + Ruby |
| Estética GUI | X11 clásico | Qt moderno |

### En OpenLane

Magic tiene 4 roles headless en el flow:
1. GDS streaming (DEF+LEF → GDS)
2. DRC signoff
3. LVS extraction (`extract` + `ext2spice`)
4. Antenna check

KLayout hace un **XOR de verificación** comparando el GDS de Magic con el de KLayout para detectar errores de streaming.

---

## 9. Flujo de diff propuesto para Riku

### Para .mag (Magic)

```
commit A (.mag) → strip timestamps → git diff → diff texto legible
commit A (.mag) → magic -dnull → a.gds ┐
commit B (.mag) → magic -dnull → b.gds ┘ → klayout XOR → diff visual (PNG/GDS)
```

### Para .gds directo

```
commit A (.gds) ┐
commit B (.gds) ┘ → strmcmp → texto legible (stdout)
                 → klayout XOR → diff.gds → renderizar PNG
                 → LayoutDiff Python API → JSON estructurado
```

### Git diff drivers

```ini
# .gitattributes
*.mag  diff=magic
*.gds  diff=klayout

# .git/config
[diff "magic"]
    textconv = miku-strip-timestamps

[diff "klayout"]
    textconv = miku-gds-textconv    # convierte GDS → texto via strmcmp o LayoutDiff
```

---

## 10. Herramientas del ecosistema relevantes para Riku

| Herramienta | Qué hace | Relevancia |
|---|---|---|
| **lytest** | pytest + KLayout XOR, testing de regresión de GDS | Referencia de arquitectura para CI de Riku |
| **GDSFactory `gf gds diff`** | CLI diff por capas con KLayout | Precedente de UX |
| **efabless/utilities `xor`** | CLI wrapper de KLayout XOR | Alternativa simple |
| **gdstk** | Leer/escribir GDS en Python | Base para textconv driver |
| **strmcmp** | Diff estructural texto, exit code semántico | Candidato directo para git textconv |

---

## 11. Conclusiones para Riku

1. **GDS como artefacto de build, no como fuente.** Si el diseñador usa Magic, `.mag` es la fuente. Si usa GDSFactory/GLayout, el Python es la fuente. GDS va a CI artifacts o Git LFS.

2. **KLayout es el motor de diff para GDS** — no hay nada mejor en open source. Usar `LayoutDiff` para JSON estructurado y XOR para verificación física.

3. **`strmcmp` es el candidato más directo** para un git textconv de GDS: produce texto legible, exit code semántico, sin necesitar Python.

4. **Magic necesita el filtro de timestamps** — sin eso, el diff en git es inservible. Este filtro es simple y sería el aporte más inmediato de Riku para la comunidad Magic.

5. **Ninguna de estas integraciones existe publicada** como git driver. Riku cubriría un gap real en los tres casos: `.sch` (Xschem), `.mag` (Magic), `.gds` (KLayout).

6. **El flujo de visualización más viable para CI:** `.mag` → `magic -dnull` → `.gds` → `klayout -b XOR` → `.gds con diferencias` → `klayout -zz render` → PNG.

---

## 12. Notas de retroalimentación — flujos reales

> Estas notas provienen del research de experiencia de usuario en foros y proyectos reales
> (open-source-silicon.dev, GitHub issues de KLayout/Magic, unic-cass, IHP PDK docs).
> Requieren revisión y posible expansión de las secciones anteriores.

### 12a. KLayout como editor primario (no solo visor/verificador)

La Sección 8 ("Rol de Magic vs. KLayout") asume implícitamente Magic como editor principal y KLayout como verificador. En la práctica esto no es universal:

- **IHP SG13G2:** KLayout es el editor primario. Magic fue añadido después y su soporte quedó interrumpido con el cierre de Efabless (feb. 2025). El IHP Certificate Course empieza con KLayout.
- **GF180MCU:** Tim Edwards (creador de Magic) declaró públicamente usar KLayout como editor principal para GF180 (*"I'm using KLayout as the main layout editor for GF180, but always double check the DRC on Magic"*).
- La razón es PDK-dependiente: Magic recibe más mantenimiento para SKY130; KLayout está mejor integrado en IHP y GF180.

**Implicación para la Sección 8:** La tabla de "Rol de Magic vs. KLayout" debe agregar una columna "PDK recomendado" o una nota que aclare que el rol de cada herramienta depende del PDK, no es fijo.

**Implicación para la Sección 9 (flujo de diff propuesto):** El flujo `.mag → magic -dnull → .gds → klayout XOR` presupone que la fuente es `.mag`. Si la fuente es un KLayout directo (`.gds` o `.oas` como fuente primaria), el flujo no aplica. El driver GDS necesita saber si el `.gds` es fuente o artefacto antes de decidir cómo hacer diff.

Fuente de campo: [open-source-silicon.dev/t/16219913](https://web.open-source-silicon.dev/t/16219913), [IHP Certificate Course](https://www.ihp-microelectronics.com/events-1/detail/title-certificate-course-in-person-analog-design-with-ihp-sg13g2-open-source-pdk), [wiki.f-si.org IHP integration](https://wiki.f-si.org/index.php?title=IHP_Open_PDK_integration_with_Magic,_Netgen,_and_LibreLane)

### 12b. OASIS como formato alternativo al GDS

El documento no menciona OASIS (`.oas`) como formato de layout — solo GDS. En proyectos KLayout-nativos, OASIS es el formato preferido:

- OASIS puede ser **1000x más compacto** que GDS equivalente (ejemplo documentado: 150 MB → 25 kB).
- OASIS preserva nombres de layers; GDS los pierde.
- KLayout ya está registrado como driver para `.oas` en la `EXTENSION_MAP` de arquitectura, pero el flujo de diff de este documento no lo contempla.

**Implicación:** La Sección 9 debe incluir un flujo paralelo para `.oas`. El XOR y LayoutDiff de KLayout funcionan igual para OASIS — el diff es idéntico, solo cambia el formato de entrada.

Fuente: [KLayout forum — OASIS as GDS successor](https://www.klayout.de/forum/discussion/2152/oasis-the-successor-to-gds)

### 12c. Bugs conocidos en la integración KLayout-SKY130 que afectan CI

El tutorial de unic-cass (2024) documenta dos bugs en la distribución oficial del SKY130 PDK que impiden usar KLayout sin patches manuales:

```bash
sed -i -e '/<?xml/d' $PDK_ROOT/$PDK/libs.tech/klayout/pymacros/sky130.lym
sed -i -e 's/sky130/sky130A/' $PDK_ROOT/$PDK/libs.tech/klayout/tech/sky130A.lyt
```

Si el CI de Riku no aplica estos patches, fallará silenciosamente en cualquier entorno SKY130 fresco. El `miku doctor` debería detectar y reportar este estado.

Fuente: [unic-cass KLayout Sky130 tutorial (2024)](https://unic-cass.github.io/training/sky130/3.3-layout-klayout.html)

### 12d. KLayout GDS exportado rechazado por herramientas de foundry

KLayout forum issue #1026 documenta que GDS exportado desde KLayout puede ser rechazado por Cadence (herramienta del foundry) por:
- Arrays 1×1 (single-instance AREFs) que Cadence ignora silenciosamente
- `$$$CONTEXT_INFO$$$` dummy cell rechazada por terceros
- Arrays no-ortogonales que generan warnings y se omiten

Workarounds: usar OASIS en vez de GDS, deshabilitar PCell context storage. Documentar esto como limitación conocida al usar Riku en flujos que terminan en foundry submission.

Fuente: [KLayout forum #1026](https://www.klayout.de/forum/discussion/1026/very-important-gds-exported-from-k-layout-not-working-on-cadence-at-foundry)

### 12e. Layout generado por Python (GDSFactory, gdstk)

El flujo de diff actual asume que el layout tiene una fuente editable (`.mag` o KLayout GUI). Existe una tercera categoría donde el layout es generado por código Python — en este caso:

- La fuente es el `.py` o `.yaml`; el `.gds` es un build artifact
- No hay nada que diffear en el GDS salvo verificación de regresión
- El diff semántico relevante es entre las dos versiones del script Python

Este caso no está cubierto por los drivers actuales. No requiere un driver nuevo — requiere que `miku.toml` pueda declarar `layout.source = "python"` y que Riku no intente diffear el GDS como si fuera fuente.

Referencia: [github.com/gdsfactory/gdsfactory](https://github.com/gdsfactory/gdsfactory), [OpenFASoC GLayout](https://openfasoc.readthedocs.io/en/latest/notebooks/glayout/glayout_opamp.html)

---

## Referencias

### KLayout
- **Repo oficial**: https://github.com/KLayout/klayout
- **Documentación Python API (klayout.db)**: https://www.klayout.de/doc/code/index.html
- **klayout en PyPI**: https://pypi.org/project/klayout/ — `pip install klayout` (sin GUI, solo DB)
- **Script DRC de ejemplo (SKY130)**: https://github.com/google/skywater-pdk/tree/main/libraries — buscar archivos `.lydrc`
- **KLayout DRC scripting**: https://www.klayout.de/doc/manual/drc_ref.html
- **strmcmp / strmxor (buddy tools)**: incluidos en instalación oficial de KLayout

### Magic VLSI
- **Repo oficial**: https://github.com/RTimothyEdwards/magic
- **Documentación**: http://opencircuitdesign.com/magic/
- **Tutorial de extracción de netlist**: http://opencircuitdesign.com/magic/tutorials/tut8.html
- **Magic con SKY130**: https://github.com/RTimothyEdwards/open_pdks

### Librerías GDS alternativas
- **gdstk**: https://github.com/heitzmann/gdstk — C++ con bindings Python, más rápido que gdspy
- **gdspy**: https://github.com/heitzmann/gdspy — precursora, ampliamente usada
- **lytest**: https://github.com/atait/lytest — framework de regresión para layouts GDS
- **GDSFactory**: https://github.com/gdsfactory/gdsfactory — diseño de chips fotónicos y CMOS

### PDKs de referencia (para pruebas)
- **SKY130 PDK**: https://github.com/google/skywater-pdk
- **GF180MCU PDK**: https://github.com/google/gf180mcu-pdk
- **IHP SG13G2**: https://github.com/IHP-GmbH/IHP-Open-PDK

### Ver también
- [headless_y_compatibilidad_herramientas.md](headless_y_compatibilidad_herramientas.md) — flags headless de KLayout y Magic
- [../operaciones/ci_drc_lvs_regresiones.md](../operaciones/ci_drc_lvs_regresiones.md) — uso de KLayout DRC en CI
- [../operaciones/cache_y_rendimiento.md](../operaciones/cache_y_rendimiento.md) — performance del XOR geométrico
- [../operaciones/estrategia_merge_archivos_mixtos.md](../operaciones/estrategia_merge_archivos_mixtos.md) — merge de .mag y conflictos de timestamp
- [../arquitectura/arquitectura_cli_y_orquestacion.md](../arquitectura/arquitectura_cli_y_orquestacion.md) — detección de tipo de archivo por magic bytes y registro de drivers
