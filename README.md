# CodePeek (`peek`)

Inspector de código nativo en Rust para abrir proyectos, revisar archivos generados por agentes, consultar configuraciones y hacer correcciones puntuales. Usa Slint y Ropey; no necesita navegador, servidor web, extensiones ni servidores de lenguaje.

## Flujo de uso

La primera vez, al abrir sin argumentos, CodePeek te invita a elegir una carpeta en lugar de indexar el directorio de trabajo del acceso directo.

1. **Abre un proyecto.** Pulsa «Abrir proyecto…» y escribe su ruta o usa «Examinar…». Queda guardado en «Tus proyectos», aunque esté en otra carpeta.
2. **Cambia con un clic.** La lista conserva tus proyectos entre sesiones, con nombres alineados a la izquierda. La × elimina una entrada de la lista, nunca su carpeta.
3. **Establece el inicio.** «Usar como inicio» fija el proyecto que se abrirá al iniciar CodePeek sin argumentos. Vuelve a pulsarlo para desactivarlo; entonces se recuerda el último proyecto. Una ruta explícita por CLI siempre tiene prioridad.
4. **Abre lo que necesitas.** Usa el árbol, `Ctrl+P` o «Entornos» para encontrar `.env`, `.env.local`, `.env.production`, etc. `Alt+C` cambia de proyecto sin abrir otra ventana.
5. **Inspecciona y corrige.** Haz doble clic sobre una línea y edítala directamente en el archivo; Enter aplica y Escape cancela. Cuando hay cambios aparecen «Guardar cambios» y «Revertir». Al navegar o cerrar también puedes guardar, descartar o cancelar.

«Tus proyectos» es una lista persistente, no una ruta ni un árbol de carpetas. El explorador separado muestra las subcarpetas del proyecto activo.

## Atajos

| Acción | Atajo |
| --- | --- |
| Abrir carpeta | `Ctrl+O` |
| Cambiar proyecto | `Alt+C` / `Ctrl+Shift+P` |
| Buscar archivo | `Ctrl+P` |
| Buscar texto literal en el archivo | `Ctrl+F`, Enter para siguiente |
| Editar una línea en el archivo | Doble clic; Enter aplica y Escape cancela |
| Copiar línea seleccionada | `Ctrl+C` |
| Guardar | `Ctrl+S` |
| Deshacer cambio de línea | `Ctrl+Z` |
| Recargar archivo e índice | `F5` |
| Mostrar/ocultar explorador | `Ctrl+B` |
| Tamaño del texto | `Ctrl++` / `Ctrl+-` |
| Desplazamiento horizontal | `Shift+rueda` o barra inferior |
| Cerrar diálogo | `Esc` |

## Comportamiento y límites

- **Configuración centralizada:** tema claro/oscuro, zoom de interfaz, zoom de código, paneles visibles y proyecto inicial viven en «Configuración». Las preferencias se guardan. En modo claro, la sintaxis y los iconos se adaptan con contraste calculado de al menos 7:1 sobre el lienzo y las filas seleccionadas.
- **Identidad de proyectos:** la lista usa `favicon.ico` o iconos PNG comunes del proyecto cuando existen; de lo contrario conserva el símbolo de carpeta. El menú `⋯` contiene «Eliminar de la lista» y admite futuras acciones sin ensanchar cada fila.
- Lecturas, indexación, búsquedas, resaltado y guardados se ejecutan fuera del hilo de la interfaz. El índice usa un trabajador independiente y se cancela al cambiar de proyecto. El visor crea únicamente las líneas del área visible.
- El índice respeta `.gitignore`, con una excepción deliberada para `.env*` dentro de carpetas visitadas. El árbol permite inspeccionar también archivos ignorados. Ambos omiten `.git`, `node_modules`, `target` y directorios de compilación/caché conocidos.
- El archivo activo se comprueba aproximadamente cada segundo de inactividad del trabajador. Los cambios externos se recargan automáticamente si no hay edición ni cambios locales pendientes. `F5` actualiza también el árbol y el índice.
- El guardado usa un temporal único, conserva permisos y comprueba el contenido original antes de reemplazar el archivo. Si otro programa lo cambió, rechaza el guardado. Esta comprobación no constituye un bloqueo exclusivo frente a escritores externos.
- Los saltos de línea, incluidos archivos mixtos LF/CRLF, y el BOM UTF-8 se conservan. Archivos binarios y otras codificaciones muestran un error sin vaciar el archivo que estaba abierto.
- Las pestañas, las carpetas desplegadas y la posición de lectura se restauran durante la sesión. En disco se guardan la lista de proyectos, el inicio, el último proyecto, el historial y los dos tamaños de fuente, **no contenidos ni valores de entornos**.
- Las líneas se ajustan automáticamente al ancho disponible sin cambiar el archivo. Los controles «Interfaz» y «Código» de la barra inferior ajustan sus tamaños por separado.
- Las pestañas y los proyectos tienen × y cierre con botón central. Los diálogos tienen cierre visible además de Esc. Los iconos de extensiones usan fuentes incrustadas recortadas de 11,7 KB, sin SVG.
- La edición es de una línea cada vez, hasta 32 KB por línea, con 50 pasos de deshacer. No incluye edición multilínea, redo, reemplazo global, terminal, LSP ni depurador.
- El resaltado es local al viewport: comentarios o bloques multilínea que empiezan fuera de pantalla pueden tener colores aproximados. El texto fuente se muestra sin transformar Markdown.
- Los objetivos de arranque inferior a 20 ms y memoria de 25–35 MB son metas, **no benchmarks verificados**.

## Instalación de escritorio

En Windows, `scripts/install.ps1` compila e instala por usuario, crea accesos directos en Inicio y escritorio y registra la aplicación. Después basta buscar **CodePeek** en Inicio. No requiere una terminal para usarla.

## Ejecutar desde el código fuente

```powershell
cargo build --release --bin peek
.\target\release\peek.exe
.\target\release\peek.exe C:\ruta\al\proyecto
.\target\release\peek.exe C:\ruta\al\proyecto\.env.local
```

En Linux/macOS, el ejecutable es `target/release/peek`. El selector de carpetas usa los diálogos del sistema mediante [RFD](https://docs.rs/rfd/0.17.2/rfd/); en Linux requiere un portal de escritorio compatible o Zenity. Esta iteración se verifica en Windows.

La configuración vive en el directorio de datos locales de usuario, bajo `CodePeek/settings.json` (`%LOCALAPPDATA%\CodePeek\settings.json` en Windows). Se migran los tamaños y recientes del formato `.txt` previo. Si el JSON está corrupto, se muestra el error y se conserva el archivo sin sobrescribirlo.

## Verificar

```powershell
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Las pruebas de UI usan el backend de pruebas de Slint sin abrir ventanas. Consulta [la auditoría](docs/AUDIT.md) y [los próximos pasos](ROADMAP.md).
