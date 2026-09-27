# Fase 7: el grafo del historial

Estado (2026-09-27): **diseño**. Resumen en [`../roadmap.md`](../roadmap.md).

Un solo motor que ubica los commits en carriles y dos formas de dibujarlo: `riku log --graph` en la terminal y un panel **Historial** en el visor, como el Git Graph de VS Code pero con el resumen semántico de cada commit (qué componentes, capas o señales cambiaron) y el diff visual a un clic.

---

## 1. Qué hay hoy (revisado contra el código)

| Qué | Dónde | Consecuencia para el grafo |
|---|---|---|
| Los commits se leen con sus padres (`CommitWithParents`) | `core/git/commit_log.rs::get_commits_with_options` | El grafo (el DAG) ya está; falta ubicarlo en carriles |
| Orden **solo por fecha** (`Sort::TIME`) | mismo archivo | Un hijo puede quedar después de su padre si los relojes no coinciden. Para dibujar hace falta **orden topológico** (`TOPOLOGICAL \| TIME`), como hace `git log --graph` |
| Los merges no llevan resumen por archivo | `log/walk.rs` (v1) | Se dibujan igual; el resumen de un merge queda como hoy |
| `--paths` omite los commits que no tocan esos archivos | `log/walk.rs` | Un commit omitido deja a sus hijos sin padre visible: hay que **reescribir los padres** al ancestro visible más cercano (lo que Git llama simplificación del historial) |
| `-n` corta el historial | `LogQuery::limit` | Los padres fuera de la ventana no se ven: la rama sigue hasta el borde y termina con una marca |
| Análisis por commit en paralelo | `core/analysis/parallel.rs` (6.6) | El panel del visor calcula los resúmenes sin trabar la interfaz |
| `load_backend_diff(repo, a, b, archivo)` | `gui/app.rs` | Abrir el diff visual de un archivo del historial ya existe |
| Textos del visor en inglés y español (`tr!`, `locales/gui.yml`) | `gui/i18n.rs` | Los textos del panel van por ahí |
| `anstream`/`anstyle` (con `clap`) y `unicode-width` | `Cargo.lock` | Colores y anchos sin dependencias nuevas |

Historias reales para probar: el repo de Riku (lineal en los últimos cientos de commits, 2 merges) y el submódulo `external/gdstk` (722 commits, 35 merges).

---

## 2. Motor: `core/analysis/graph.rs` (sin interfaz)

Entrada: los commits en orden topológico, cada uno con sus padres. Salida: una fila por commit con su columna y los tramos que la unen con la fila siguiente. No sabe de colores de terminal ni de egui: los dos dibujantes leen lo mismo.

```rust
pub struct GraphRow {
    /// Columna del nodo (0 = izquierda).
    pub column: usize,
    /// Carril del nodo: identidad estable de la rama visual (para su color).
    pub lane: usize,
    /// Tramos entre esta fila y la siguiente: (columna arriba, columna abajo,
    /// carril). Un tramo recto (c, c) es una rama que pasa; uno oblicuo es
    /// una rama que se abre, se une o se corre de columna.
    pub edges: Vec<(usize, usize, usize)>,
    /// Padres fuera de la ventana (`-n`) o no cargados: su rama termina acá.
    pub truncated: bool,
}

pub fn layout(commits: &[(String, Vec<String>)]) -> Vec<GraphRow>;
```

**Algoritmo** (asignación de carriles, O(commits × carriles activos)):

1. Se mantiene la lista de columnas activas; cada una espera un commit (el próximo padre de esa rama).
2. Para cada commit: las columnas que lo esperan se juntan en él. Su columna es la de más a la izquierda; las otras terminan acá con un tramo oblicuo hacia ella (el cierre de un merge visto desde abajo).
3. El **primer padre** hereda la columna y el carril del commit: la línea principal de una rama queda recta y de un solo color.
4. Cada padre extra (merge) reusa una columna que ya lo espere o toma la primera libre, con un carril nuevo: tramo oblicuo hacia ella.
5. Las columnas que quedan libres a la derecha se compactan corriéndolas un lugar, con tramos oblicuos, para que el grafo no se ensanche sin fin.
6. Un padre que no está en la lista (fuera de `-n`) no ocupa columna: el commit se marca `truncated`.

**Con `--paths`:** antes del algoritmo, los padres de cada commit visible se reescriben al ancestro visible más cercano (recorriendo los omitidos dentro de la ventana cargada), y los duplicados se quitan. Así el grafo muestra cómo se relacionan los commits que tocaron esos archivos.

**Tests** (propiedades, sobre DAG sintéticos y sobre la historia de gdstk):
- cada commit aparece una vez; filas en el orden de entrada;
- cada tramo que sale de un commit llega, fila a fila, a la fila de su padre (o al borde si está truncado);
- el primer padre de un commit no merge está en la misma columna;
- nunca dos tramos en la misma columna entre las mismas filas;
- casos: lineal, rama y merge, merge de merge, octopus (3+ padres), ramas cruzadas (*criss-cross*), historia con dos raíces, ventana `-n` que corta en medio de una rama.

---

## 3. `riku log --graph` (terminal)

```text
● a3f9c1 (HEAD, main)  ajuste de W en M5
│   carlos · 2026-09-27 14:32
│   op_amp.sch  1 componente modificado
●─╮ 7be210  Merge rama layout
│ ● 51c0de (layout)  pad nuevo en metal1
│ │   chip.gds  +2 polys en 8/0
● │ e02a4b  valor de R2
├─╯
● 9d1f00  inicial
```

