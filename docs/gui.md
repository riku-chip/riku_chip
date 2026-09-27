# El visor (`riku gui`)

Visor de escritorio de Riku (egui/eframe), incluido en el ejecutable `riku` (feature `gui`, activada por defecto; el código vive en `riku/src/gui/`). Abre esquemáticos Xschem (`.sch`, `.sym`), layouts (`.gds`, `.oas`, `.mag` de Magic con su jerarquía) y simulaciones de ngspice (`.raw`, con su propia vista de curvas: [`spice.md`](spice.md)), y muestra el diff visual entre dos commits. Es de solo lectura: no edita los archivos.

## Uso

```bash
riku gui                                   # árbol del directorio actual
riku gui archivo.sch
riku gui layout.gds
riku gui libreria.gds --cell NOMBRE        # abre una celda concreta
riku gui --repo R --commit-a A --commit-b B archivo   # modo diff
riku open archivo                          # igual, sin bloquear la terminal
```

Normalmente el modo diff se abre desde la CLI: `riku diff A B archivo -f visual` o `riku show COMMIT archivo -f visual`. `open` y el modo visual relanzan el propio ejecutable como un proceso aparte (`riku gui …`), así la terminal y el shell quedan libres. Sin escritorio gráfico (`DISPLAY`/`WAYLAND_DISPLAY`), `riku gui` lo explica y la CLI sigue funcionando.

### Controles

| Acción | Cómo |
|---|---|
| Mover la vista | arrastrar; al soltar rápido sigue por inercia (un clic la frena) |
| Zoom | rueda (anclado al cursor) o **+** / **−** |
| Encuadrar todo | botón **Encuadrar** o **F** (animado) |
| Mostrar/ocultar textos | botón **Etiquetas** o **L** |
| Sin animaciones ni inercia | **Ajustes → Reducir movimiento** (se recuerda) |
| Dibujar cada polígono aunque sea diminuto | desmarcar **Ajustes → Simplificar al alejar** (se recuerda; más lento en layouts grandes) |
| Tema | **Claro / Oscuro / Sistema** (arriba a la derecha; se recuerda) |
| Coordenadas y escala | barra de estado (abajo): `x`, `y` del cursor y tamaño de 1 px |
| Ver todos los archivos | **Proyecto → Todos los archivos** (por defecto solo lo que se puede abrir: `.sch`, `.sym`, `.gds`, `.oas`, `.mag`, `.raw`) |
| Abrir un archivo | clic en el panel **Proyecto**, arrastrarlo a la ventana, o **Recientes** en la pantalla inicial |
| Info de un polígono (GDS) | dejar el cursor encima: capa, tamaño, área |
| Ocultar capas (GDS) | checkboxes en **Details → Capas** (se mantienen al cambiar de celda) |
| Cambiar de celda (GDS) | panel **Celdas**: buscador, "solo top cells", "solo con cambios" |
| Ir a un cambio (diff GDS) | clic en **Details → Cambios** |
| Comparar versiones | vistas **Diff / Before / After** (la vista se conserva) |
| Historial del repo | botón **History** o **H**: panel abajo con el grafo de ramas (ver abajo) |

## Historial (**History**, tecla **H**)

Un panel abajo, a todo el ancho, con el historial del repo del proyecto:

- **Grafo de ramas y merges** con curvas y un color por rama (el mismo motor que `riku log --graph`); nodo hueco para un merge; chips de `HEAD` (relleno), ramas y tags. Pasar el mouse por un chip atenúa las demás ramas.
- **Resumen por commit** a la derecha: formatos tocados y `+añadidos −eliminados ~modificados`. El grafo aparece al instante y los resúmenes se calculan en segundo plano (en paralelo), sin trabar el visor.
- **Clic en un commit:** su mensaje, autor, fecha y archivos. **Clic en un archivo** (o doble clic en el commit, o **Enter**): su diff contra el primer padre en el lienzo, con **Diff / Before / After**, para `.sch`, `.gds`/`.oas`/`.mag` y `.raw` (un `.mag` lee sus sub-celdas del mismo commit). La ruta sobre el lienzo empieza por `History`.
- **↑/↓** cambian de commit; **H** cierra. **Filtrar archivos** (un glob, `*.gds`) o **Only this file** con un archivo abierto: el grafo se simplifica como `riku log --paths`. Se cargan 200 commits; **Load more** trae más.
- El panel entra y sale por abajo con un resorte interrumpible; su alto se recuerda. Con **Reduce motion** no se anima.

## Arquitectura

