# Diseño: ejecutable único `riku` para Linux

Un solo programa `riku` con todo adentro (CLI, shell, visor y todos los módulos), instalable copiando un archivo, con Linux como plataforma oficial.

**Fecha:** 2026-09-26 · **Base:** `main` `1621fa3`

---

## 1. Situación actual

### 1.1 Dos ejecutables que repiten lo mismo

```
  riku  (3,3 MB)                          riku-gui  (11 MB)
  ┌──────────────────────────┐            ┌──────────────────────────┐
  │ CLI · shell · diff        │            │ visor egui               │
  │ riku (lib)                │            │ riku (lib)          ◄─── copia
  │ gds-renderer              │            │ gds-renderer        ◄─── copia
  │ gdstk-rs + gdstk C++      │            │ gdstk-rs + gdstk C++◄─── copia
  │ xschem-viewer-rust        │            │ xschem-viewer-rust  ◄─── copia
  │ viewer-core               │            │ viewer-core         ◄─── copia
  └──────────────────────────┘            └──────────────────────────┘
          │  riku open / diff -f visual busca y lanza  ▲
          └────────────────────────────────────────────┘
```

**Aclaración:** ninguna librería está dos veces *dentro de un mismo* ejecutable. Rust enlaza cada librería una vez por programa. Lo que pasa es que hay **dos programas** y cada uno lleva su propia copia de gdstk, del motor de Xschem y del resto: 14,3 MB en total, con unos 3 MB repetidos entre ambos. Verificado con `nm` sobre los binarios de release: los dos contienen `gdstk::read_gds`, `xschem_viewer` (252–305 funciones) y `gds_renderer`.

Además:
- `riku` tiene que *encontrar* `riku-gui` (variable de entorno, carpeta vecina, `target/` o `cargo run`). Si no lo encuentra, `riku open` falla.
- Cada crate compila gdstk (C++) por su cuenta: 4 veces en la CI y en local.

### 1.2 Dependencias del sistema hoy (`ldd` del binario de release)

| Librería | Por qué | ¿Hace falta? |
|---|---|---|
| `libssl`, `libcrypto` | `git2` con sus features por defecto (https/ssh para clonar) | **No**: Riku solo lee repos locales (no usa remotos ni `fetch`) |
| `libz` | gdstk (GDS comprimido, OASIS) | sí, pero puede ir **dentro** del binario (`libz.a` existe) |
| `libqhull_r` | gdstk (envolvente convexa) | sí, pero puede ir **dentro** del binario (compilando qhull estático) |
| `libstdc++`, `libgcc_s` | el C++ de gdstk | puede ir dentro (`-static-libstdc++`) |
| `libc`, `libm` | todo programa | **sí, del sistema** (glibc no se enlaza estática) |
| X11 / Wayland / OpenGL / xkbcommon | ventana del visor | **del sistema**, pero se cargan recién al abrir la ventana (winit y glutin usan `dlopen`), así que la CLI funciona en un servidor sin gráficos |

**glibc mínima:** compilado en Ubuntu 24.04, el binario pide `GLIBC_2.39` y no corre en Ubuntu 22.04. La versión de release se compila en **Ubuntu 22.04** (glibc 2.35), que cubre Ubuntu 22.04+, Debian 12+, Fedora 36+ y el contenedor iic-osic-tools.

---

## 2. Objetivo

```
  riku  (~12 MB, un archivo)
  ┌───────────────────────────────────────────────┐
  │ main(): decide qué modo arrancar               │
  │   ├─ riku …          → CLI / shell (riku lib)  │
  │   └─ riku gui …      → visor (módulo riku::gui) │
  │ riku (núcleo + cli + gui) · gds-renderer       │
  │ gdstk-rs + gdstk C++ · xschem-viewer-rust      │
  │ zlib · qhull · libstdc++   (estáticos)         │
  └───────────────────────────────────────────────┘
      depende del sistema solo de: glibc ≥ 2.35
      y, al abrir la ventana: X11 o Wayland + OpenGL
```

| Requisito | Criterio |
|---|---|
| Un archivo | `riku` es lo único que se instala; nada que "encontrar" |
| Todo adentro | gdstk-rs, xschem-viewer-rust, gds-renderer, viewer-core, riku y el visor |
| CLI igual que hoy | `riku diff/log/status/doctor/open`, el shell y `--json` sin cambios |
| Shell usable con el visor abierto | `open` dentro del shell no bloquea el prompt |
| Portable en Linux | corre en Ubuntu 22.04+ sin instalar paquetes, salvo el escritorio gráfico para el visor |
| Tamaño | ~12 MB sin símbolos, ~5 MB comprimido (estimado; se mide en la implementación) |
| Headless | la CLI funciona sin `DISPLAY` (servidores, CI) |

