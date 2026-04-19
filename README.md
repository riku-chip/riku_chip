# 🏺 Riku-Chip

**The High-Performance VCS for Analog IC Design.**

Riku es un sistema de control de versiones (VCS) especializado para semiconductores, diseñado para entender la semántica de archivos GDSII y esquemáticos Xschem. A diferencia de las herramientas tradicionales, Riku combina la seguridad y el paralelismo de **Rust** con el motor probado de **gdstk (C++)** para ofrecer comparaciones ultrarrápidas y precisas.

---

## 🚀 ¿Por qué Riku?

El diseño analógico moderno maneja archivos GDS de varios gigabytes y jerarquías complejas que saturan los VCS tradicionales. Riku resuelve esto mediante:

*   **⚡ Motor de Alto Rendimiento:** Basado en `gdstk-rs`, un binding de Rust para el core C++ de gdstk, logrando un speedup de **~2.6×** frente a implementaciones puras en Python.
*   **🧵 Paralelismo Real (No GIL):** Gracias a Rust, Riku puede paralelizar comparaciones XOR y parseo de múltiples archivos usando todos los cores de la CPU sin las limitaciones del Global Interpreter Lock de Python.
*   **🔍 Diferencial Semántico e Inteligente:** 
    *   **Esquemáticos:** Identifica cambios en parámetros (`W`, `L`, `nf`) y conectividad.
    *   **Layouts:** Ejecuta operaciones booleanas XOR para detectar diferencias geométricas precisas.
*   **📦 Eficiencia de Memoria:** Consume hasta **26× menos memoria** que soluciones basadas en multiprocessing de Python al procesar múltiples archivos simultáneamente.

---

## 📊 Benchmarks (Rust vs Python)

*Mediciones realizadas sobre archivos GDS reales (ej: tinytapeout.gds).*

| Escenario | Python (gdstk) | Riku (Rust/C++) | Ganancia |
| :--- | :--- | :--- | :--- |
| **Startup + Parse** | 169 ms | **65 ms** | 2.6x más rápido |
| **Paralelismo (8 archivos)** | 476.9 ms | **158.7 ms** | 3.0x más rápido |
| **Memoria (Pico)** | 277.7 MB | **10.6 MB** | 26x más eficiente |

---

## 🛠️ Arquitectura

Riku opera bajo una filosofía **Read-Only**: no construye geometría, solo la lee, la compara y atribuye cambios. Esto permite una superficie de API mínima, segura y extremadamente optimizada.

*   **Core:** Rust 1.75+ con `cxx` bridge.
*   **Geometría:** gdstk (C++) + Clipper.
*   **Git:** `pygit2` (acceso directo a blobs, sin subprocess).
*   **CLI:** Typer (Python) para una experiencia de usuario moderna y extensible.

---

## 📖 Guía de Uso Rápido

### Comparación de Esquemáticos (Xschem)
```bash
# Ver cambios semánticos en terminal
riku diff cell.sch --commit-a HEAD~1 --format text

# Generar reporte visual anotado (SVG)
riku diff cell.sch --commit-a HEAD~1 --format visual
```

### Comparación de Layouts (GDS)
```bash
# Ejecutar XOR geométrico entre versiones
riku diff layout.gds --commit-a v1.0 --commit-b v1.1 --mode xor
```

---

## 🚧 Estado del Proyecto

Actualmente Riku se encuentra en fase de **MVP Avanzado**. 

*   [x] Integración nativa con Git via libgit2.
*   [x] Driver completo para Xschem (Diff Visual).
*   [x] Motor `gdstk-rs` funcional (Fases 1-12 completadas).
*   [ ] Integración total del motor Rust en la CLI de Python.
*   [ ] Soporte nativo para OASIS.

---

## 🤝 Contribuciones

Riku está diseñado para la comunidad de Open Source Silicon. Si eres ingeniero de IC o desarrollador de sistemas, ¡eres bienvenido!

1. Clona el repo: `git clone https://github.com/riku-chip/riku_chip.git`
2. Instala dependencias de Rust y Python.
3. Ejecuta `pip install -e .`

---
*Optimizado para PDKs modernos: SKY130, GF180, IHP SG13G2.*
