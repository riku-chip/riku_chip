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
