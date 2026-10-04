# Pendientes

Todo lo que falta, en un solo lugar. Esfuerzo: **S** = horas, **M** = 1–2 días, **L** = varios días. Lo hecho no se lista: está en el código y en la historia de Git (la documentación de diseño de las fases 6–9 y la investigación inicial quedaron en el tag [`docs-historia`](https://github.com/riku-chip/riku_chip/tree/docs-historia)).

**Estado:** última versión [v0.1.0](https://github.com/riku-chip/riku_chip/releases/tag/v0.1.0) (las versiones se reiniciaron el 2026-09-28). Revisado 2026-09-28.

## En curso: demos y README

`riku demo` ya crea `ota` (OTA de SKY130: esquemático, layout y simulación; 11 commits, una rama y un corto que se arregla) y `sram` (SRAM 16×8 de OpenRAM: cambios en sub-celdas, un renombre y relleno en una rama). Los arman los scripts de `tools/demos/` (ver [`desarrollo.md`](desarrollo.md)); el diseño está en la historia de Git (`feat(cli): riku demo`).

| Qué | Por qué | Esf. |
|---|---|---|
| Demo `inversor` en Magic y Xschem (el inversor de `demo_sky130A` de iic-osic-tools) | Mostrar Magic: capas por nombre, un cambio en el transistor visto desde la celda de arriba, un puerto que cambia de clase, un abierto | M |
| Demo `chip` grande (la SRAM de 1 KB de OpenRAM de `sky130_sram_macros`, 10 MB) en un repo aparte, que `riku demo chip` clone | Ver el rendimiento con un diseño grande sin inflar el ejecutable | M |

## Mejoras

| Qué | Por qué | Esf. |
|---|---|---|
| **Extracción jerárquica con memoria por huella** | Hoy las redes se arman aplanando la celda (tope: 2 millones de polígonos) y se recalculan en cada commit. Como el diff ya tiene un árbol de la jerarquía con una huella (hash) por celda, se puede guardar la netlist de cada celda por su huella y reusarla: una celda que no cambió no se re-analiza, y el padre solo une los pines de sus hijas (como KLayout). Parecido a un DOM virtual: un árbol de netlists donde se recalcula solo la rama que cambió. Permitiría analizar un chip entero y bajar el costo de un `log` | L |
| Costo de `log` con cambios en la celda de arriba de un layout mediano | En la SRAM del repo, un commit que agrega relleno en la celda de arriba cuesta ~1,7 s (las redes de la macro, en los dos lados). Lo resuelve la extracción jerárquica; mientras tanto, la caché de diffs solo guarda layouts de más de 1 MiB | M |
| Renombres de celdas que además cambiaron | Hoy solo se detectan los puros; uno con cambios sale como baja + alta. Emparejar por bbox y huellas en común (`gds_diff.rs::detect_renames`) | M |
| Zoom cercano en zonas muy densas (~45 ms por cuadro) | Con píxeles más chicos que la celda más fina de la pirámide se dibuja todo uno a uno. Pirámide más fina con bitsets dispersos (`viewer-core/src/index.rs`) | M |
| Medir el diff con un wrapper de SKY130/Caravel | Las mediciones de rendimiento son con un chip de IHP; otro PDK puede tener otro peor caso (`profile_diff`) | S |
| `log --graph`: tope de ramas con `…` | Limitar el ancho del grafo y marcar con `…` las ramas que no caben | S |
| `cargo fmt` y Clippy en la CI | El formato no es uniforme (un commit grande solo de formato); Clippy primero sin bloquear | S |
| Subir `actions/upload-artifact` a una versión con Node 24 | GitHub deja de soportar Node 20 en las acciones | S |
| Magic: `.mag.gz`, `MASKHINTS_*` como geometría, vistas `maglef/` | Cubrir el resto del formato | M |
| TUI (`ratatui`) para el historial | Opcional: navegar el grafo sin el visor | L |

## A coordinar con Carlos (`xschem-viewer-rust`)

| Qué | Por qué | Esf. |
|---|---|---|
| Pico de RAM del visor (~2,6 GB con el chip de 42 MB) | Bajarlo pide guardar los puntos de `DrawElement` en `f32`/CSR: cambia el contrato de `viewer-core` | M |
| Caché de `.sym` del proceso en `RenderOptions` | Cada esquemático relee sus símbolos (~1,6 ms; en un `log` de 1 000 commits, segundos) | S |
| Cancelar entre fases dentro de `build_index` | Cambiar de celda rápido deja trabajo en curso; cambia una firma que usa su crate | S |

## Solo si hace falta (medir antes)

- **Piezas recursivas al aplanar** (TOP → CORE): ~1,2 GB estimados, nunca medidos. Esperar un diseño real que lo muestre.
- **Malla en la GPU** para el dibujo de layouts: solo si un diseño más grande baja de 60 fps.
- **Leer blobs por OID** en `log` (`old_oid`/`new_oid` en `ChangedFile`): poca ganancia medida.
- **Caché de la lista de ondas:** solo con 100 000+ señales.
- **Apagar el nivel de detalle** desde Ajustes (hoy se puede con **Simplificar al alejar**, que no lo apaga del todo en layouts enormes).

## Ideas a futuro

**Integración con Git** (nada de esto existe todavía):
- **`textconv`** (`cachetextconv=true`) o un difftool, para que un `git diff` o `git log -p` normal muestre el diff semántico (`*.sch diff=riku`).
- **Merge drivers** por formato en `.gitattributes`, instalados localmente con `[include] path=.riku/gitconfig`, nunca globales:
  - `.mag`: normalizar los `timestamp` y aplicar `git merge-file`;
  - `.sch`: componentes disjuntos se fusionan, y un Move All mezclado con cambios funcionales se avisa;
  - `.gds`/`.oas`: celdas disjuntas desde la base, y si se pisan, conflicto (cuidado con sub-celdas compartidas y la top tocada en las dos ramas).
- **Git LFS:** hoy Riku ve el puntero, no el archivo, así que no compara versiones guardadas con LFS. Soportarlo, o avisarlo claro. LFS es complementario: conviene para GDS grandes.
- **Artefactos derivados viejos** después de un merge (un `.gds` más viejo que su `.mag`, un `.spice` más viejo que su `.sch`): avisar sin bloquear. Pide declarar en `.riku.toml` qué es fuente y qué derivado.
- **Comandos:** `blame --semantic` (quién tocó por última vez un componente o celda), `log --cell`/`--component` y `log --sim-metric` (una medida a lo largo del historial).

**Lo eléctrico** (transistores, resistores, redes, abiertos y cortos ya están: [`formatos.md`](formatos.md#transistores-y-redes)):
- **LVS: lo que falta** (`riku lvs`, `--log`, `--ci`, la vista con los dos lados resaltando, `log --lvs`, `status --lvs` y la caché por dependencias ya están; ver [`lvs.md`](lvs.md)): un clic en una red o un transistor de un lado que resalte su pareja en el otro aunque coincida (leer la tabla de equivalencias de Netgen, S/M); dispositivos dentro de sub-celdas (la extracción es por celda aplanada, M); cuando Netgen junta en paralelo dispositivos de distinto L, resaltar solo los del L que no coincide (S); abrir el LVS de dos versiones en un diff (M); el LVS en el panel **Historial** del visor (lo mismo que `log --lvs`, S/M); un PR en la CI (base contra head, como `status --lvs`, S); `[lvs] in_status = true` en `.riku.toml` (S); correr Netgen en paralelo para varias versiones, después de medir (M); escribir solo los archivos leídos en `Tree::commit` en vez del árbol entero (S); ver si el LVS de KLayout está a la par de Netgen.
- **Chequeos eléctricos** (L), con herramientas externas y su diferencia entre commits: ERC (compuertas o pines sin conectar, pozos sin polarizar), antena (las reglas del deck de DRC del PDK), parásitos (Magic `ext2spice` con `cthresh`/`rthresh`: "la red `out` subió 12 fF") y post-layout (simular la netlist extraída y comparar las `.meas`).
- **Extracción jerárquica** (L): cada celda una vez, con sus pines hacia arriba (como KLayout), para analizar un chip entero; hoy el tope es 2 millones de polígonos aplanados por celda.
- **Diodos y capacitores**, y los resistores en el diff (hoy están en la netlist, pero un resistor que cambia no se lista) (M).

**Verificación en CI:**
- **DRC por diferencia** entre base y head: `klayout -b -r script.drc` y leer el `.lyrdb` (`ReportDatabase`). Bloquear solo si suben las violaciones, así se toleran las que ya había.
- **Regresión de `.meas`:** leer `nombre = valor` del log de ngspice y comparar con tolerancias, contra el padre o un nominal. Solo corridas de la misma fase (pre o post layout).
- **Un comentario del PR que se actualiza**, marcado con `<!-- riku-ci -->`, y `ci init` con plantillas sobre la imagen de iic-osic-tools.
- **Claves de caché de verificaciones:** versión de la herramienta, hash del PDK y, en LVS, el `setup.tcl`; nunca fechas de archivo. Una caché compartida (S3/R2) solo si hay equipo.

**Proyecto y PDK:**
- **Fijar el PDK por proyecto** en `.riku.toml` (`pdk.version`, un commit, como volare/ciel), verificado por `riku doctor`. También `layout.source = magic|klayout|python`.
- **`riku doctor` podría detectar:** SKY130 con KLayout necesita dos `sed` (`sky130.lym`, `sky130A.lyt`); sin ellos, falla en silencio.
- **Formatos nuevos:**
  - un `.sch` que no es de Xschem (Qucs-S `<Qucs Schematic`, KiCad `EESchema Schematic File`): detectarlo por la cabecera y caer a diff de texto;
  - netlists `.spice`/`.cdl`: canonizar o usar el JSON de Netgen; un `.spice` no siempre deriva del `.sch`;
  - `.kicad_sch`.
- **Origen de un polígono aplanado:** saber de qué instancia y con qué rotación viene ("instancia de `amp` rotada 90°").

## Cosas a saber de los formatos

- **`spice_sym_def`:** los símbolos de SKY130 apuntan a una netlist externa; el diff del `.sch` no ve cambios dentro de ella.
- **Etiquetas lejos del ancla:** algunos símbolos de PDK dibujan su texto lejos del ancla (`cap_mim_m3_1`, ~20 px); no usar la etiqueta como posición del componente.
- **`timestamp` de Magic:** Magic los reescribe en la celda y sus padres en cada guardado. Es ruido para un diff de texto o un merge; el diff geométrico lo ignora.
- **Comparar GDS contra OASIS en KLayout:** `LayoutDiff` necesita `IgnoreDuplicates`, o da falsos positivos.
- **GDS exportado por KLayout** puede ser rechazado por Cadence en la fundición (AREF de 1×1, celda `$$$CONTEXT_INFO$$$`).
- **`.gitignore` según el flujo:** el `.gds` es fuente en proyectos con KLayout como editor, y salida con Magic o Python; `.ext` y `.osdi` son salida; `.va` (IHP) es fuente.
- **Límite de 50 MB por archivo:** uno más grande no se compara (se informa como error).

## Límites conocidos

- **Espaciado entre letras por tamaño:** egui no lo permite ajustar (revisar en cada versión nueva).
- **Mensajes del núcleo y de los módulos** (errores de Git, avisos de un formato): todavía en español, sin traducir.
- **Pruebas con usuarios reales:** el visor se probó con clics simulados en Xvfb. Falta ver a diseñadores usándolo, y probar el marco propio de la ventana (mover, maximizar, redimensionar) en un escritorio real.
