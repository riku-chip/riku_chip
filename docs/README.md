# Documentación de Riku

| Documento | Para quién | Qué tiene |
|---|---|---|
| [`cli.md`](cli.md) | usuarios | todos los comandos, salidas JSON, códigos de salida y variables de entorno |
| [`gui.md`](gui.md) | usuarios | el visor: controles, diff visual, layouts grandes |
| [`xschem.md`](xschem.md) | usuarios | esquemáticos: qué compara el diff, formato `.sch`, símbolos y PDK |
| [`layouts.md`](layouts.md) | usuarios | GDS/OASIS: el diff geométrico, verificación contra KLayout, medición |
| [`arquitectura.md`](arquitectura.md) | desarrolladores | crates, núcleo y módulos, contratos, reglas de dependencia |
| [`desarrollo.md`](desarrollo.md) | desarrolladores | compilar, entorno, tests, CI, release, herramientas |
| [`roadmap.md`](roadmap.md) | todos | fases, estado y pendientes |
| [`diseno/`](diseno/) | desarrolladores | diseños de lo que está en curso ([`fase6.md`](diseno/fase6.md)) |
| [`archivo/`](archivo/README.md) | referencia | planes terminados e investigación inicial; no se mantienen |

Cada crate con algo propio tiene su README: [`riku-mod-layout`](../riku-mod-layout/README.md), [`tools/verify`](../tools/verify/README.md).
