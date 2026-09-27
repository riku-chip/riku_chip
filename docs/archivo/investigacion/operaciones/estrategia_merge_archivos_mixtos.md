# Estrategia de Merge para Archivos Mixtos en Riku

Riku gestiona cuatro tipos de archivo con propiedades radicalmente distintas: GDS (binario, layout físico), .mag (texto, Magic VLSI), .sch (texto, Xschem), y .sp/.spice (texto, NGSpice). Un merge de rama involucra habitualmente cambios en más de uno de estos tipos al mismo tiempo, y la relación de derivación entre ellos introduce dependencias que Git ignora completamente. Este documento define la estrategia de merge de Riku: qué se puede fusionar automáticamente, qué requiere intervención, cómo representar conflictos de forma útil, y cómo registrar todo esto en el sistema de drivers de Git.

---

## 1. El problema de dependencias cruzadas

### Grafo de derivación

```
.sch (Xschem)
    │
    ├─→ export netlist ─→ .sp/.spice (simulación)
    │                          │
    │                          └─→ ngspice -b ─→ .raw (artefacto, no versionar)
    │
    └─→ (diseño) ─→ .mag (Magic VLSI layout)
                         │
                         └─→ magic gds write ─→ .gds (artefacto derivado)
                                                     │
                                                     └─→ DRC / LVS / tapeout
```

El GDS es siempre derivado. El .mag es la fuente del layout. El .sch es la fuente del esquemático. El .spice exportado desde Xschem es también derivado (aunque se versiona por comodidad).

### Qué puede salir mal en un merge

**Escenario 1 — El GDS queda desactualizado:**  
El diseñador A modifica `inverter.mag` y hace commit. El diseñador B tiene un `inverter.gds` viejo commiteado (o en artifacts). Después del merge, el .gds no corresponde al .mag nuevo.

**Escenario 2 — El netlist es inconsistente con el esquemático:**  
El diseñador A modifica `amp.sch` (cambia W/L de un transistor). El diseñador B tiene `amp.spice` exportado antes de ese cambio. Ambas ramas convergen: el .spice en el árbol de trabajo no refleja el .sch mergeado.

**Escenario 3 — Layout y esquemático divergen topológicamente:**  
Después de un merge, el .mag tiene la celda `nand2` con una conexión nueva pero el .sch todavía la tiene con la conexión vieja. El LVS falla.

### Detección: el registro de dependencias de Riku

Riku debe mantener un archivo `.miku/deps.toml` versionado que registra relaciones de derivación:

```toml
# .miku/deps.toml
[cells.inverter]
source_sch = "schematics/inverter.sch"
source_mag = "layout/inverter.mag"
derived_gds = "gds/inverter.gds"  # artefacto — puede estar en LFS o ausente
derived_spice = "netlists/inverter.sp"

[cells.amp]
source_sch = "schematics/amp.sch"
source_mag = "layout/amp.mag"
derived_gds = "gds/amp.gds"
derived_spice = "netlists/amp.sp"
```

**En el hook `post-merge`**, Riku recorre este registro y verifica:

```python
# miku/hooks/post_merge.py — lógica central
import pygit2, tomllib, sys
from pathlib import Path

def check_staleness(repo_path: str) -> list[str]:
    repo = pygit2.Repository(repo_path)
    deps = tomllib.loads(Path(".miku/deps.toml").read_text())
    warnings = []

    for cell, spec in deps["cells"].items():
        source = spec.get("source_mag") or spec.get("source_sch")
        derived = spec.get("derived_gds") or spec.get("derived_spice")
        if not source or not derived:
            continue

        # Comparar mtime del último commit que tocó cada archivo
        source_commit = last_commit_touching(repo, source)
        derived_commit = last_commit_touching(repo, derived)

        if source_commit and derived_commit:
            if source_commit.commit_time > derived_commit.commit_time:
                warnings.append(
                    f"STALE: {derived} fue generado antes del último "
                    f"cambio en {source} (celda '{cell}')"
                )
    return warnings
```

**La advertencia en el terminal** no bloquea el flujo (no falla el merge), pero es prominente:

```
[miku] ADVERTENCIA — artefactos desactualizados después del merge:
  • gds/inverter.gds  generado antes del último cambio en layout/inverter.mag
  • netlists/amp.sp   generado antes del último cambio en schematics/amp.sch

  Regenerar con: miku build inverter amp
  Verificar LVS:  miku lvs inverter amp
```

**Decisión de diseño:** Advertir, no bloquear. Bloquear el merge porque el GDS está desactualizado sería demasiado agresivo — el diseñador puede querer revisar antes de regenerar, o puede que el GDS desactualizado sea aceptable temporalmente. La UX correcta es información clara con acción sugerida.

---

## 2. Estrategia de merge para GDS binario

### Por qué el merge automático de GDS completo es inviable

GDS es un stream de records binarios sin estructura de texto que Git pueda unir. Incluso si se tuvieran dos versiones modificadas del mismo .gds, un merge byte a byte produciría un archivo corrupto. La única opción real es operar a nivel de la jerarquía de celdas.

### Taxonomía de escenarios de merge

