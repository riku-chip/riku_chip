# Ronda 4: demos (requisitos)

Dos proyectos de ejemplo más para `riku demo`, como `ota` y `sram`: repos Git con historia de diseños reales, para probar Riku sin un diseño propio. Diseño en [`design.md`](design.md), pasos en [`tasks.md`](tasks.md).

## Contexto (revisado el 2026-10-04 sobre `main`, `b137928`)

- `riku demo [nombre] [--dir] [--list]` clona bundles **embebidos en el ejecutable** (`include_bytes!`): `ota` (286 KB) y `sram` (268 KB). Los arman scripts deterministas en `tools/demos/` (`common.py`: autor y fechas fijos, ramas, merges, tags).
- **Inversor:** `/foss/examples/demo_sky130A/ana` de iic-osic-tools: `inv.mag` (celda de arriba con las capas de Magic por nombre y cuatro puertos con clase) que instancia dos sub-celdas de transistores de 5 V (`sky130_fd_pr__nfet_g5v0d10v5_H9JWFY.mag`, `…pfet…_5AEDG4.mag`), `inv.sch`, `inv.sym`, `tb_inv.sch`. Unos 12 KB.
- **SRAM de 1 KB:** `$PDK_ROOT/sky130A/libs.ref/sky130_sram_macros/gds/sky130_sram_1kbyte_1rw1r_32x256_8.gds`, 9,9 MB. Embebida en cada demo la inflaría; va en un repo aparte.

## Lo que se encontró al probar (antes de diseñar)

- **`riku lvs` sobre el inversor original no coincide:** el layout sale con 3 dispositivos de modelo `Ignore` y el esquemático con 2 (`nfet_g5v0d10v5`, `pfet_g5v0d10v5`). El `.tech` de SKY130 tiene, antes de los transistores de 5 V, reglas `device msubcircuit Ignore mvnfet … +npn,pnp` para la parte de los bipolares de alta tensión; `+npn,pnp` exige que el transistor esté junto a un `npn`/`pnp`. Riku no evalúa esa condición y toma la regla para todos los transistores de 5 V. **Sin arreglarlo, el demo no puede arrancar coincidiendo.**
- Los ejemplos `nets` y `lvs_probe` leen un `.mag` sin los archivos de su carpeta: en un layout jerárquico no encuentran las sub-celdas (`riku lvs` sí, porque les pasa los archivos).

## R16. Reglas de dispositivos con `+tipos`

- **R16.1** Una regla `device … +t1,t2` del `.tech` vale solo para los dispositivos cuya compuerta toca (o se superpone a) alguno de esos tipos.
- **R16.2** En el inversor, los transistores salen como `sky130_fd_pr__nfet_g5v0d10v5` y `sky130_fd_pr__pfet_g5v0d10v5`, con su W y L, y `riku lvs` da "coinciden" (el pfet de dos fingers de 4 µm contra `W=8 nf=2`).
- **R16.3** Las celdas que ya se verificaban (la verificación contra Magic de `tools/verify/devices/` y `tools/verify/nets/`, el demo `ota`, las 437 celdas `sky130_fd_sc_hd`) dan lo mismo que antes.
- **R16.4** Los ejemplos `nets` y `lvs_probe` leen un `.mag` con su carpeta (las sub-celdas).

## R17. Demo `inversor` (Magic + Xschem)

**Historia.** Como diseñador que usa Magic, quiero ver qué hace Riku con mis layouts: capas por nombre, la jerarquía de celdas, los puertos y la conectividad, y cómo el LVS sigue la historia.

