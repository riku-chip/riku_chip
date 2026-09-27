# Fase 7: el grafo del historial

Estado (2026-09-27): **hecha** (7.1, 7.2 y 7.3; la TUI de 7.4 queda opcional). Resumen en [`../roadmap.md`](../roadmap.md).

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

## 4. Panel **History** en el visor (7.3) — diseño revisado contra el código, 2026-09-27

### 4.1 Dónde va: un panel **abajo**, a todo el ancho

La columna izquierda del visor mide ~200 px y ya cambia de contenido según el modo (árbol del proyecto, o las vistas Diff/Before/After en un diff). Una fila de historial necesita el ancho completo: grafo, id, ramas, mensaje, autor, fecha y resumen. Por eso el historial es un **panel inferior redimensionable** (`egui::Panel::bottom`), como la terminal de VS Code: el lienzo sigue arriba, y al abrir un archivo desde el historial su diff aparece ahí mismo, sin perder la lista.

```
┌ Riku ─ [Fit] [Reload] [Labels] [History ▾] ────────────────────────────────────────────┐
│ Project │                                                               │ Details     │
│ …       │                  lienzo: el diff del archivo elegido          │ …           │
├─────────┴───────────────────────────────────────────────────────────────┴─────────────┤
│ History  [filter: *.gds      ] [Only this file]                  main · 240 commits    │
│ ●━━  a3f9c1  main  HEAD   ajuste de W en M5         carlos   2 h  ▣ 1 sch            ││ a3f9c1 · ajuste de W en M5
│ ○━┓  7be210            Merge rama layout             ana      3 h                     ││ carlos · 2026-09-27 14:32
│ ┃ ●  51c0de  layout      pad nuevo en metal1   ◀     ana     ayer ▣ 1 gds            ││ ─────────────────────────
│ ● ┃  e02a4b            valor de R2                   carlos  ayer ▣ 1 sch            ││ ▸ op_amp.sch   1 modificado
│ ●━┛  9d1f00            inicial                       carlos   lun                     ││   [Diff] [Before] [After]
└───────────────────────────────────────────────────────────────────────────────────────┘
```

- **Abrir y cerrar:** botón **History** en la barra y tecla **H**; alto por defecto 260 px, se recuerda. Solo con un proyecto dentro de un repo Git (si no, el botón explica por qué está deshabilitado).
- **Dos columnas dentro del panel:** a la izquierda la lista con el grafo; a la derecha, el commit seleccionado.

### 4.2 Datos: en dos fases, sin trabar la interfaz

| Fase | Qué | Costo medido | Cómo |
|---|---|---|---|
| 1. Grafo | commits, padres, refs y carriles | ~0,01 s cada 200 commits en disco local | `walk_with_summary` con una opción nueva `summaries: false` (salta la segunda pasada de 6.6) + `graph::layout` |
| 2. Resúmenes | qué cambió en cada commit (sch, gds, raw) | 0,05 s (repo de Riku) a segundos (historial de layouts), en paralelo | la segunda pasada de 6.6 sobre los mismos commits |
| Al seleccionar | todos los archivos del commit, también merges y archivos sin módulo | lo de `riku show` | `analyze_show`, en cache por commit |

- Cada fase corre en un hilo aparte (`poll_promise::Promise::spawn_thread`, ya en las dependencias); adentro, `rayon` reparte los commits como en 6.6. La interfaz dibuja lo que ya llegó: primero el grafo, después aparecen los resúmenes.
- **Páginas de 200 commits;** **Load more** trae los siguientes y vuelve a ubicar los carriles de toda la lista (secuencial y lineal: 1 000 commits en 0,08 s).
- **Filtro:** un glob (`*.gds`) o **Only this file** con el archivo abierto; recarga con `--paths` y el grafo simplificado (7.1).

### 4.3 Dibujo (`gui/history_view.rs`)