| Escenario | Condición | Estrategia |
|---|---|---|
| **Celdas disjuntas** | A modifica celda X, B modifica celda Y | Merge automático posible via KLayout API |
| **Misma celda, capas disjuntas** | A toca metal1 de `nand2`, B toca poly de `nand2` | Merge automático posible con cautela |
| **Misma celda, misma capa** | A y B modifican poly de `nand2` | Rechazar — requiere resolución manual |
| **Renombrado de celda** | A renombra `nand2` → `nand2_v2`, B modifica `nand2` | Detectar con SmartCellMapping, advertir |
| **Cambio de DBU** | Cualquier cambio de unidades base | Rechazar siempre — incompatibilidad fundamental |

### Merge a nivel de celda con KLayout Python API

Para el caso de celdas disjuntas (el más común en equipos pequeños), Riku puede construir un GDS mergeado extrayendo cada celda del GDS ganador y combinándolas:

```python
# miku/merge/gds_merge.py
import klayout.db as db

def merge_gds_cell_level(
    base_gds: str,    # ancestro común
    ours_gds: str,    # HEAD actual
    theirs_gds: str,  # rama entrante
    output_gds: str,
) -> dict:
    """
    Merge automático de GDS cuando los cambios son en celdas distintas.
    Retorna un dict con el resultado y cualquier conflicto detectado.
    """
    base   = db.Layout(); base.read(base_gds)
    ours   = db.Layout(); ours.read(ours_gds)
    theirs = db.Layout(); theirs.read(theirs_gds)

    # Detectar qué celdas cambió cada rama respecto al ancestro
    ours_changed   = cells_changed_from_base(base, ours)
    theirs_changed = cells_changed_from_base(base, theirs)

    conflicts = ours_changed & theirs_changed  # intersección

    if conflicts:
        return {
            "status": "conflict",
            "conflicting_cells": list(conflicts),
            "auto_merged": [],
        }

    # Sin conflictos — construir layout mergeado
    result = db.Layout()
    result.dbu = ours.dbu

    # Copiar todas las celdas de "ours" (incluye celdas sin cambios)
    ours.copy_tree(result)

    # Sobreescribir con las celdas modificadas de "theirs"
    for cell_name in theirs_changed:
        cell_theirs = theirs.cell(cell_name)
        if cell_theirs:
            cell_result = result.cell(cell_name)
            if cell_result:
                cell_result.clear()
            else:
                cell_result = result.create_cell(cell_name)
            theirs.copy_cell(cell_theirs.cell_index(), result)

    result.write(output_gds)
    return {
        "status": "ok",
        "conflicting_cells": [],
        "auto_merged": list(theirs_changed),
    }


def cells_changed_from_base(base: db.Layout, variant: db.Layout) -> set[str]:
    """Usa LayoutDiff para detectar qué celdas difieren del ancestro."""
    changed = set()
    diff = db.LayoutDiff()
    diff.on_begin_cell = lambda ca, cb: None  # placeholder
    # Registrar celdas con cualquier diferencia
    current = [None]
    diff.on_begin_cell = lambda ca, cb: current.__setitem__(0, (ca or cb).name)
    def mark_changed(*args): changed.add(current[0])
    for ev in ["on_polygon_in_a_only","on_polygon_in_b_only",
               "on_box_in_a_only","on_box_in_b_only",
               "on_path_in_a_only","on_path_in_b_only",
               "on_text_in_a_only","on_text_in_b_only"]:
        setattr(diff, ev, mark_changed)
    flags = (db.LayoutDiff.Verbose |
             db.LayoutDiff.SmartCellMapping |
             db.LayoutDiff.IgnoreDuplicates)
    diff.compare(base, variant, flags)
    return changed
```

**Limitaciones importantes:**
- Esta estrategia solo es correcta cuando las celdas son verdaderamente independientes. Si la celda X referencia subceldas que B también modificó, el merge puede producir un layout inconsistente internamente. Riku debe verificar las dependencias de subcelda antes de declarar "celdas disjuntas".
- KLayout no tiene un equivalente a `git merge` con resolución de conflictos — la API de copia de celdas es de bajo nivel y requiere manejo cuidadoso de índices de layer y referencias.

### Cuando rechazar y pedir resolución manual

Si hay conflictos en la misma celda, Riku produce un reporte claro en lugar de un archivo GDS corrupto:

```
[miku] CONFLICTO GDS — no se puede hacer merge automático de gds/top.gds

Celdas modificadas en ambas ramas:
  • nand2    (modificada en feature/routing y en feature/timing-fix)
  • buf4     (modificada en ambas ramas)

Opciones:
  1. Resolver visualmente:   miku merge-tool gds/top.gds
     (abre KLayout con ambas versiones en capas separadas)

  2. Tomar una versión completa:
     miku merge --ours   gds/top.gds   # usar nuestra versión
     miku merge --theirs gds/top.gds   # usar la versión entrante

  3. Editar manualmente y marcar como resuelto:
     miku merge --resolved gds/top.gds

Archivos fuente relacionados:
  • layout/nand2.mag  → intentar merge en .mag primero, luego regenerar GDS
```

