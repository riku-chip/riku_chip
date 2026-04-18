# Flujos reales de diseño: variaciones no-ideales y pain points

Fuente: foros de la comunidad EDA (open-source-silicon.dev, efabless), issues de GitHub (KLayout, Xschem, NGSpice, Magic, skywater-pdk, OpenLane), blogs técnicos (ChipFlow 2025, Electronic Design, Keysight), y documentación de proyectos reales (IIC-OSIC-TOOLS, CACE, GDSFactory, volare).

Propósito: identificar las variaciones al flujo canónico (Xschem → NGSpice → Magic → KLayout) que Miku debe soportar, degradar con gracia, o declarar explícitamente fuera de alcance.

---

## Flujo canónico asumido en el research anterior

```
Xschem (.sch) → NGSpice (.spice + .raw) → Magic (.mag) → GDS → KLayout (DRC)
```

Todo lo que sigue es una desviación documentada de ese flujo, observada en proyectos reales.

---

## 1. KLayout como herramienta primaria de layout (no secundaria)

El propio Tim Edwards (mantenedor de Magic y open_pdks) declaró directamente:

> "I'm using KLayout as the main layout editor for GF180, but always double check the DRC on Magic."
> — open-source-silicon.dev

La razón: Magic recibe más mantenimiento para SKY130; para GF180 e IHP SG13G2, KLayout está mejor integrado. La elección es PDK-dependiente, no herramienta-dependiente.

**El flujo IHP es explícitamente**: Xschem → NGSpice → KLayout → DRC en KLayout. Magic es opcional y puede no usarse.

El curso oficial del IHP Certificate Course empieza con KLayout como editor de layout, no con Magic. Desde el cierre de Efabless (febrero 2025), el soporte de Magic para IHP quedó interrumpido.

**Implicación para Miku:** No se puede asumir que `.mag` = fuente de layout. En proyectos IHP/GF180, el `.gds` o `.oas` puede ser la fuente canónica.

**Condición de refutación:** Si el mantenimiento de Magic para IHP SG13G2 se reanuda y alcanza paridad con KLayout, este caso vuelve al flujo canónico.

