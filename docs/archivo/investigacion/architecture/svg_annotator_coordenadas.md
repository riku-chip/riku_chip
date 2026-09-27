# Sistema de Anotación SVG — Coordenadas y Calibración

Documento de referencia para entender cómo Riku calcula la transformación
de coordenadas `.sch → SVG` y cómo se anotan los diffs visuales.

---

## El problema de coordenadas

Xschem usa dos sistemas de coordenadas distintos:

1. **Sistema .sch** — coordenadas enteras en unidades de esquemático. El origen es arbitrario (puede ser negativo). Un componente en `(2690, -40)` es normal.
2. **Sistema SVG** — píxeles, siempre positivos, dentro del viewport (típicamente 900×532px).

La relación entre ambos es lineal:

```
svg_x = (sch_x + xorigin) * mooz
svg_y = (sch_y + yorigin) * mooz
```

Donde:
- `xorigin`, `yorigin` — punto de referencia que Xschem calcula internamente al hacer `zoom_full`. Dependen del bounding box del esquemático.
- `mooz` — inverso del zoom (`1/zoom`). Depende del tamaño del viewport y del esquemático.

Ninguno de estos valores es fijo ni predecible sin consultar a Xschem.

---

## Extracción de origins

Al generar el SVG, `XschemDriver.render()` ejecuta:

```tcl
xschem zoom_full
set _f [open $env(RIKU_ORIGINS_PATH) w]
puts $_f [xschem get xorigin]
puts $_f [xschem get yorigin]
close $_f
xschem print svg {ruta_cache}
```

Esto guarda `xorigin` y `yorigin` en `origins.txt` junto al `render.svg` en el directorio de cache. Ambos archivos tienen la misma clave SHA256, por lo que están sincronizados.

**Por qué env var y no interpolación directa:** con `shell=True`, el carácter `$` en un string Python es interpretado por el shell como variable de entorno antes de llegar a TCL. `$RIKU_ORIGINS_PATH` se convierte en el PID del proceso. La solución es pasar la ruta en `os.environ` y que TCL la lea como `$env(RIKU_ORIGINS_PATH)`.

---

## Calibración de mooz

### Intento inicial: textos de nombres (`#cccccc`)

Los textos de color `#cccccc` en el SVG son los nombres de instancia de los componentes (`C1`, `M1`, `x2`...). Se pueden extraer y cruzar con las coordenadas del `.sch`.

**Problema:** Xschem coloca estos textos con un offset tipográfico variable por símbolo — el texto no está en el anchor del símbolo sino a una distancia que depende de cada celda. Esto sesga el `mooz` estimado:

- mooz desde textos: **0.4541**
- mooz real: **0.4517**
- Sesgo: **0.5%** — parece pequeño pero causa ~5-10px de desfase en wires lejanos del origen.

### Solución: calibración desde wire endpoints

Los paths `M x yL x y` del SVG son los wires del esquemático, dibujados directamente en coordenadas SVG sin offset tipográfico. Sus endpoints cumplen la fórmula exacta.

**Algoritmo:**

1. Estimar `mooz_preliminar` desde textos (eje X, menos ruidoso que Y).
2. Para cada endpoint `(sch_x, sch_y)` de los wires del `.sch`, calcular la posición predicha `(px, py) = ((sch_x + xorigin) * mooz_preliminar, ...)`.
3. Buscar el endpoint SVG más cercano a `(px, py)`. Si `dist < 8px`, aceptar el par.
4. Con los pares validados, calcular `mooz` exacto desde ambos ejes: `mooz = (mean_mooz_x + mean_mooz_y) / 2`.
5. Descartar outliers `> 2σ` antes de la media.

**Resultado:** con 135 pares en `example_por.sch`, `mooz = 0.4517 ± 0.0007`. Error de predicción de wires `<0.01px`.

### Fallback cuando no hay origins.txt

Cuando el SVG fue generado sin Riku (o el cache está frío y xschem no está disponible), se usa fit libre por mínimos cuadrados sobre los textos de nombres, con eliminación de outliers `2σ`. El error típico es `<5px`, suficiente para bounding boxes pero insuficiente para wires de precisión.

---

## Anotaciones generadas

### Bounding boxes de componentes

```python
# Anchor: posición del texto del nombre en el SVG (exacta por definición)
if cd.name in svg_positions:
    cx, cy = svg_positions[cd.name]
else:
    comp = source.components[cd.name]
    cx, cy = transform.to_svg(comp.x, comp.y)
```

El anchor en `svg_positions[name]` tiene error 0px — es el pixel exacto donde Xschem dibujó el texto. El fallback al transform tiene ~2-5px de error (offset tipográfico).

**Colores:**
- Verde (`rgba(0,200,0,...)`) — componente añadido
- Rojo (`rgba(200,0,0,...)`) — componente eliminado
- Amarillo (`rgba(255,180,0,...)`) — componente modificado

### Trayectos de wires

Para cada wire del `.sch` cuyo label pertenece a una net añadida o eliminada:

```python
x1, y1 = transform.to_svg(w.x1, w.y1)
x2, y2 = transform.to_svg(w.x2, w.y2)
# => <line x1=... y1=... x2=... y2=... stroke="verde/rojo"/>
```

Con `mooz` calibrado desde wire endpoints, el error es `<0.01px`.

**Nota:** los wires de nets eliminadas se obtienen del `sch_a` (versión anterior), que no tiene SVG propio — por eso se usa el transform del `sch_b` que sí fue renderizado. Esto es correcto si el layout no cambió drásticamente entre versiones.

---

## Estructura del cache

```
~/.cache/riku/ops/<sha256>/
    render.svg      — SVG generado por Xschem
    origins.txt     — xorigin en línea 1, yorigin en línea 2
```

La clave SHA256 = `sha256(xschem_version + b"::" + sch_content)`. Si el contenido del `.sch` o la versión de Xschem cambian, se genera un nuevo directorio. No hay invalidación manual necesaria.

---

## Gotchas

| Problema | Causa | Solución |
|---|---|---|
| 0 nombres encontrados en SVG | `toggle_colorscheme` cambia `#cccccc` → `#222222` | Eliminar `toggle_colorscheme` del comando TCL |
| `$_f` se interpreta como PID en shell | `$` especial en bash con `shell=True` | Usar `$env(RIKU_ORIGINS_PATH)` vía variable de entorno |
| Wires desfasados ~5-10px | mooz sesgado por offsets tipográficos de textos | Calibrar mooz desde wire endpoints (paths SVG) |
| Bounding boxes desfasados | Usar `transform.to_svg()` en lugar de anchor directo | Anclar a `svg_positions[name]` cuando disponible |
| C1 outlier de 19px en Y | El texto del símbolo `cap_mim_m3_1` está muy lejos del anchor | Eliminación de outliers 2σ en fit libre |