**Decisión de diseño:** La recomendación de resolver en .mag es clave para la UX. En la mayoría de los flujos open source, el GDS es derivado del .mag. Si hay un conflicto en el GDS, la forma correcta de resolverlo es resolver el conflicto en el .mag y regenerar. Riku debe hacer esta ruta evidente.

### Casos de uso reales: dos diseñadores en celdas distintas del mismo GDS

**Caso A — Trabajo en celdas distintas (el caso más frecuente en diseño jerárquico):**

El diseñador 1 modifica la celda `alu` (routing de metal2). El diseñador 2 modifica la celda `register_file` (ajuste de sizing en poly). Ambas celdas son independientes (ninguna es subcelda de la otra).

Resultado esperado: merge automático exitoso. Riku extrae los cambios de cada diseñador, combina los GDS, y produce un top-level válido.

**Condición de éxito:** Que el GDS esté organizado jerárquicamente con celdas bien definidas (como es la práctica estándar). Los GDS completamente aplanados (flat) no tienen estructura de celda explotable.

**Caso B — Top cell con instancias de ambas celdas modificadas:**

Aunque las celdas hoja no conflictuen, la top cell (`chip_top`) contiene instancias de ambas. Si ningún diseñador tocó la top cell directamente, el merge es limpio. Si ambos modificaron la top cell (por ejemplo, para agregar nuevas instancias), hay conflicto en la top cell aunque las celdas hoja sean disjuntas.

**Caso C — El GDS está desactualizado en una de las ramas:**

El diseñador 1 tiene su GDS actualizado (generado hoy). El diseñador 2 tiene un GDS de hace tres días (antes de los cambios de 1). Riku detecta este caso comparando los timestamps de los commits fuente y advierte antes de intentar el merge.

**Veredicto:** El merge automático de GDS es posible y útil para el caso de celdas disjuntas en diseños jerárquicos bien organizados, que es el caso normal en equipos. Es el gap que Riku puede cubrir de forma práctica.

---

## 3. Merge para .mag y .sch

### 3a. Merge de archivos .mag (Magic VLSI)

El formato .mag es texto plano con secciones por capa (`<< metal1 >>`, `<< poly >>`, etc.) y secciones de referencias (`use`). Esto lo hace parcialmente mergeable con estrategias de texto, pero con semántica que Git desconoce.

**Partes mergeables automáticamente:**

| Sección | Condición para merge automático |
|---|---|
| `<< layername >>` + `rect` | Diseñadores modificaron capas distintas |
| `<< labels >>` | Etiquetas añadidas sin solapamiento |
| Sección `use` (referencias) | Añadir instancias en distintas posiciones |
| Propiedades de celda | Solo cambia un lado |

**Partes que requieren intervención:**

| Sección | Por qué |
|---|---|
| Misma capa, misma zona | Rects solapados o contradictorios |
| `transform` de instancia `use` | Si ambos mueven la misma instancia |
| `timestamp` | Siempre distinto — ignorar (ver más abajo) |
| Cambio de tech | Nunca debería pasar, pero es catastrófico si ocurre |

**El problema del timestamp en .mag:** Magic actualiza el campo `timestamp` en cada save. Un merge produce siempre un conflicto trivial en esta línea aunque no haya cambios reales. Riku registra un merge driver que ignora timestamps:

```ini
# .gitattributes
*.mag merge=magic
```

```python
# El merge driver de Riku para .mag
# Se invoca como: miku-mag-merge %O %A %B %L %P
# %O = ancestro, %A = ours (se modifica in-place), %B = theirs

import sys, re
from pathlib import Path

def normalize_timestamps(content: str) -> str:
    return re.sub(r'^timestamp \d+$', 'timestamp 0', content, flags=re.MULTILINE)

base_path, ours_path, theirs_path = sys.argv[1], sys.argv[2], sys.argv[3]

base   = normalize_timestamps(Path(base_path).read_text())
ours   = normalize_timestamps(Path(ours_path).read_text())
theirs = normalize_timestamps(Path(theirs_path).read_text())

# Escribir versiones normalizadas para que git merge las procese
Path(base_path).write_text(base)
Path(ours_path).write_text(ours)
Path(theirs_path).write_text(theirs)

# Llamar a git merge-file con las versiones normalizadas
import subprocess
result = subprocess.run(
    ["git", "merge-file", "-L", "ours", "-L", "base", "-L", "theirs",
     ours_path, base_path, theirs_path],
    capture_output=True
)
sys.exit(result.returncode)
```

**Para conflictos reales en .mag**, Riku produce marcadores con contexto de capa:

```
<< metal1 >>
<<<<<<< ours (feature/routing — Carlos, hace 2h)
rect 100 200 500 300
rect 100 350 500 450
||||||| base
rect 100 200 500 300
=======
rect 100 200 500 300
rect 600 200 900 300
>>>>>>> theirs (feature/timing — Ana, hace 45min)
```

Este es el formato estándar de Git (`diff3`) pero Riku lo presenta con información adicional: quién hizo el cambio, en qué rama, y hace cuánto.

### 3b. Merge de archivos .sch (Xschem)

El .sch de Xschem es una línea por objeto (wire `N`, componente `C`, texto `T`). Esto lo hace naturalmente mergeable en muchos casos.

**Partes mergeables automáticamente:**

