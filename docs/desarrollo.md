# Desarrollo

Cómo compilar, probar, verificar y publicar Riku, cómo está armado y las reglas que hay que respetar al cambiarlo. Lo que falta hacer está en [`pendientes.md`](pendientes.md).

## Entorno y compilación

**Linux x86_64** es la plataforma oficial (Windows no se soporta: el linker de gdstk falla con MSVC). Lo más simple es el contenedor [iic-osic-tools](https://github.com/iic-jku/iic-osic-tools), que trae zlib, qhull, KLayout y los PDK en `/foss/pdks`; basta `rustup default stable`. Fuera de él: Rust estable, toolchain C++, `zlib1g-dev` y `libqhull-dev`; para el visor, `libxkbcommon-dev`, `libwayland-dev` y `libgl1-mesa-dev`.

```bash
git clone --recurse-submodules https://github.com/riku-chip/riku_chip
cd riku_chip
cargo build --release                                  # target/release/riku: CLI, shell y visor
cargo build --release -p riku --no-default-features    # solo terminal
```

Es un workspace (un `Cargo.lock`, un `target/`). Los submódulos de `external/` entran por path; después de cada `git pull`, `git submodule update`.

## Tests

```bash
cargo test --workspace            # todo; la CI lo corre con RUSTFLAGS="-D warnings"
cargo test -p riku                # CLI, análisis, visor, módulos; tests/basic.rs, gds_e2e.rs, mag_e2e.rs, stress.rs
cargo test -p riku-mod-layout     # diff de layouts, cache, estilo de capas, escena
cargo test -p viewer-core         # contrato del visor e índice espacial
```

- Los fixtures de layouts (`riku-mod-layout/tests/fixtures/`) se generan con los scripts Python de esa carpeta (con KLayout); los de Magic están escritos en los tests. Las tablas compiladas se regeneran con `tools/palettes/gen_*.py` cuando cambia un PDK.
- El lector de Magic vive en `external/gdstk/rust` (`cargo test --test magic`); `pdk_corpus` y `klayout_testdata` están ignorados porque necesitan archivos de afuera (ver ese repo).
- Verificación contra KLayout, Magic y Netgen, mediciones y pruebas del visor sin mouse: [Verificación](#verificación).
- Medir `log`, `status` o el visor en una copia en `/tmp`: un repo en un montaje lento (9p, red) distorsiona los tiempos.

## Arquitectura

**Monolito modular con microkernel:** un solo ejecutable, varios crates. Un núcleo que no conoce ningún formato define los contratos y cada formato es un módulo que se registra en él, enlazado al compilar (features de Cargo, no `.so`: Rust no tiene ABI estable).

```
riku/                 ejecutable
  src/core/           git (git2), análisis (diff_pair, diff_set, show, log, status), repo_path
  src/modules/        módulos de formato; mod.rs::registry() es el ÚNICO lugar que los lista
  src/cli/, src/gui/  CLI + shell y visor egui (feature `gui`)
riku-kernel/          tipos de cambio (FileChange, Change, Element, Detail), FormatModule, Registry
riku-mod-layout/      GDS/OASIS/Magic: diff geométrico, cache, estilo por PDK, GdsBackend,
                      transistores (devices/) y redes (nets/) con las reglas del PDK
viewer-core/          contrato del visor: ViewerBackend, Scene, DrawElement, SceneIndex
external/gdstk/       gdstk_rust: binding de gdstk (C++) y lector de Magic (Rust)   [submódulo]
external/xschem-viewer-rust/   parser y semántica de Xschem, de Carlos Cueva       [submódulo]
```

```
riku (cli · gui) ──Registry──► modules/xschem · riku-mod-layout · modules/spice
                                    │               │
                          xschem-viewer-rust    gdstk_rust
                                    └──── riku-kernel · viewer-core (no conocen formatos)
```

**Contratos.** `FormatModule` (kernel): `extensions()` (lo que consulta `for_path`), `info()` (para `doctor`), `detect()` por firma, `diff()` y `diff_with()` (con los otros archivos de cada versión, para Magic), `viewer()`. `ViewerBackend` (viewer-core): `load`, `load_entry`, `load_diff` y sus variantes `*_with`; devuelven una `Scene` neutra, así el visor dibuja todos los formatos por la misma ruta. Las formas de onda son la excepción: no son planos y tienen su vista (`gui/wave_view.rs`).

**`riku-mod-layout`** es lo único que usa `gdstk_rust`; `riku` lo ve por `modules/layout.rs`. Su API pública es la que usa `riku`: `diff_layout_sides` (cualquier formato, con los archivos de cada versión) y `diff_cell` (una celda, con los polígonos del XOR); `GdsDiffReport` con geometría, puertos, transistores (`devices`) y redes (`nets`); `DiffCache`; `mag::{collect, build, port_changes}`; `pdk_tech` (el `.lyp` y el `.tech` del PDK instalado); `GdsBackend` para el visor. Las reglas eléctricas salen del `.tech` de Magic (`devices/rules.rs`, con la copia compilada en `devices_generated.rs`); las regiones se evalúan con `boolean_owned` y `offset_owned` de gdstk_rust. En el visor, la escena lleva una sonda de redes (`viewer_core::NetProbe`) para el tooltip y resaltar una red.

**Flujo de un diff.** `core/analysis/diff_pair.rs` es el único camino (CLI y visor): cada lado es una `Version` (`Rev`, `WorkTree` o `Absent`) con su ruta, y `OnError` dice si un error de Git se propaga (`diff`, `show`) o queda en el archivo (`status`, `log`). Los archivos de otra versión llegan como `FileSource` (`GitFiles` del mismo commit, `DiskFiles` del disco).

**Features:** `xschem`, `layout`, `spice` y `gui`, todas por defecto; la CI compila cada combinación.

## Reglas del proyecto

1. **El núcleo no conoce formatos.** `riku-kernel` no depende de ningún módulo ni motor (la CI lo verifica con `cargo tree`). Un módulo depende del kernel, de `viewer-core` y de su motor, nunca de otro módulo. Los motores no saben de Riku. Sumar un formato es un módulo en `riku/src/modules/` (o un crate `riku-mod-*`) y una línea en `registry()`.
2. **Lo que es de un formato lo decide el módulo:** qué claves son ubicación (`Detail::placement`), qué extensiones abre (`extensions()`, `Registry::openable()`), qué error es un archivo dañado (`ViewerError::Corrupt`).
3. **El crate de Carlos (`xschem-viewer-rust`) no se modifica desde Riku.** Lo que lo toque (caché de `.sym`, la forma de `DrawElement`, la firma de `build_index`) se acuerda con él.
4. **Contratos compatibles:** todo lo nuevo en `viewer-core` y `FormatModule` lleva valor por defecto (la CI compila el crate de Carlos contra el `viewer-core` actual). El próximo parámetro de carga o de diff va en un struct de pedido con método por defecto, no en otro `*_with`.
5. **JSON:** una sola forma tipada (v2) en todos los comandos. Un cambio incompatible sube la versión del schema; un campo nuevo opcional no.
6. **Salida estable:** un refactor no cambia el texto ni el JSON de la CLI; se compara contra el binario anterior. Las áreas de layouts se verifican contra KLayout.
7. **Cache de layouts:** la clave incluye la versión de `riku-mod-layout`; si cambia la salida del diff, se sube esa versión.
8. **Hilos:** un solo pool de `rayon` por proceso (`--jobs`/`RIKU_JOBS`); `tokio` solo para la carga del visor. Nunca bloquear un hilo de `rayon` esperando a otro (semáforo, `Condvar`): la memoria se planifica antes, en tandas (`core/analysis/parallel.rs`, la mitad de `MemAvailable`).
9. **Motor entre hilos:** `gdstk_rust` es `Send + Sync` porque todo se escribe al cargar (`finish_load`); una caché nueva en el motor se llena al cargar o usa `OnceLock`.
10. **Magic:** donde Magic y KLayout difieren, manda Magic; se compara en capas de Magic; el oráculo es KLayout ≥ 0.30.12.
11. **El visor solo dibuja** en su hilo: cargas, resúmenes y E/S van en otro hilo, despiertan la UI (`request_repaint`) y se pueden cancelar.
12. **Medir antes de optimizar**, y no agregar lo que no se pagó: sin R-tree/BVH (alcanzan las grillas y la pirámide), sin partir `GitRepository`, sin plugins dinámicos. `FileFormat` es un enum cerrado: aceptable con tres formatos.

## Rendimiento

Cómo se llega a comparar un chip de 42 MB (6,2 millones de polígonos) en 1,4–2,3 s con menos de 1 GB, y a dibujarlo en ~2 ms por cuadro:

- **Huella jerárquica** (árbol de Merkle sobre la jerarquía): dos celdas con la misma huella aplanan a lo mismo y se descartan sin aplanar.
- **Instancias gemelas:** en una celda que difiere, las instancias iguales en las dos versiones se cancelan; solo se aplanan las demás, **por pedazos** (nunca el chip entero en memoria).
- **Huella por capa** en forma canónica (vértices cuantizados, sin repetidos, antihorario, desde el menor): dos polígonos con el mismo hash de 64 bits son iguales (riesgo aceptado). Una capa igual se salta sin XOR.
- **XOR solo de lo que cambió**, y **por cuadrantes** (quadtree) cuando hay más de 2 000 polígonos: Clipper es casi cuadrático con miles de rectángulos alineados (una capa de relleno: 358 s → 0,28 s). El conteo de polígonos puede cambiar por los bordes; las áreas no.
- **Visor:** cada backend arma un `SceneIndex` al cargar (grillas por tamaño, triangulación una vez, pirámide de cobertura). Por cuadro se consulta solo lo visible; lo que mide menos de un píxel se pinta como una imagen por capa. `malloc_trim` al terminar devuelve lo del aplanado.
- **Historial:** `log`, `show` y `status` reparten commits y archivos, con una conexión a Git por hilo.

`RIKU_PROFILE=1` imprime tiempos del diff y de cada cuadro; `riku-mod-layout/examples/profile_*` miden cada etapa (ver [Medir](#medir-riku-mod-layoutexamples)).

## Verificación

Los scripts de `tools/verify/` comprueban que Riku lee los layouts igual que KLayout, que sus transistores y redes coinciden con las netlists del PDK (Netgen) y con Magic, y prueban el visor sin mouse. No corren en la CI porque necesitan KLayout, Magic, Netgen, los PDK y un servidor X. Se corren a mano en el contenedor **iic-osic-tools**, antes de tocar el render, las etiquetas o el diff.

```bash
docker exec -it <contenedor-iic-osic-tools> bash
cd /foss/designs/riku_chip
tools/verify/compare.sh
```

### Geometría, etiquetas y XOR

| Script | Qué hace |
|---|---|
| `compare.sh` | Compila `verify_dump` (ejemplo de `riku-mod-layout`), vuelca cada librería con Riku y con KLayout y compara los dos textos. Sale con código 1 si hay diferencias |
| `klayout_dump.py` | Lado KLayout del volcado (`cells` y `xor`) |
| `riku-mod-layout/examples/verify_dump.rs` | Lado Riku, mismo formato |

```bash
tools/verify/compare.sh                          # SKY130, GF180 e IHP (librerías de celdas estándar)
tools/verify/compare.sh mi_chip.gds otro.oas     # librerías propias (GDSII u OASIS)
tools/verify/compare.sh --xor a.gds b.gds CELDA  # área añadida/eliminada por capa
```

El volcado de `cells` tiene, por cada top cell:
- `BBOX`, o `BBOX empty` si la celda no tiene geometría;
- por capa, el número de polígonos y el área (µm², sin fusionar solapes);
- cada etiqueta de toda la jerarquía con su posición en la celda raíz.

Los archivos quedan en `$OUT` (por defecto `/tmp/riku-verify`) para revisar diferencias con `diff`.

Variables: `PDK_ROOT` (por defecto `/foss/pdks`), `OUT` y `CARGO_TARGET_DIR`.

Resultados de referencia (2026-09): idéntico a KLayout en todo.

| Qué | Dónde |
|---|---|
| Geometría por celda: bbox, polígonos y área por capa, labels con posición | 437 celdas de `sky130_fd_sc_hd`, 230 de `gf180mcu_fd_sc_mcu7t5v0`, 78 de `sg13g2_stdcell` (~30 s) |
| Lo mismo leyendo OASIS | `hier_inv_b.oas` |
| XOR por capa | `inv_1` con met1 añadido, mcon borrado, poly movido 0,05 µm, licon movido 5 nm |
| XOR jerárquico | met1 dentro de `inv_2` → `macro_sparecell`; AREF 3×2 |
| Layout de 42 MB | reexportado sin cambios, y con cambios en las capas 6/0 y 19/0 |

### Magic (`mag/`)

`mag/compare_mag.sh` compara jerarquías `.mag` leídas por `gdstk-rs` (ejemplo `mag_area`) y por KLayout (`mag/klayout_mag_area.py`): aplanadas y sin unir, la **misma cantidad de polígonos** y la **misma suma de áreas** por capa (exacto y rápido aun en jerarquías de cientos de celdas).

```bash
tools/verify/mag/compare_mag.sh                         # 8 jerarquías de SKY130 y GF180
tools/verify/mag/compare_mag.sh top.mag 0.01 DIR...     # una propia: lambda en µm y dónde buscar sub-celdas
```

`mag/mag_bench.sh` mide el diff de una jerarquía real (la librería `sky130_fd_io` en un repo en `/tmp`, con una sub-celda editada y otra re-escrita en tiras): tiempo, memoria y cantidad de cambios.

Hace falta **KLayout 0.30.12 o más nuevo** (0.30.4 y anteriores ignoran `magscale`): `python3 -m venv /tmp/kl && /tmp/kl/bin/pip install klayout==0.30.12` y `KLAYOUT_PY=/tmp/kl/bin/python`. Las capas que Riku deja fuera a propósito (`checkpaint`, `error_*`…) no cuentan como diferencia.

### Transistores (`tools/verify/devices/`)

Los transistores que reconoce Riku (modelo, W y L de cada finger; ver [`docs/formatos.md`](formatos.md#transistores-y-redes)) contra la netlist de referencia de las celdas estándar de cada PDK, celda por celda:

```bash
cargo build --release -p riku-mod-layout --example devices
tools/verify/devices/compare_stdcells.sh "$CARGO_TARGET_DIR/release/examples/devices"
```

Resultados de referencia (2026-09): SKY130 **437 de 437** celdas iguales (GDS y `.mag`), GF180MCU 228 de 229, IHP SG13G2 73 de 74. Las dos que difieren (`gf180mcu_fd_sc_mcu7t5v0__clkbuf_1`, `sg13g2_dfrbp_1`) son de la netlist del PDK, que no coincide con su layout: Riku da lo mismo que el extractor de KLayout (`klayout_gates.py <gds> <celda> <difusión> <poly>`). También la SRAM de `examples/GDS/`: 2271 transistores, W y L iguales a KLayout uno por uno.

### Redes (`tools/verify/nets/`)

La netlist que extrae Riku (redes, transistores y resistores; ver [`docs/formatos.md`](formatos.md#transistores-y-redes)) contra la de referencia de las celdas estándar, con **Netgen** y el `setup.tcl` de cada PDK: compara la topología entera, como un LVS.

```bash
cargo build --release -p riku-mod-layout --example nets
tools/verify/nets/compare_stdcells.sh "$CARGO_TARGET_DIR/release/examples/nets"
tools/verify/nets/magic_vs_riku.sh sky130A <layout.gds> <celda…>   # Magic como segundo oráculo
```

Resultados de referencia (2026-09): GF180MCU **219 de 219** (GDS y `.mag`), SKY130 422 de 427 (GDS) y 423 de 426 (`.mag`), IHP SG13G2 68 de 73; sin contar las celdas sin transistores. Las que difieren están listadas en `compare_stdcells.sh`, y en todas la extracción de Magic da lo mismo que Riku: la netlist del PDK no coincide con su layout (pines que el GDS no dibuja, pilas en otro orden) o le falta un resistor de metal que el layout tiene (`probe_p_8`).

### Medir (`riku-mod-layout/examples/`)

- `profile_diff a.gds b.gds`: tiempo y memoria de cada etapa del diff (`SKIP_FP=1`, `PRINTS=1`, `CANON=1` para diagnósticos).
- `profile_view layout.gds`: cuánto tarda el visor en armar la escena y el índice.
- `profile_prints layout.gds [celda] [hilos]`: la huella por pedazos (reparto, tiempos, memoria, escalado por hilos).
- `profile_xor a.gds b.gds <celda> <layer> <datatype>`: el XOR de una capa, entero y por cuadrantes (`SKIP_WHOLE=1` si el entero tarda minutos).
- `verify_dump`: el volcado que usa `compare.sh`; `mag_area` (en `external/gdstk/rust/examples`), el de `compare_mag.sh`.
- `devices layout [celda…]` (o una carpeta de `.mag`): los transistores de cada celda, el volcado de `compare_stdcells.sh`.

Correrlos en release y sobre una copia en `/tmp`: un montaje lento distorsiona los tiempos.

### Comparación visual

```bash
python3 tools/verify/klayout_snapshot.py \
  /foss/pdks/sky130A/libs.ref/sky130_fd_sc_hd/gds/sky130_fd_sc_hd.gds \
  /foss/pdks/sky130A/libs.tech/klayout/tech/sky130A.lyp \
  /tmp/klayout_inv1.png sky130_fd_sc_hd__inv_1 800 600
```

Genera la captura de KLayout con la paleta oficial, para ponerla al lado de una del visor abierto en la misma celda (`riku gui <archivo> --cell <celda>`).

### Pruebas de la GUI sin mouse (`gui/`)

`xt.py` usa XTest para simular clics, arrastres, rueda y teclado sobre la ventana del visor, y captura la ventana con `xwd`:

```bash
env -u WAYLAND_DISPLAY cargo run --release -- gui archivo.gds &   # XWayland para poder capturar
W=$(xwininfo -root -tree | grep 'riku-gui")' | awk '{print $1}')
python3 tools/verify/gui/xt.py $W raise
python3 tools/verify/gui/xt.py $W click 120 80
python3 tools/verify/gui/xt.py $W drag 600 400 500 350 300     # arrastre de 300 ms
python3 tools/verify/gui/xt.py $W keytap f                     # atajo "encuadrar"
python3 tools/verify/gui/xt.py $W shot /tmp/riku.png
```

`gui/xwd2png.py` convierte una captura `xwd` suelta a PNG. Para matar la GUI usar `pkill -f "riku gui"` (cuidado: `-f` también mata la shell que lo lanzó si su línea contiene ese texto).

## CI y release

- **CI** (`.github/workflows/ci.yml`): tests del workspace con `-D warnings`, cada combinación de features, `riku-kernel` sin motores, y el crate de Carlos (`viewer-core-compat`) contra el `viewer-core` actual.
- **Release** (`release.yml`, con cada tag `v*`): primero la CI entera sobre ese commit (si falla, no se publica nada); después el binario estático (solo depende de glibc) en Ubuntu 22.04, pruebas de humo, y publica `riku-<versión>-linux-x86_64.tar.gz` (con `install.sh`), `riku_<versión>-1_amd64.deb` y `SHA256SUMS`.
- **Publicar:** subir `version` en `riku/Cargo.toml`, commitear, `git tag -a vX.Y.Z -m "Riku X.Y.Z" && git push origin vX.Y.Z`, y escribir las notas en GitHub (lo incompatible primero). **Actions → Release → Run workflow** lo prueba sin publicar.

## Traducciones

Los textos están en `riku/locales/` (`en.yml` por defecto y de respaldo, `es.yml`). Agregar un idioma: copiar `en.yml` a `<código>.yml`, traducir los valores (nunca las claves ni las variables `%{…}`), poner su nombre en `lang.name` y correr `cargo test -p riku i18n`, que dice qué falta o sobra. Un texto nuevo en el código: `tr!("seccion.clave")` y la clave en **todos** los `.yml`. Los mensajes del núcleo y de los módulos todavía están en español en el código.

## Commits

Formato convencional, `tipo(alcance): descripción` (`feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `build`, `chore`). Antes de un PR, `cargo test --workspace` en verde.
