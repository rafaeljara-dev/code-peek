# Guía de Extensiones de Lenguaje y Frameworks — CodePeek (`peek`)

Esta guía documenta el funcionamiento del motor de sintaxis y visualización de **CodePeek**, así como el procedimiento paso a paso para añadir soporte a nuevos lenguajes, frameworks o formatos de archivo.

---

## 1. Filosofía de Rendimiento

A diferencia de los editores convencionales que ejecutan servidores pesados o motores de expresiones regulares que recorren secuencialmente cientos de miles de líneas antes de dibujar un fotograma, **CodePeek utiliza un sistema de extensiones nativas ultra-rápidas en Rust**:

* **Tokenización en tiempo $O(1)$ por línea:** La GPU solo renderiza el bloque de líneas visibles en el viewport (~35–60 líneas). Cada línea se clasifica y tokeniza en microsegundos de forma aislada, sin congelar la interfaz incluso si navegas por la línea 350.000 de un archivo gigantesco.
* **Salto de línea inteligente (Word Wrap):** Los textos y párrafos largos (especialmente en Markdown y comentarios de código) se ajustan automáticamente al ancho visible del editor (`wrap: word-wrap`), eliminando los molestos puntos suspensivos (`...`) y evitando barras de desplazamiento horizontal.
* **Fallback a Syntect:** Si se abre un archivo para el que aún no existe una extensión nativa de alta velocidad, el registro delega automáticamente en las gramáticas TextMate de `syntect`.

---

## 2. Extensiones Oficiales Incluidas

### 📝 Markdown (`.md`, `.markdown`, `.mdown`, `.mkdn`)
La extensión de Markdown transforma la lectura de documentación y notas en una experiencia moderna y fluida:
* **Encabezados dinámicos:**
  * `# H1`: Tipografía Segoe UI, mayor tamaño (`font_size + 4px`), negrita y acento azul cielo (`#38bdf8`).
  * `## H2`: Mayor tamaño (`font_size + 2px`), negrita y acento índigo (`#818cf8`).
  * `### H3`: Negrita y acento violeta (`#a78bfa`).
* **Bloques de cita (`>`):** Incorpora una barra vertical lateral izquierda en pizarra suave (`#64748b`) y texto en cursiva (`#cbd5e1`).
* **Listas y tareas:**
  * Viñetas (`- `, `* `): Punto de lista destacado en color ámbar (`#f59e0b`).
  * Casillas pendientes (`- [ ]`): Indicador atenuado.
  * Casillas completadas (`- [x]`): Resaltado en verde esmeralda con marca `[✓]`.
