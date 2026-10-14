# Auditoría nativa y editor de código — 14 de octubre de 2026

## Método y alcance

Lintapple 0.1.19 evalúa un DOM en Chromium. CodePeek es una ventana Slint nativa, sin URL ni DOM, por lo que no se ejecutó el CLI ni se generó una puntuación Lintapple. No se añadió una interfaz web para medirla.

La revisión inicial se hizo sobre el código y los tokens de color; quedó registrada en `.audit-ui/antes.json`. No se capturó una imagen del estado previo. La revisión posterior utiliza el renderizador software nativo de Slint sin ventanas, con capturas y comprobaciones de posición de los controles. El renderizador de pruebas es una dependencia de desarrollo; la aplicación conserva su renderizador GPU.

La matriz cubre 960 y 1320 píxeles, fuentes de interfaz de 15 y 22 píxeles, temas claro/oscuro y estados de Editor y Buscar/reemplazar: 16 capturas. No equivale a medir las escalas reales 125/150/200 % de Windows.

## Hallazgos y correcciones

| Hallazgo inicial | Resultado de la implementación |
| --- | --- |
| Editor sin colores y copia completa del texto en TextEdit | Superficie de código virtualizada sobre Ropey; resaltado de los tokens visibles durante la edición |
| Búsqueda por línea sin reemplazo | Rangos exactos, varias coincidencias por línea, contador, navegación circular, mayúsculas/palabra completa y reemplazo individual/global |
| Historial sin rehacer | Hasta 100 operaciones con deshacer/rehacer y restauración de cursor/selección; editar después de deshacer invalida rehacer |
| Comentarios oscuros con contraste insuficiente | Ajuste de luminancia con contraste mínimo 4,5:1 frente a fondo, línea activa y selección; el tema claro mantiene 7:1 |
| Filas de proyectos sin activación por teclado | Foco visible y activación con Enter/Espacio, alturas estables e indicador de proyecto activo |
| Controles demasiado anchos en ventanas pequeñas | Acciones del editor con iconos incrustados y ancho mínimo coherente; comprobación de límites en la matriz |
| Pestañas con texto y cierres inconsistentes | Tamaños y posiciones explícitos, indicador activo, acciones accesibles, ayudas con la ruta completa y desplazamiento al inicio al cambiar de archivo |

Durante la revisión por capturas se corrigieron también la posición de los números de línea y los iconos que no existían en la fuente del sistema. Una prueba del portapapeles detectó que el TextInput auxiliar convertía saltos de línea en espacios; ahora es multilínea y conserva el texto pegado.

## Funciones del editor

- Selección con arrastre, doble clic, Shift+flechas, Ctrl+Shift+flechas y Ctrl+A; copiar/cortar/pegar y menú contextual.
- Cursor por grafemas, navegación por palabras, Inicio/Fin, Ctrl+Inicio/Fin y páginas.
- Ctrl+G para ir a línea; búsqueda y reemplazo con Ctrl+F / Ctrl+H y F3 / Shift+F3.
- Tab / Shift+Tab, sangría de cuatro espacios, Enter con sangría y pares de `()`, `[]` y `{}`.
- Ctrl+/ para comentar/descomentar en lenguajes compatibles y Ctrl+D para duplicar la línea.
- Líneas físicas con desplazamiento horizontal al mover el cursor o usar Shift+rueda; Lectura/Vista previa conserva ajuste visual.
- Guardado explícito y atómico, indicador de cambios, protección frente a cambios externos y confirmación al navegar/cerrar con cambios.

La gramática de Markdown del editor conserva los marcadores del archivo; la presentación con títulos y estilos se limita a Vista previa. Rust usa Syntect en la edición, TypeScript/TSX conserva su proveedor específico y los demás lenguajes usan las gramáticas incluidas.

## Verificación reproducible

```powershell
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --release --bin peek

# Repetir capturas nativas sin abrir una ventana ni navegador:
$env:CODEPEEK_AUDIT_DIR = Join-Path (Get-Location) '.audit-ui'
cargo test -p peek-ui native_ui_matrix -- --nocapture
```

La batería completa pasa con **52 pruebas**, y la matriz produce **16 capturas nativas**. `cargo fmt`, `cargo check`, Clippy estricto y la compilación release finalizaron correctamente. El ejecutable local ocupa 17.629.696 bytes (16.8 MiB).

Las pruebas cubren texto Unicode con acentos, grafemas y emoji, BOM, LF/CRLF mixtos, copiar/pegar multilínea, navegación y recuperación del foco, reemplazos y su historial, guardado, eventos de un archivo anterior y edición al final de un documento de 50.000 líneas. Las capturas y los informes crudos son locales y están excluidos de Git.

## Límites pendientes

- No incluye LSP, autocompletado semántico, diagnóstico de compilación, terminal, depurador ni múltiples cursores.
- Buscar/reemplazar es literal y por línea, sin expresiones regulares ni coincidencias que atraviesen saltos de línea.
- El contexto de sintaxis es acotado: los bloques que comienzan más de 64 líneas antes del viewport pueden mostrar colores aproximados. Faltan checkpoints incrementales.
- Vista previa de Markdown sigue copiando la línea seleccionada; no ofrece selección de rangos.
- La selección visible y el buffer pertenecen al editor virtualizado. El pequeño TextInput recibe texto e IME, pero no expone el documento completo a lectores de pantalla. La accesibilidad completa del contenido requiere un adaptador nativo adicional.
- IME real, lectores de pantalla, composición de emoji con fuentes alternativas, DPI del sistema, Linux/macOS y las metas de RAM/latencia requieren validación específica. Las capturas sin ventana no certifican esos aspectos.
