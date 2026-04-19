# Proyecto: Visualizador de evolución de layouts con Git + KLayout

## 1. Resumen ejecutivo

Este proyecto busca construir una herramienta que permita **navegar la evolución de un layout de chip a través del historial de Git**, con foco en una experiencia visual y técnicamente útil para ingeniería de diseño físico.

La idea central no es usar Git como visor, sino usar:

- **Git** como fuente de verdad del historial y de los cambios del proyecto.
- **KLayout** como motor de visualización/comparación de layouts.
- **Python** como lenguaje de automatización e integración natural con KLayout.
- **Opcionalmente Rust** como backend o capa de producto en fases futuras.

La hipótesis principal es que hoy existen piezas sueltas:

- historial de versiones,
- layout viewer,
- diff geométrico,
- comparación estructural,

pero no una experiencia integrada y enfocada en:

> “Ver cómo evoluciona visualmente un chip o bloque físico commit a commit”.

---

## 2. Objetivo del proyecto

Construir una aplicación capaz de:

1. Leer un repositorio Git.
2. Identificar commits y archivos relevantes de layout.
3. Abrir el layout asociado a uno o dos commits.
4. Mostrar la evolución visual del diseño.
5. Comparar versiones con herramientas de KLayout como **Diff** y **XOR**.
6. Generar una capa adicional de análisis estructurado (`glayout estructurado`) para resumir cambios.

---

## 3. Problema que resuelve

En el flujo tradicional:

- Git sí versiona archivos del proyecto.
- KLayout sí permite abrir layouts y compararlos.
- Pero no existe una experiencia integrada, clara y orientada a commits.

Problemas actuales:

- Git no sirve bien para interpretar visualmente un `.gds`.
- Los archivos GDS son binarios y su diff textual es poco útil.
- Comparar manualmente versiones de layout consume tiempo.
- No hay una interfaz tipo timeline o explorador histórico visual del diseño.

El proyecto apunta a cerrar ese vacío.

---

## 4. Conceptos clave consolidados

### 4.1 Git no es el visor

Git debe usarse para:

- listar commits,
- obtener archivos por commit,
- detectar qué paths cambiaron,
- recuperar snapshots históricos.

Git **no** debe encargarse de:

- renderizado,
- comparación geométrica visual,
- navegación del layout.

### 4.2 KLayout sí es el núcleo visual

KLayout es la pieza ideal para:

- abrir archivos GDS/OASIS/LEF/DEF,
- visualizar layout,
- hacer zoom y paneo,
- usar herramientas nativas de comparación,
- automatizar tareas mediante scripting.

### 4.3 Dos niveles de representación

#### Nivel 1: Layout visual
Es el layout real, normalmente en formatos como:

- `.gds`
- `.oas`
- `.def`

Sirve para:

- visualización interactiva,
- comparación visual,
- inspección por capas,
- navegación geométrica.

#### Nivel 2: “glayout estructurado”
Es una representación derivada, más fácil de analizar por software. Puede ser JSON u otro formato interno.

Ejemplos de datos:

- top cell,
- bbox,
- capas usadas,
- cantidad de celdas,
- número de instancias,
- métricas agregadas,
- resumen de diferencias entre versiones.

Este nivel **no reemplaza** al layout visual: lo complementa.

---

## 5. Diff, XOR y glayout estructurado

Esta fue una de las confusiones clave y ya quedó consolidada:

### 5.1 Diff
`Diff` en KLayout compara dos layouts de manera **cell-by-cell y object-by-object**. Es más estructural y estricta. Sirve para responder preguntas como:

- ¿qué celdas cambiaron?
- ¿qué instancias difieren?
- ¿qué objetos geométricos exactos cambiaron a nivel de estructura?

### 5.2 XOR
`XOR` en KLayout compara dos layouts a nivel **geométrico capa por capa**. Sirve para responder:

- ¿dónde cambió físicamente la geometría?
- ¿qué shapes están en A y no en B, y viceversa?

### 5.3 Glayout estructurado
No es lo mismo que Diff/XOR.

- **Diff/XOR** son mecanismos de comparación sobre el layout visual.
- **Glayout estructurado** es una capa de interpretación y resumen generada por nuestra herramienta.

En otras palabras:

- GDS = layout real
- Diff/XOR = formas de comparar el layout
- glayout estructurado = modelo derivado para análisis, UI y resúmenes

---

## 6. Decisión arquitectónica principal

### Conclusión consolidada
La arquitectura más sólida para el proyecto es:

- **Python** como lenguaje principal para el MVP.
- **KLayout** como motor/visor.
- **Git** como historial.
- **Qt + LayoutViewWidget** si se busca la experiencia más fluida e integrada.

Se discutieron varias variantes, pero la conclusión fue:

