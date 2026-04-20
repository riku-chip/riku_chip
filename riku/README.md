# Riku

VCS semántico para diseño de chips. Compara esquemáticos por estructura, no como diff de texto.

Actualmente soporta Xschem (`.sch`). Integración con gdstk en desarrollo.

## Qué hace

- `diff` semántico entre dos commits de un archivo `.sch`
- `log` del historial por archivo, con resumen de cambios semánticos
- `doctor` para verificar herramientas externas instaladas
- salida `text`, `json` y `visual` (SVG anotado)
- caché de renders por contenido + versión de herramienta

## Instalación de Rust

### Linux / Docker

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

### Windows

Descarga e instala `rustup-init.exe` desde [rustup.rs](https://rustup.rs). Luego abre una nueva terminal y verifica:

```powershell
cargo --version
```

## Compilación

Desde la carpeta `riku/`:

```bash
cargo build          # debug
cargo build --release  # optimizado
cargo test           # todos los tests
```

El binario queda en:
- Linux: `target/debug/riku` o `target/release/riku`
- Windows: `target\debug\riku.exe` o `target\release\riku.exe`

## Uso

### Con cargo run (desarrollo)

```bash
cargo run -- diff HEAD~1 HEAD ../examples/SH/op_sim.sch
cargo run -- diff HEAD~1 HEAD ../examples/SH/op_sim.sch --format json
cargo run -- log ../examples/SH/op_sim.sch --semantic
cargo run -- doctor
```

### Binario instalado

```bash
cargo install --path .
riku diff HEAD~1 HEAD ../examples/SH/op_sim.sch
riku log ../examples/SH/op_sim.sch --limit 10 --semantic
riku doctor
```

### Opciones de diff

```
riku diff <commit_a> <commit_b> <archivo.sch> [opciones]

Opciones:
  --repo <path>      Ruta al repositorio git (default: .)
  --format <fmt>     text | json | visual  (default: text)
```

### Opciones de log

```
riku log <archivo.sch> [opciones]

Opciones:
  --repo <path>      Ruta al repositorio git (default: .)
  --limit <n>        Número máximo de commits a mostrar (default: 20)
  --semantic         Mostrar resumen de cambios por commit
```

## Dependencias externas

- `xschem` — solo requerido para `--format visual`
  - En Docker (iic-osic-tools): disponible en `/foss/tools/bin/` tras activar el entorno
  - La caché de renders vive en `~/.cache/riku/ops/` (Linux) o `%LOCALAPPDATA%\riku\ops\` (Windows)
- Git se lee directamente desde los objetos del repositorio, sin hacer checkout

## Docker (iic-osic-tools)

```bash
# Verificar que xschem está disponible
docker exec -it <contenedor> bash -c 'PATH=/foss/tools/bin:$PATH riku doctor'

# Correr tests
docker exec -it <contenedor> bash -c 'cd /foss/designs/riku_chip/riku && cargo test'

# Diff visual (requiere display :0)
docker exec -it <contenedor> bash -c 'cd /foss/designs/riku_chip && riku diff HEAD~1 HEAD examples/SH/op_sim.sch --format visual'
```

## Tests

```bash
cargo test                    # todos
cargo test --test stress      # stress: throughput, GDS, git bajo carga
cargo test --test basic       # parser, diff semántico, git service
cargo test --test parity      # paridad Python vs Rust (requiere Python + deps)
```

## Estado

- Implementación Rust completa (parser, diff, git, SVG annotator, CLI)
- Tests de estrés validados: GDS 870KB en 54ms, 100× parse+diff a 26ms/iter
- Integración con `gdstk/rust` en desarrollo (fase siguiente)
