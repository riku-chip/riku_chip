# Riku

Riku es una herramienta de VCS semantico para diseno de chips. Hoy trabaja sobre esquematicos Xschem (`.sch`), lee Git directamente con `git2` y muestra diffs semanticos y visuales sobre el contenido real del archivo, no sobre el texto crudo del commit.

## Estado actual

- Un solo driver implementado: Xschem `.sch`
- Comandos: `diff`, `log`, `doctor` y `render`
- Salidas de `diff`: `text`, `json` y `visual`
- Cache de renders por SHA-256 en el cache del sistema
- Resolucion de simbolos via `.xschemrc`, `PDK_ROOT` y `PDK`
- `GitService` lee blobs y commits sin hacer checkout

## Que hace

- `diff` compara dos commits de un archivo y reporta componentes y nets agregados, eliminados o modificados.
- `log` lista commits y puede resumir cambios semanticos por revision.
- `doctor` verifica el repo Git, el PDK y el directorio de cache.
- `render` genera SVG del `.sch` y lo abre con el visor del sistema.

## Instalacion

Desde la carpeta `riku/`:

```bash
cargo build --release
cargo test
```

El proyecto usa Rust con edition 2024 y descarga automaticamente sus dependencias, incluyendo `xschem-viewer` desde GitHub.

## Uso rapido

```bash
cargo run -- diff HEAD~1 HEAD ../examples/SH/op_sim.sch
cargo run -- diff HEAD~1 HEAD ../examples/SH/op_sim.sch --format json
cargo run -- diff HEAD~1 HEAD ../examples/SH/op_sim.sch --format visual
cargo run -- log ../examples/SH/op_sim.sch --semantic --limit 10
cargo run -- doctor
cargo run -- render ../examples/SH/op_sim.sch
```

Tambien puedes usar el binario instalado:

```bash
cargo install --path .
riku diff HEAD~1 HEAD ../examples/SH/op_sim.sch
```

## Estructura

```text
riku/
  src/
    main.rs
    cli.rs
    lib.rs
    core/
    adapters/
    parsers/
  tests/
    basic.rs
    stress.rs
```

## Dependencias clave

- `git2`
- `clap`
- `xschem-viewer`
- `serde` y `serde_json`
- `sha2`
- `thiserror`
- `tempfile`
- `dirs`

## Pruebas

```bash
cargo test
cargo test --test basic
cargo test --test stress
```

## Lo que aun no esta

- Drivers para KLayout, Magic y NGSpice
- Diff visual multi-archivo
- Topologia pin-a-net
- Modo CI con exit code estricto
- Comando `show`