### Opción más fuerte a largo plazo
**B1: integrar la vista interactiva real de KLayout dentro de una aplicación propia usando `LayoutViewWidget`.**

### Opción más simple para empezar
Usar KLayout externamente o usar scripting con Python sin embebido pleno.

Sin embargo, dado que el interés principal es una experiencia visual “movible”, fluida y no limitada a imágenes, la opción que mejor encaja como meta final es **B1**.

---

## 7. Variantes de integración analizadas

## 7.1 Variante A: usar KLayout como aplicación completa externa

### Cómo funciona
La aplicación del proyecto:

1. lista commits,
2. extrae archivos del commit,
3. abre KLayout completo con esos archivos.

### Ventajas

- implementación más simple,
- aprovecha toda la UI existente de KLayout,
- menos fricción inicial,
- ideal para probar la idea.

### Desventajas

- la experiencia queda partida entre tu app y KLayout,
- menos control de UX,
- menos sensación de producto integrado.

### Conclusión
Buena para prototipo rápido, pero no ideal como experiencia final.

---

## 7.2 Variante B1: embebido visual real con `LayoutViewWidget`

### Qué es
Consiste en usar la vista de layout de KLayout **embebida dentro de una aplicación Qt propia**, en lugar de abrir KLayout completo.

### Qué permite

- zoom y paneo reales,
- navegación interactiva fluida,
- capas,
- jerarquía,
- selección,
- experiencia tipo aplicación visual especializada.

### Ventajas

- máxima fluidez,
- máxima integración,
- mejor base para una UX tipo timeline visual,
- evita limitarse a screenshots.

### Desventajas

- complejidad mayor,
- requiere apoyarse en Qt,
- el host natural es Python/Qt más que Rust puro.

### Conclusión
Es la **meta recomendada** si el producto apunta a una experiencia rica y visual.

---

## 7.3 Variante B2: usar KLayout como motor/backend técnico

### Qué es
Usar KLayout sin embeder su vista visual interactiva. En esta variante, KLayout se usa para:

- abrir archivos,
- analizar layouts,
- correr comparaciones,
- generar previews,
- exportar metadata.

### Qué no ofrece naturalmente

- la misma fluidez visual embebida de B1,
- una experiencia tipo visor interactivo integrado.

### Conclusión
Sirve para automatización y análisis, pero no es la mejor variante si la prioridad es la experiencia visual fluida.

---

## 8. Lenguaje recomendado

## 8.1 Decisión consolidada

### MVP y camino recomendado
**Python**.

### Razones

- KLayout soporta scripting integrado en Python.
- La integración con la API de KLayout es natural.
- La integración con Qt es muy viable.
- Permite llegar más rápido al prototipo funcional.

## 8.2 ¿Rust queda descartado?
No necesariamente.

Rust puede tener sentido después para:

- backend de alto rendimiento,
- gestión de estado,
- capa de producto,
- integración con Git,
- servicios adicionales.

Pero para la parte de **embebido visual con KLayout**, Python + Qt es la ruta más natural.

### Recomendación final

- **Primera etapa:** Python + Qt + KLayout
- **Etapas futuras opcionales:** introducir Rust donde realmente aporte valor

---

## 9. Experiencia de usuario objetivo

La UX objetivo ideal sería algo así:

### Panel izquierdo
- lista de commits,
- filtros,
- búsqueda,
- selección A/B.

### Panel central
- visor embebido del layout,
- zoom,
- paneo,
- selección,
- capas,
- navegación visual.

### Panel derecho
- diff lógico por Git,
- resumen Diff de KLayout,
- resumen XOR,
- métricas estructuradas,
- observaciones del cambio.

### Modos de operación
- ver un commit,
- comparar A vs B,
- comparar con commit anterior,
- timeline,
- overlays,
- vistas lado a lado.

---

## 10. Alcance funcional propuesto

### 10.1 Funcionalidades mínimas

1. Seleccionar un repositorio.
2. Detectar archivos de layout relevantes.
3. Listar commits.
4. Abrir el layout de un commit.
5. Comparar dos commits.
6. Mostrar archivos cambiados por Git.

### 10.2 Funcionalidades intermedias

1. Ejecutar Diff.
2. Ejecutar XOR.
3. Mostrar top cell.
4. Mostrar bbox.
5. Mostrar capas afectadas.
6. Mostrar cambios resumidos.
7. Cachear resultados intermedios.

### 10.3 Funcionalidades avanzadas

1. Timeline visual.
2. Overlays interactivos.
3. Glayout estructurado enriquecido.
4. Miniaturas por commit.
5. Heatmaps de cambios.
6. Navegación entre commits con preload.
7. Comparación semántica de bloques/celdas.

---

## 11. Roadmap recomendado

## Fase 0: definición y convenciones

