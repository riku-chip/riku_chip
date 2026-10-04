# El visor

Visor de escritorio incluido en `riku`. Abre esquemáticos (`.sch`, `.sym`), layouts (`.gds`, `.oas`, `.mag`) y simulaciones (`.raw`), y muestra el diff visual entre versiones. Es de solo lectura. La interfaz está en inglés por defecto; **Settings → Language** (o `RIKU_LANG=es`) la pone en español, que es como se nombra acá.

```bash
riku open                      # pantalla de inicio en la carpeta actual; la terminal queda libre
riku gui                       # igual, en este proceso (la terminal queda ocupada)
riku open amp.sch              # un archivo (también: riku gui chip.gds --cell INV)
riku diff HEAD~3 HEAD -f visual          # la lista de todo lo que cambió
riku diff HEAD~1 HEAD chip.gds -f visual # el diff de un archivo
```

Necesita un escritorio gráfico (`DISPLAY` o `WAYLAND_DISPLAY`); sin él, lo dice y la CLI sigue funcionando. El instalador deja además un acceso "Riku" en el menú de aplicaciones.

## Pantalla de inicio

Lo que se ve sin nada abierto, y con el botón **Inicio**:

| Qué | Equivale a |
|---|---|
| **Proyecto:** la carpeta, su rama y cuántos archivos tienen cambios sin commitear; **Abrir carpeta…** y carpetas recientes | `riku gui /ruta` |
| **Cambios sin commitear:** cada archivo con su resumen; un clic abre su diff contra `HEAD` | `riku status`, `riku diff ARCHIVO -f visual` |
| **Historial** | `riku log --graph` (tecla **H**) |
| **Comparar versiones…:** un archivo entre dos versiones (commit, rama, tag o el disco), o todos los que cambiaron | `riku diff A B [ARCHIVO] -f visual` |
| **Diagnóstico** | `riku doctor` |
| **Archivos recientes** | — |