| Tipo de cambio | Condición |
|---|---|
| Añadir componentes (`C`) en zonas distintas | Las coordenadas no solapan |
| Añadir nets (`N`) en zonas distintas | Sin cruce conflictivo |
| Cambiar propiedades de componentes distintos | A modifica `C4`, B modifica `R2` |
| Cambiar valor de un componente (`value=`) | Solo un lado lo modifica |
| Añadir/quitar etiquetas de texto | En posiciones distintas |

**Partes que requieren intervención:**

| Tipo de cambio | Por qué |
|---|---|
| Mismo componente, misma propiedad | Ambos cambian `value=` de `R1` |
| "Move all" (todas las coordenadas cambian) | El diff completo del archivo es ruidoso, merge casi imposible |
| Cambio de versión del archivo (`v {xschem version=...}`) | Tratarlo como ignorable si la diferencia es solo de versión de herramienta |
| Propiedad `schprop` global | Si ambos modifican propiedades del esquemático global |

**El "move all" es el caso más problemático para .sch.** Si un diseñador reorganiza el layout visual del esquemático (mueve todos los componentes para mejor legibilidad), cambian todas las coordenadas de todas las líneas `C` y `N`. Un merge con otra rama que también hizo cambios funcionales es prácticamente inmanejable automáticamente.

**Solución de diseño para "move all":** Riku puede detectar si más del 80% de las líneas `C` y `N` de un archivo cambiaron sus coordenadas pero no sus propiedades — si es así, clasificar el commit como "reorganización visual" y advertir al hacer merge con ramas que tienen cambios funcionales. La resolución es manual pero la detección es automática.

```python
def classify_sch_change(diff_text: str) -> str:
    """Clasifica si un diff de .sch es mayoritariamente cosmético o funcional."""
    coord_changes = 0
    property_changes = 0
    for line in diff_text.split('\n'):
        if line.startswith(('+C ', '-C ', '+N ', '-N ')):
            # Línea de componente o wire — extraer si solo cambiaron coordenadas
            # Formato: C {sym} x y rot flip {propiedades}
            parts = line[1:].split()
            if len(parts) >= 4 and all(p.lstrip('-').isdigit() for p in parts[1:3]):
                coord_changes += 1
            else:
                property_changes += 1
    if coord_changes > 0 and property_changes == 0:
        return "cosmetic"
    elif coord_changes > 0 and property_changes > 0:
        return "mixed"
    return "functional"
```

### 3c. Merge de archivos .sp/.spice (NGSpice)

Los netlists SPICE son texto, un componente por línea. El merge es funcionalmente idéntico al de .sch cuando los netlists son escritos a mano. Cuando son exportados desde Xschem, el problema es que son derivados y no deberían mergearse directamente — deberían regenerarse del .sch mergeado.

**Política de Riku para .spice exportados:**

```toml
# .miku/deps.toml
[cells.amp]
source_sch = "schematics/amp.sch"
derived_spice = "netlists/amp.sp"
export_tool = "xschem"
```

Si `amp.sp` está marcado como derivado de `amp.sch`, Riku usa una estrategia diferente en el merge:

1. Hacer merge de `amp.sch` primero (es la fuente).
2. Si `amp.sch` mergea limpiamente, regenerar `amp.sp` automáticamente (`xschem -q --no_x --netlist amp.sch`).
3. Si `amp.sch` tiene conflictos, marcar `amp.sp` como "pendiente de regeneración" y no intentar mergear el .spice.

Para netlists escritos a mano (sin correspondencia con .sch), aplicar la misma lógica que para .sch: merge de texto con normalización previa (eliminar timestamps de comentarios generados).

---

## 4. Representación visual de conflictos

### Por qué los markers `<<<<` de Git son insuficientes para diseño de chips

Los markers de texto de Git son adecuados para código fuente donde el contexto es legible. Para archivos EDA:
- En .mag: `rect 1200 3400 1800 4200` no comunica nada sin ver el layout.
- En .sch: `C {nfet_01v8.sym} 890 -160 0 0 {name=M3 W=1 L=0.15}` es legible para un experto pero no comunica si hay solapamiento visual o conflicto eléctrico.
- En GDS: no hay markers posibles — es binario.

### El reporte de conflicto de Riku

Riku genera un archivo `.miku/merge_report.json` después de cada merge con conflictos:

```json
{
  "merge_id": "merge-2026-04-18-1423",
  "base_commit": "a1b2c3d",
  "ours_commit":  "e4f5g6h",
  "theirs_commit": "i7j8k9l",
  "conflicts": [
    {
      "file": "layout/nand2.mag",
      "type": "mag_layer_conflict",
      "layer": "metal1",
      "our_change": {
        "author": "Carlos",
        "description": "rects: [[100,200,500,300], [100,350,500,450]]"
      },
      "their_change": {
        "author": "Ana",
        "description": "rects: [[100,200,500,300], [600,200,900,300]]"
      },
      "preview_ours":   ".miku/previews/nand2_ours.png",
      "preview_theirs": ".miku/previews/nand2_theirs.png",
      "preview_xor":    ".miku/previews/nand2_xor.png"
    },
    {
      "file": "schematics/amp.sch",
      "type": "sch_property_conflict",
      "component": "M3",
      "property": "W",
      "ours_value": "1.5",
      "theirs_value": "2.0",
      "preview_ours":   ".miku/previews/amp_ours.svg",
      "preview_theirs": ".miku/previews/amp_theirs.svg"
    }
  ],
  "auto_merged": [
    { "file": "layout/buf4.mag", "method": "layer-disjoint" },
    { "file": "schematics/inv.sch", "method": "component-disjoint" }
  ],
  "stale_artifacts": [
    "gds/nand2.gds"
  ]
}
```

