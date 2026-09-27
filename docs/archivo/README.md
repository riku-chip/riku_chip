# Archivo

Documentos que ya no se mantienen. Se conservan porque explican de dónde salen decisiones actuales, pero **pueden no coincidir con el código**: la referencia vigente está en [`../`](../README.md).

| Qué | Contenido |
|---|---|
| [`plan_migracion_microkernel.md`](plan_migracion_microkernel.md) | Revisión de la arquitectura de 2026-09 y el plan de fases 0–5 (monolito modular + microkernel), con el avance de cada fase. El estado resultante está en [`../arquitectura.md`](../arquitectura.md) |
| [`investigacion/`](investigacion/README.md) | Investigación inicial del proyecto: herramientas EDA (Xschem, KLayout, Magic, NGSpice), CI con DRC/LVS, merge de archivos mixtos, cache, UX. Varias partes son de la etapa en Python |

Otros diseños terminados (ejecutable único, pendientes de CI y GUI, especificación de `status`/`log`, plan de la integración GDS) se borraron en la consolidación de la documentación; están en el historial de git:

```bash
git log --diff-filter=D --name-only -- docs riku/docs
```