Fuente: [open-source-silicon.dev/t/16219913](https://web.open-source-silicon.dev/t/16219913), [unic-cass KLayout Sky130 tutorial](https://unic-cass.github.io/training/sky130/3.3-layout-klayout.html), [IHP Certificate Course](https://www.ihp-microelectronics.com/events-1/detail/title-certificate-course-in-person-analog-design-with-ihp-sg13g2-open-source-pdk)

---

## 2. Layout generado por código (Python → GDS, sin editor gráfico)

Una categoría entera de diseñadores nunca abre Magic ni KLayout como editor. Generan GDS directamente desde Python:

- **GDSFactory** (backend gdstk + KLayout DRC): usado en fotónica, MEMS, quantum, y cada vez más en analógico. Entrada: `.py` o `.yaml`. Salida: GDS/OASIS.
- **OpenFASoC / GLayout**: generación de layout analógico basada en PCells con optimización por RL. PDK-agnóstico.
- **KLayout scripting (Ruby/Python)**: para PCells paramétricas. Tim Edwards usa esta modalidad en GF180.
- **gdstk directo**: manipulación de polígonos GDS a bajo nivel.

En estos flujos, los archivos `.py` son la fuente de verdad. El `.gds` es un artefacto de build — nunca debería commitearse como fuente.

**Implicación para Miku:** El campo `layout.source` en `miku.toml` debe poder declararse como `"python"`, no solo `"magic"` o `"klayout"`. Miku no puede inferir la fuente de verdad del layout sin configuración explícita.

Fuente: [github.com/gdsfactory/gdsfactory](https://github.com/gdsfactory/gdsfactory), [OpenFASoC GLayout](https://openfasoc.readthedocs.io/en/latest/notebooks/glayout/glayout_opamp.html), [codeberg.org KLayout Sky130](https://codeberg.org/mole99/klayout-sky130-inverter)

---

## 3. Dos fases de simulación distintas (pre-layout y post-layout)

El flujo canónico asume una sola simulación después del esquemático. En práctica hay dos fases separadas:

**Fase 1 — Pre-layout (desde esquemático):**
```
Xschem (.sch) → netlist.spice → NGSpice → .meas / .raw
```

**Fase 2 — Post-layout (desde extracción de parásitos):**
```
Magic → extract all → .ext files → ext2spice → netlist_extracted.spice → NGSpice → .meas_postlayout
```

Los resultados post-layout pueden diferir significativamente de los pre-layout. Diseñadores iteran: modifican layout, re-extraen, re-simulan. Esta iteración puede durar días.

El netlist extraído nunca es editado manualmente — siempre se regenera desde el `.mag`. Los archivos `.ext` son intermedios (deben ir en `.gitignore`).

**Implicación para Miku:** Una comparación de resultados de simulación entre commits requiere saber si estamos comparando runs pre-layout o post-layout. Un diff de `.meas` sin esta etiqueta es ambiguo. Propuesta: el `miku.toml` o el commit metadata debería registrar el tipo de simulación.

**Oportunidad:** Definir un estado de diseño explícito: `SCHEMATIC → SIMULATED_PRE → LAYOUT → EXTRACTED → SIMULATED_POST → DRC_CLEAN → LVS_CLEAN`. Ninguna herramienta hace esto.

Fuente: [ngspice sourceforge discussion real tapeouts](https://sourceforge.net/p/ngspice/discussion/120972/thread/69a4488f56/), [unic-cass analog design flow](https://unic-cass.github.io/training/1.4-analog-design-flow-intro.html)

---

## 4. SPICE escrito a mano como artefacto primario (no derivado de Xschem)

El flujo canónico asume: `.sch` → `.spice`. Hay tres variantes reales:

1. **Canónico**: `.spice` generado por Xschem (artefacto derivado)
2. **SPICE-first**: diseñador escribe netlist a mano para explorar topologías; el esquemático se formaliza después si acaso
3. **SPICE como spec de celdas estándar**: las celdas SKY130 en Xschem no tienen esquemático propio — solo tienen un símbolo con atributo `spice_sym_def` que apunta a un netlist externo. El `.spice` es la fuente de verdad, no el `.sch`.

Existe una solicitud de feature abierta en Xschem (issue #35) para importar netlists SPICE y generar representaciones gráficas — lo que implica que el flujo inverso es demandado pero no soportado.

**Implicación para Miku:** Un archivo `.spice` puede ser fuente primaria o artefacto derivado dependiendo del proyecto. No se puede aplicar "SPICE siempre va en `.gitignore`" como regla global.

Fuente: [github.com/StefanSchippers/xschem/issues/35](https://github.com/StefanSchippers/xschem/issues/35), [xschem tutorial sky130](https://xschem.sourceforge.io/stefan/xschem_man/tutorial_xschem_sky130.html), [KLayout forum #2395](https://www.klayout.de/forum/discussion/2395/how-to-start-from-a-spice-netlist)

---

## 5. Flujo mixto digital + analógico (Yosys/OpenROAD + Xschem en el mismo repo)

En proyectos que incluyen bloques digitales sintetizados y celdas analógicas diseñadas a mano, el repo contiene simultáneamente:

- Verilog (fuente digital) → Yosys → OpenROAD → GDS (bloque digital)
- Xschem `.sch` → NGSpice → Magic `.mag` → GDS (bloque analógico)
- Un wrapper GDS que integra ambos (patrón caravel/caravan de Efabless)

El sistema CACE de Tim Edwards fue creado explícitamente porque "los flujos open-source para diseño analógico y mixed-signal están rezagados... hay poca estandarización de estructura de proyecto, especificaciones y metodología de testbench."

**Implicación para Miku:** Un proyecto puede tener archivos `.v`, `.sch`, `.spice`, `.mag`, y GDS exportados de OpenROAD, todos como fuentes de diseño simultáneamente. Los drivers de Miku deben poder coexistir en el mismo proyecto sin colisiones.

**Tipos de archivo no anticipados que aparecen en estos proyectos:** `.lef`, `.def`, `.lib` (Liberty), `.v` stubs, `config.json` / `config.tcl` (OpenLane).

Fuente: [github.com/The-OpenROAD-Project/OpenLane/issues/1420](https://github.com/The-OpenROAD-Project/OpenLane/issues/1420), [wiki.f-si.org CACE](https://wiki.f-si.org/index.php?title=CACE:_Defining_an_open-source_analog_and_mixed-signal_design_flow)

---

## 6. Herramientas comerciales + PDK open-source

Múltiples equipos usan Cadence Virtuoso/Genus para esquemático y síntesis (licencias universitarias), y KLayout o Magic para layout con PDKs abiertos.

El proyecto `sky130_cds` provee scripts Tcl para Genus e Innovus configurados para SKY130 — un flujo completamente Cadence sobre PDK abierto. Efabless chipIgnite lo declaró explícitamente soportado: "designers can also employ proprietary design tools."

**Implicación para Miku:** Algunos proyectos no tendrán `.sch` de Xschem. El esquemático puede ser un `.cdl` o `.sdb` de Cadence que Miku no puede leer. El netlist SPICE puede ser exportado de Virtuoso con convenciones diferentes.

Fuente: [github.com/stineje/sky130_cds](https://github.com/stineje/sky130_cds)

---

## 7. Qucs-S o KiCad en lugar de Xschem

- **Qucs-S**: GUI para NGSpice con mejor display de waveforms y Monte Carlo más sencillo. Schemáticos en formato Qucs (`.sch`, distinto del formato Xschem). Usado por diseñadores que prefieren UI sobre control de línea de comandos.
- **KiCad Eeschema**: Exporta netlists NGSpice-compatibles. Usado por diseñadores con background en PCB que entran al mundo IC.

Ambos producen archivos `.sch` que son sintácticamente distintos e incompatibles con el formato Xschem.

**Implicación para Miku:** El driver de Xschem no puede asumir que todo `.sch` es formato Xschem. El sistema de detección de tipo de archivo debe distinguir el origen del `.sch`.

Fuente: [ngspice.sourceforge.io/ngspice-eeschema.html](https://ngspice.sourceforge.io/ngspice-eeschema.html), [github.com/ra3xdh/qucs_s](https://github.com/ra3xdh/qucs_s/issues/25)

---

## 8. Verilog-A y OpenVAF para PDKs con modelos BiCMOS/RF

El PDK IHP SG13G2 incluye dispositivos HBT (heterojunction bipolar transistors) con modelos compact en Verilog-A. NGSpice no puede usarlos nativamente. El flujo agrega un paso de compilación:

```
modelo.va → OpenVAF → modelo.osdi → cargado en NGSpice via OSDI interface
```

Los archivos `.va` son fuentes de modelo (no deben ir en `.gitignore`). Los `.osdi` son binarios compilados (artefactos de build).

**Implicación para Miku:** En proyectos IHP/RF, los archivos `.va` son parte del árbol de fuentes de diseño y deben versionarse. El `.gitignore` generado por `miku init` debe ser PDK-aware.

Fuente: [ngspice.sourceforge.io/osdi.html](https://ngspice.sourceforge.io/osdi.html), [github.com/pascalkuthe/OpenVAF/discussions/22](https://github.com/pascalkuthe/OpenVAF/discussions/22)

---

## 9. PDK como dependencia versionada (no instalación ambiental)

El flujo canónico asume PDK instalado globalmente en el sistema. En práctica, diferentes proyectos están anclados a diferentes commits de open_pdks:

- `volare` (Efabless) y `ciel` (FOSSi Foundation) son gestores de versiones de PDK, análogos a `nvm` o `pyenv`.
- Un diseño que pasa DRC en commit `abc123` puede fallar en commit `def456` por actualizaciones de reglas.
- Las bugs documentadas en la integración KLayout-SKY130 (XML parsing errors en pymacros, referencias de archivos incorrectas) son específicas de versiones del PDK.

**Implicación para Miku:** El `miku.toml` debe incluir un campo `pdk.version` con el commit hash del PDK usado. `miku doctor` debe verificar que la versión local coincide con la del proyecto. Sin esto, la reproducibilidad de DRC/LVS no está garantizada.

**Oportunidad:** Análogo a `package-lock.json` para PDKs. Ninguna herramienta lo hace hoy.

Fuente: [github.com/chipfoundry/volare](https://github.com/chipfoundry/volare), [github.com/fossi-foundation/ciel](https://github.com/fossi-foundation/ciel)

---

## 10. Proyectos multi-PDK (SKY130 + GF180 + IHP en paralelo)

Equipos que portan diseños a múltiples PDKs mantienen ramas paralelas o subdirectorios por PDK. El `open_pdks` installer puede construir ambos PDKs pero "debido al enorme overhead de procesamiento y espacio en disco, no se recomienda" hacerlo simultáneamente.

GDSFactory tiene un enfoque PDK-agnóstico con paquetes Python por PDK (`skywater130`, `gf180`), pero es un workflow no estándar.

**Implicación para Miku:** Un proyecto puede tener múltiples `miku.toml` en subdirectorios, cada uno con un PDK diferente, o un único `miku.toml` con un array `pdks = ["sky130", "gf180"]`. El sistema de configuración debe soportar proyectos multi-PDK sin requerir repos separados.

---

## 11. Tabla de tipos de archivo no anticipados en proyectos reales

| Extensión | Contexto | Tratamiento en Miku |
|---|---|---|
| `.oas` / `.oasis` | Layout KLayout-nativo; 1000x más compacto que GDS | Tratar igual que `.gds` |
| `.va` | Modelos Verilog-A (IHP, RF BiCMOS) | Texto versionable, diff como texto |
| `.osdi` | Compilado de Verilog-A por OpenVAF | Artefacto de build, `.gitignore` |
| `.ext` | Extracción intermedia de Magic (por celda) | Artefacto de build, `.gitignore` |
| `.lef` | Abstract view para integración digital | No manejado en MVP |
| `.def` | Placement/routing de OpenROAD | No manejado en MVP |
| `.lib` | Liberty timing (output de caracterización) | No manejado en MVP |
| `.v` / `.verilog` | Stubs Verilog de bloques analógicos para síntesis | No manejado en MVP |
| `.yaml` / `.pic.yml` | Componentes GDSFactory | No manejado en MVP |
| `.kicad_sch` | Esquemático KiCad (diseñadores de PCB) | No manejado en MVP |
| `.cir` / `.sp` | Variantes de extensión SPICE | Detectar como SPICE |
| `config.json` / `config.tcl` | Configuración OpenLane | Texto plano, sin driver especial |

---

## 12. Pain points documentados con git en diseño de chips

Fuente primaria: [Medium — Code Meets Silicon](https://medium.com/@tercel04/code-meets-silicon-navigating-gits-limits-in-semiconductor-chip-design-engineering-e175f1e74a84), [Keysight blog](https://www.keysight.com/blogs/en/tech/sim-des/how-to-optimize-and-manage-large-design-files), [ChipFlow 2025](https://www.chipflow.io/blog/open-source-analog-in-2025-reality-check-friction-points-and-chipflows-hybrid-path)

1. **Archivos grandes (100 MB - varios GB):** git clone fuerza descarga completa. Delta compression de git no funciona en GDS binario — incluso mover un wire produce un blob completamente diferente.
2. **Merge de schemáticos y layouts no es automatizable:** la industria usa flujos lock-based (checkout exclusivo), no merge-based. Git no tiene primitiva de lock.
3. **Las herramientas EDA no tienen integración nativa con Git:** adds/deletes/changes son difíciles de rastrear desde dentro de la herramienta.
4. **Control de acceso granular imposible:** un engineer de layout puede modificar accidentalmente schemáticos; un contratista puede ver todo.
5. **Cambios en un archivo requieren revisar otros:** Git trata archivos como unidades independientes; el diseño de chips trata los archivos como un compuesto.
6. **LVS debugging es el bloqueante más común:** mismatches no-obvios entre netlist de esquemático y netlist extraído de layout. Causas: port order mismatch, `spice_sym_def`, diferencias en convenciones Magic vs KLayout.
7. **Simulación estadística (Monte Carlo) requiere conversión de modelos:** SKY130 incluye parámetros estadísticos en formato Spectre; NGSpice no puede leerlos sin un script de conversión externo (contribuido por la comunidad, no por Google/SkyWater).
8. **PDK XML bugs requieren patches manuales antes de trabajar:** en KLayout+SKY130, dos bugs en los archivos del PDK bloquean el uso sin aplicar `sed` manualmente. Cualquier CI/CD que no aplique estos patches falla silenciosamente.

---

## 13. Impacto del cierre de Efabless (febrero 2025)

Efabless cerró en febrero 2025. Consecuencias directas para el ecosistema:

- 500+ proyectos Tiny Tapeout en limbo de fabricación
- `efabless/mpw_precheck` (pipeline CI estándar de tape-out) sin mantenimiento
- Trabajo de soporte Magic para IHP PDK interrumpido
- OpenLane re-adoptado como LibreLane bajo FOSSi Foundation

**Implicación para Miku:** La plataforma estándar para tape-out open-source ya no existe. Los nuevos flujos se están formando alrededor de IHP acceso directo, Tiny Tapeout migrando a IHP/GF180, y LibreLane. Miku puede posicionarse como capa de metadata sobre git que registra estado de tape-out, versión del PDK, estado DRC, y shuttle — rol que Efabless cumplía.

Fuente: [semiwiki.com Efabless shutdown](https://semiwiki.com/forum/threads/efabless-just-shut-down.22217/), [Tom's Hardware](https://www.tomshardware.com/tech-industry/semiconductors/efabless-shuts-down-fate-of-tiny-tapeout-chip-production-projects-unclear), [wiki.f-si.org IHP integration](https://wiki.f-si.org/index.php?title=IHP_Open_PDK_integration_with_Magic,_Netgen,_and_LibreLane)

---

## 14. Oportunidades identificadas que nadie cubre hoy

### 14.1 PDK version pinning por proyecto
Ninguna herramienta registra qué commit de `open_pdks` se usó para producir un GDS o ejecutar un DRC. Miku puede requerir un `pdk.lock` — análogo a `package-lock.json`.

### 14.2 Declaración explícita de fuente de verdad de layout
El campo `layout.source` en `miku.toml` podría declarar: `"magic"` (`.mag` canónico, `.gds` derivado), `"klayout"` (`.gds`/`.oas` canónico), o `"python"` (scripts Python son la fuente, `.gds` es build artifact). Elimina la ambigüedad actual donde repos tienen `.mag` y `.gds` sin indicación de cuál es ground truth.

### 14.3 Estado de diseño como máquina de estados
`SCHEMATIC → SIMULATED_PRE → LAYOUT → EXTRACTED → SIMULATED_POST → DRC_CLEAN → LVS_CLEAN → TAPED_OUT`. Las transiciones son commits con semántica explícita. Ninguna herramienta en el ecosistema define esto.

### 14.4 Storage OASIS para reducir bloat de binarios
OASIS puede ser 1000x más compacto que GDS (ejemplo documentado: 150 MB → 25 kB). Miku podría convertir automáticamente GDS → OASIS para storage, y revertir en checkout — reduciendo tamaño del repo sin overhead de LFS, preservando nombres de layers (que GDS pierde).

### 14.5 Diff semántico de `.sch` Xschem
Git diff de `.sch` muestra cambios de coordenadas crudos, ilegibles. Un diff semántico podría reportar: "Resistor R1 cambió de 1k a 10k" o "Net VDD desconectado del gate de M3". No existe en ninguna herramienta.

### 14.6 Tracking pre-layout vs post-layout en regresiones de simulación
Ninguna herramienta compara runs de simulación distinguiendo la fase. Miku podría almacenar métricas clave (ganancia, BW, potencia) como sidecar de texto por commit, permitiendo ver "este commit cambió la ganancia de 45 dB a 42 dB."

### 14.7 Templates CI/CD para DRC/LVS en cada commit
No existe un template estándar "run DRC on every push to main" para el ecosistema EDA open-source. El precheck de Efabless solo corría en tape-out. Miku podría distribuir GitHub Actions / GitLab CI templates que usen IIC-OSIC-TOOLS Docker image.

### 14.8 .gitignore generation PDK-aware
No existe un `.gitignore` template estándar para proyectos IC que cubra todos los archivos intermedios de todas las herramientas. `miku init` podría generarlo condicionado al PDK y flujo declarado en `miku.toml`.

---

## Resumen: suposiciones del diseño original que deben revisarse

| Suposición | Realidad documentada | Acción para Miku |
|---|---|---|
| Layout siempre en Magic (`.mag`) | Para IHP/GF180, KLayout puede ser primario | `layout.source` en `miku.toml` |
| `.gds` siempre es artefacto derivado | Para Python/GDSFactory, `.gds` ES el output; para KLayout-primary, `.gds` es fuente | Configuración explícita requerida |
| `.spice` siempre derivado de `.sch` | SPICE puede ser fuente primaria (hand-written, celdas estándar) | No agregar `.spice` a `.gitignore` globalmente |
| Una sola fase de simulación | Pre-layout y post-layout son fases distintas con diferentes netlists | Metadata de fase en resultados de simulación |
| Todo `.sch` es formato Xschem | Qucs-S y KiCad también producen `.sch` con otro formato | Detección por magic bytes / header, no solo extensión |
| PDK instalado globalmente | PDK está versionado por proyecto (volare/ciel) | `pdk.version` en `miku.toml` |
| Proyectos mono-PDK | Equipos portan diseños a SKY130 + GF180 + IHP | Soporte multi-PDK en configuración |
| Un solo flujo por proyecto | Proyectos mixtos digital+analógico tienen dos flujos en paralelo | Drivers coexisten sin colisiones |

---

## Condiciones de refutación

- Si KLayout integra soporte nativo de formato `.mag` con paridad funcional, el caso de proyectos con dos herramientas de layout se simplifica.
- Si Magic reanuda mantenimiento activo para IHP SG13G2 y GF180, el flujo canónico se aplica a más PDKs.
- Si Xschem implementa importación de netlists SPICE (issue #35), el caso "SPICE como fuente primaria" se reduce.
- Si el ecosistema adopta un estándar de PDK version pinning (volare o ciel se vuelve universal), el campo `pdk.version` puede delegarse a esa herramienta en lugar de ser responsabilidad de Miku.