### Visualización en el terminal

```
[miku] Resultado del merge feature/timing ─→ main:

  Auto-mergeado (2):
    ✓ layout/buf4.mag       (capas disjuntas — merge automático)
    ✓ schematics/inv.sch    (componentes disjuntos — merge automático)

  Conflictos a resolver (2):
    ✗ layout/nand2.mag
        Capa metal1 — Carlos añadió rect en zona oeste, Ana en zona este
        Ver diferencia:  miku show-conflict layout/nand2.mag
        Previews:        .miku/previews/nand2_ours.png
                         .miku/previews/nand2_xor.png

    ✗ schematics/amp.sch
        Componente M3.W — Carlos: 1.5 μm, Ana: 2.0 μm
        Ver esquemático: miku show-conflict schematics/amp.sch

  Artefactos desactualizados:
    ⚠ gds/nand2.gds  (regenerar con: miku build nand2)

Resolver con:  miku mergetool
```

### Comando `miku show-conflict`

Para un conflicto en .mag, invoca KLayout con ambas versiones superpuestas en capas distintas:

```bash
miku show-conflict layout/nand2.mag
# Genera: .miku/previews/nand2_ours.gds  (de .mag via Magic headless)
#         .miku/previews/nand2_theirs.gds
# Abre:   klayout con ambos GDS cargados en capas distintas
#         Capa 1: versión "ours" en azul
#         Capa 2: versión "theirs" en rojo
#         Capa 3: XOR (diferencias) en amarillo
```

Para un conflicto en .sch, abre Xschem con el diff visual:

```bash
miku show-conflict schematics/amp.sch
# Extrae las dos versiones a /tmp/
# Invoca: xschem --diff /tmp/amp_ours.sch /tmp/amp_theirs.sch
```

**Decisión de diseño:** Riku no construye su propio viewer desde cero. Delega la visualización a las herramientas nativas (KLayout, Xschem) que ya tienen la infraestructura correcta para mostrar estos formatos. La inversión de Riku es en la orquestación y en el reporte estructurado, no en reimplementar un renderer.

---

## 5. Merge drivers en .gitattributes

### Registro completo

```ini
# .gitattributes — en la raíz del repositorio Riku

# Layouts Magic — merge con normalización de timestamps
*.mag  merge=miku-mag  diff=miku-mag

# Esquemáticos Xschem — merge con detección de cambios cosméticos
*.sch  merge=miku-sch  diff=miku-sch

# Netlists SPICE — merge con normalización de comentarios
*.sp     merge=miku-spice  diff=miku-spice
*.spice  merge=miku-spice  diff=miku-spice
*.net    merge=miku-spice  diff=miku-spice

# GDS binario — driver de merge personalizado (nunca merge automático de Git)
*.gds  merge=miku-gds  diff=miku-gds  binary

# OASIS — igual que GDS
*.oas  merge=miku-gds  diff=miku-gds  binary
*.oasis merge=miku-gds diff=miku-gds  binary
```

### Configuración en `.miku/gitconfig` (incluido desde `.git/config`)

```ini
# .miku/gitconfig — commiteado al repo, incluido via:
# [include] path = .miku/gitconfig  (en .git/config)

[merge "miku-mag"]
    name = Riku merge driver para Magic VLSI (.mag)
    driver = miku merge-driver mag %O %A %B %L %P

[merge "miku-sch"]
    name = Riku merge driver para Xschem (.sch)
    driver = miku merge-driver sch %O %A %B %L %P

[merge "miku-spice"]
    name = Riku merge driver para netlists SPICE
    driver = miku merge-driver spice %O %A %B %L %P

[merge "miku-gds"]
    name = Riku merge driver para GDS/OASIS (binario)
    driver = miku merge-driver gds %O %A %B %L %P

[diff "miku-mag"]
    textconv = miku diff-textconv mag
    cachetextconv = true

[diff "miku-sch"]
    textconv = miku diff-textconv sch
    cachetextconv = true

[diff "miku-spice"]
    textconv = miku diff-textconv spice
    cachetextconv = true

[diff "miku-gds"]
    textconv = miku diff-textconv gds
    binary = true
    cachetextconv = true
```

### Interfaz del subcomando `miku merge-driver`

```
miku merge-driver <tipo> <base> <ours> <theirs> <marker-size> <path>

Tipos soportados:
  mag    — Magic VLSI .mag (normaliza timestamps, merge por capas)
  sch    — Xschem .sch (detecta cambios cosméticos, merge por componentes)
  spice  — Netlists SPICE (normaliza comentarios, merge línea a línea)
  gds    — GDS/OASIS binario (merge por celdas o rechaza con reporte)

Exit codes:
  0  — merge limpio, %A actualizado con resultado
  1  — conflicto, %A contiene markers, registrado en .miku/merge_report.json
  2  — error fatal (archivo corrupto, herramienta no disponible)
```

