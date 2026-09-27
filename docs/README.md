# Documentación de Riku

| Documento | Para quién | Qué tiene |
|---|---|---|
| [`cli.md`](cli.md) | usuarios | todos los comandos, salidas JSON, códigos de salida y variables de entorno |
| [`gui.md`](gui.md) | usuarios | el visor: controles, diff visual, layouts grandes |
| [`xschem.md`](xschem.md) | usuarios | esquemáticos: qué compara el diff, formato `.sch`, símbolos y PDK |
| [`layouts.md`](layouts.md) | usuarios | GDS, OASIS y Magic: el diff geométrico, verificación contra KLayout, medición |
| [`spice.md`](spice.md) | usuarios | simulaciones de ngspice (`.raw`): qué compara, tolerancia, la vista de formas de onda |
| [`arquitectura.md`](arquitectura.md) | desarrolladores | crates, núcleo y módulos, contratos, reglas de dependencia |
| [`desarrollo.md`](desarrollo.md) | desarrolladores | compilar, entorno, tests, CI, release, herramientas |
| [`roadmap.md`](roadmap.md) | todos | fases, estado y pendientes |
| [`diseno/`](diseno/) | desarrolladores | diseños: [`fase6.md`](diseno/fase6.md) (rendimiento, hecha), [`fase7.md`](diseno/fase7.md) (grafo del historial, hecha), [`fase8.md`](diseno/fase8.md) (Magic, hecha), [`fase9.md`](diseno/fase9.md) (revisión: bugs, rendimiento, estructura, librerías; plan) |
| [`archivo/`](archivo/README.md) | referencia | planes terminados e investigación inicial; no se mantienen |

Cada crate con algo propio tiene su README: [`riku-mod-layout`](../riku-mod-layout/README.md), [`tools/verify`](../tools/verify/README.md).