### Objetivo
Definir convenciones del proyecto para que el sistema tenga un comportamiento predecible.

### Tareas

- definir estructura mínima del repo,
- definir qué archivos son candidatos de layout,
- definir top cell esperada,
- definir ubicación de metadata auxiliar,
- definir estrategia de temporales y caché.

### Entregable
Documento de convenciones.

---

## Fase 1: exploración del repositorio Git

### Objetivo
Poder leer el historial y detectar commits/archivos útiles.

### Tareas

- seleccionar repositorio,
- listar commits,
- detectar paths relevantes,
- obtener diff de archivos entre commits,
- construir modelo interno de historial.

### Entregable
Panel de commits y diff básico de paths.

---

## Fase 2: apertura de layout de un commit

### Objetivo
Abrir visualmente el layout asociado a un commit.

### Tareas

- resolver archivo de layout por commit,
- extraer snapshot a temporal,
- cargarlo en KLayout,
- validar top cell,
- manejar errores comunes.

### Entregable
Visualización de un commit.

---

## Fase 3: comparación A/B

### Objetivo
Comparar dos commits con Git + KLayout.

### Tareas

- seleccionar commit A y B,
- extraer ambos layouts,
- abrirlos,
- presentar diff de Git,
- preparar base para Diff/XOR.

### Entregable
Comparación básica A/B.

---

## Fase 4: integración de Diff y XOR

### Objetivo
Enriquecer la comparación con herramientas nativas de KLayout.

### Tareas

- ejecutar Diff,
- ejecutar XOR,
- capturar resultados,
- resumir diferencias,
- ligar visual y textual.

### Entregable
Comparación estructural y geométrica.

---

## Fase 5: glayout estructurado

### Objetivo
Crear una capa propia de análisis y resumen.

### Tareas

- definir esquema JSON,
- extraer metadata del layout,
- resumir diferencias A/B,
- construir representación estructurada reutilizable por UI.

### Entregable
`glayout.json` o equivalente por snapshot/comparación.

---

## Fase 6: experiencia integrada embebida

### Objetivo
Pasar a la experiencia objetivo con `LayoutViewWidget`.

### Tareas

- embeder la vista en la app,
- sincronizar selección de commits con la vista,
- diseñar paneles,
- agregar controles de navegación,
- optimizar UX.

### Entregable
Prototipo interactivo fluido.

---

## 12. Estructura propuesta del proyecto

```text
project/
  app/
    main.py
    ui/
      main_window.py
      commit_panel.py
      compare_panel.py
      layout_panel.py
      summary_panel.py
    core/
      git_service.py
      layout_service.py
      compare_service.py
      cache_service.py
      glayout_service.py
      models.py
    klayout/
      scripts/
        open_layout.py
        compare_diff.py
        compare_xor.py
        export_glayout.py
  cache/
  temp/
  docs/
  tests/
```

---

## 13. Modelo conceptual de módulos

## 13.1 Git service
Responsable de:

- listar commits,
- leer diffs,
- extraer archivos por commit,
- identificar cambios relevantes.

## 13.2 Layout service
Responsable de:

- resolver layout principal,
- abrir layout,
- manejar temporales,
- coordinar carga visual.

## 13.3 Compare service
Responsable de:

- comparar commit A/B,
- lanzar Diff,
- lanzar XOR,
- consolidar resultados.

## 13.4 Glayout service
Responsable de:

- generar metadata estructurada,
- resumir métricas,
- crear modelo consumible por UI.

## 13.5 UI layer
Responsable de:

- mostrar historial,
- mostrar estado,
- sincronizar navegación,
- permitir comparación y exploración.

---

## 14. Estrategia de archivos por commit

Recomendación principal:

### No depender solo del archivo abierto en la carpeta viva del repo

Porque aunque KLayout puede detectar cambios en disco y ofrecer recargar, basar el sistema entero en eso es frágil.

### Estrategia recomendada

- extraer archivos del commit a temporales,
- abrir desde esos temporales,
- aislar vistas A/B,
- evitar conflictos con checkout del working tree.

### Beneficios

- reproducibilidad,
- menos errores,
- comparación segura,
- soporte limpio para A/B.

---

## 15. Estrategia para glayout estructurado

Propuesta de campos iniciales:

```json
{
  "commit": "abc1234",
  "top_cell": "TOP",
  "file": "layout/top.gds",
  "bbox": [0, 0, 124000, 98000],
  "layers": ["M1", "M2", "M3", "VIA1"],
  "cell_count": 203,
  "instance_count": 12540,
  "notes": []
}
```

### Para comparaciones