### Por qué `cachetextconv = true`

Los textconv de diff (para GDS especialmente) son costosos — invocan KLayout. Con `cachetextconv = true`, Git cachea el resultado indexado por el blob hash. En un repositorio con muchos commits, esto evita re-procesar el mismo GDS múltiples veces en `git log --all -p`.

### Instalación del .gitattributes sin contaminar el repo del usuario

Un problema práctico: el usuario puede tener sus propios `.gitattributes` en el repositorio. Riku puede requerir que el repositorio incluya `.gitattributes` de Riku, o puede instalar sus drivers globalmente:

```bash
# Instalación global (afecta todos los repos del usuario)
miku install --global

# Instalación local (solo este repo — recomendado)
miku init
# → crea/actualiza .gitattributes con los atributos de Riku
# → agrega [include] path = .miku/gitconfig en .git/config
# → no toca ~/.gitconfig
```

**Decisión de diseño:** La instalación local es la default. El driver registrado globalmente puede causar problemas en repos que no son de Riku (si alguien tiene archivos .mag en un repo de config, por ejemplo). La opción `--global` existe pero requiere confirmación explícita.

---

## 6. Casos de uso reales

### Caso 1 — Dos diseñadores, celdas distintas, flujo feliz

**Setup:** Ana y Carlos trabajan en la misma jerarquía de chip. Ana tiene la rama `feature/alu`, Carlos tiene `feature/sram`. La celda top es `chip_top.mag`.

```
Ancestro común: nand2.mag, alu.mag, sram.mag, chip_top.mag
Ana modifica:   alu.mag (nuevo bloque de suma en metal2)
Carlos modifica: sram.mag (ajuste de bitlines en metal1)
Nadie toca:     nand2.mag, chip_top.mag
```

**Resultado del merge (comando):**
```bash
git checkout main
git merge feature/sram feature/alu   # octopus merge
# Riku intercepta via merge driver

[miku] Analizando conflictos en .mag...
  alu.mag:  solo modificado en feature/alu → merge trivial
  sram.mag: solo modificado en feature/sram → merge trivial
  chip_top.mag: sin modificaciones → merge trivial

[miku] Merge limpio. Regenerando artefactos derivados...
  $ magic -dnull -noconsole -c "load alu; gds write gds/alu.gds; quit"
  $ magic -dnull -noconsole -c "load sram; gds write gds/sram.gds; quit"
  $ magic -dnull -noconsole -c "load chip_top; gds write gds/chip_top.gds; quit"

[miku] Verificando LVS post-merge...
  alu:      OK
  sram:     OK
  chip_top: OK

Commit de merge listo.
```

**Esto es automático.** El merge de las fuentes (.mag) es trivial para Git (archivos distintos). El valor de Riku está en la regeneración automática de GDS y la verificación de LVS post-merge.

### Caso 2 — Mismo archivo .mag, capas distintas

```
Ana modifica nand2.mag:   << metal1 >> (nuevo routing)
Carlos modifica nand2.mag: << poly >>  (ajuste de sizing)
```

**Git sin Riku:** Intenta merge de texto. Si las secciones `<< metal1 >>` y `<< poly >>` son bloques contiguos, puede haber conflictos de contexto aunque los cambios sean en capas distintas.

**Riku con merge driver:**
1. Parsea el .mag en secciones por capa.
2. Detecta que los cambios son en secciones distintas (`<< metal1 >>` vs `<< poly >>`).
3. Toma las rect modificadas de Ana para metal1 y las de Carlos para poly.
4. Reconstruye el .mag mergeado con ambos cambios.
5. Actualiza el timestamp a `now` (no conflicto de timestamp).

Esto requiere que el merge driver entienda la estructura de secciones del .mag — no es un merge de texto línea a línea sino un merge semántico por sección.

### Caso 3 — Mismo componente en .sch, propiedades distintas

```
Ana modifica M3 en amp.sch:   W=1.5 (optimización de velocidad)
Carlos modifica M3 en amp.sch: W=2.0 (optimización de corriente)
```

Este es un conflicto real de diseño, no de herramientas. Riku no puede resolverlo automáticamente y no debería intentarlo. Lo que sí puede hacer:

1. Detectar que es la misma propiedad del mismo componente.
2. Mostrar el impacto simulado de cada opción: correr NGSpice con `W=1.5` y con `W=2.0` y mostrar las métricas de `.meas` (frecuencia de corte, corriente de saturación) en el reporte de conflicto.

```
[miku] Conflicto de propiedad en schematics/amp.sch

  Componente M3 (nfet_01v8)
  Propiedad W:
    ours   (Ana):    1.5 μm   → f_T = 48 GHz, I_D = 1.2 mA
    theirs (Carlos): 2.0 μm   → f_T = 39 GHz, I_D = 1.8 mA

  Simulación corrida automáticamente con cada valor.
  Decidir:  miku merge --pick W=1.5 schematics/amp.sch
            miku merge --pick W=2.0 schematics/amp.sch
```

