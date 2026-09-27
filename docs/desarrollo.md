# Desarrollo

## Entorno

**Linux x86_64** es la plataforma oficial. Lo más simple es el contenedor [iic-osic-tools](https://github.com/iic-jku/iic-osic-tools): trae zlib, qhull, KLayout y los PDKs en `/foss/pdks`; basta `rustup default stable`. Fuera del contenedor: Rust estable, toolchain C++ y los paquetes `zlib1g-dev` y `libqhull-dev` (Debian/Ubuntu) o equivalentes. Para el visor, `libxkbcommon-dev`, `libwayland-dev` y `libgl1-mesa-dev`.

```bash
git clone --recurse-submodules https://github.com/riku-chip/riku_chip
cd riku_chip                      # (sin --recurse-submodules: git submodule update --init --recursive)
cargo build --release             # target/release/riku: CLI, shell y visor
cargo build --release -p riku --no-default-features    # solo terminal
cargo install --path riku         # instalarlo en ~/.cargo/bin
```

Es un workspace: un `Cargo.lock` y un `target/` para `riku`, `riku-kernel`, `riku-mod-layout` y `viewer-core`. Los submódulos (`external/`) entran como dependencias por path.

**Windows** no es una plataforma soportada ni se prueba en la CI: con MSVC 2019 local falla el linker de gdstk-rs.

## Tests

```bash
cargo test --workspace            # todo (la CI lo corre con RUSTFLAGS="-D warnings")
cargo test -p riku                # CLI, análisis, visor, módulos; tests/basic.rs, gds_e2e.rs, stress.rs
cargo test -p riku-mod-layout     # diff de layouts, cache, paletas, escena
cargo test -p viewer-core         # contrato del visor e índice espacial
```

Los fixtures de layouts (`riku-mod-layout/tests/fixtures/*.gds`, `.oas`) se generan con los scripts Python de esa misma carpeta.

## CI (`.github/workflows/ci.yml`)

| Job | Qué hace |
|---|---|
| `test (linux)` | tests del workspace con `-D warnings`; compila cada combinación de features; verifica con `cargo tree` que `riku-kernel` no dependa de ningún motor |
| `xschem-viewer-rust (viewer-core-compat)` | el crate de Carlos compila contra el `viewer-core` actual |

## Release (`.github/workflows/release.yml`)

Con cada tag `v*`: compila en Ubuntu 22.04 con zlib, qhull y libstdc++ estáticas (`GDSTK_STATIC=1`), verifica que el binario solo dependa de glibc, corre pruebas de humo y publica `riku-<versión>-linux-x86_64.tar.gz` (con `install.sh`, entrada de menú e icono de `packaging/`), `riku_<versión>-1_amd64.deb` y `SHA256SUMS`. El binario ocupa ~13 MB instalado; los paquetes, ~5 MB.

## Herramientas

| Dónde | Para qué |
|---|---|
| `tools/verify/` | comparar la lectura de layouts y el XOR contra KLayout; capturas lado a lado; `gui/xt.py` maneja el visor con clics y teclas simulados (ver su [README](../tools/verify/README.md)) |
| `tools/palettes/gen_palettes.py` | generar las tablas de capas de GF180 e IHP desde sus `.lyp` |
| `riku-mod-layout/examples/` | `profile_diff`, `profile_prints`, `profile_xor`, `profile_view`, `verify_dump` (ver [`layouts.md`](layouts.md)) |

**Capturas reproducibles del visor:** `xt.py` usa `$DISPLAY`, así que se puede correr sobre un X virtual (`Xvfb :99 &`, `DISPLAY=:99`) sin depender del escritorio. El zoom con la rueda es animado y depende del tiempo entre cuadros; para comparar capturas usar **+**/**−**, que siempre terminan en el mismo zoom. `RIKU_PROFILE=1` imprime el tiempo de cada cuadro.

## Commits

Formato convencional: `tipo(alcance): descripción` (`feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `chore`). Antes de un PR: `cargo test --workspace` en verde.
