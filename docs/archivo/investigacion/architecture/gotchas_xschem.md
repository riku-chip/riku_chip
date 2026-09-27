# Gotchas técnicos — Xschem y SVG annotator

Problemas concretos encontrados durante el desarrollo, con causa y solución.
Complementa `gotchas_tecnicos.md` (que cubre el binding Rust de gdstk).

---

## 1. `toggle_colorscheme` rompe la detección de nombres

**Síntoma:** `_extract_name_positions()` devuelve 0 posiciones. El SVG anotado no tiene ninguna anotación.

**Causa:** el comando TCL de render incluía `xschem toggle_colorscheme` para intentar forzar fondo blanco. Esta operación cambia el color de los textos de nombres de componentes de `#cccccc` a `#222222`. El regex de detección busca exactamente `#cccccc`.

**Fix:** eliminar `toggle_colorscheme` del comando. Xschem exporta con su colorscheme activo por defecto — los nombres de instancia siempre son `#cccccc` en ese colorscheme.

**Lección:** no asumir que una operación de "cambio de color" solo afecta al fondo.

---

## 2. `$` en TCL con `shell=True` se interpreta como PID de bash

**Síntoma:** `origins.txt` se escribe en una ruta como `/tmp/12345/origins.txt` (donde 12345 es el PID del proceso bash), no en el path deseado.

**Causa:** al usar `subprocess.run(cmd, shell=True)`, el string pasa por bash antes de llegar a xschem. Bash interpreta `$_f` como la variable `$_` seguida de `f`, o directamente como PID (`$$`).

**Intentos fallidos:**
- Escapar con `\$_f` — TCL lo recibe como literal `\$_f`, no como variable.
- Usar `--tcl <archivo>` — la versión de xschem en Docker no soporta este flag.
- Interpolación de la ruta como string literal — bash corrompe el path si tiene espacios o caracteres especiales.

**Fix correcto:** pasar la ruta vía variable de entorno del OS y leerla desde TCL como `$env(RIKU_ORIGINS_PATH)`. Las variables de entorno no son interpretadas por bash en el string del comando.

```python
env = {**os.environ, "RIKU_ORIGINS_PATH": str(origins_path)}
# TCL: set _f [open $env(RIKU_ORIGINS_PATH) w]
```

---

## 3. Textos `#cccccc` incluyen nombres de tipo de símbolo, no solo instancia

**Síntoma:** el dict `svg_positions` contiene entradas como `cap_mim_m3_1`, `pfet_g5v0d10v5` además de `C1`, `M1`. Al cruzar con `schematic.components`, los tipos no matchean (el parser usa el nombre de instancia como clave, no el tipo).

**Causa:** Xschem dibuja dos textos `#cccccc` por componente: el nombre de instancia (`C1`) y el tipo de símbolo (`cap_mim_m3_1`). Ambos tienen el mismo color.

**Impacto:** ninguno — el dict usa el texto como clave, y los tipos de símbolo no existen como claves en `schematic.components`. El cruce `if name in schematic.components` filtra automáticamente los tipos.

**Trampa:** si en el futuro se agregan símbolos cuyo nombre de instancia colisiona con el nombre de tipo de otro símbolo, el dict sobreescribirá una entrada con la otra. Improbable en diseños reales pero vale tenerlo en mente.

---

## 4. C1 outlier de 19px en Y — offset tipográfico específico del símbolo

**Síntoma:** en el fit de coordenadas, `C1` tiene un error Y de 19.8px mientras el resto de componentes tiene error <2px.

**Causa:** el símbolo `cap_mim_m3_1` (capacitor MIM) dibuja su texto de nombre a ~20px por debajo del anchor del símbolo. Este offset es específico de la celda del PDK y no tiene relación con los otros símbolos.

**Impacto en el fit libre:** este outlier sesga `offset_y` de todo el transform, causando que todos los wires aparezcan desplazados hacia abajo.

**Fix:** eliminación de outliers `2σ` en `_lstsq_free()` y en `_lstsq_fixed_origins()`. C1 queda excluido del fit. Con el resto de componentes el error cae a <2px.

**Lección:** los símbolos de PDK tienen posicionamiento de texto arbitrario. Nunca asumir que el texto del nombre está en el anchor del símbolo.

---

## 5. mooz sesgado por offsets tipográficos

**Síntoma:** los wires aparecen desplazados ~5-10px hacia abajo y a la derecha, incluso con origins exactos.

**Causa:** `mooz` se estimaba del cociente `svg_x / (sch_x + xorigin)` usando las posiciones de los textos `#cccccc`. Pero estos textos no están en el anchor del símbolo — tienen un offset tipográfico que varía por símbolo. El mooz resultante (0.4541) difiere del mooz real de los wires (0.4517) en ~0.5%.

**Medición:** `mooz` desde textos = 0.4541, `mooz` desde wire endpoints = 0.4517. Con 135 pares de wire endpoints, `std = 0.0007`.

**Fix:** calibrar `mooz` desde los endpoints de los paths SVG (`M x yL x y`), que son los wires del esquemático dibujados sin offset tipográfico. Ver `_extract_wire_endpoints()` y la lógica de matching en `_lstsq_fixed_origins()`.

---

## 6. El bounding box centrado en `transform.to_svg(comp.x, comp.y)` no alinea con el símbolo

**Síntoma:** el recuadro de anotación aparece desplazado respecto al símbolo visual en el SVG.

**Causa:** `comp.x, comp.y` es el anchor del símbolo en el .sch, que Xschem usa para posicionarlo. Pero el texto del nombre se dibuja a un offset tipográfico del anchor (variable por símbolo). El transform calcula bien la posición del anchor, pero el anchor no es visualmente el "centro" perceptivo del símbolo.

**Fix:** anclar el bounding box directamente a `svg_positions[cd.name]` — la posición exacta del texto en el SVG, que es el punto de referencia más visible para el usuario. Si el nombre no aparece en el SVG (componente removido), fallback al transform.

```python
if cd.name in svg_positions:
    cx, cy = svg_positions[cd.name]  # exacto — 0px de error
else:
    cx, cy = transform.to_svg(comp.x, comp.y)  # fallback — ~2-5px
```

---

## 7. Formato de paths SVG de Xschem — sin comas

**Síntoma:** el regex `M([\d.\-]+),([\d.\-]+)L([\d.\-]+),([\d.\-]+)` no encuentra ningún path.

**Causa:** Xschem genera paths con espacios como separadores, no comas: `M770.221 404.358L779.251 404.358`. El estándar SVG permite ambos pero Xschem elige espacios.

**Fix:** usar `M([\d.\-]+) ([\d.\-]+)L([\d.\-]+) ([\d.\-]+)` (espacio en lugar de coma).

---

## 8. El SVG cacheado existe pero `origins.txt` no (cache de versión anterior)

**Síntoma:** `_fit_transform()` no encuentra `origins.txt` y cae al fit libre, produciendo menor precisión.

**Causa:** el cache fue generado antes de implementar la escritura de `origins.txt`. El directorio de cache del SVG anterior no tiene el archivo.

**Consecuencia:** el transform usa fit libre con outlier rejection — error ~2-5px en lugar de <0.01px. Suficiente para bounding boxes, insuficiente para wires de alta precisión.

**Resolución natural:** al siguiente cambio del `.sch` o actualización de xschem, se genera un nuevo hash y el nuevo cache incluirá `origins.txt`.

**Forzar regeneración:** eliminar el directorio de cache manualmente: `rm -rf ~/.cache/riku/ops/<hash>/`.