- Filas de alto fijo (22 px) con `ScrollArea::show_rows`: solo se dibujan las visibles, así miles de commits no pesan.
- **Grafo:** columnas de 14 px; nodos como círculos (huecos en un merge); tramos verticales como líneas; tramos oblicuos como **curvas Bézier cúbicas** (`epaint::CubicBezierShape`, en egui 0.34), que salen y llegan verticales para que las ramas se vean suaves, como en VS Code.
- **Colores por carril:** una paleta de 8 para claro y 8 para oscuro, con contraste verificado contra el fondo del panel (`theme::contrast`).
- **Fila:** id corto (monoespaciado), chips redondeados de refs (`HEAD` resaltado, ramas, tags), primera línea del mensaje (recortada), autor, fecha relativa ("2 h", "ayer"), y un resumen por formato a la derecha (`▣ 2 sch · 1 gds`, en el color de la categoría: semántico o cosmético).
- La **geometría** (dónde va cada nodo y cada curva) sale de una función pura `row_shapes(row, x0, y0) -> Vec<Primitive>`, testeable sin ventana; el `Painter` solo la dibuja.

### 4.4 Interacción

| Acción | Efecto |
|---|---|
| Clic en un commit | Lo selecciona; a la derecha, su mensaje completo, autor, fecha, padres y archivos (con su resumen) |
| Clic en un archivo del commit | Abre su diff en el lienzo contra el primer padre (`load_backend_diff`; sirve para `.sch`, `.gds`/`.oas` y `.raw`) con **Diff / Before / After**. En el commit inicial se compara contra vacío |
| ↑ / ↓ con el panel enfocado | Cambia de commit |
| Doble clic en un commit | Abre su primer archivo con cambios |
| Pasar el mouse por un chip de rama | La rama se resalta en el grafo |

### 4.5 Arquitectura

- **Microkernel:** el panel usa solo el núcleo (`log`, `show`, `graph`) y la carga de diffs que ya existe (por el registro de módulos). No sabe de formatos: un formato nuevo (Magic) aparece en el historial sin tocar el panel.
- **Estado separado del dibujo:** `HistoryModel` (commits, selección, páginas, filtro, pedidos pendientes) es una estructura sin egui, con tests de sus transiciones (seleccionar, cargar más, pedir abrir un archivo → `Request::OpenDiff { parent, commit, path }`). `app.rs` solo atiende esos pedidos.
- **Multinúcleo donde sirve:** resúmenes en paralelo (6.6) y fuera del hilo de la interfaz; los carriles, en un núcleo.
- **Textos** en `locales/gui.yml` (`history.*`, inglés y español); el test existente verifica que no falte ningún idioma.

### 4.6 Pruebas

- Tests del modelo y de `row_shapes` (curvas que empiezan y terminan en el centro del nodo, colores por carril, filas visibles).
- `summaries: false` da el mismo grafo que con resúmenes.
- Capturas sobre Xvfb con `xt.py`: el repo de Riku y el clon de gdstk (merges anidados), en claro y oscuro; abrir un archivo desde el historial muestra su diff.

### 4.7 Criterios de diseño (guías de Apple: *Designing Fluid Interfaces*, *Principles of Great Design*)

El visor ya los sigue (`gui/motion.rs`: resortes críticamente amortiguados que parten del valor en pantalla, inercia con la proyección de Apple, **Reduce motion** en Ajustes). El panel reusa esas piezas; no se agrega otro sistema de animación.

**Movimiento: solo donde ayuda a entender, siempre interrumpible**

| Qué se mueve | Cómo | Por qué |
|---|---|---|
| Abrir y cerrar el panel (**H**) | La altura con `spring_step` (amortiguamiento 1, respuesta 0,3 s), **desde la altura actual**: apretar **H** a mitad de camino lo da vuelta sin salto | Interrumpible; entra y sale por el mismo camino (abajo), así queda claro de dónde vino |
| La marca de selección al moverse con ↑/↓ | Se desliza con el mismo resorte desde donde está; con ↑↓ repetido, cada tecla solo cambia el destino | Continuidad: el ojo sigue la selección en vez de buscarla |
| La lista al seleccionar fuera de la vista | Desplaza lo justo para mostrar la fila (`scroll_to_rect`) | Nunca perder de vista lo elegido |
| Los resúmenes que llegan en segundo plano | Aparecen con un fundido corto de opacidad, sin mover nada | Llegan tarde, pero no reacomodan la fila que se está leyendo |
| El grafo, las filas, los chips | **No se animan** | Es información: tiene que estar quieta para leerse |
| Con **Reduce motion** | Todo lo anterior es instantáneo (o un fundido) | Accesibilidad: sin movimientos vestibulares |

