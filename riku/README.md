# riku — el ejecutable

El binario `riku`: CLI, shell interactivo y visor, con los módulos de formato adentro.

```
src/
  main.rs, lib.rs
  cli/        comandos, shell (con Tab), doctor, gui y formatos de salida (format/)
  core/
    git/      blobs, commits, ramas y working tree (git2)
    analysis/ diff entre commits, show, log y status; reciben el registro de módulos
    domain/   modelos, errores y puertos (traits)
  modules/    mod.rs::registry() — el único lugar que lista los módulos
    xschem.rs, xschem_view.rs, xschem_pdk.rs   esquemáticos (feature `xschem`)
    layout.rs                                  layouts, sobre riku-mod-layout (feature `layout`)
  gui/        visor egui (feature `gui`)
tests/
  basic.rs    integración con repos git reales
  gds_e2e.rs  de punta a punta con layouts
  stress.rs   rendimiento y casos límite
```

- Uso: [`docs/cli.md`](../docs/cli.md) y [`docs/gui.md`](../docs/gui.md).
- Cómo encaja con los demás crates: [`docs/arquitectura.md`](../docs/arquitectura.md).
- Compilar y probar: [`docs/desarrollo.md`](../docs/desarrollo.md) (`cargo test -p riku`).
