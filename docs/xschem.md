# Esquemáticos Xschem

Riku lee `.sch` y `.sym` con [`xschem-viewer-rust`](https://github.com/carloscl03/xschem-viewer-rust) (submódulo, de Carlos Cueva): parser, semántica y escena. No hace falta tener `xschem` instalado. El módulo que lo conecta con Riku está en `riku/src/modules/` (`xschem.rs` para el diff, `xschem_view.rs` para el visor, `xschem_pdk.rs` para los símbolos).

## Qué compara el diff

- **Componentes** por nombre (`R1`, `M5`): añadidos, eliminados, modificados (con cada parámetro antes y después) y renombrados.
- **Nets** añadidas y eliminadas.
- **Cosmético:** si todo se movió igual (Move All) o un componente solo cambió de lugar, se marca como cosmético.
- **Archivo nuevo o borrado:** un lado vacío cuenta como esquemático sin nada; todo aparece añadido o eliminado.

## El formato `.sch`

Texto plano. Las líneas que importan:

| Prefijo | Significado |
|---|---|
| `v` | Versión: el archivo empieza con `v {xschem version=`; así se detecta el formato |
| `C {símbolo.sym} X Y rot mirror {atributos}` | Instancia de un componente (`rot` 0–3 en múltiplos de 90°, `mirror` 0/1) |
| `N x1 y1 x2 y2 {lab=…}` | Wire (horizontal o vertical) con su net |

Los atributos son `clave=valor` dentro de `{}` y pueden ocupar varias líneas. Los importantes: `name` (identificador único), `value`, `model`, `W`/`L` en transistores y `lab` en etiquetas de net.

## Símbolos y PDK

Para dibujar los componentes y conectar pines hacen falta los `.sym`. Riku los busca en este orden:

1. **`.xschemrc`** del directorio actual o de `~`:

   | Directiva | Efecto |
   |---|---|
   | `set PDK_ROOT /ruta` + `set PDK sky130A` | `$PDK_ROOT/$PDK/libs.tech/xschem` |
   | `set XSCHEM_SHAREDIR /ruta` | `$XSCHEM_SHAREDIR/xschem_library/devices` |
   | `append XSCHEM_LIBRARY_PATH :/ruta` | cada ruta separada por `:` |

2. **Variables de entorno:** `$PDK_ROOT` + `$PDK` → `$PDK_ROOT/$PDK/libs.tech/xschem`; `$TOOLS` → `$TOOLS/xschem/share/xschem/xschem_library/devices`. En iic-osic-tools, `sak-pdk sky130A` las define.

3. **Detección por símbolos** (si `$PDK` no está definida): Riku mira los PDKs instalados en `$PDK_ROOT` (o `/foss/pdks`) y elige el que tiene los símbolos que usa el esquemático (`sky130_fd_pr/nfet_01v8.sym` → `sky130A`). Si el diseño mezcla PDKs, carga todos los necesarios; en un empate prefiere `sky130A`, `gf180mcuD` e `ihp-sg13g2`.

Solo se usan rutas que existen. El visor muestra en **Detalles** de dónde salió el PDK ("sky130A (detectado)") y avisa si faltan símbolos, con la causa. `riku doctor` resume todo esto.