```json
{
  "commit_a": "abc1234",
  "commit_b": "def5678",
  "changed_files": [
    "rtl/top.v",
    "layout/top.gds"
  ],
  "diff_summary": {
    "changed_cells": 8,
    "changed_instances": 27
  },
  "xor_summary": {
    "layers_affected": ["M1", "M2"],
    "regions_changed": 14
  },
  "glayout_delta": {
    "bbox_changed": false,
    "instance_delta": 120,
    "cell_delta": 3
  }
}
```

---

## 16. Riesgos técnicos

## 16.1 Ambigüedad del archivo principal
No todos los repos tendrán un `top.gds` único o predecible.

Mitigación:

- convenciones,
- configuración por proyecto,
- autodetección con fallback manual.

## 16.2 Layouts pesados
Los layouts grandes pueden afectar UX y tiempos.

Mitigación:

- caché,
- vistas parciales,
- preload,
- miniaturas.

## 16.3 Integración Qt
El embebido con `LayoutViewWidget` requiere más trabajo de UI y arquitectura.

Mitigación:

- MVP primero,
- transición progresiva a embebido.

## 16.4 Repos sin artefactos de layout en cada commit
A veces el commit no tendrá `.gds` generado.

Mitigación:

- modo “best effort”,
- resolución por configuración,
- marcar commits incompletos,
- futura integración con pipeline de regeneración.

---

## 17. Decisiones ya consolidadas

Estas son las decisiones más fuertes y ya bastante asentadas:

1. **KLayout es la base correcta.**
2. **Git solo debe encargarse del historial y paths.**
3. **Diff y XOR no sustituyen al glayout estructurado.**
4. **Sí conviene tener glayout estructurado.**
5. **La experiencia más fluida es B1 con `LayoutViewWidget`.**
6. **Python es el lenguaje más natural para el MVP.**
7. **Rust puede entrar después, pero no debe forzar la primera implementación.**
8. **No conviene depender de recarga automática del mismo archivo vivo del repo.**
9. **La estrategia de temporales por commit es más robusta.**
10. **La UX final debe priorizar vista interactiva, no solo imágenes.**

---

## 18. Recomendación final de implementación

### Camino recomendado

#### Etapa 1
- Python
- Git
- KLayout scripting
- comparación básica
- glayout estructurado inicial

#### Etapa 2
- Python + Qt
- `LayoutViewWidget`
- app integrada con visor embebido

#### Etapa 3
- optimizaciones,
- timeline,
- overlays,
- análisis enriquecido,
- exploración más avanzada.

### Resumen de la apuesta técnica

Si el objetivo es construir algo serio, útil y con buena UX:

> **La mejor apuesta es Python + Qt + KLayout (`LayoutViewWidget`) + Git + una capa propia de glayout estructurado.**

---

## 19. Posible pitch corto del proyecto

> Herramienta para explorar visualmente la evolución de layouts de chip a través del historial de Git, usando KLayout como motor de visualización y comparación, y una capa propia de análisis estructurado para resumir cambios de forma comprensible.

---

## 20. Referencias técnicas clave

- KLayout Documentation: https://www.klayout.de/doc.html
- KLayout scripting / Python: https://www.klayout.de/doc-qt5/programming/index.html
- KLayout Python usage: https://www.klayout.de/0.29/doc/programming/python.html
- KLayout Diff Tool: https://www.klayout.de/doc/manual/diff.html
- KLayout XOR Tool: https://www.klayout.de/doc/manual/xor.html
- KLayout Loading / Reload behavior: https://www.klayout.de/doc/manual/loading.html
- LayoutViewWidget API: https://www.klayout.de/doc/code/class_LayoutViewWidget.html
- LayoutView API: https://www.klayout.de/doc/code/class_LayoutView.html
- KLayout class/module index: https://www.klayout.de/doc/code/module_lay.html
- Discussion on `LayoutView` vs `LayoutViewWidget`: https://www.klayout.de/forum/discussion/2259/layoutview-documentation-on-web-site-does-not-include-qt-binding-methods

---

## 21. Próximos pasos sugeridos para Codex

1. Generar el esqueleto del proyecto en Python.
2. Crear módulo `git_service.py`.
3. Crear módulo `layout_service.py`.
4. Probar apertura de un GDS desde un commit.
5. Crear modelo de datos para `glayout`.
6. Definir primera UI en Qt.
7. Investigar integración mínima de `LayoutViewWidget`.
8. Implementar comparación A/B.
9. Agregar Diff.
10. Agregar XOR.

---

## 22. Conclusión final

La idea completa sí tiene sentido y sí puede convertirse en una herramienta valiosa.

No debe pensarse como:

- un Git raro,
- un simple visor,
- o un comparador de imágenes.

Debe pensarse como:

> **una plataforma de exploración histórica de layouts, basada en Git + KLayout + análisis estructurado propio.**

Ese es el enfoque más sólido, más coherente con la documentación de KLayout y más prometedor para un producto útil.
