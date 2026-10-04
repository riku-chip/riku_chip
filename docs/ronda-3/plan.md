# Ronda 3: el LVS en el historial y antes de commitear

Llevar el LVS a donde el diseñador ya mira: `riku log --lvs` (en qué commit apareció cada discrepancia) y `riku status --lvs` (¿lo que estoy por commitear rompe el LVS?). Es la Fase 2 de [`lvs.md`](../lvs.md). Documentos: [`requirements.md`](requirements.md), [`design.md`](design.md), [`tasks.md`](tasks.md).

**Sin tocar `xschem-viewer-rust`:** se usa su API pública como hoy (`spice::netlist`, `tcleval::rc_vars`, `spice::VERSION`). Todo lo que su netlister lee pasa por un `FileSource` nuestro (`viewer-core`), y ahí se registra.

## Qué hay (revisado el 2026-10-04 sobre `main`, `549f3a5`)

- `riku lvs` (un par o todos, en el disco o en un commit), `-f json` (`riku-lvs/v1`), `--ci` (0/1/2) y `--log` (`riku-lvs-log/v1`): el veredicto por commit del primer padre, con `← dejó de coincidir / empeoró / mejoró / volvió a coincidir`.
- Caché en `~/.cache/riku/lvs`, con clave = versión de Riku + `spice::VERSION` + ids de Git de **las dos carpetas** del par.
- `riku log` y `riku status` no saben nada del LVS.

## Lo que le falta a un diseñador

1. **Qué cambió, no solo el veredicto.** `--log` dice "parámetros distintos (5)" en tres commits seguidos; no dice que en `0f05efe` aparecieron `M3`/`M4 w 18 ≠ 20`, que en `02496a4` pasaron a `18 ≠ 19` y que el merge los trajo a `main`. Eso es lo que se arregla o se discute en una revisión.
2. **El LVS junto a los cambios del commit**, en el mismo `log` (y con `--graph`, también en las ramas), no en un comando aparte.
3. **Antes de commitear:** el working tree contra `HEAD`, con un código de salida para un hook o la CI: fallar si empeora, avisar si sigue igual de mal.
4. **Que la caché no mienta.** Hoy un símbolo del proyecto en otra carpeta, una sub-celda `.mag` de otra carpeta, otra versión del PDK o de Netgen no invalidan el resultado: puede quedar un "coincide" viejo. Para un LVS, un falso PASS es el peor error posible.

## Orden

| # | Tarea | Esf. |
|---|---|---|
| 9 | **Discrepancias con identidad y su delta** entre dos resultados (apareció / se arregló / cambió) | S |
| 10 | **Caché por dependencias**: qué archivos leyó cada corrida (del proyecto y del PDK) y con qué contenido; sirve igual para un commit y para el disco | M |
| 11 | `riku log --lvs` (texto, `--graph`, JSON) | M |
| 12 | `riku status --lvs` y `--ci` (empeoró → 1) | S |
| 13 | `riku lvs --log` muestra el delta también | S |
| 14 | Escribir las 4 decisiones en `lvs.md` | S |
| 15 | Validar con el demo `ota` (la historia de M1/M2, M3/M4, M8 y el corto) y con un cambio fuera de las carpetas del par | M |

Primero 9 y 10: son la base de 11–13 y cambian el formato de la caché (una sola vez).

## Las 4 decisiones (propuesta; se escriben en `lvs.md` en T14)

| # | Pregunta | Propuesta | Por qué |
|---|---|---|---|
| 1 | ¿Netgen o comparador propio? | **Netgen**, por ahora. Se revisa con Carlos si aparece un caso concreto (Netgen no instalable donde se usa Riku, o mensajes que no se pueden mejorar). | Es el estándar de SKY130, GF180 e IHP; sus reglas por PDK (paralelos, propiedades, *dummies*) ya hicieron falta en la ronda 2. Un comparador propio es otro proyecto. |
| 2 | ¿Emparejar por nombre o en `.riku.toml`? | **Las dos, con prioridad** (ya implementado): `--sch/--layout` > `[[lvs]]` > mismo nombre (`.gds` > `.oas` > `.mag`). Se agrega un aviso cuando el nombre elige entre varios layouts. | El caso común (mismo nombre) no pide configuración; el raro se escribe una vez. |
| 3 | ¿El LVS en `log`/`status` por defecto? | **Solo con `--lvs`.** | Una corrida nueva son segundos por par y versión; el `log` de siempre no debe esperar a Netgen ni fallar si no está. Un `[lvs] in_status = true` en `.riku.toml` queda para después. |
| 4 | ¿Versionar el resultado? | **No: se recalcula, con caché local** (y la de la CI). | Es un derivado que depende del PDK y de Netgen; versionarlo deja resultados viejos commiteados y conflictos en cada merge. |

## Fuera de esta ronda

- La insignia del LVS en el panel **Historial** del visor (usa lo mismo que `log --lvs`; es GUI).
- Correr Netgen en paralelo para varias versiones (medir primero: con la caché, casi todo se repite).
- `[lvs] in_status` en `.riku.toml`; el comparador propio; KLayout LVS.