**Abrir carpeta…** es un selector propio: se navega (las carpetas que son un repo llevan la marca `git`) o se pega una ruta. En una laptop con Windows (Riku en el contenedor o en WSL) también se puede pegar la ruta de Windows, con `\` o entre comillas como la copia el Explorador: `C:\Users\…\designs\mi_chip` abre `/foss/designs/mi_chip`. Arriba están los atajos a las carpetas de Windows que el contenedor ve (**En la laptop**), y abajo de la ruta, cómo se llama la carpeta actual en Windows. Una carpeta de Windows que no está montada en el contenedor no se puede abrir: el selector lo dice y muestra cuáles sí. Cambiar de carpeta recarga el árbol y pasa el Historial al repo nuevo. Los cambios sin commitear se calculan en segundo plano; **↻** los vuelve a revisar.

Con algo abierto, en la barra: **Comparar…** (con ese archivo ya elegido) y **Exportar → PNG / SVG** (la ruta de la imagen queda en el portapapeles).

## Diff de todo el repo

**Comparar versiones… → Todos los archivos que cambiaron**, o `riku diff A B -f visual` / `riku show COMMIT -f visual` sin archivo, abren arriba del panel izquierdo la lista **Cambios A → B**: cada archivo con su estado (**A** añadido, **M** modificado, **D** borrado, **R** renombrado) y su resumen. La lista sale enseguida y los resúmenes llegan después, sin trabar el visor. Un clic abre el diff de ese archivo y la lista queda; **↑/↓** pasan al siguiente; **×** la cierra.

## Ver un diff

Las vistas **Diff**, **Before** y **After** (en español, **Diff**, **Antes** y **Después**; panel **Vistas**) muestran la diferencia y cada versión; cambiar de vista conserva el zoom para comparar la misma zona. En **Detalles**: **Resumen**, **Cambios** (un clic encuadra el cambio) y **Capas** (ocultar o mostrar; se mantiene al cambiar de celda). El panel **Celdas** lista las celdas de un layout y, en un esquemático, su jerarquía (el `.sch` y los sub-esquemáticos del proyecto que usa); tiene buscador, "solo top cells" (si hay más de una) y "solo con cambios", y en un diff marca lo que cambió, también por dentro. **Doble clic** en una instancia de una sub-celda o de un sub-esquemático la abre (el tooltip lo dice); **← Volver** (junto a la ruta), **Backspace** o **Alt + ←** vuelven al nivel de arriba con la vista que tenía, un nivel por vez. Lo propio de cada formato está en [`formatos.md`](formatos.md).

## Controles

| Acción | Cómo |
|---|---|
| Mover la vista | arrastrar (al soltar rápido sigue por inercia; un clic la frena) |
| Zoom | rueda (hacia el cursor), pellizcar en el touchpad o Ctrl + rueda, o **+** / **−** |
| Touchpad | dos dedos a los lados mueven la vista; arriba/abajo hacen zoom como la rueda, o mueven con **Ajustes → Dos dedos / rueda: Mover** (el zoom queda en pellizcar) |
| Encuadrar todo | **Encuadrar** o **F** |
| Mostrar u ocultar textos | **Etiquetas** o **L** |
| Qué capa es cada color | **Leyenda** o **G**: las capas de lo que está a la vista, abajo a la izquierda (con el zoom lejos, las del archivo) |
| Resaltar una capa | pasar el cursor por su nombre en **Capas** o en la leyenda (el resto se atenúa); un clic la deja resaltada, otro clic o **Esc** la suelta |
| Ver los transistores | **Capas → Transistores** (oculta al abrir): cada compuerta en amarillo con su modelo, W y L; en **Resumen**, cuántos hay ("4 (2 N, 2 P)"). Se reconocen con las reglas del PDK (SKY130, GF180MCU, IHP), en GDS, OASIS y Magic; en una celda de más de 2 millones de polígonos, no (abrir una sub-celda) |
| Ver una red | pasar el cursor por un polígono: el tooltip suma `red: Y` (una red sin etiqueta se nombra por un transistor que toca); **clic** en el polígono resalta la red entera en amarillo y atenúa el resto (la barra de estado dice cuántos polígonos tiene); otro clic o **Esc** la suelta. En **Resumen**, `Redes: 8 (7 con nombre)`. Con las mismas reglas del PDK que los transistores |
| Abiertos y cortos (diff) | primero en **Cambios**, en rojo y con `!` (`corto · B = Y`, `B, Y → B = Y`), con un recuadro sobre el layout; un clic encuadra dónde está |
| Info de un polígono | dejar el cursor encima: capa, tamaño, área |
| Coordenadas y escala | barra de estado: `x`, `y` y tamaño de 1 px |
| Abrir un archivo | panel **Proyecto**, arrastrarlo a la ventana, o **Recientes** |
| Ver todos los archivos del árbol | **Proyecto → Todos los archivos** |
| Tema | **Claro / Oscuro / Sistema** |
| Sin animaciones ni inercia | **Ajustes → Reducir movimiento** |
| Dibujar cada polígono aunque sea diminuto | desmarcar **Ajustes → Simplificar al alejar** (más lento en layouts grandes) |

Las preferencias se recuerdan entre sesiones.

## Historial

Panel abajo (**Historial** o **H**) con el grafo de ramas y merges (el mismo que `riku log --graph`), las refs y un resumen por commit que se calcula en segundo plano. Un clic en un commit muestra sus archivos; un clic en un archivo (o **Enter**) abre su diff contra el primer padre. **↑/↓** cambian de commit. **Tab** pasa a los archivos del commit (marca el primero que se puede abrir): ahí **↑/↓** eligen otro y **Enter** abre el marcado; **Tab** otra vez (o **Shift+Tab**) vuelve a los commits. Con el Historial cerrado, Tab recorre los botones como siempre. **Filtrar archivos** (un glob, `*.gds`) o **Solo este archivo** simplifican el grafo; se cargan 200 commits y **Cargar más** trae el resto.

## La ventana

La barra superior es el título: se arrastra para mover la ventana, doble clic maximiza, y a la derecha están minimizar, maximizar y cerrar, con fondo al pasar el puntero (cerrar en rojo). Los bordes cambian el tamaño. **Ajustes → Usar el marco del sistema** vuelve al marco del escritorio.

Si la ventana abre fuera de pantalla, borrar la clave `"window"` de `~/.local/share/riku-gui/app.ron`.
