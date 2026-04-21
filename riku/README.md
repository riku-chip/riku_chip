# Riku

Riku es una herramienta de VCS semantico para diseno de chips. La version actual se enfoca en esquematicos Xschem (`.sch`) y usa `git2` para leer el historial directamente desde los objetos Git.

## Que hace hoy

- `diff` semantico entre dos commits de un archivo `.sch`
- `log` del historial por archivo, con opcion de resumen semantico
- `doctor` para validar repo Git, PDK y cache
- `render` para generar un SVG del esquematico y abrirlo con el visor del sistema
- salida en `text`, `json` y `visual`
- cache de renders por SHA-256

## Requisitos

- Rust con soporte para edition 2024
- Un repositorio Git valido
- Opcionalmente `PDK_ROOT`, `PDK` o un `.xschemrc` para mejorar la resolucion de simbolos

No necesitas un binario `xschem` instalado para usar el renderer actual.

## Compilar

Desde esta carpeta:

```bash
cargo build
cargo build --release
cargo test
```

El binario queda en `target/debug/riku` o `target/release/riku`.

## Uso

```bash
cargo run -- diff HEAD~1 HEAD ../examples/SH/op_sim.sch
cargo run -- diff HEAD~1 HEAD ../examples/SH/op_sim.sch --format json
cargo run -- diff HEAD~1 HEAD ../examples/SH/op_sim.sch --format visual
cargo run -- log ../examples/SH/op_sim.sch --semantic --limit 10
cargo run -- doctor
cargo run -- render ../examples/SH/op_sim.sch
```

Tambien puedes instalarlo:

```bash
cargo install --path .
riku diff HEAD~1 HEAD ../examples/SH/op_sim.sch
```

## Opciones de CLI

### `diff`

```text
riku diff <commit_a> <commit_b> <archivo.sch> [--repo <path>] [--format text|json|visual]
```

### `log`

```text
riku log [archivo.sch] [--repo <path>] [--limit <n>] [--semantic]
```

### `doctor`

```text
riku doctor [--repo <path>]
```

### `render`

```text
riku render <archivo.sch>
```

## Estructura

```text
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

## Dependencias

- `git2`
- `clap`
- `xschem-viewer`
- `serde` / `serde_json`
- `sha2`
- `thiserror`
- `tempfile`
- `dirs`

## Tests

```bash
cargo test
cargo test --test basic
cargo test --test stress
```

## Notas

- `diff --format visual` y `render` abren el archivo generado con el visor del sistema
- el cache se guarda en el directorio de cache del usuario bajo `riku/ops`
- por ahora solo existe soporte para Xschem `.sch`
