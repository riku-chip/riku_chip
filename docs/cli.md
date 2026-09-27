# La línea de comandos

Todos los comandos de `riku`. Funcionan igual en la terminal y dentro del shell interactivo.

| Comando | Qué hace |
|---|---|
| `riku` | Shell interactivo |
| `riku diff A B archivo` | Cambios semánticos de un archivo entre dos commits |
| `riku show COMMIT [archivo]` | Cambios de un commit respecto a su padre |
| `riku log [archivo]` | Historial con resumen semántico por commit |
| `riku status` | Cambios del working tree respecto a `HEAD` |
| `riku open [archivo]` / `riku gui [archivo]` | Visor (ver [`gui.md`](gui.md)) |
| `riku doctor` | Diagnóstico del entorno |

Formatos: `.sch`/`.sym` (Xschem, diff semántico), `.gds`/`.oas` (layouts, diff geométrico) y `.raw` (simulaciones de ngspice, diff de formas de onda: [`spice.md`](spice.md)). Un archivo que ningún módulo reconoce se lista sin diff.

---

## Shell interactivo

`riku` sin argumentos abre un shell con el directorio y el repo actuales en el prompt. Acepta todos los comandos de arriba (sin escribir `riku`) y además `ls`, `cd`, `help` y `exit`. **Tab** completa comandos, flags, carpetas, ramas, tags, commits recientes y archivos de diseño; ↑↓ recorren el historial.

```text
riku schematics (git)> log circ_RM.sch
riku schematics (git)> diff 7d9e4a2 a3f2b1c circ_RM.sch
riku schematics (git)> cd ../layout
```

---

## `riku diff`

```bash
riku diff <commit_a> <commit_b> <archivo> [-f text|json|json-v1|visual] [--ci]
          [--cosmetic-threshold-um2 X] [--no-cache] [--expr EXPR]… [-r REPO]
```

**Esquemático, texto:**

```text
Archivo : design/op_amp.sch
Cambios : 3

  + M5
      symbol: sky130_fd_pr/nfet_01v8_lvt.sym
  - R2
  ~ C1
      value: 1p → 2p
```

Marcas: `+` añadido, `-` eliminado, `~` modificado, `r` renombrado. Un reordenamiento visual (Move All) cuenta como **cosmético**. Un archivo nuevo o borrado lista todo como añadido o eliminado.

**Layout, texto:**

```text
  ~ sky130_fd_sc_hd__inv_1:L66/20
      +1 polys / +0.125 µm²
      -3 polys / -0.125 µm²
      bbox: (0.320, 0.105) → (0.800, 2.615) µm
  + TOP:L1/0:INV
      origen: TOP → INV (en 6 instancias)
  r cell:INV → INV_X1
```

Cada cambio es `celda:Lcapa/datatype`; si nace en una sub-celda se agrega su nombre y el bbox queda en coordenadas de la celda que la instancia. Un cambio con área total bajo el umbral (0,01 µm² por defecto, debajo del piso DRC de SKY130/GF180) es cosmético. Detalles del diff de layouts en [`layouts.md`](layouts.md).

**JSON** (`-f json`, schema `riku-diff/v2`):

```json
{
  "schema": "riku-diff/v2",
  "file": "design/op_amp.sch",
  "format": "xschem",
  "warnings": [],
  "changes": [
    { "kind": "added",    "element": { "type": "component", "name": "M5" }, "cosmetic": false },
    { "kind": "renamed",  "element": { "type": "component", "name": "vin_diff" }, "renamed_from": "vin", "cosmetic": false },
    { "kind": "modified", "element": { "type": "component", "name": "C1" }, "cosmetic": false,
      "details": [ { "key": "value", "before": "1p", "after": "2p" } ] },
    { "kind": "added",    "element": { "type": "net", "name": "vbias" }, "cosmetic": false },
    { "kind": "added",    "element": { "type": "geometry", "cell": "TOP", "layer": 68, "datatype": 20,
                                        "via": { "path": ["INV"], "instances": 2 } },
      "cosmetic": false, "location": { "min_x": 12.0, "min_y": 10.0, "max_x": 13.0, "max_y": 11.0 },
      "details": [ { "key": "added_area_um2", "after": 0.25 } ] }
  ]
}
```

Tipos de `element`: `component`, `net`, `whole` (todo el archivo, p. ej. un Move All), `cell` y `geometry`. Los `details` llevan números reales, no texto. `-f json-v1` da la forma anterior (`components`, `nets_added`, `nets_removed`, `is_move_all`), idéntica byte a byte, y se mantiene durante una versión.

**Visual** (`-f visual`): abre el visor con las vistas **Diff**, **Before** y **After** (ver [`gui.md`](gui.md)).