---

## 3. Arquitectura

### 3.1 Crates: tres alternativas evaluadas

Hechos del proyecto que deciden:
- `riku-gui` depende de `riku`, pero solo usa **5 cosas del núcleo**: `GitService`, `GitRepository`, `DiffView`, `XschemDriver`/`parse` y los modelos `DiffReport`/`ChangeKind`. Son unas 4 000 líneas en 12 archivos.
- `riku` ya separa la librería (`src/lib.rs`) del binario (`src/main.rs`, una línea: `riku::cli::run()`).
- Los tests end-to-end de la CLI usan `CARGO_BIN_EXE_riku`: necesitan que el binario `riku` salga del paquete `riku`.
- `riku-gui` declara `gdstk-rs` como dependencia y no lo usa (se elimina en cualquier opción).
- En un workspace, dos paquetes que producen un binario con el mismo nombre (`riku`) chocan en `target/` (cargo lo reporta como colisión de salida).

| | A. Crate nuevo `riku-app` | B. Visor como módulo de `riku` (feature `gui`) | C. `riku-gui` produce el binario `riku` |
|---|---|---|---|
| Estructura | `riku-app` → `riku` + `riku-gui` | `riku` contiene `src/gui/` (hoy `riku-gui/src/`) | `riku-gui` bin `riku` → `riku::cli::run()` |
| Crates de producto | 3 (`riku`, `riku-gui`, `riku-app`) | **1** (`riku`) | 2 |
| Choque de binarios `riku` | sí: hay que quitar o renombrar el bin de `riku` y mover los e2e | **no** | sí: el mismo problema |
| Ciclo de dependencias | se evita con el crate extra | **no existe** (el visor usa el núcleo desde adentro) | se evita |
| Build sin gráficos (servidor, tests rápidos del núcleo) | `cargo build -p riku` | `cargo build --no-default-features` | `cargo build -p riku` |
| Qué se mueve | nada; se agrega un crate | `riku-gui/src/*` → `riku/src/gui/` (`git mv`, conserva el historial) | nada, pero el nombre engaña ("riku-gui" produce "riku") |
| Lanzador `cli/gui.rs` (buscar otro binario, `cargo run` de respaldo) | se reemplaza por re-ejecución | **se reemplaza por re-ejecución**, y se borra la búsqueda | se reemplaza |

**Recomendación: B.** Revisando el proyecto, la primera versión de este diseño proponía A, y tenía un costo no visto: el choque de nombres obliga a mover los tests e2e y a mantener dos binarios `riku` distintos. Con B:
- queda **un solo crate de producto y un solo binario**, sin crate extra ni lanzador que busque programas;
- el visor sigue aislado como módulo (`riku::gui`) detrás de la feature `gui`, que viene activada por defecto. El núcleo no depende de él;
- los tests de la CLI no cambian;
- el costo es un `git mv` de 12 archivos (el historial se sigue con `git log --follow`).

```
riku (paquete único, bin "riku")
 ├─ src/core/, src/adapters/   núcleo (sin cambios)
 ├─ src/cli/                   CLI + shell (sin cambios, salvo el lanzador)
 └─ src/gui/                   visor  ← hoy riku-gui/src/   [feature "gui"]
      usa: eframe, tokio, poll-promise, earcutr, viewer-core,
           xschem_viewer/viewer-core-compat
 depende de: gds-renderer → gdstk-rs → gdstk C++ ; viewer-core ← xschem-viewer-rust
```

```toml
# riku/Cargo.toml
[features]
default = ["gui"]
gui = ["dep:eframe", "dep:tokio", "dep:poll-promise", "dep:earcutr", "dep:viewer-core",
       "xschem_viewer/viewer-core-compat"]
```

- Los 43 tests del visor pasan a ser `riku::gui::…` y siguen corriendo con `cargo test`. En la CI se agrega una pasada `--no-default-features` para asegurar que el núcleo compila sin el visor.
- `riku-gui/` se elimina. Quien tenga el hábito `cargo run` dentro de `riku-gui` pasa a `cargo run -- gui [archivo]` en `riku/`.

### 3.2 Modos de arranque

