# Riku Rust

`riku_rust` es la implementacion oficial en Rust de Riku, un VCS semantico para diseno de chips. Compara esquematicos por estructura, no como diff de texto, empezando por Xschem.

Autor: Ariel Amado Frias Rojas

## Que hace

- `diff` semantico entre dos commits de un archivo `.sch`
- `log` del historial por archivo
- `doctor` para verificar herramientas externas
- salida `text`, `json` y `visual`
- render SVG anotado para Xschem
- cache de renders por contenido + version de herramienta

## Instalacion de Rust

### Windows

1. Descarga `rustup-init.exe` desde la pagina oficial de Rust.
2. Ejecutalo y sigue el instalador.
3. Abre una nueva terminal y verifica:

```powershell
rustc --version
cargo --version
```

### Linux o WSL

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
rustc --version
cargo --version
```

## Compilacion

Desde `C:\Users\ariel\Documents\riku_chip\riku_rust` o el equivalente en Linux:

```bash
cargo build
cargo test
```

Para binario optimizado:

```bash
cargo build --release
```

El ejecutable queda en:

- Windows: `target\debug\riku.exe` o `target\release\riku.exe`
- Linux: `target/debug/riku` o `target/release/riku`

## Uso rapido

### Con `cargo run`

```bash
cargo run -- diff <commit_a> <commit_b> <archivo.sch>
cargo run -- log <archivo.sch>
cargo run -- doctor
```

### Binario local

```bash
./target/debug/riku diff <commit_a> <commit_b> <archivo.sch>
./target/debug/riku log <archivo.sch>
./target/debug/riku doctor
```

En Windows PowerShell:

```powershell
.\target\debug\riku.exe diff <commit_a> <commit_b> <archivo.sch>
.\target\debug\riku.exe log <archivo.sch>
.\target\debug\riku.exe doctor
```

### Como comando instalado

```bash
cargo install --path .
riku diff <commit_a> <commit_b> <archivo.sch>
riku log <archivo.sch>
riku doctor
```

## Ejemplos por sistema

### Windows

```powershell
cargo run -- diff HEAD~1 HEAD examples/SH/op_sim.sch
.\target\debug\riku.exe diff HEAD~1 HEAD examples/SH/op_sim.sch
.\target\debug\riku.exe diff HEAD~1 HEAD examples/SH/op_sim.sch --format json
```

### Linux

```bash
cargo run -- diff HEAD~1 HEAD examples/SH/op_sim.sch
./target/debug/riku diff HEAD~1 HEAD examples/SH/op_sim.sch
./target/debug/riku diff HEAD~1 HEAD examples/SH/op_sim.sch --format json
```

## Dependencias externas

- `xschem` se usa solo para `--format visual`
- Git se lee directamente desde los commits, sin checkout
- la cache de render vive en `~/.cache/riku/ops` o equivalente del sistema

## Estado del proyecto

- La base Rust ya esta implementada.
- La paridad con la version Python fue validada con tests.
- Python queda como referencia historica y de comparacion.
- La integracion con `gdstk/rust` queda para una fase posterior.
