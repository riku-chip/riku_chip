# La línea de comandos

Todos los comandos de `riku`. Funcionan igual en la terminal y dentro del shell interactivo.

| Comando | Qué hace |
|---|---|
| `riku` | Shell interactivo |
| `riku diff [A] [B] [archivo]` | Cambios semánticos entre dos versiones (commits o el disco), de un archivo o de todos |
| `riku show COMMIT [archivo]` | Cambios de un commit respecto a su padre |
| `riku log [archivo]` | Historial con resumen semántico por commit |
| `riku status` | Cambios del working tree respecto a `HEAD` |
| `riku open [archivo]` / `riku gui [archivo]` | Visor (ver [`gui.md`](gui.md)) |
| `riku render archivo [--rev R]` | Imagen (PNG o SVG) de una versión de un archivo, sin ventana |
| `riku doctor` | Diagnóstico del entorno y formatos soportados |
| `riku completions <shell>` | Autocompletado para bash, zsh, fish, powershell o elvish |

Todos los comandos aceptan `-f json` (salida con `schema` versionado) y `--help` con ejemplos. La salida y la ayuda están en inglés por defecto; `RIKU_LANG=es` las pone en español (los ejemplos de este documento están en español). Para usar Riku desde scripts, CI o agentes de IA, ver [Scripts y agentes](#scripts-y-agentes).

Formatos: `.sch`/`.sym` (Xschem, diff semántico), `.gds`/`.oas`/`.mag` (layouts, diff geométrico; Magic con sus sub-celdas del mismo commit: [`layouts.md`](layouts.md#magic-mag)) y `.raw` (simulaciones de ngspice, diff de formas de onda: [`spice.md`](spice.md)). Un archivo que ningún módulo reconoce se lista sin diff.

---

## Shell interactivo

`riku` sin argumentos abre un shell con el directorio y el repo actuales en el prompt. Acepta todos los comandos de arriba (sin escribir `riku`) y además `ls`, `cd`, `help` y `exit`. **Tab** completa comandos, flags, carpetas, ramas, tags, commits recientes y archivos de diseño; ↑↓ recorren el historial. Las líneas se parten como en una shell: comillas para rutas con espacios y expresiones (`diff v1 v2 tb.raw --expr "gain = v(out)/v(in)"`, `cd "mi carpeta"`).

Los archivos se nombran **desde donde uno está**, como en Git: en `repo/sub`, `riku diff amp.sch` es `sub/amp.sch` (una ruta desde la raíz del repo también vale si existe). Igual en `show`, `log`, `render --rev` y el shell.

```text
riku schematics (git)> log circ_RM.sch
riku schematics (git)> diff 7d9e4a2 a3f2b1c circ_RM.sch
riku schematics (git)> cd ../layout
```

---

## `riku diff`

```bash
riku diff [A] [B] [archivo] [-f text|json|visual|png|svg] [--compact] [--ci]
          [--cosmetic-threshold-um2 X] [--no-cache] [--expr EXPR]… [-r REPO]
```

Como `git diff`: sin `B` se compara contra el **working tree** (los archivos en disco, sin commitear); sin `A`, contra `HEAD`; sin archivo, todos los que cambiaron.

| Forma | Compara |
|---|---|
| `riku diff` | disco contra `HEAD`, todos los archivos |
| `riku diff amp.sch` | ese archivo, disco contra `HEAD` |
| `riku diff main` | disco contra `main`, todos |
| `riku diff main amp.sch` | ese archivo, disco contra `main` |
| `riku diff HEAD~1 HEAD` | todo lo que cambió entre dos commits |
| `riku diff HEAD~1 HEAD amp.sch` | ese archivo entre dos commits |

Un argumento es un archivo si algún módulo conoce su extensión o si existe en el disco; si no, es un commit (hash, rama, tag, `HEAD~2`). Sin archivo, la salida de texto es la de `riku show` por archivo (los que ningún módulo reconoce se listan al final) y `-f json` usa el schema `riku-diff-set/v1` (`from`, `to` y `files`, cada uno como en `riku-show/v1`). `-f visual` sin archivo abre el visor con la lista de todos los que cambiaron (un clic abre el diff de cada uno; ver [`gui.md`](gui.md)); con el disco como `B`, el visor muestra `worktree`.

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
  "error": null,
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

Tipos de `element`: `component`, `net`, `whole` (todo el archivo, p. ej. un Move All), `cell`, `geometry` (con `layer_name` si el archivo nombra sus capas, como Magic: `"layer_name": "metal1"`), `port` (puerto de un layout de Magic: `cell` y `name`; sus `details` dicen qué cambió, p. ej. `class` de `input` a `inout`) y `signal` (simulaciones). Los `details` llevan números reales, no texto. `error` no es `null` cuando el módulo no pudo comparar el archivo (un lado roto o ilegible): entonces `changes` viene vacío y no significa "sin cambios".

**Visual** (`-f visual`): abre el visor con las vistas **Diff**, **Before** y **After** (ver [`gui.md`](gui.md)).

| Opción | Efecto |
|---|---|
| `--cosmetic-threshold-um2 X` | Umbral de área para marcar cosmético un cambio de layout |
| `--no-cache` (o `RIKU_NO_CACHE=1`) | No usar ni guardar la cache de diffs de layouts grandes (`~/.cache/riku/diff`) |
| `--tolerance TOL` | Tolerancia de formas de onda: fracción (`0.005`) o porcentaje (`0.5%`) del rango de cada señal |
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

El commit inicial se compara contra vacío (todo aparece añadido); un merge, contra su primer padre. Los archivos sin módulo se listan al final. Acepta las mismas opciones que `diff`; `-f visual` sin archivo abre la lista de lo que cambió el commit (salvo en el commit inicial, que no tiene padre).

Con `--compact`, el JSON de `diff` y `show` sale en una línea (como en `log` y `status`); sin él, indentado.

```json
{
  "schema": "riku-show/v1",
  "commit": { "oid": "077931d3…", "short_id": "077931d", "author": "…", "timestamp": 1790477193,
              "message": "…", "parents": ["fbb15d00…"] },
  "files": [ { "file": "a.sch", "status": "modified", "old_path": null, "format": "xschem",
               "error": null, "warnings": [], "changes": [ … como en riku-diff/v2 … ] } ]
}
```

---

## `riku log`

```bash
riku log [archivo] [-n N] [--detail|--full] [-f text|json [--compact]] [--paths PAT]… [--branch REF] [--graph [--ascii]]
```

Los últimos 20 commits (o `-n N`) con sus refs (rama, tag, `HEAD`) y, por archivo con módulo, un resumen de lo que cambió respecto al primer padre. Los merges se marcan `[merge]` sin diff por archivo. `--detail` agrega una entrada por componente/net; `--full`, el reporte completo del módulo. `--paths` (o el archivo) filtra por glob (se puede repetir): `-n` cuenta solo los commits que tocan esos archivos respecto a su primer padre, como `git log -n N -- archivo`; un merge que no los toca tampoco se muestra.

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
  "schema": "riku-log/v2",
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
riku status [--detail|--full] [-f text|json [--compact]] [--paths PAT]… [--include-unknown] [--ci]
```

Cada archivo modificado respecto a `HEAD` se clasifica como `semantic` (cambios funcionales), `cosmetic` (solo reposicionamiento), `unchanged` (el módulo no ve cambios), `unknown` (sin módulo; se listan con `--include-unknown`) o `error` (no se pudo comparar, por ejemplo un archivo roto o de más de 50 MB; el mensaje va en `errors`, y `status` termina con 2). Los avisos del módulo (una sub-celda de Magic que no aparece…) van en `warnings` de cada archivo; en `log`, un archivo sin cambios pero con avisos no se oculta.

```json
{
  "schema": "riku-status/v2",
  "branch": { "name": "master", "head_oid": "077931d3…", "head_short": "077931d",
              "upstream": null, "ahead": 0, "behind": 0 },
  "files": [ { "path": "a.sch", "format": "xschem", "category": "semantic", "counts": { "nets_added": 1 } } ],
  "warnings": []
}
```

Con `--detail`, cada archivo trae `details`: qué cambió, con el elemento tipado como en `riku diff -f json`, `renamed_from` en un renombre y los parámetros que cambiaron (sin la ubicación). Con `--full`, además `full_report`: el reporte completo del módulo, con los mismos cambios tipados que un archivo de `riku diff -f json`. Vale igual para cada archivo de `riku log --json`.

```json
"details": [
  { "kind": "component_modified", "element": { "type": "component", "name": "M3" },
    "params": { "W": "4u → 8u" } },
  { "kind": "component_renamed", "element": { "type": "component", "name": "vin_diff" },
    "renamed_from": "vin" }
]
```

---

## Códigos de salida y CI

`riku status` siempre (acepta `--ci` por uniformidad), y `riku diff` / `riku show` con `--ci`, terminan con:

| Código | Significado |
|---|---|
| 0 | Sin cambios, o solo cosméticos |
| 1 | Hay cambios funcionales |
| 2 | Error (commit o archivo inexistente, repo inválido…, o algún archivo que no se pudo comparar, como un GDS roto: lo demás se imprime igual) |

Con `-f json`, un error también sale como JSON en stdout: `{"schema": "riku-error/v1", "error": "commit no encontrado: v9"}`.

Sin `--ci`, `diff` y `show` terminan en 0 (o 1 si hay error).

```yaml
# GitHub Actions: avisar si un commit cambia el circuito
- run: riku show HEAD --ci || echo "::warning::el commit cambia el circuito"
```

**Estabilidad del JSON:** cada salida lleva su `schema`. Un cambio incompatible sube la versión (`v2` → `v3`); un campo nuevo opcional no.

---

## `riku doctor`

Informa el repo Git, el `.xschemrc`, `$PDK_ROOT`/`$PDK`/`$TOOLS` (o los PDKs instalados que se detectarán por símbolos, ver [`xschem.md`](xschem.md)), las librerías `.mag` de los PDK (para layouts de Magic) y los módulos de formato compilados. Con `-f json` (schema `riku-doctor/v1`), `modules` lista cada formato con su `name`, `format`, `extensions` y si está `available`: así un script sabe qué archivos puede comparar este `riku`.

## Configuración del proyecto (`.riku.toml`)

Un `.riku.toml` en la raíz del repo fija las opciones de diff del proyecto, para que todos (personas, CI y agentes) comparen igual sin repetir flags. Lo usan `diff`, `show`, `log` y `status`; los flags de la línea de comandos ganan sobre el archivo, y las expresiones de `--expr` se suman a las del archivo.

```toml
[layout]
cosmetic_threshold_um2 = 0.01        # µm²: un cambio de menos área es cosmético

[waveform]
tolerance = "0.5%"                   # o 0.005: fracción del rango de cada señal (por defecto 0,1 %)
expressions = [                      # señales calculadas que se comparan siempre (ver spice.md)
  "gain = v(out)/v(in)",
  "tran: vpk = max(v(out))",
]
```

Una clave que no existe es un error (con la clave más parecida), no se ignora en silencio. `riku doctor` dice si hay archivo y si se puede leer.

## Imágenes: `riku render` y `-f png|svg`

Riku dibuja una imagen sin abrir ventanas (no hace falta pantalla ni GPU): sirve en CI, por SSH y para agentes de IA, que pueden mirarla. Se ve igual que el visor: esquemáticos y layouts (`.sch`, `.gds`, `.oas`, `.mag`) con los colores del diff, y formas de onda (`.raw`) con A punteada, B continua y el error B − A.

```bash
riku diff HEAD~1 HEAD amp.sch -f png         # imagen del diff; imprime la ruta
riku diff amp.sch -f svg -o cambios.svg      # el disco contra HEAD, en SVG
riku show HEAD top.gds -f png --cell INV     # lo que cambió un commit, en una celda
riku render tb.raw --rev v1 --expr "gain = v(out)/v(in)"
riku render amp.sch --theme dark --size 2400x1500
```

- **Salida:** imprime la ruta del archivo. Sin `-o`, va a la carpeta temporal de riku (`/tmp/riku/<archivo>-<versiones>.png`).
- **Opciones:** `-o ARCHIVO`, `--size ANCHOxALTO` (por defecto `1600x1000`), `--theme light|dark` (por defecto `light`), `--cell CELDA` (layouts) y `--expr` (formas de onda; también las de `.riku.toml`).
- **`riku render`** dibuja una sola versión: el archivo en disco (con la ruta tal como se escribe, sin necesitar un repo) o la de un commit con `--rev`.
- Las formas de onda muestran las señales que más cambiaron (o las primeras, sin diff) y las expresiones que den una curva.

## `riku completions`

```bash
riku completions bash > ~/.local/share/bash-completion/completions/riku
riku completions zsh  > "${fpath[1]}/_riku"
riku completions fish > ~/.config/fish/completions/riku.fish
```

## Scripts y agentes

Riku está pensado para usarse también sin persona delante (CI, scripts, agentes de IA):

- **Salida:** `-f json` en todos los comandos. Cada JSON trae `schema` (`riku-diff/v2`, `riku-diff-set/v1`, `riku-show/v1`, `riku-log/v2`, `riku-status/v2`, `riku-doctor/v1`, `riku-error/v1`); un cambio incompatible sube la versión. Todos describen los cambios de la misma forma tipada (la de `riku diff -f json`).
- **Cambio incompatible (2026-09):** se quitó la forma anterior de los cambios (texto con convenciones como `"cell:INV"` o `"TOP:L1/0:INV"` y mapas `before`/`after` de strings). `riku diff -f json-v1` ya no existe, y `riku-status`/`riku-log` pasaron a v2: `details[].element` es el elemento tipado (con `renamed_from` en un renombre) y `full_report` lleva los cambios tipados. `path`, `category`, `counts`, `errors` y `warnings` no cambiaron.
- **Resultado:** el código de salida dice si hubo cambios funcionales (ver [Códigos de salida](#códigos-de-salida-y-ci)); con `-f json` los errores también son JSON.
- **Descubrir:** `riku doctor -f json` lista los formatos soportados; `riku <comando> --help` trae ejemplos.
- **Ver:** `riku diff A B archivo -f png` escribe una imagen del diff e imprime su ruta, para mirarla (ver [Imágenes](#imágenes-riku-render-y--f-pngsvg)).
- **Sin interacción:** `riku` sin comando abre el shell solo si hay una terminal; desde un script imprime la ayuda y termina. `-f visual` abre una ventana: no usarlo en automatizaciones.
- **Tuberías:** cortar la salida (`riku log | head`) termina sin error, como `git`.

Flujo típico de un agente que editó un diseño:

```bash
riku status -f json            # qué archivos cambiaron (código 1 si hay cambios funcionales)
riku diff -f json              # el detalle, disco contra HEAD
riku diff HEAD~1 HEAD -f json  # qué cambió el último commit
riku diff amp.sch -f png       # y cómo se ve (imprime la ruta de la imagen)
```

## Variables de entorno

| Variable | Efecto |
|---|---|
| `PDK_ROOT`, `PDK`, `TOOLS` | Símbolos de Xschem ([`xschem.md`](xschem.md)) |
| `RIKU_LANG` | Idioma de la CLI y del visor: `en` (por defecto) o `es`. En el visor también se elige en Settings → Language |
| `RIKU_NO_CACHE=1` | Sin cache de diffs de layouts |
| `RIKU_MAG_PATH=dir1:dir2` | Directorios extra donde buscar las celdas `.mag` que usa un layout de Magic (antes que el PDK) |
| `RIKU_MAG_LAMBDA=µm` | Lambda de Magic, si el `.tech` de la tecnología del `.mag` no está en `$PDK_ROOT` |
| `RIKU_JOBS=N` | Hilos para el trabajo pesado (igual que `--jobs N`, que vale en cualquier comando); por defecto, los núcleos disponibles. `RIKU_JOBS=1` deja todo en un hilo |
| `RIKU_PROFILE=1` | El visor imprime el tiempo de cada cuadro ([`gui.md`](gui.md)); el diff de layouts, el tiempo de cada capa que difiere (polígonos propios, comunes cercanos y Clipper) |
| `RIKU_LOD_PX` | Lado de los texels del nivel de detalle del visor, en píxeles (1 por defecto) |
