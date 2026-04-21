# gds-renderer

Motor de escena para layouts GDS usando `gdstk-rs` como fuente de geometria.

Estado actual:
- acepta una `RenderScene` con comandos de dibujo
- puede construir la escena desde una celda como compatibilidad temporal
- agrupa por `layer/datatype`
- soporta labels basicos
- admite una capa extra de highlights

El backend inicial sigue siendo SVG, pero el contrato ya se parece a un canvas:
- `Viewport`
- `LayerCatalog` / `LayerStyle`
- `DrawCommand`
- `HighlightSet`

En esta primera etapa el objetivo es tener una base funcional, testeable y lista para migrar a un renderer tipo canvas.
La parte de UI interactiva y el backend nativo quedan para despues.

Estructura:
- `src/scene.rs`: comandos, escena y highlights
- `src/style.rs`: colores, capas, config y metadatos
- `src/viewport.rs`: camara, bbox y `viewBox`
- `src/output.rs`: salida SVG y metadata
- `src/compat.rs`: adaptador temporal desde `gdstk-rs`
- `src/renderer.rs`: construccion del SVG desde escena
- `src/lib.rs`: fachada publica del crate