- **Caracteres:** Unicode de dibujo (`● ○ │ ─ ╮ ╯ ╭ ╰ ├ ┤ ┆`); `○` para un merge, `┆` al final de una rama truncada por `-n`. Las líneas de detalle (autor, archivos) llevan la parte vertical del grafo para que las ramas sigan sin cortarse.
- **Tramos oblicuos:** cuando algún tramo cambia de columna, se agrega una línea intermedia solo de grafo (`├─╯`, `│ ╰─╮`), como hace Git.
- **Colores:** uno por carril (paleta de 6, rotativa), con `anstream`: se apagan solos si la salida no es una terminal o con `NO_COLOR`. `--color always|never|auto`.
- **ASCII:** `--ascii` (o `RIKU_ASCII=1`) usa `* | / \ -` para terminales o fuentes sin Unicode.
- **Anchos:** con `unicode-width`, así los acentos y el grafo no desalinean el texto.
- **Solo con `--graph` cambia el orden a topológico;** el `log` de siempre queda idéntico (regresión).
- **JSON:** `riku log --json --graph` agrega a cada commit `graph: {column, lane, edges, truncated}` (campo nuevo opcional: el schema sigue `riku-log/v1`).

**Tests:** salidas de texto esperadas para los DAG sintéticos (con y sin color, Unicode y ASCII); el `log` sin `--graph` igual a antes (`riku_phase1_regress.sh`).

---

## 4. Panel **Historial** en el visor

```
┌ History ───────────────────────────────────────┬ 51c0de · pad nuevo en metal1 ──┐
│ ●━━ main   ajuste de W en M5            2 h    │ chip.gds                       │
│ ○━┓        Merge rama layout             3 h   │   + 8/0 · 2 polys · 4,0 µm²    │
│ ┃ ● layout pad nuevo en metal1    ◀     ayer   │     en sg13g2_IOPadIn          │
│ ● ┃        valor de R2                  ayer   │   [Diff] [Before] [After]      │
│ ●━┛        inicial                      lun    │                                │
└────────────────────────────────────────────────┴────────────────────────────────┘
```

- **Dónde:** una pestaña **History** junto a **Project** en el panel izquierdo (el árbol de archivos y el historial comparten lugar); atajo **H**. El repo es el del proyecto abierto (`GitService::open` sobre la carpeta, o `--repo`).
- **Dibujo** (`gui/history_view.rs`, egui `Painter`): cada carril con su color (la paleta del tema, clara u oscura); los tramos oblicuos como curvas Bézier cúbicas (`CubicBezierShape`); los nodos como círculos (hueco para un merge); refs como chips redondeados (`HEAD`, ramas, tags). Filas de alto fijo con `ScrollArea::show_rows`: solo se dibuja lo visible, así un historial de miles de commits no pesa.
- **Datos:** primero la lista y el grafo (instantáneo: solo Git); los resúmenes semánticos se calculan en segundo plano con el análisis en paralelo de 6.6 y aparecen a medida que llegan. Se cargan 200 commits; **Load more** trae los siguientes.
- **Clic en un commit:** el panel derecho muestra sus archivos con el resumen (`analyze_show`); cada archivo abre el diff visual contra el primer padre con **Diff / Before / After** (`load_backend_diff`, que ya sirve para `.sch`, `.gds`/`.oas` y `.raw`).
- **Filtro:** un campo para `--paths` (glob) y un botón **Only this file** cuando hay un archivo abierto: el grafo se simplifica como en la terminal.
- **Textos** en `locales/gui.yml` (inglés y español).

**Tests:** el modelo de la vista (qué filas, qué seleccionada, qué pide abrir) sin ventana; capturas sobre Xvfb con `xt.py` en el repo de Riku y en una historia con merges.

---

## 5. Qué no cambia

- `riku log` sin `--graph`: mismo orden y misma salida.
- `riku-kernel`, los módulos de formato y `viewer-core`: el grafo es del núcleo del ejecutable (`core/analysis`) y del visor.
- Sin dependencias nuevas (egui, `anstream`/`anstyle` y `unicode-width` ya están en el árbol).

---

## 6. Orden, esfuerzo y criterio

| Paso | Qué | Dónde | Esfuerzo | Listo cuando |
|---|---|---|---|---|
| 7.1 | Motor de carriles, orden topológico y reescritura de padres con `--paths` | `core/analysis/graph.rs`, `core/git/commit_log.rs` | S | Tests de propiedades verdes en los DAG sintéticos y en los 722 commits de gdstk |
| 7.2 | `riku log --graph` (Unicode, ASCII, colores, JSON) | `cli/format/log_text.rs`, `cli/mod.rs` | S | Salidas esperadas; `log` sin `--graph` idéntico |
| 7.3 | Panel **Historial** en el visor | `gui/history_view.rs`, `gui/app.rs`, `locales/gui.yml` | M | Grafo con curvas, clic → cambios → diff visual; capturas en claro y oscuro |
| 7.4 | (Opcional) TUI con `ratatui` sobre el mismo motor | `cli/tui.rs` | M | Si hace falta usar Riku sin escritorio |

---

## 7. Riesgos

| Riesgo | Mitigación |
|---|---|
| Historias con muchas ramas abiertas a la vez: el grafo se ensancha | Compactar columnas libres (paso 5); en la terminal, más de 12 carriles se dibujan con `…` y se avisa |
| Orden topológico distinto del de Git en empates | Mismos criterios que Git (`TOPOLOGICAL \| TIME`); los tests comparan la topología (quién es padre de quién), no el dibujo exacto |
| El panel calcula resúmenes de muchos commits con layouts | Se calculan en segundo plano, en tandas por memoria (6.6), solo para los commits cargados |