```
src/
├── main.rs           arranque y fuentes
├── launch.rs         argumentos (--repo, --commit-a, --commit-b, --cell)
├── app.rs            estado, carga async por backend, paneles
├── project.rs        árbol de archivos
├── (los esquemáticos los dibuja el backend del módulo Xschem: riku/src/modules/xschem_view.rs)
├── scene_painter.rs  ruta neutra: ScreenXform (mundo↔pantalla, eje Y),
│                     fit/zoom, hit-test y tooltip
├── motion.rs         springs interrumpibles e inercia de la vista
├── polygon_fill.rs   relleno de polígonos en escenas sin índice
├── entry_picker.rs   selector de celdas con buscador y filtros
├── label_layout.rs   colocación de etiquetas sin solaparse
├── theme.rs          colores por tema, tipografía y escala de espaciado
└── toast.rs          mensajes temporales (estado, completado, aviso, error)
```

- **Una sola ruta de render.** Esquemáticos y layouts llegan como `Arc<dyn RenderableScene>` desde el `ViewerBackend` de su módulo (`viewer-core`); la GUI no conoce tipos de gdstk ni de Xschem.
- **Layouts grandes** (`viewer_core::index`). Al cargar, el backend arma un índice de la escena (en paralelo): bbox de cada elemento, grillas por tamaño, triangulación de los cóncavos y una pirámide de cobertura por capa. En cada cuadro se consulta solo lo visible. Si eso no pasa de 60 000 elementos se dibuja todo como siempre; si pasa, lo que mide pocos píxeles o menos de un píxel de ancho se pinta como una imagen por capa (una textura por nivel, en cache) y el resto uno a uno, con los rellenos de cada capa juntos en una malla. Un layout de 42 MB (6,2 millones de polígonos) pasó de 11 GB y ~600 ms por cuadro a 2,5 GB y ~2 ms. `RIKU_PROFILE=1` imprime el tiempo de cada cuadro, los elementos dibujados y el nivel usado; `RIKU_LOD_PX` ajusta el lado de los texels (1 px por defecto).
- **Cargas async.** Runtime Tokio con `poll-promise`; una carga nueva cancela la anterior (`CancellationToken`) y la escena actual sigue visible hasta que llega la nueva.
- **Coordenadas.** Mundo (Y-up en GDS) → vista (Y-down, relativa al panel, donde vive el `Viewport`) → pantalla. `ScreenXform` concentra las tres para que dibujo, culling, fit, zoom y hit-test usen la misma cuenta.
- **Etiquetas legibles** (`label_layout.rs`). Tamaño fijo en pantalla (10–14 px); se ocultan si el zoom es tan lejano que serían ruido. Las del mismo punto se fusionan (`VPB · VPWR`). Cada una es una pastilla con halo, desplazada del anclaje (marcado con un punto) para no tapar el pin; si choca, prueba otras posiciones y, si no entra, se omite y la barra de estado lo avisa.
- **Tema** (`theme.rs`). Fondo, halos, colores de capa, overlays de diff y el painter de Xschem se adaptan a claro/oscuro. El contraste de las etiquetas (WCAG AA, ≥ 4.5:1) se verifica en tests para los colores de los tres PDKs.
- **Movimiento** (`motion.rs`, criterios de *Designing Fluid Interfaces*, WWDC 2018). Encuadrar e ir a un cambio usan un spring críticamente amortiguado (respuesta 0,3 s, sin rebote) sobre centro + log de escala; cualquier arrastre o rueda lo interrumpe desde el valor en pantalla. Al soltar un arrastre rápido la vista sigue con la velocidad del puntero y desacelera a 0,998 por ms (proyección de momento de iOS). "Reducir movimiento" lo reemplaza por saltos directos.

## Compilar y probar

`cargo test -p riku gui::` corre solo los tests del visor; el resto, en [`desarrollo.md`](desarrollo.md).

- **WSLg:** la ventana aparece en el escritorio de Windows. Para capturarla con herramientas X11, lanzar con `env -u WAYLAND_DISPLAY` (usa XWayland). El visor recuerda la posición de la ventana; si alguna vez abre minimizada o fuera de pantalla, borrar la clave `"window"` de `~/.local/share/riku-gui/app.ron`.

## Criterios de interfaz

- **Jerarquía.** Títulos con peso, secundarios tenues y chicos, cifras en monoespaciada; espaciado de una sola escala (4/8/12/16 px) y esquinas coherentes.
- **Orientación.** La ruta sobre el lienzo (`commits › archivo › celda › vista`) y el título de la ventana dicen qué se está viendo; la pantalla inicial explica cómo empezar y ofrece los recientes.
- **Agrupación.** Detalles en secciones plegables (Resumen, Cambios, Capas, Símbolos sin resolver): lo relacionado junto y lo largo se puede plegar.
- **Feedback.** Mensajes temporales sobre el lienzo: estado e *hecho* se van solos; los avisos duran más; los errores quedan hasta cerrarlos, en lenguaje claro con el detalle técnico entre paréntesis.
