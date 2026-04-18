# Xschem Diff y Ecosistema EDA — Investigación para Miku

## 1. Formato `.sch` de Xschem

Los archivos `.sch` son **texto plano, un objeto por línea**, lo que los hace naturalmente compatibles con `git diff`.

Cada línea empieza con una letra que indica el tipo de objeto:

| Código | Objeto |
|--------|--------|
| `v` | Versión del archivo |
| `N` | Wire (net) |
| `C` | Instancia de componente |
| `T` | Etiqueta de texto |
| `L` | Línea gráfica |
| `B` | Rectángulo / caja de pin |
| `P` | Polígono |
| `A` | Arco o círculo |

**Ejemplo concreto:**
```
v {xschem version=3.4.7 file_version=1.3}
N 890 -130 890 -110 {lab=ANALOG_GND}
C {capa.sym} 890 -160 0 0 {name=C4 m=1 value=10u device="tantalium capacitor"}
T {3 of 4 NANDS} 500 -580 0 0 0.4 0.4 {font=Monospace layer=4}
```

**Qué se ve en `git diff`:**
- Agregar/quitar un componente → una línea `C` añadida/eliminada
- Mover un componente → cambio de coordenadas en esa misma línea
- Cambiar un valor (`value=10u` → `value=100u`) → diff inline legible

**Fricción principal:** Si se hace un "move all" (reposicionar todo el esquemático), el diff es ruidoso porque cambian todas las coordenadas. El propio equipo de Xschem documenta el formato como "bueno para herramientas de versionado como git/subversion".

---

## 2. Diff visual nativo de Xschem

**Xschem tiene diff visual incorporado desde la versión 3.4.0.**

### Uso por línea de comandos
```bash
xschem --diff schematic_a.sch schematic_b.sch
```
Workflow típico con git:
```bash
git show HEAD~1:mi_circuito.sch > /tmp/anterior.sch
xschem --diff /tmp/anterior.sch mi_circuito.sch
```

### Uso interactivo
- Atajo `Alt-x` dentro de Xschem
- Compara el esquemático abierto con la versión guardada en disco

### Cómo se visualiza
- Elementos presentes solo en A → **gris**
- Elementos presentes solo en B → **rojo** (overlay fantasma)
- Aplica a nets, instancias y propiedades

**Limitación:** requiere correr la GUI de Xschem. No hay un output standalone exportable (imagen/SVG de la comparación).

---

## 3. Herramientas externas de diff visual para esquemáticos

No existe ninguna herramienta externa que soporte el formato `.sch` de Xschem específicamente. El ecosistema más maduro es el de **KiCad**, que sirve de referencia y modelo a seguir.