**Respuesta inmediata**
- Resaltado de la fila **al pasar el mouse y al apretar** (no al soltar); la selección se confirma al soltar, y arrastrar fuera cancela.
- El grafo aparece al instante (fase 1, solo Git); lo lento (resúmenes) nunca bloquea: el encabezado muestra el avance como estado ("Analizando 34/200…"), no un spinner que tape la lista.
- Redimensionar el panel y el divisor interno sigue al puntero 1:1 (egui).

**Jerarquía y tipografía** (la fila se lee de izquierda a derecha, de lo más importante a lo menos)
- **Mensaje** en el peso y tamaño normales del visor, es lo primero que se lee. **Id** en monoespaciado, más chico y gris. **Autor y fecha** en tono terciario, alineados a la derecha, la fecha con números tabulares ("2 h", "ayer"; la fecha exacta al pasar el mouse).
- Grilla de 4/8 px (`theme::space`): fila de 24 px, columna del grafo de 14 px, nodos de 8 px, chips con 4 px de relleno y esquinas redondeadas de 6 px. Todo alineado a la misma línea base; nada "a ojo".
- Texto largo con puntos suspensivos y el texto completo en el tooltip; nunca se corta la fila en dos líneas.

**Color: al servicio del significado, nunca solo**
- Una paleta de 8 carriles para claro y otra para oscuro, con contraste verificado contra el fondo (`theme::contrast` ≥ 3:1 para líneas).
- Nada se comunica **solo** con color: un merge es un nodo **hueco**; `HEAD` es el único chip **relleno**; el resumen dice "sch" / "gds" / "raw" además del color de categoría.
- **Enfocar atenuando:** al pasar el mouse por un chip de rama, su carril queda al 100 % y los demás al 35 %, para seguir una rama entre merges sin perder el contexto.
- El cambio de tema claro↔oscuro usa el fundido que ya tiene el visor (sin saltos de brillo).

**Orientación (dónde estoy, a dónde puedo ir, cómo vuelvo)**
- Encabezado del panel: rama actual, cantidad de commits y el filtro activo, en una línea.
- Al abrir un archivo desde el historial, la ruta sobre el lienzo lo dice: `History › 51c0de › chip.gds › Diff`. El commit sigue seleccionado abajo, así volver es mirar la lista.
- **Esc** devuelve el foco al lienzo; **H** cierra el panel. Nada queda atrapado.

**Simplicidad y control**
- Lo común primero: la lista y el detalle. El filtro por glob es un campo chico en el encabezado; **Only this file** aparece solo si hay un archivo abierto.
- Todo es de solo lectura: el panel no cambia de rama ni toca el working tree, así que no hacen falta confirmaciones.
- Teclado completo: ↑/↓ (commit), **Enter** (abre el primer archivo con cambios), **Tab** (pasa a la lista de archivos), **Esc**, **H**. El alto del panel y la posición del divisor se recuerdan.

**Proceso:** prototipo interactivo antes de pulir. En 7.3b, capturas en claro y oscuro y una grabación cuadro a cuadro del abrir/cerrar y de ↑↓ rápido, para revisar que el movimiento no salte ni se atrase.

### 4.8 Pasos

| Paso | Qué | Listo cuando |
|---|---|---|
| 7.3a | `summaries: false` en `LogOptions`; `HistoryModel` con sus tests | grafo sin resúmenes idéntico; transiciones testeadas |
| 7.3b | Panel inferior, lista virtual y grafo con curvas (`row_shapes`) | captura del repo de Riku y de gdstk en los dos temas |
| 7.3c | Resúmenes en segundo plano, detalle del commit, abrir el diff | clic en un archivo → diff en el lienzo |
| 7.3d | Filtro, **Load more**, teclado, textos en/es, docs (`gui.md`) | capturas finales; `cargo test` y CI en verde |

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
| 7.3 | Panel **History** abajo en el visor (ver §4.8: 7.3a–d) | `gui/history_view.rs`, `gui/app.rs`, `locales/gui.yml`, `LogOptions::summaries` | M | Grafo con curvas, clic → cambios → diff visual; capturas en claro y oscuro |
| 7.4 | (Opcional) TUI con `ratatui` sobre el mismo motor | `cli/tui.rs` | M | Si hace falta usar Riku sin escritorio |

