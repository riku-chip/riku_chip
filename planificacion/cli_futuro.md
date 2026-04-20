# Futuro del CLI de Riku Rust

## Estado actual

El CLI actual de `riku_rust` ya cubre lo esencial:

- `diff` entre dos commits de un archivo `.sch`
- `log` del historial por archivo
- `doctor` para verificar herramientas externas
- salidas `text`, `json` y `visual`
- ejecución como `cargo run -- ...` o como binario instalado

La prioridad actual es mantener este flujo estable y predecible.

## Visión a futuro

El CLI debe crecer sin romper la forma de uso actual.

La idea es conservar los subcomandos clásicos para automatización y scripting, y agregar una experiencia más cómoda para exploración interactiva cuando haga falta.

Objetivos:

- no romper scripts existentes
- no mezclar lógica de negocio con parsing del CLI
- reutilizar los mismos comandos base en todas las interfaces
- mantener el proyecto fácil de extender

## Modo interactivo

Una posible evolución es un modo interactivo tipo consola:

```bash
riku
```

Dentro de esa sesión se podrían usar comandos como:

- `/diff`
- `/log`
- `/doctor`
- `/help`
- `/quit`

Ese modo serviría para:

- explorar commits sin repetir el comando completo
- lanzar varias consultas seguidas
- tener una experiencia más guiada para el usuario

Importante:

- el modo interactivo sería opcional
- el CLI tradicional seguiría existiendo
- el núcleo de `diff`, `log` y `doctor` no debería duplicarse

## Compatibilidad

La compatibilidad es prioritaria.

La evolución del CLI debe respetar estas reglas:

- `riku diff ...` sigue funcionando
- `riku log ...` sigue funcionando
- `riku doctor ...` sigue funcionando
- `cargo run -- ...` sigue siendo válido en desarrollo
- el binario instalado debe seguir siendo usable desde terminal

## Ruta de evolución sugerida

### Fase 1: pulir la experiencia actual
- mejorar mensajes de ayuda
- añadir ejemplos más claros al `README`
- revisar mensajes de error
- documentar mejor el uso con binario instalado

### Fase 2: modo interactivo mínimo
- abrir un prompt cuando se ejecute `riku` sin subcomandos
- aceptar `/diff`, `/log`, `/doctor`, `/help`, `/quit`
- reutilizar la lógica existente del CLI

### Fase 3: ergonomía
- historial de comandos
- autocompletado básico
- mensajes contextuales más claros
- comandos compuestos simples

### Fase 4: refinamiento
- mejor navegación entre resultados
- posibles accesos rápidos para render visual
- mejorar la experiencia sin alterar la semántica del núcleo

## Criterios de éxito

El CLI futuro será bueno si cumple esto:

- los comandos actuales siguen igual
- el usuario puede elegir entre modo clásico e interactivo
- la lógica de dominio permanece fuera del parser del CLI
- la experiencia es cómoda tanto para scripting como para exploración manual

## Nota final

La dirección recomendada es simple:

- mantener el CLI actual como base estable
- agregar interfaz interactiva solo como capa opcional
- evitar reescrituras que dupliquen la lógica existente