```
riku                         → shell interactivo          (riku lib)
riku diff A B chip.gds       → diff en la terminal         (riku lib)
riku open chip.gds           → lanza "riku gui chip.gds"   (proceso hijo, no bloquea)
riku diff A B chip.gds -f visual
                             → lanza "riku gui --repo … --commit-a A --commit-b B chip.gds"
riku gui [args]              → visor en este proceso       (riku::gui)   
riku --version / --help      → como hoy (+ "gui" en la ayuda)
```

- **Por qué un proceso hijo y no abrir la ventana en el mismo proceso:** egui necesita el hilo principal y bloquea hasta que se cierra la ventana. Dentro del shell, `open` dejaría el prompt congelado. Re-ejecutar el mismo archivo (`current_exe()`) no necesita buscar nada y mantiene el shell libre. El costo es un `exec` del mismo binario, que ya está en caché del disco.
- **`riku gui`** queda en la ayuda como "abrir el visor" (subcomando de clap `Gui { args: Vec<String> }` con `trailing_var_arg`). Así `riku gui chip.gds --cell inv_1` también sirve directo.
- `LaunchArgs::parse_args()` del visor pasa a recibir los argumentos (`parse_from(iter)`) en lugar de leer `std::env::args()`, para que funcione detrás de `riku gui`.
- Si no hay `DISPLAY` ni `WAYLAND_DISPLAY`, `riku gui` termina con un mensaje claro ("el visor necesita un escritorio gráfico; la CLI funciona igual") en lugar del error crudo de winit.
- Compilado sin la feature `gui` (`--no-default-features`), `riku gui` y `open` responden "esta versión de riku se compiló sin visor" y el resto funciona igual.

### 3.3 Workspace de Cargo en la raíz (recomendado, commit aparte)

Hoy cada crate tiene su `Cargo.lock` y su `target/`: gdstk se compila 4 veces y las versiones de dependencias pueden divergir.

```toml
# Cargo.toml (raíz)
[workspace]
resolver = "2"
members = ["viewer-core", "gds-renderer", "riku"]
exclude = ["external"]          # los submódulos siguen compilando solos

[profile.release]
lto = "thin"          # menos tamaño y un poco más rápido
codegen-units = 1
strip = true          # sin símbolos de depuración: el tamaño que se distribuye
panic = "unwind"      # se mantiene: la GUI captura panics de carga
```

- **Beneficios:** un solo `Cargo.lock` y un solo `target/`, así que gdstk y egui se compilan una vez (CI y builds locales más rápidos). `cargo test --workspace` corre todo.
- **Riesgo:** los submódulos quedan dentro del árbol del workspace y, sin `exclude`, cargo se quejaría al compilarlos solos (el job `xschem-compat` lo hace). Se verifica en la CI antes de mergear.
- Los 4 `Cargo.lock` actuales se reemplazan por el de la raíz.

### 3.4 Dependencias estáticas (build de release)

| Qué | Cómo | Dónde |
|---|---|---|
| OpenSSL | `git2 = { version = "0.20", default-features = false }` | `riku/Cargo.toml` |
| zlib | `ZLIB_STATIC=1` → `build.rs` pide `libz.a` a pkg-config (`statik(true)`) | `gdstk-rs/build.rs` |
| qhull | Ubuntu no trae `libqhull_r.a`: el job de release compila qhull 8.0.2 con CMake (`BUILD_SHARED_LIBS=OFF`) y lo pasa con `QHULL_DIR` + `QHULL_STATIC=1` | `release.yml` + `build.rs` |
| libstdc++ | `GDSTK_STATIC_STDCXX=1` → `cargo:rustc-link-arg=-static-libstdc++` y `-static-libgcc` | `gdstk-rs/build.rs` |

Todo es **opt-in por variable de entorno**: el build de desarrollo sigue usando las librerías del sistema y no cambia para nadie.

**Verificación automática** en el job de release: `ldd riku` solo puede listar `libc`, `libm`, `libdl`, `libpthread`, `librt`, `ld-linux` y `linux-vdso`. Cualquier otra librería hace fallar el job.

### 3.5 Empaquetado y publicación

`.github/workflows/release.yml`, disparado por un tag `v*`:

```
ubuntu-22.04
 ├─ compilar qhull estático
 ├─ cargo build --release -p riku      (con las variables de §3.4)
 ├─ verificar ldd (lista permitida)
 ├─ smoke tests:  riku --version
 │                riku diff … sobre un repo de prueba (-f json)
 │                riku gui --help   (sin DISPLAY: debe salir con el mensaje claro)
 ├─ empaquetar
 │   riku-<ver>-linux-x86_64.tar.gz
 │     ├─ riku
 │     ├─ install.sh        (copia a ~/.local/bin o /usr/local/bin con --system)
 │     ├─ riku.desktop + riku.svg   (entrada de menú "Riku" → riku gui)
 │     ├─ README.md · LICENSE
 │   riku_<ver>_amd64.deb   (cargo-deb: binario, .desktop, icono; sin dependencias extra)
 └─ publicar en GitHub Releases con SHA256SUMS
```

- **AppImage: no hace falta.** Con §3.4 el binario ya es portable, y un AppImage solo agregaría ~3 MB del runtime sin ganar compatibilidad. Si algún día hay que meter librerías gráficas, se reconsidera.
- **Instalación para el usuario:**
  ```bash
  tar xf riku-1.0.0-linux-x86_64.tar.gz && ./riku-1.0.0/install.sh
  riku            # desde cualquier carpeta
  ```
  o `sudo apt install ./riku_1.0.0_amd64.deb`.
- **Versión:** `riku --version` muestra la versión del crate, más el commit de riku_chip y de cada submódulo (`gdstk_rust ca86886`, `xschem-viewer-rust 1496863`) inyectados por un `build.rs` de `riku`. Así un reporte de error dice exactamente qué código lleva el ejecutable.

### 3.6 Datos que no van en el ejecutable

- **Símbolos de Xschem de los PDKs:** los lee de `$PDK_ROOT` / `.xschemrc` como hoy (son del usuario y pesan cientos de MB).
- **Preferencias del visor** (tema, recientes): `~/.local/share/riku-gui/` (eframe). Se mantiene el `app_id`, así que no se pierden al pasar al binario único.
- **Cache del diff:** `~/.cache/riku/diff`.

---

## 4. Plataformas

| Plataforma | Estado propuesto |
|---|---|
| **Linux x86_64** | **Oficial**: release, instaladores y soporte |
| Linux aarch64 | Posible más adelante (mismo workflow con `ubuntu-22.04-arm`); no se promete |
| Windows | **Sin soporte oficial.** El job de CI (hoy en verde) queda como alerta temprana, sin instaladores |
| macOS | Sin soporte |

---

## 5. Fases de implementación

| Fase | Contenido | Commit | Verificación |
|---|---|---|---|
| 1 | `git mv riku-gui/src riku/src/gui` + feature `gui` + dependencias opcionales; quitar `gdstk-rs` sin uso | `refactor` | `cargo test` (núcleo + 43 del visor) y `cargo build --no-default-features` |
| 2 | Subcomando `gui` (`parse_from`), re-ejecución en `open` y `-f visual`, borrar la búsqueda de `riku-gui` | `feat` | `riku open` desde el shell no bloquea; `riku gui chip.gds --cell X` abre; sin `DISPLAY`, mensaje claro |
| 3 | Workspace raíz + perfil de release (lto, strip) | `build` | CI verde; un solo `target/`; tamaño medido |
| 4 | `git2` sin OpenSSL + opciones estáticas en gdstk-rs (commit en gdstk_rust) | `build` | `ldd` solo muestra glibc |
| 5 | `release.yml` (tar.gz, .deb, SHA256SUMS) + `install.sh` + `.desktop` | `ci` | tag de prueba `v0.1.0-rc1` publica los artefactos; instalación en un contenedor Ubuntu 22.04 limpio |
| 6 | README (instalación), `pendientes.md` (#3 cerrado; Windows solo como alerta) | `docs` | — |

**Riesgos:**

| Riesgo | Mitigación |
|---|---|
| El workspace choca con los submódulos | `exclude = ["external"]`; el job `xschem-compat` lo verifica |
| El qhull estático necesita `-fPIC` o nombres distintos | se compila con `CMAKE_POSITION_INDEPENDENT_CODE=ON`; `build.rs` acepta `qhullstatic_r` |
| `-static-libstdc++` con el C++ de gdstk | es el uso típico de ese flag; el smoke test lo valida con un diff real |
| El tamaño supera lo estimado | `lto`/`strip` lo compensan; se mide en la Fase 3 y se reporta |
| Scripts que llaman a `riku-gui` directo | el `.deb` y `install.sh` crean un enlace `riku-gui` → script que ejecuta `riku gui "$@"`, durante una versión de transición |