| Opción | Efecto |
|---|---|
| `--cosmetic-threshold-um2 X` | Umbral de área para marcar cosmético un cambio de layout |
| `--no-cache` (o `RIKU_NO_CACHE=1`) | No usar ni guardar la cache de diffs de layouts grandes (`~/.cache/riku/diff`) |
| `--expr EXPR` | Señal calculada a comparar en un `.raw` (repetible): `--expr "gain = v(out)/v(in)"`. Ver [`spice.md`](spice.md#expresiones) |
| `--ci` | Códigos de salida de CI (abajo) |
| `-r REPO` | Repositorio (por defecto, el directorio actual) |

---

## `riku show`

Como `git show`, pero semántico: los cambios de un commit respecto a su primer padre, archivo por archivo.

```bash
riku show HEAD                                 # todos los archivos del commit
riku show abc123 design/op_amp.sch             # uno (= riku diff abc123~1 abc123 …)
riku show abc123 chip.gds -f json              # schema riku-show/v1
riku show abc123 design/op_amp.sch -f visual   # el diff de ese commit en el visor
```

El commit inicial se compara contra vacío (todo aparece añadido); un merge, contra su primer padre. Los archivos sin módulo se listan al final. Acepta las mismas opciones que `diff` salvo `json-v1`; `-f visual` necesita el archivo.

```json
{
  "schema": "riku-show/v1",
  "commit": { "oid": "077931d3…", "short_id": "077931d", "author": "…", "timestamp": 1790477193,
              "message": "…", "parents": ["fbb15d00…"] },
  "files": [ { "file": "a.sch", "status": "modified", "old_path": null, "format": "xschem",
               "warnings": [], "changes": [ … como en riku-diff/v2 … ] } ]
}
```

---

## `riku log`

```bash
riku log [archivo] [-n N] [--detail|--full] [--json [--compact]] [--paths PAT]… [--branch REF] [--graph [--ascii]]
```

Los últimos 20 commits (o `-n N`) con sus refs (rama, tag, `HEAD`) y, por archivo con módulo, un resumen de lo que cambió respecto al primer padre. Los merges se marcan `[merge]` sin diff por archivo. `--detail` agrega una entrada por componente/net; `--full`, el reporte completo del módulo. `--paths` filtra por glob (se puede repetir).

**`--graph`** dibuja las ramas y los merges a la izquierda, en orden topológico (cada commit antes que sus padres), como `git log --graph`:

```text
○   f510bb8 [merge]  Merge pull request #231 from mabrains/main
├─╮
│ ○   ac3c577 [merge]  Merge pull request #1 from mabrains/Add_polygon_perimeter_method
│ ├─╮
│ │ ● f964d1e  Undo changes to setup.py
│ │ ● 2e0a69b  Add perimeter function and python interface
├─┴─╯
● c23297b  Release 0.9.49
```

- `●` commit, `○` merge, `┆` una rama que sigue más allá de `-n`. Un color por rama si la salida es una terminal (sin colores si se redirige, con `NO_COLOR`; `CLICOLOR_FORCE=1` los fuerza).
- `--ascii` (o `RIKU_ASCII=1`) usa `* | / \ -` para terminales o fuentes sin Unicode.
- Con `--paths`, los commits que no tocan esos archivos no se muestran y sus hijos se conectan al ancestro visible más cercano.
- Con `--json`, cada commit lleva `graph`: `column`, `lane` (la rama, para el color), `passing` (otras ramas que pasan por la fila), `edges` (`[columna aquí, columna en la fila siguiente, rama]`) y `truncated`.

```json
{
  "schema": "riku-log/v1",
  "commits": [
    { "oid": "077931d3…", "short_id": "077931d", "message": "d", "author": "t", "timestamp": 1790477193,
      "parents": ["fbb15d00…"], "refs": ["HEAD", "master"], "is_merge": false,
      "files": [ { "path": "a.sch", "format": "xschem", "category": "semantic",
                   "counts": { "components_added": 2, "nets_removed": 1 } } ] }
  ]
}
```

---

## `riku status`

```bash
riku status [--detail|--full] [--json [--compact]] [--paths PAT]… [--include-unknown]
```

Cada archivo modificado respecto a `HEAD` se clasifica como `semantic` (cambios funcionales), `cosmetic` (solo reposicionamiento), `unchanged` (el módulo no ve cambios) o `unknown` (sin módulo; se listan con `--include-unknown`).

```json
{
  "schema": "riku-status/v1",
  "branch": { "name": "master", "head_oid": "077931d3…", "head_short": "077931d",
              "upstream": null, "ahead": 0, "behind": 0 },
  "files": [ { "path": "a.sch", "format": "xschem", "category": "semantic", "counts": { "nets_added": 1 } } ],
  "warnings": []
}
```

---

## Códigos de salida y CI

`riku status` siempre, y `riku diff` / `riku show` con `--ci`, terminan con:

| Código | Significado |
|---|---|
| 0 | Sin cambios, o solo cosméticos |
| 1 | Hay cambios funcionales |
| 2 | Error (commit o archivo inexistente, repo inválido…) |

Sin `--ci`, `diff` y `show` terminan en 0 (o 1 si hay error).

```yaml
# GitHub Actions: avisar si un commit cambia el circuito
- run: riku show HEAD --ci || echo "::warning::el commit cambia el circuito"
```

**Estabilidad del JSON:** cada salida lleva su `schema`. Un cambio incompatible sube la versión (`v2` → `v3`); un campo nuevo opcional no.

---

## `riku doctor`

Informa el repo Git, el `.xschemrc`, `$PDK_ROOT`/`$PDK`/`$TOOLS` (o los PDKs instalados que se detectarán por símbolos, ver [`xschem.md`](xschem.md)) y los módulos de formato compilados.

## Variables de entorno

| Variable | Efecto |
|---|---|
| `PDK_ROOT`, `PDK`, `TOOLS` | Símbolos de Xschem ([`xschem.md`](xschem.md)) |
| `RIKU_NO_CACHE=1` | Sin cache de diffs de layouts |
| `RIKU_JOBS=N` | Hilos para el trabajo pesado (igual que `--jobs N`, que vale en cualquier comando); por defecto, los núcleos disponibles. `RIKU_JOBS=1` deja todo en un hilo |
| `RIKU_PROFILE=1` | El visor imprime el tiempo de cada cuadro ([`gui.md`](gui.md)); el diff de layouts, el tiempo de cada capa que difiere (polígonos propios, comunes cercanos y Clipper) |
| `RIKU_LOD_PX` | Lado de los texels del nivel de detalle del visor, en píxeles (1 por defecto) |