**Decisión de diseño:** Este es el caso donde Riku invierte trabajo extra (correr NGSpice dos veces) para mejorar la UX. El diseñador tiene información para decidir, no solo un conflicto de texto. Es consistente con la prioridad del proyecto de invertir más trabajo si mejora la UX.

### Caso 4 — GDS en LFS, .mag como fuente

Si el proyecto usa Git LFS para GDS (recomendado para archivos >50MB):

```ini
# .gitattributes
*.gds filter=lfs diff=miku-gds merge=miku-gds
```

LFS y el merge driver de Riku son compatibles. Cuando Git invoca el merge driver, ya ha descargado los tres blobs (base, ours, theirs) desde LFS. El driver los recibe como archivos locales normales — no necesita saber que vienen de LFS.

**Sin embargo:** Con LFS, el merge de GDS implica descargar tres versiones del archivo (potencialmente 3×500MB = 1.5GB). Riku debe advertir esto antes del merge si detecta que el GDS está en LFS y es grande:

```
[miku] ADVERTENCIA: El merge de gds/chip_top.gds requiere descargar ~1.4 GB de LFS.
  Alternativa recomendada: hacer merge en .mag y regenerar el GDS.
  ¿Continuar? [s/N]
```

---

## Notas de retroalimentación — flujos reales

> Estas notas provienen del research de experiencia de usuario. Identifican casos que el modelo
> de dependencias de este documento no contempla, y que pueden requerir extensión del diseño.

### Nota 1: El grafo de derivación asume flujo Xschem→Magic. Hay flujos sin este eje.

La Sección 1 modela:
```
.sch → .spice
.sch → .mag → .gds
```

En proyectos reales existen flujos donde este eje no existe:

- **Flujo digital (OpenLane):** Verilog → Yosys → OpenROAD → `.def` → Magic streaming → `.gds`. No hay `.sch` ni `.mag` como fuente de layout. El `.gds` viene de OpenROAD, no de Magic.
- **GDSFactory:** Python → `.gds`. No hay `.sch`, `.mag`, ni `.spice` primario.
- **Flujo mixto digital+analógico:** Un mismo repo puede tener Verilog (digital), `.sch`+`.mag` (analógico), y un wrapper GDS que integra ambos. El `deps.toml` debe poder representar múltiples sub-grafos de derivación por subsistema.

**Extensión propuesta para `deps.toml`:** Agregar un campo `source_type = "xschem" | "python" | "openlane" | "manual"` por celda para que el hook `post-merge` sepa qué herramienta invocar para regenerar artefactos desactualizados.