* **Bloques de código (```` ``` ````):** Fondo oscurecido (`#16161a`) y tipografía monoespaciada *Cascadia Code*.
* **Líneas divisorias (`---`, `***`):** Renderizadas como una barra divisoria continua elegante.
* **Salto de línea:** Párrafos de cualquier longitud saltan de línea automáticamente adaptándose al tamaño de la ventana o al nivel de zoom.

### 🦀 Rust (`.rs`)
Diseñado para la inspección precisa y rápida de código en el ecosistema Rust:
* **Palabras clave:** `fn`, `pub`, `struct`, `enum`, `impl`, `trait`, `let`, `mut`, `match`, `if`, `else`, `async`, `await`, etc., destacadas en púrpura (`#c084fc`).
* **Tipos estándar y primitivos:** `i32`, `usize`, `String`, `Option`, `Result`, `Vec`, `PathBuf`, etc., en azul cielo (`#38bdf8`).
* **Macros y atributos:** `println!`, `vec!`, `#[derive(...)]`, `#[test]` en amarillo dorado (`#eab308`) y rosa (`#f43f5e`).
* **Cadenas y caracteres:** En verde lima (`#a3e635`).
* **Comentarios:** En gris zinc (`#71717a`) en cursiva.
* **Tiempos de vida:** `'a`, `'static` en verde azulado (`#2dd4bf`).

### ⚛️ Next.js & React (`.tsx`, `.jsx`, `.ts`, `.js`, `.mjs`, `.cjs`, `.json`, `.css`)
Soporte de primer nivel para proyectos modernos de Next.js y React:
* **Archivos especiales reconocidos:** `next.config.js`, `next.config.mjs`, `next.config.ts`, `package.json`, `tsconfig.json`, `tailwind.config.js`.
* **Etiquetas y componentes JSX:** `<div`, `</Button>`, `<Component>` en azul cielo (`#38bdf8`).
* **Atributos y props:** `className=`, `onClick=`, `id=`, `key=` en naranja (`#fb923c`).
* **Hooks de React:** `useState`, `useEffect`, `useMemo`, `useCallback`, `useRef`, etc., en rosa vibrante (`#f43f5e`).
* **APIs de Next.js:** `useRouter`, `useSearchParams`, `cookies`, `headers`, `redirect`, `notFound`, `metadata`, `revalidateTag`, `Link`, `Image` en rosa/esmeralda.
* **Palabras clave de TypeScript:** `interface`, `type`, `import`, `export default`, `async`, `await` en púrpura (`#c084fc`).

---

## 3. Arquitectura del Motor de Extensiones

El sistema reside en el crate [`crates/peek-syntax`](file:///C:/Users/rafael/dev/PersonalProjects/code-peek/crates/peek-syntax):

```
crates/peek-syntax/src/
├── languages/
│   ├── traits.rs     # Trait LanguageExtension y estructuras (HighlightSpan, HighlightedLine)
│   ├── markdown.rs   # Implementación para Markdown
│   ├── rust.rs       # Implementación para Rust
│   ├── nextjs.rs     # Implementación para Next.js / TypeScript / React JSX
│   ├── registry.rs   # ExtensionRegistry con búsqueda rápida y fallback a syntect
│   └── mod.rs
├── highlighter.rs    # Motor Syntect para lenguajes adicionales
├── themes.rs         # Paleta de colores Fluent Dark (#121214)
└── lib.rs
```

### El Trait `LanguageExtension`

Cada extensión implementa el trait definido en [`traits.rs`](file:///C:/Users/rafael/dev/PersonalProjects/code-peek/crates/peek-syntax/src/languages/traits.rs):

```rust
pub trait LanguageExtension: Send + Sync {
    /// Identificador único de la extensión (ej. "python", "go", "svelte")
    fn id(&self) -> &'static str;

    /// Nombre amigable para mostrar en la interfaz
    fn name(&self) -> &'static str;

    /// Extensiones de archivo que maneja (sin el punto)
    fn file_extensions(&self) -> &'static [&'static str];

    /// Nombres de archivo exactos especiales (ej. "Dockerfile", "go.mod")
    fn special_filenames(&self) -> &'static [&'static str] {
        &[]
    }

    /// Indica si el formato es Markdown (para aplicar layout y fuentes tipográficas)
    fn is_markdown(&self) -> bool {
        false
    }

    /// Tokeniza y da formato a una línea individual de texto en tiempo O(1)
    fn tokenize_line(
        &self,
        line: &str,
        line_number: usize,
        ctx: &mut LineContext,
    ) -> HighlightedLine;
}
```

---

## 4. Guía Paso a Paso: Cómo Añadir una Nueva Extensión

A continuación se muestra cómo añadir un nuevo lenguaje (por ejemplo, **Python** o **Go**):

### Paso 1: Crear el archivo de la extensión
Crea un nuevo archivo en `crates/peek-syntax/src/languages/python.rs`:

```rust
use super::traits::{HighlightSpan, HighlightedLine, LanguageExtension, LineContext, LineType};
use crate::themes::ColorRgb;

pub struct PythonExtension;

const PYTHON_KEYWORDS: &[&str] = &[
    "def", "class", "import", "from", "as", "return", "if", "elif", "else",
    "for", "while", "break", "continue", "try", "except", "finally", "with",
    "async", "await", "lambda", "yield", "pass", "raise", "in", "is", "not",
    "and", "or", "None", "True", "False",
];

impl LanguageExtension for PythonExtension {
    fn id(&self) -> &'static str {
        "python"
    }

    fn name(&self) -> &'static str {
        "Python"
    }

    fn file_extensions(&self) -> &'static [&'static str] {
        &["py", "pyw", "pyi"]
    }

    fn special_filenames(&self) -> &'static [&'static str] {
        &["requirements.txt", "Pipfile", "pyproject.toml"]
    }

    fn tokenize_line(
        &self,
        line: &str,
        line_number: usize,
        _ctx: &mut LineContext,
    ) -> HighlightedLine {
        // Implementar tokenización ligera por línea:
        // - Comentarios (# ...)
        // - Cadenas ("..." o '...')
        // - Palabras clave (def, class, import)
        // - Decoradores (@decorator)
        // Retornar HighlightedLine con los spans correspondientes
        ...
    }
}
```

### Paso 2: Exponer la extensión en `languages/mod.rs`
En [`crates/peek-syntax/src/languages/mod.rs`](file:///C:/Users/rafael/dev/PersonalProjects/code-peek/crates/peek-syntax/src/languages/mod.rs):

```rust
pub mod python;
pub use python::PythonExtension;
```

### Paso 3: Registrar la extensión en `registry.rs`
En [`crates/peek-syntax/src/languages/registry.rs`](file:///C:/Users/rafael/dev/PersonalProjects/code-peek/crates/peek-syntax/src/languages/registry.rs):

```rust
impl ExtensionRegistry {
    pub fn new() -> Self {
        let mut registry = Self { ... };

        registry.register(Arc::new(MarkdownExtension));
        registry.register(Arc::new(RustExtension));
        registry.register(Arc::new(NextJsExtension));
        registry.register(Arc::new(PythonExtension)); // <-- Tu nueva extensión

        registry
    }
}
```

### Paso 4: (Opcional) Asignar icono oficial Codicon en el árbol
En [`crates/peek-tree/src/icons.rs`](file:///C:/Users/rafael/dev/PersonalProjects/code-peek/crates/peek-tree/src/icons.rs), asocia la extensión al glifo correspondiente de la fuente Codicons:

```rust
"py" | "pyw" => codicons::FILE_CODE, // o icono de python
```

### Paso 5: Validar y compilar
Ejecuta la suite de verificación para confirmar que pasa todas las pruebas y estándares estrictos:

```powershell
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --release --bin peek
.\target\release\peek.exe --install
```

---

## 5. Resumen de Colores Estándar del Tema (Fluent Dark)

| Elemento Semántico | Color HEX | Uso Típico |
|---|---|---|
| **Palabras Clave / Declaraciones** | `#c084fc` (Púrpura) | `fn`, `import`, `export`, `struct`, `def`, `class` |
| **Tipos / Interfaces / Componentes** | `#38bdf8` (Azul cielo) | `String`, `Option`, `useState`, `<Component>` |
| **Cadenas de Texto** | `#a3e635` (Lima) | `"texto"`, `'valor'`, `` `template` `` |
| **Comentarios / Docstrings** | `#71717a` (Zinc) | `// comentario`, `/* bloque */`, `# nota` |
| **Números y Constantes** | `#fb923c` (Naranja) | `42`, `3.14`, `0xFF` |
| **Atributos / Decoradores / Props** | `#fb923c` / `#f43f5e` | `className=`, `#[derive(...)]`, `@decorator` |
| **Encabezados H1 (Markdown)** | `#38bdf8` (Azul cielo) | Título principal de sección |
| **Encabezados H2 (Markdown)** | `#818cf8` (Índigo) | Subtítulo |
| **Encabezados H3 (Markdown)** | `#a78bfa` (Violeta) | Secciones terciarias |
| **Citas en Bloque (Markdown)** | `#64748b` / `#cbd5e1` | Barra lateral y texto en cursiva |
| **Bloques de Código (Markdown)** | `#16161a` / `#e2e8f0` | Fondo oscurecido con código monoespaciado |
| **Texto Base** | `#d4d4d8` | Código y prosa no resaltada |
