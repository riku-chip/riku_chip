# Desarrollo

Cómo compilar, probar y publicar Riku, cómo está armado y las reglas que hay que respetar al cambiarlo. Lo que falta hacer está en [`pendientes.md`](pendientes.md).

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

- Los fixtures de layouts (`riku-mod-layout/tests/fixtures/`) se generan con los scripts Python de esa carpeta; los de Magic están escritos en los tests.
- El lector de Magic vive en `external/gdstk/rust` (`cargo test --test magic`); `pdk_corpus` y `klayout_testdata` están ignorados porque necesitan archivos de afuera (ver ese repo).
- Verificación contra KLayout, mediciones y pruebas del visor sin mouse: [`tools/verify/`](../tools/verify/README.md).
- Medir `log`, `status` o el visor en una copia en `/tmp`: un repo en un montaje lento (9p, red) distorsiona los tiempos.

## Arquitectura

**Monolito modular con microkernel:** un solo ejecutable, varios crates. Un núcleo que no conoce ningún formato define los contratos y cada formato es un módulo que se registra en él, enlazado al compilar (features de Cargo, no `.so`: Rust no tiene ABI estable).

```
riku/                 ejecutable
  src/core/           git (git2), análisis (diff_pair, diff_set, show, log, status), repo_path
  src/modules/        módulos de formato; mod.rs::registry() es el ÚNICO lugar que los lista
  src/cli/, src/gui/  CLI + shell y visor egui (feature `gui`)
riku-kernel/          tipos de cambio (FileChange, Change, Element, Detail), FormatModule, Registry
riku-mod-layout/      GDS/OASIS/Magic: diff geométrico, cache, estilo por PDK, GdsBackend
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

`RIKU_PROFILE=1` imprime tiempos del diff y de cada cuadro; `riku-mod-layout/examples/profile_*` miden cada etapa (ver `tools/verify/`).

## CI y release

- **CI** (`.github/workflows/ci.yml`): tests del workspace con `-D warnings`, cada combinación de features, `riku-kernel` sin motores, y el crate de Carlos (`viewer-core-compat`) contra el `viewer-core` actual.
- **Release** (`release.yml`, con cada tag `v*`): primero la CI entera sobre ese commit (si falla, no se publica nada); después el binario estático (solo depende de glibc) en Ubuntu 22.04, pruebas de humo, y publica `riku-<versión>-linux-x86_64.tar.gz` (con `install.sh`), `riku_<versión>-1_amd64.deb` y `SHA256SUMS`.
- **Publicar:** subir `version` en `riku/Cargo.toml`, commitear, `git tag -a vX.Y.Z -m "Riku X.Y.Z" && git push origin vX.Y.Z`, y escribir las notas en GitHub (lo incompatible primero). **Actions → Release → Run workflow** lo prueba sin publicar.

## Traducciones

Los textos están en `riku/locales/` (`en.yml` por defecto y de respaldo, `es.yml`). Agregar un idioma: copiar `en.yml` a `<código>.yml`, traducir los valores (nunca las claves ni las variables `%{…}`), poner su nombre en `lang.name` y correr `cargo test -p riku i18n`, que dice qué falta o sobra. Un texto nuevo en el código: `tr!("seccion.clave")` y la clave en **todos** los `.yml`. Los mensajes del núcleo y de los módulos todavía están en español en el código.

## Commits

Formato convencional, `tipo(alcance): descripción` (`feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `build`, `chore`). Antes de un PR, `cargo test --workspace` en verde.