Fuente: [OpenLane issue #1420 — gate-level mixed-signal](https://github.com/The-OpenROAD-Project/OpenLane/issues/1420), [wiki.f-si.org CACE](https://wiki.f-si.org/index.php?title=CACE:_Defining_an_open-source_analog_and_mixed-signal_design_flow)

### Nota 2: `.spice` derivado vs. `.spice` fuente — la distinción no siempre es clara

La Sección 3c establece que si `amp.sp` está marcado como derivado, Riku lo regenera del `.sch` mergeado. Hay casos donde el `.spice` NO es derivado:

- Celdas estándar SKY130 en Xschem: el `.spice` del PDK es la fuente; el símbolo Xschem lo referencia con `spice_sym_def`. Si Riku intenta "regenerar" este archivo desde el `.sch`, destruiría el modelo del PDK.
- Netlists escritos a mano para exploración topológica — no tienen `.sch` fuente.
- Netlists exportados de Cadence Virtuoso — el `.sch` no existe en el repo de Riku.

**El `deps.toml` necesita distinguir:** `derived_spice` (regenerable desde `.sch`) vs. `primary_spice` (fuente, no tocar). Sin esta distinción, el merge driver puede corromper archivos de modelos del PDK.

Fuente: [xschem tutorial sky130 — spice_sym_def](https://xschem.sourceforge.io/stefan/xschem_man/tutorial_xschem_sky130.html)

### Nota 3: GDS como fuente primaria en flujos KLayout-primary

La Sección 2 asume que resolver conflictos GDS siempre implica "resolver en `.mag` y regenerar GDS". Esto no aplica cuando:

- El diseñador usa KLayout como editor primario (IHP SG13G2, GF180MCU) — no hay `.mag`.
- El layout fue generado por Python (GDSFactory) — el `.gds` es el output final; la fuente es el `.py`.

El mensaje de resolución de conflictos:
```
Archivos fuente relacionados:
  • layout/nand2.mag  → intentar merge en .mag primero, luego regenerar GDS
```
Solo debería aparecer si el proyecto tiene `.mag` declarado como fuente en `deps.toml`. Si `layout.source = "klayout"` o `layout.source = "python"`, el mensaje debe ser diferente.

Fuente: ver `ux/flujos_reales_y_variaciones.md` Sección 1 y 2.

### Nota 4: Merge de archivos `.sch` de Qucs-S o KiCad en el mismo repo

Si el repo contiene esquemáticos de múltiples herramientas (Xschem + Qucs-S, o Xschem + KiCad legacy), el merge driver `miku-sch` solo debe activarse para archivos Xschem. Para los demás, debe hacer fallback a diff de texto estándar.

La detección por header (ya contemplada en la arquitectura) resuelve esto: si el header del `.sch` no es `v {xschem version=...}`, el driver Xschem no se activa.

---

## ¿Cuándo refutar estas decisiones?

**"Advertir, no bloquear cuando el GDS está desactualizado"** deja de funcionar si:
- El equipo comete errores repetidos por ignorar la advertencia — si se convierte en ruido habitual, el costo de bloquear es menor que el costo de los bugs de diseño que se escapan. Refutar si hay evidencia de que la advertencia se está ignorando sistemáticamente.

**"Merge automático solo para celdas disjuntas"** es demasiado conservador si:
- Experimentamos con merges reales y resulta que el 90% de los conflictos son en celdas que cambiaron propiedades no-geométricas (metadata, colores de capa). Esos podrían mergearse automáticamente con menos riesgo del asumido.

**"Delegar visualización a KLayout y Xschem"** no funciona si:
- KLayout o Xschem no están instalados en el entorno del revisor del PR — que es el caso más común (el revisor es un manager o colaborador sin herramientas EDA). En ese caso sí necesitamos un renderer web propio o una imagen embebida en el PR.

**"Regenerar .spice del .sch en vez de mergearlo"** falla si:
- El flujo no tiene Xschem como fuente del .spice (ej. netlists escritos a mano, o generados por otra herramienta). En ese caso el .spice sí es fuente primaria y debe mergearse, no regenerarse.

## 7. Resumen de decisiones de diseño

| Decisión | Elección | Justificación |
|---|---|---|
| GDS como artefacto vs. fuente | Artefacto derivado | El .mag es la fuente versionable; el GDS es costoso de mergear y puede regenerarse |
| Merge automático de GDS | Solo celdas disjuntas | Es el único caso técnicamente correcto; el resto debe ser explícito |
| Bloquear merge por artefactos desactualizados | No — advertir | Bloquear es demasiado agresivo; el diseñador necesita flexibilidad |
| Timestamp en .mag | Ignorar en merge/diff | Es ruido puro; no tiene información de diseño |
| "Move all" en .sch | Detectar y advertir | No es un conflicto de diseño pero hace el merge muy difícil |
| Visualización de conflictos | Delegar a KLayout/Xschem | No reimplementar un renderer — usar lo que ya existe |
| Netlists SPICE derivados | Regenerar del .sch | El merge del derivado es propenso a errores; la fuente es canónica |
| Merge driver: local vs. global | Local por defecto | Evitar efectos secundarios en repos ajenos |
| Correr simulación en conflictos de propiedad | Sí, con `.meas` | Inversión de trabajo justificada por la mejora de UX |
| Formato de reporte de conflictos | JSON + texto en terminal | JSON para integraciones futuras (CI, web UI); texto para uso inmediato |

---

## Referencias

### Merge drivers en Git
- **gitattributes(5)**: https://git-scm.com/docs/gitattributes — sección "Defining a custom merge driver"
- **git merge-driver tutorial**: https://git-scm.com/book/en/v2/Customizing-Git-Git-Attributes
- **textconv para binarios**: https://git-scm.com/docs/gitattributes#_performing_text_diffs_of_binary_files

### KLayout API para merge de celdas
- **`LayoutDiff` con `SmartCellMapping`**: https://www.klayout.de/doc/code/class_LayoutDiff.html
- **`Layout` cell operations**: https://www.klayout.de/doc/code/class_Layout.html
- **Repo KLayout**: https://github.com/KLayout/klayout

### Manejo de dependencias entre archivos (referencia conceptual)
- **Makefile / ninja**: modelo de dependencias fuente→derivado más simple posible
- **Bazel**: https://bazel.build — modelo avanzado de dependencias con caché remoto (referencia para deps.toml)
- **DVC**: https://github.com/iterative/dvc — manejo de artefactos derivados en proyectos ML

### Herramientas de visualización de conflictos de referencia
- **KiRI**: https://github.com/leoheck/kiri — merge visual para KiCad (referencia de UX)
- **git-imerge**: https://github.com/mhagger/git-imerge — merge incremental para conflictos complejos

### Ver también
- [../herramientas/gds_klayout_magic_diff.md](../herramientas/gds_klayout_magic_diff.md) — API de KLayout para operaciones de celda
- [../herramientas/xschem_diff_y_ecosistema_eda.md](../herramientas/xschem_diff_y_ecosistema_eda.md) — formato .sch y merge de componentes
- [../herramientas/ngspice_diff_y_versionado.md](../herramientas/ngspice_diff_y_versionado.md) — .spice como derivado regenerable
- [ci_drc_lvs_regresiones.md](ci_drc_lvs_regresiones.md) — verificación post-merge con DRC/LVS
- [../arquitectura/arquitectura_cli_y_orquestacion.md](../arquitectura/arquitectura_cli_y_orquestacion.md) — cómo `miku merge` despacha a los drivers por tipo de archivo
- [../herramientas/headless_y_compatibilidad_herramientas.md](../herramientas/headless_y_compatibilidad_herramientas.md) — invocación de Magic y Xschem durante resolución de conflictos
