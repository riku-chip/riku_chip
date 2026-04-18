# GDS / KLayout / Magic — Diff y Git Integration para Miku

## 1. El problema raíz: GDS es binario

**GDSII (.gds)** es un formato binario — `git diff` solo muestra "binary file changed". No hay texto que comparar. Esta es la diferencia fundamental con Xschem (.sch) y Magic (.mag), que son texto plano.

**Estrategia de Miku para GDS:** tratar GDS como artefacto de build (como un ejecutable compilado), no como fuente. La fuente versionable es .mag o código Python (GDSFactory/GLayout).

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

**Solución para Miku:** git textconv/clean filter que normalice o elimine timestamps:

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

## 9. Flujo de diff propuesto para Miku

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

## 10. Herramientas del ecosistema relevantes para Miku

| Herramienta | Qué hace | Relevancia |
|---|---|---|
| **lytest** | pytest + KLayout XOR, testing de regresión de GDS | Referencia de arquitectura para CI de Miku |
| **GDSFactory `gf gds diff`** | CLI diff por capas con KLayout | Precedente de UX |
| **efabless/utilities `xor`** | CLI wrapper de KLayout XOR | Alternativa simple |
| **gdstk** | Leer/escribir GDS en Python | Base para textconv driver |
| **strmcmp** | Diff estructural texto, exit code semántico | Candidato directo para git textconv |

---

## 11. Conclusiones para Miku

1. **GDS como artefacto de build, no como fuente.** Si el diseñador usa Magic, `.mag` es la fuente. Si usa GDSFactory/GLayout, el Python es la fuente. GDS va a CI artifacts o Git LFS.

2. **KLayout es el motor de diff para GDS** — no hay nada mejor en open source. Usar `LayoutDiff` para JSON estructurado y XOR para verificación física.

3. **`strmcmp` es el candidato más directo** para un git textconv de GDS: produce texto legible, exit code semántico, sin necesitar Python.

4. **Magic necesita el filtro de timestamps** — sin eso, el diff en git es inservible. Este filtro es simple y sería el aporte más inmediato de Miku para la comunidad Magic.

5. **Ninguna de estas integraciones existe publicada** como git driver. Miku cubriría un gap real en los tres casos: `.sch` (Xschem), `.mag` (Magic), `.gds` (KLayout).

6. **El flujo de visualización más viable para CI:** `.mag` → `magic -dnull` → `.gds` → `klayout -b XOR` → `.gds con diferencias` → `klayout -zz render` → PNG.
