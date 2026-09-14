# Hoja de Ruta de Desarrollo (Roadmap) — CodePeek (`peek`)

Este roadmap divide la construcción de CodePeek en fases incrementales y verificables para avanzar de forma limpia y organizada.

---

### Fase 1: Andamiaje del Workspace de Cargo
* [ ] Crear el `Cargo.toml` raíz del workspace con optimizaciones de release (LTO, codegen-units=1, panic="abort", opt-level=3).
* [ ] Crear las carpetas de los crates modulares:
  * `crates/peek-core`
  * `crates/peek-tree`
  * `crates/peek-syntax`
  * `crates/peek-ui`
  * `crates/peek-app`
* [ ] Configurar `.gitignore` e inicializar el repositorio local `git init`.

### Fase 2: Motor de Buffer y Archivos (`peek-core`)
* [ ] Integrar `ropey` para envolver la estructura de texto.
* [ ] Métodos para cargar archivos desde disco (con fallback a `memmap2` para archivos > 10 MB).
* [ ] Detección automática de codificación UTF-8 / ASCII.
* [ ] Sistema de guardado atómico con archivos temporales y `ReplaceFileW` en Windows.
* [ ] Pruebas unitarias para manipulación de líneas, saltos de línea (`\r\n` vs `\n`) y edición sin pérdidas.

### Fase 3: Explorador de Directorios de Alta Velocidad (`peek-tree`)
* [ ] Implementar el crawler recursivo con el crate `ignore`.
* [ ] Filtrado estricto de `.gitignore`, `.git`, `.next`, `node_modules`, `target`, `dist`.
* [ ] Representación en memoria de la jerarquía de árbol en un modelo plano indexado para la UI.
* [ ] Detección de iconos por extensión de archivo (Rust, TS, JS, Python, Markdown, JSON, etc.).

### Fase 4: Buscador Difuso (*Fuzzy Finder* `Ctrl+P`)
* [ ] Integrar `nucleo` / `nucleo-matcher`.
* [ ] Indexación instantánea en memoria de todas las rutas del proyecto en segundo plano.
* [ ] Interfaz de búsqueda difusa con resaltado de caracteres coincidentes.
* [ ] Atajo de teclado global `Ctrl+P` para alternar la paleta de búsqueda.

### Fase 5: Interfaz Gráfica con Slint (`peek-ui`)
* [ ] Diseño Fluent Dark Mode (#1e1e1e / #252526) en `dashboard/app.slint`.
* [ ] Split-pane resizable: Panel izquierdo para árbol de archivos (250px) y panel derecho para visualización de código.
* [ ] Viewport virtualizado: Renderizar únicamente las ~50 líneas visibles con sus respectivos números de línea.
* [ ] Soporte para scroll con rueda de ratón suave y barra de desplazamiento mínima.

### Fase 6: Resaltado de Sintaxis (`peek-syntax`)
* [ ] Cargar el conjunto de sintaxis estándar de `syntect` (o `tree-sitter`).
* [ ] Tokenizador por rangos de líneas bajo demanda (solo para el viewport visible).
* [ ] Paleta de colores estándar (One Dark Pro / Tokyo Night / Fluent Dark).

### Fase 7: Micro-Edición Quirúrgica
* [ ] Soporte para cursor y selección de texto.
* [ ] Edición de caracteres, borrado (Backspace / Delete) y saltos de línea.
* [ ] Atajos universales: `Ctrl+A`, `Ctrl+C`, `Ctrl+V`, `Ctrl+Z` (Undo), `Ctrl+Y` (Redo), `Ctrl+S` (Guardar).
* [ ] Indicador de archivo modificado (punto circular junto al nombre del archivo).

### Fase 8: Integración Windows 11 y Empaquetado
* [ ] Atributos DWM de modo oscuro (`DwmSetWindowAttribute`).
* [ ] CLI `peek [ruta]` para abrir archivos o carpetas desde la terminal de Windows / PowerShell.
* [ ] Script de instalación en PowerShell (`scripts/install.ps1`) para copiar `peek.exe` a `%LOCALAPPDATA%\Programs\CodePeek` y agregarlo al `PATH`.
* [ ] Opción en el menú contextual de Windows: *"Abrir con CodePeek"*.