---

## 7. Riesgos

| Riesgo | Mitigación |
|---|---|
| Historias con muchas ramas abiertas a la vez: el grafo se ensancha | Compactar columnas libres (paso 5); en la terminal, más de 12 carriles se dibujan con `…` y se avisa |
| Orden topológico distinto del de Git en empates | Mismos criterios que Git (`TOPOLOGICAL \| TIME`); los tests comparan la topología (quién es padre de quién), no el dibujo exacto |
| El panel calcula resúmenes de muchos commits con layouts | Se calculan en segundo plano, en tandas por memoria (6.6), solo para los commits cargados |

---

## Avance

| Paso | Estado | Notas |
|---|---|---|
| 7.1 | Hecho (2026-09-27) | `core/analysis/graph.rs`: `layout` (carriles) y `simplify` (padres reescritos con `--paths`); `LogQuery::topological`; `LogCommit::graph`. Tests de propiedades: siguiendo los tramos desde cada nodo se llega exactamente a sus padres, nunca dos ramas en una columna. Casos sintéticos (lineal, merge, merge de merge, octopus, criss-cross, dos raíces, corte con `-n`) y la historia real de gdstk: 722 commits y 35 merges, entera, cortada a 100 y con un tercio visible. 1 000 commits: 0,08 s |
| 7.2 | Hecho (2026-09-27) | `cli/format/log_graph.rs`: cada celda de transición se arma con las direcciones que conecta (arriba, abajo, izquierda, derecha) y de ahí sale el carácter (`├ ┴ ╯ ┼`…), así cualquier cruce se dibuja bien; ASCII con la misma tabla. `--graph`, `--ascii`, `graph` en el JSON. El `log` sin `--graph` queda igual (regresión) |
| 7.3 | Hecho (2026-09-27) | `gui/history/`: `model.rs` (estado sin egui, con tests), `geometry.rs` (nodos, líneas y curvas Bézier; función pura con tests), `mod.rs` (panel inferior, lista virtual con `show_rows`, detalle, carga en hilos aparte). `LogOptions::skip_summaries` para el grafo al instante. `load_backend_diff` compara contra vacío el commit inicial y un archivo borrado. Textos en/es (`history.*`, `time.*`). Probado con un repo de demostración (ramas, merges, tag, `.sch`/`.gds`/`.raw`) y capturas en claro y oscuro sobre Xvfb |

**Diferencias con el diseño:**
- **Ancho por fila**, no global: con un ancho único, los tramos lineales heredaban el ancho de la zona más ramificada (8 columnas en gdstk) y el texto quedaba muy corrido. Igual que `git log --graph`.
- **Sin dependencias nuevas:** los colores son códigos ANSI directos (detectando terminal con `IsTerminal`, `NO_COLOR` y `CLICOLOR_FORCE`), sin `anstream`. `--color` y el tope de 12 carriles quedaron afuera: no hicieron falta en las historias probadas.
- **Un solo núcleo:** ubicar los carriles es secuencial (cada fila depende de la anterior) y lineal; lo que sí se reparte entre núcleos son los resúmenes de cada commit (6.6).

**Diferencias con el diseño (7.3):**
- **Tab** para pasar a la lista de archivos no se hizo: los archivos se abren con clic, **Enter** abre el primero con cambios.
- Con un filtro, los merges se siguen mostrando aunque no toquen el archivo (así funciona `riku log`); se puede revisar si conviene ocultarlos en el panel.
- Los resúmenes aparecen con un fundido; la marca de selección se desliza con el resorte de `motion.rs`; el alto del panel también.
