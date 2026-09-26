# riku-gui

Visor de escritorio de Riku (egui/eframe). Abre esquemáticos Xschem (`.sch`) y layouts GDS (`.gds`), y muestra el diff visual entre dos commits. Es de solo lectura: no edita los archivos.

## Uso

```bash
riku-gui                                   # árbol del directorio actual
riku-gui archivo.sch
riku-gui layout.gds
riku-gui libreria.gds --cell NOMBRE        # abre una celda concreta
riku-gui --repo R --commit-a A --commit-b B archivo   # modo diff
```

Normalmente el modo diff se abre desde la CLI: `riku diff A B archivo -f visual`. La CLI busca el binario en `$RIKU_GUI_BIN`, junto al ejecutable de `riku` o en `target/{release,debug}`.

### Controles

| Acción | Cómo |
|---|---|
| Mover la vista | arrastrar; al soltar rápido sigue por inercia (un clic la frena) |
| Zoom | rueda (anclado al cursor) o **+** / **−** |
| Encuadrar todo | botón **Encuadrar** o **F** (animado) |
| Mostrar/ocultar textos | botón **Etiquetas** o **L** |
| Sin animaciones ni inercia | **Ajustes → Reducir movimiento** (se recuerda) |
| Tema | **Claro / Oscuro / Sistema** (arriba a la derecha; se recuerda) |
| Coordenadas y escala | barra de estado (abajo): `x`, `y` del cursor y tamaño de 1 px |
| Ver todos los archivos | **Proyecto → Todos los archivos** (por defecto solo `.sch`, `.sym`, `.gds`) |
| Abrir un archivo | clic en el panel **Proyecto**, arrastrarlo a la ventana, o **Recientes** en la pantalla inicial |
| Info de un polígono (GDS) | dejar el cursor encima: capa, tamaño, área |
| Ocultar capas (GDS) | checkboxes en **Details → Capas** (se mantienen al cambiar de celda) |
| Cambiar de celda (GDS) | panel **Celdas**: buscador, "solo top cells", "solo con cambios" |
| Ir a un cambio (diff GDS) | clic en **Details → Cambios** |
| Comparar versiones | vistas **Diff / Before / After** (la vista se conserva) |

## Arquitectura

```
src/
├── main.rs           arranque y fuentes
├── launch.rs         argumentos (--repo, --commit-a, --commit-b, --cell)
├── app.rs            estado, carga async por backend, paneles
├── project.rs        árbol de archivos
├── sch_painter.rs    ruta rica de Xschem (fantasmas, anotaciones)
├── scene_painter.rs  ruta neutra: ScreenXform (mundo↔pantalla, eje Y),
│                     fit/zoom, hit-test y tooltip
├── motion.rs         springs interrumpibles e inercia de la vista
├── polygon_fill.rs   relleno de polígonos cóncavos (earcut)
├── entry_picker.rs   selector de celdas con buscador y filtros
├── label_layout.rs   colocación de etiquetas sin solaparse
├── theme.rs          colores por tema, tipografía y escala de espaciado
└── toast.rs          mensajes temporales (estado, completado, aviso, error)
```

- **Dos rutas de render.** Xschem conserva su painter propio. Todo lo demás (GDS) llega como `Arc<dyn RenderableScene>` desde un `ViewerBackend` de `viewer-core`; la GUI no conoce tipos de gdstk.
- **Cargas async.** Runtime Tokio con `poll-promise`; una carga nueva cancela la anterior (`CancellationToken`) y la escena actual sigue visible hasta que llega la nueva.
- **Coordenadas.** Mundo (Y-up en GDS) → vista (Y-down, relativa al panel, donde vive el `Viewport`) → pantalla. `ScreenXform` concentra las tres para que dibujo, culling, fit, zoom y hit-test usen la misma cuenta.
- **Etiquetas legibles** (`label_layout.rs`). Tamaño fijo en pantalla (10–14 px); se ocultan si el zoom es tan lejano que serían ruido. Las del mismo punto se fusionan (`VPB · VPWR`). Cada una es una pastilla con halo, desplazada del anclaje (marcado con un punto) para no tapar el pin; si choca, prueba otras posiciones y, si no entra, se omite y la barra de estado lo avisa.
- **Tema** (`theme.rs`). Fondo, halos, colores de capa, overlays de diff y el painter de Xschem se adaptan a claro/oscuro. El contraste de las etiquetas (WCAG AA, ≥ 4.5:1) se verifica en tests para los colores de los tres PDKs.
- **Movimiento** (`motion.rs`, criterios de *Designing Fluid Interfaces*, WWDC 2018). Encuadrar e ir a un cambio usan un spring críticamente amortiguado (respuesta 0,3 s, sin rebote) sobre centro + log de escala; cualquier arrastre o rueda lo interrumpe desde el valor en pantalla. Al soltar un arrastre rápido la vista sigue con la velocidad del puntero y desacelera a 0,998 por ms (proyección de momento de iOS). "Reducir movimiento" lo reemplaza por saltos directos.

## Compilar y probar

```bash
cd riku-gui
cargo build --release
cargo test
```

Se recomienda Linux (por ejemplo el contenedor iic-osic-tools): ver `docs/integracion_gds_estado.md` para los problemas conocidos de MSVC 2019 y vcpkg en Windows. Con WSLg la ventana aparece en el escritorio de Windows.

### Windows

Si el binario compila pero falla con `STATUS_DLL_NOT_FOUND` (0xc0000135), falta en el `PATH` la carpeta de DLLs de vcpkg que usa gdstk-rs:

```powershell
$env:PATH = "$env:VCPKG_ROOT\installed\x64-windows\bin;" + $env:PATH
```

## Criterios de interfaz

- **Jerarquía.** Títulos con peso, secundarios tenues y chicos, cifras en monoespaciada; espaciado de una sola escala (4/8/12/16 px) y esquinas coherentes.
- **Orientación.** La ruta sobre el lienzo (`commits › archivo › celda › vista`) y el título de la ventana dicen qué se está viendo; la pantalla inicial explica cómo empezar y ofrece los recientes.
- **Agrupación.** Detalles en secciones plegables (Resumen, Cambios, Capas, Símbolos sin resolver): lo relacionado junto y lo largo se puede plegar.
- **Feedback.** Mensajes temporales sobre el lienzo: estado e *hecho* se van solos; los avisos duran más; los errores quedan hasta cerrarlos, en lenguaje claro con el detalle técnico entre paréntesis.
