# Ronda 1: mejoras rápidas

Cinco tareas chicas (S, horas) de [`pendientes.md`](../pendientes.md). Ninguna depende de Carlos ni cambia un contrato entre crates. Documentos de esta ronda: [`requirements.md`](requirements.md) (qué), [`design.md`](design.md) (cómo) y [`tasks.md`](tasks.md) (pasos y verificación).

## Orden

| # | Tarea | Toca | Riesgo |
|---|---|---|---|
| 1 | `cargo fmt` y Clippy en la CI | `rustfmt.toml`, todo el código (solo formato), `ci.yml` | medio: un commit grande |
| 2 | `actions/upload-artifact` a Node 24 | `release.yml` (y revisar `ci.yml`) | bajo |
| 3 | `riku render` respeta `LayerPaint::hidden` | `riku/src/export/svg.rs` | bajo |
| 4 | `log --graph --color always\|never\|auto` | `cli/mod.rs`, `cli/commands.rs`, `format/color.rs`, `format/log_graph.rs`, `docs/cli.md` | bajo |
| 5 | Historial: **Tab** pasa a la lista de archivos | `gui/history/*`, `docs/gui.md` | medio: foco y teclado en egui |

## Por qué este orden

- **Primero el formato.** Es el único cambio que toca casi todos los archivos. Si va al principio, ningún otro commit de la ronda se mezcla con él ni genera conflictos en una rama paralela (`lvs-visor`, `i18n`).
- **Después la CI** (2), porque comparte archivo con la tarea 1 y se prueba en la misma corrida.
- **Luego lo que es CLI y exportación** (3, 4): se prueban con `cargo test` y a mano, sin abrir el visor.
- **Al final lo del visor** (5): es lo único que pide probar con la ventana abierta (Xvfb en el contenedor).

## Reglas de trabajo

- Se edita y se commitea en `Documents`; se compila y se prueba en el contenedor con `sync-riku.sh` (no crear targets nuevos: el disco C: se llena).
- Un commit por tarea, en español, con prefijo (`style:`, `ci:`, `fix(render):`, `feat(cli):`, `feat(gui):`) y sin líneas de coautoría.
- El commit de formato (1) no lleva nada más. Se le añade a `.git-blame-ignore-revs` para que `git blame` lo salte.
- Cada commit deja `cargo test --workspace --locked` en verde con `RUSTFLAGS=-D warnings`, como la CI.

## Fuera de esta ronda

- Tope de ramas con `…` en `log --graph` (aparece junto a `--color` en `pendientes.md`; es otra tarea).
- Opción para dibujar las capas ocultas en `riku render` (`--layers`/`--all-layers`): solo si alguien la pide.
- Clippy que bloquee la CI: queda para cuando los avisos estén en cero.
- Cambiar el color de los otros comandos (`status`, `diff`) por flag: usarán la misma variable interna, pero la opción pública es solo de `log`.