- **R17.1** `riku demo inversor` crea un repo con unos 10 commits, una rama con merge y un tag, a partir del inversor de `demo_sky130A`, con su licencia y la atribución.
- **R17.2** La historia muestra, cada cosa en un commit y con un mensaje que lo dice:
  1. **El inicial** (coincide en LVS).
  2. **Capas por nombre:** un cambio en una capa de Magic que se ve con su nombre (`metal2`, `locali`), no como un número GDS.
  3. **Un transistor que cambia en su sub-celda** (el nfet más ancho), que `riku diff inv.mag` muestra desde la celda de arriba; el LVS deja de coincidir (W distinto al del esquemático).
  4. **El esquemático que lo acompaña** (W nuevo): vuelve a coincidir.
  5. **Un puerto que cambia de clase** (`in` pasa a `input`, `out` a `output`): Riku lo informa como cambio de puerto, no de geometría.
  6. **Un abierto** (`out` se corta en `metal2`): Riku lo informa como abierto; el LVS empeora.
  7. **El arreglo del abierto.**
  8. **Una re-grabación de Magic** que solo cambia los `timestamp`: Riku no ve cambios.
  9. **Una rama** (p. ej. un pmos más ancho en esquemático y layout) con su merge.
- **R17.3** Un `README.md` en el repo con qué probar y qué se ve en cada caso (`riku log --graph --lvs`, `riku diff`, `riku show`, el visor con el Historial y la vista de LVS).
- **R17.4** El bundle cabe embebido (decenas de KB) y `riku demo` lo trae como a `ota` y `sram`.
- **R17.5** Lo que el README dice que se ve, se ve: cada caso de R17.2 comprobado con la salida real de Riku (y el LVS de cada commit con `riku log --lvs`).
- **R17.6** Los `.mag` editados siguen siendo válidos para Magic: Magic los abre y su `extract` + Netgen da el mismo veredicto que `riku lvs` en cada commit.

## R18. Demo `chip` (SRAM de 1 KB, repo aparte)

**Historia.** Como quien evalúa Riku, quiero ver cuánto tarda con un layout de verdad grande, sin bajar un ejecutable de decenas de MB.

- **R18.1** `riku demo chip` clona un repo aparte (por defecto `https://github.com/riku-chip/riku-demo-chip.git`; `RIKU_DEMO_CHIP_URL` lo cambia) con Git, en `--dir` como los demás. No va embebido.
- **R18.2** `riku demo` sin nombre **no** lo clona (es grande y pide red); `--list` lo muestra con su tamaño y "se descarga".
- **R18.3** Sin red, o sin el repo, un error claro que dice la URL; no deja una carpeta a medio clonar.
- **R18.4** La historia (unos 6 commits, una rama con merge): el macro inicial; un cambio en una celda hoja muy instanciada (visto desde la de arriba en miles de instancias); relleno o *straps* en la celda de arriba; un renombre de celda; un pin que se mueve; la rama con un arreglo y su merge.
- **R18.5** El repo pesa lo menos posible: se mide; la meta es menos de 40 MB clonado. Sin Git LFS (Riku no lo soporta todavía).
- **R18.6** Un `README.md` con los tiempos medidos en ese repo: `riku log -n 10`, `riku show` de cada commit, `riku diff` entre los extremos, abrir el visor, y con `RIKU_PROFILE=1` la etapa más cara. Los mismos números en `docs/desarrollo.md` (Rendimiento).
- **R18.7** La licencia del macro (OpenRAM / `sky130_sram_macros`) se confirma en su fuente antes de publicar, y va en el repo con la atribución.
- **R18.8** **Crear el repo en GitHub y subirlo lo hace el usuario** (o con su permiso explícito): es publicar en la organización.

## R19. Documentación

- **R19.1** `README.md` de Riku y `docs/cli.md`: `riku demo` con los cuatro demos, qué muestra cada uno y que `chip` se descarga.
- **R19.2** `docs/desarrollo.md`: cómo regenerar los bundles (`tools/demos/*.py`) y el repo del chip.

## No funcionales

- **NF1** Scripts deterministas (mismas fechas, autor y contenido en cada corrida), como `common.py`.
- **NF2** El ejecutable crece solo lo del bundle del inversor.
- **NF3** La prueba de `riku demo` sigue sin red: el clonado de `chip` se prueba con un repo local (`RIKU_DEMO_CHIP_URL=file://…`).
- **NF4** Sin cambios en el crate de Carlos.
