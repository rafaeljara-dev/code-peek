# AGENTS.md — CodePeek (peek)

Guía canónica de desarrollo para agentes de IA (Antigravity, Claude Code, Codex) y contribuidores en este repositorio.

---

## 1. Misión del Proyecto

**CodePeek** (`peek`) es un visor e inspector de código nativo de escritorio para Windows 11 / Linux / macOS, escrito en **Rust puro**, diseñado específicamente para la era de la programación asistida por agentes de IA.

* **El problema que resuelve:** Los editores modernos (VS Code, Cursor) tardan entre 1.5 y 4 segundos en abrirse, consumen entre 400 MB y 1.5 GB de RAM por ventana, y están sobrecargados de extensiones cuando lo único que necesitas el 80% del tiempo es **inspeccionar código generado por IA, revisar diffs, navegar carpetas y hacer ajustes quirúrgicos de 1 o 2 líneas**. Herramientas alternativas como `px0.ai` cometen el error de levantar un servidor web y abrir pestañas en el navegador, consumiendo cientos de MB de Chromium y generando fricción con los atajos de teclado.
* **La promesa técnica:**
  * **Arranque instantáneo:** En pantalla en **menos de 20 milisegundos**.
  * **Consumo de memoria mínimo:** **Menos de 25-35 MB de RAM**.
  * **Cero motor web:** Cero Electron, cero Chromium, cero pestañas de navegador. Renderizado nativo directo por GPU (Direct3D 11/12 / Vulkan / Metal).
  * **Manejo de archivos inmensos:** Capaz de abrir archivos de 500.000 líneas al instante sin congelar la interfaz gracias a estructuras de datos *Rope*.
  * **Buscador difuso instantáneo (`Ctrl+P`):** Filtra 100.000 archivos en menos de 1 milisegundo.
  * **Edición quirúrgica:** No es un IDE completo, pero permite editar texto, corregir erratas y guardar atómicamente (`Ctrl+S`).

---

## 2. Pila Tecnológica (Stack)

* **Lenguaje:** Rust (Edición 2021+, estándar determinista y sin recolección de basura).
* **Framework de Interfaz de Usuario (UI):**
  * Opción principal evaluada: **Slint** (render nativo por Direct3D/Software/Skia, compilación ahead-of-time a Rust, tipado estricto, temas fluidos y animaciones aceleradas por hardware).
  * Alternativa de respaldo para canvas de texto puro: **cosmic-text** / **vger** / **wgpu** o **egui** para layouts de altísima densidad.
* **Estructura de Texto / Buffer:** `ropey` (árbol B de trozos de texto UTF-8, operaciones en tiempo $O(\log N)$).
* **Exploración de Directorios y File Tree:** `ignore` (el motor multi-hilo de `ripgrep`, respeta `.gitignore` y omite basura como `node_modules/` o `target/` a velocidad de kernel).
* **Buscador Difuso (Fuzzy Finder):** `nucleo` / `nucleo-matcher` (el motor de búsqueda difusa multihilo más rápido del ecosistema Rust).
* **Resaltado de Sintaxis (Syntax Highlighting):** `syntect` (gramáticas TextMate nativas compiladas) o `tree-sitter` incremental.
* **Mapeo de Memoria:** `memmap2` para lectura ultra rápida de archivos grandes sin saturar el heap.
* **Notificaciones del Sistema de Archivos:** `notify` (detecta cambios en tiempo real si el agente o compilador modifica archivos en disco).

---

## 3. Reglas de Trabajo y Convenciones

### Idioma y Comunicación
* **Español** para explicaciones, documentación, issues y diseño de producto.
* **Inglés** para código, nombres de identificadores, structs, enums, funciones, mensajes de commit y ramas git.
* Ortografía española impecable: acentos y `ñ` siempre presentes.

### Rendimiento y Cero Basura
* **Prohibido añadir dependencias web pesadas:** Ni WebView2, ni Chromium, ni Node.js, ni empaquetadores web.
* **Renderizado virtualizado estricto:** El componente de visualización de código NUNCA debe instanciar elementos gráficos para líneas que están fuera del viewport visible. Solo se colorean y dibujan las ~40-60 líneas visibles en la pantalla en cada fotograma.
* **Todo I/O de disco pesado va a hilos secundarios:** El hilo de la UI nunca debe bloquearse esperando leer un directorio o un archivo grande.

### Estilo de Código en Rust
* `clippy` debe pasar sin advertencias (`cargo clippy --workspace --all-targets -- -D warnings`).
* Modularizar por responsabilidades en un espacio de trabajo (*Cargo Workspace*):
  * `crates/peek-core`: Manejo de buffers (`ropey`), estado del workspace, gestión de archivos y configuración.
  * `crates/peek-tree`: Árbol de archivos multihilo con filtrado `.gitignore` y búsqueda difusa.
  * `crates/peek-syntax`: Proveedor de resaltado sintáctico virtualizado por línea.
  * `crates/peek-ui`: Shell de ventana nativa, atajos de teclado y componentes gráficos.
  * `crates/peek-app`: Binario principal (`peek.exe`).

---

## 4. Comandos de Verificación

```powershell
cargo check --workspace          # Verificación rápida de tipos
cargo test --workspace           # Batería de pruebas unitarias
cargo clippy --workspace         # Linter estricto
cargo run --bin peek -- .         # Ejecuta el visor abriendo el directorio actual
cargo build --release --bin peek # Binario final de producción ultra-optimizado
```