### plotgitsch / plotkicadsch
- Diff visual para KiCad `.sch` almacenados en git
- Dos modos: (a) diff de bitmaps, (b) diff vectorial → SVG con adiciones en verde y eliminaciones en rojo
- Funciona como git difftool driver
- **Relevancia para Miku:** misma arquitectura aplicable a Xschem
- [github.com/jnavila/plotkicadsch](https://github.com/jnavila/plotkicadsch)

### KiRI (KiCad Review Inspector)
- Exporta ambas revisiones como SVG con `kicad-cli`, las compara en una interfaz web side-by-side
- **Relevancia para Miku:** Xschem tiene `xschem -q --svg` headless — el mismo patrón es directamente replicable
- [github.com/leoheck/kiri](https://github.com/leoheck/kiri)

### CADLAB.io
- Plataforma web (freemium) para diff visual de KiCad, Eagle, Altium en GitHub/GitLab
- No soporta Xschem
- Referencia de UX: muestra componentes añadidos/eliminados en el navegador con color
- [cadlab.io](https://cadlab.io)

### Enfoque ImageMagick (genérico)
- Exportar ambas revisiones a PNG → `imagemagick composite` para overlay con color
- Funciona con cualquier herramienta que exporte imagen, incluido Xschem
- ```bash
  xschem -q --no_x --png --plotfile rev_a.png schematic_a.sch
  xschem -q --no_x --png --plotfile rev_b.png schematic_b.sch
  composite -stereo 0 rev_a.png rev_b.png diff.png
  ```
- [evilmadscientist.com — Visual Diffs](https://www.evilmadscientist.com/2011/improving-open-source-hardware-visual-diffs/)

---

## 4. Flags headless de Xschem (clave para automatización)

```bash
xschem -q --no_x --png --plotfile output.png schematic.sch   # exportar PNG
xschem -q --no_x --svg --plotfile output.svg schematic.sch   # exportar SVG
xschem --diff rev_anterior.sch rev_actual.sch                 # diff visual en GUI
```

- `-q` / `--quit`: sale después de procesar (no interactivo)
- `--no_x` / `-x`: sin display (headless, para CI)
- `--tcl <script>`: scripting Tcl para automatización avanzada

---

## 5. Git diff driver para Xschem (gap actual)

**No existe un driver publicado por la comunidad.** Es un gap que Miku puede cubrir.

Configuración base:

```ini
# .gitattributes
*.sch diff=xschem

# .git/config o ~/.gitconfig
[diff "xschem"]
    command = miku-xschem-diff.sh
```

Donde `miku-xschem-diff.sh` llama a Xschem headless + ImageMagick o genera un SVG pair para el viewer de Miku.

---

## 6. Tabla resumen

| Herramienta | Formato | Diff visual | Integración git | Estado |
|---|---|---|---|---|
| **Xschem `--diff`** | Xschem .sch | Sí (overlay en GUI) | Manual | Activo v3.4.0+ |
| **plotgitsch** | KiCad .sch | Sí (SVG vectorial) | Sí (difftool) | Activo |
| **KiRI** | KiCad .sch | Sí (web, side-by-side) | Sí | Activo |
| **CADLAB.io** | KiCad/Eagle/Altium | Sí (web) | Sí (GitHub/GitLab) | Comercial |
| **ImageMagick** | Cualquiera (necesita PNG) | Sí (raster) | Via git hooks | Setup manual |
| **Driver git para Xschem** | Xschem .sch | Posible | No publicado | **Gap de Miku** |

---

## 7. Conclusiones para Miku

1. **El texto plano de Xschem ya funciona con `git diff`** — no hay que hacer nada para el diff básico. Es una ventaja sobre GDS.

2. **Xschem ya tiene diff visual nativo** (`--diff` y `Alt-x`). Miku podría simplemente invocar esto o wrapearlo, en lugar de construirlo desde cero.

3. **El camino más eficiente para diff visual integrado en Miku:**
   - Exportar ambas revisiones como SVG con `xschem -q --no_x --svg`
   - Mostrarlas en un viewer web side-by-side (siguiendo el patrón de KiRI)
   - Esto no requiere depender del GUI de Xschem

4. **El git diff driver para `.sch` es el aporte concreto que Miku puede publicar** — nadie lo ha hecho todavía para Xschem.

5. **Para GDS el problema es más duro** (binario) → ver investigación de KLayout.

---

## 8. Notas de retroalimentación — flujos reales

> Estas notas provienen del research de experiencia de usuario en foros y comunidades EDA reales.
> Identifican casos que el modelo de este documento no contempla actualmente.

### 8a. `.sch` no siempre es formato Xschem

El driver de Xschem en la arquitectura asume que todo archivo `.sch` es formato Xschem. En proyectos reales, la extensión `.sch` la usan también:

- **Qucs-S:** GUI alternativa para NGSpice con mejor visualización de waveforms y Monte Carlo más simple. Sus archivos `.sch` tienen sintaxis completamente distinta al formato Xschem (XML-like, no TCL-like). Qucs-S tiene demanda real entre diseñadores que prefieren UI sobre CLI.
- **KiCad Eeschema:** La versión heredada de KiCad también usa `.sch` (el formato nuevo es `.kicad_sch`). Diseñadores con background en PCB que entran al mundo IC pueden tener este archivo.

**El contenido del archivo resuelve la ambigüedad:**
- Xschem: primera línea `v {xschem version=...}`
- Qucs-S: header XML o `<Qucs Schematic ...>`
- KiCad legacy: línea `EESchema Schematic File Version N`

La detección por contenido de la Sección 2 del documento de arquitectura ya contempla `b'xschem version=' in h[:80]` — esto es correcto y suficiente para no confundir formatos.

**Implicación práctica:** Si el usuario tiene archivos `.sch` de Qucs-S o KiCad en el mismo repo que archivos Xschem, Miku no los confundirá si la detección por contenido está implementada correctamente. No se requiere un driver Qucs-S para el MVP — basta con hacer fallback a diff de texto cuando el header no coincide.

Fuente: [ngspice.sourceforge.io/ngspice-eeschema.html](https://ngspice.sourceforge.io/ngspice-eeschema.html), [github.com/ra3xdh/qucs_s](https://github.com/ra3xdh/qucs_s)

### 8b. Proyectos sin ningún archivo Xschem

Casos documentados donde un proyecto de chips no tiene ningún `.sch` de Xschem:

1. **Flujo puramente digital (OpenLane/OpenROAD):** Todo el diseño parte de Verilog. No hay esquemático analógico.
2. **GDSFactory/GLayout (layout generado por Python):** El "esquemático" es implícito en el código; no hay `.sch`.
3. **Cadence Virtuoso + PDK open:** El esquemático vive en Virtuoso (`.sdb`/`.cdl`). Miku nunca ve el esquemático.
4. **SPICE-first workflow:** El diseñador escribe el netlist directamente y nunca formaliza un esquemático gráfico.

**Implicación:** `miku init` no debe requerir que exista un `.sch` para configurar el proyecto. Los drivers son opcionales — si no hay archivos Xschem, el driver simplemente nunca se invoca.

### 8c. Importación de netlists SPICE en Xschem (gap de la herramienta)

Xschem no puede importar un netlist SPICE existente y convertirlo en esquemático gráfico. Esto es un gap conocido de la herramienta — hay un feature request abierto (issue #35) sin implementación.

El workaround actual: crear un símbolo vacío con los puertos correctos y el atributo `spice_sym_def` apuntando al netlist externo. Así funcionan todas las celdas estándar de SKY130 en el ambiente Xschem — no tienen esquemático gráfico, solo símbolos con netlists externos.

**Implicación para el diff:** Cuando Miku hace diff de un `.sch` que incluye celdas via `spice_sym_def`, el diff semántico correcto requiere resolver esas referencias para entender qué cambió en el circuito. Un diff de solo el `.sch` sin resolver las referencias externas puede no detectar cambios significativos en el comportamiento del circuito.

Fuente: [github.com/StefanSchippers/xschem/issues/35](https://github.com/StefanSchippers/xschem/issues/35)

### 8d. Diff semántico de `.sch` — oportunidad no cubierta

El diff de texto de un `.sch` muestra cambios de coordenadas crudos que son difíciles de interpretar. Nadie ha publicado un diff semántico para archivos Xschem que reporte:

> "Resistor R1 cambió de 1k a 10k"  
> "Net VDD desconectado del gate de M3"  
> "Componente C4 eliminado del esquemático"

Esto es distinto al diff visual (SVG side-by-side) — es un diff estructurado legible en texto, como el diff semántico de JSON o XML. Sería el aporte de mayor impacto de Miku en el espacio Xschem, y no existe en ninguna herramienta.

---

## Referencias

### Xschem
- **Repo oficial**: https://github.com/StefanSchippers/xschem
- **Documentación**: https://xschem.sourceforge.io/stefan/xschem_man/xschem_man.html
- **Flags headless (`--no_x`, `--svg`, `--png`)**: sección "Command line options" de la documentación
- **Flag `--diff`** (v3.4.0+): https://github.com/StefanSchippers/xschem/blob/master/CHANGELOG

### Herramientas de diff visual para esquemáticos
- **plotgitsch** (KiCad): https://github.com/jnavila/plotkicadsch — referencia de arquitectura para Miku
- **KiRI**: https://github.com/leoheck/kiri — diff web para KiCad, modelo de UX a seguir
- **CADLAB.io**: https://cadlab.io — plataforma propietaria con diff visual para KiCad/Eagle

### Ecosistema EDA open-source relacionado
- **IIC-OSIC-TOOLS**: https://github.com/iic-jku/iic-osic-tools — entorno Docker con todas las herramientas EDA
- **OpenLane**: https://github.com/The-OpenROAD-Project/OpenLane — flujo RTL-to-GDS automatizado
- **OpenROAD**: https://github.com/The-OpenROAD-Project/OpenROAD — herramientas de síntesis y P&R

### Formato .sch — especificación informal
- No hay spec formal; la mejor referencia es el código fuente de Xschem y los archivos de ejemplo incluidos en el repo.

### Ver también
- [headless_y_compatibilidad_herramientas.md](headless_y_compatibilidad_herramientas.md) — Xschem sin X11
- [../operaciones/estrategia_merge_archivos_mixtos.md](../operaciones/estrategia_merge_archivos_mixtos.md) — merge de .sch
- [../operaciones/ci_drc_lvs_regresiones.md](../operaciones/ci_drc_lvs_regresiones.md) — exportación de netlist desde Xschem en CI
- [../arquitectura/arquitectura_cli_y_orquestacion.md](../arquitectura/arquitectura_cli_y_orquestacion.md) — detección de .sch por cadena `xschem version=` y registro de driver
- [../operaciones/cache_y_rendimiento.md](../operaciones/cache_y_rendimiento.md) — almacenamiento de artefactos SVG/PNG en caché de artefactos
