# CodePeek (`peek`)

Inspector de código nativo en Rust para abrir proyectos, revisar archivos generados por agentes, consultar configuraciones y editar código con resaltado, selección multilínea y búsqueda con reemplazo. Usa Slint y Ropey; no necesita navegador, servidor web, extensiones ni servidores de lenguaje.

> CodePeek no reemplaza Visual Studio Code. Es una alternativa complementaria para el momento en que solo necesitas revisar código, cambiar de proyecto rápidamente y salir sin cargar un IDE completo.

## Descargar para Windows

[**Descargar el instalador más reciente — CodePeek-Setup.exe**](https://github.com/rafaeljara-dev/code-peek/releases/latest/download/CodePeek-Setup.exe)

Abre el archivo descargado. Instala CodePeek para tu usuario, crea accesos directos en Inicio y escritorio, registra «Abrir con CodePeek» y abre la aplicación. Windows puede mostrar SmartScreen mientras el proyecto aún no tenga firma de código; revisa que el archivo provenga de este repositorio público.

## Flujo de uso

La primera vez, al abrir sin argumentos, CodePeek te invita a elegir una carpeta en lugar de indexar el directorio de trabajo del acceso directo.

1. **Abre un proyecto.** Pulsa «Abrir proyecto…» y escribe su ruta o usa «Examinar…». Queda guardado en «Tus proyectos», aunque esté en otra carpeta.
2. **Cambia con un clic.** La lista conserva tus proyectos entre sesiones, con nombres alineados a la izquierda y el favicon de cada aplicación cuando existe. El menú `⋯` permite eliminar una entrada de la lista, nunca su carpeta.
3. **Establece el inicio.** En «Configuración», elige «Usar el proyecto actual». Puedes quitarlo después; entonces se recuerda el último proyecto. Una ruta explícita por CLI siempre tiene prioridad.
4. **Encuentra lo que necesitas.** La lupa abre una búsqueda flotante con dos modos: «Archivos» filtra por nombre o ruta y «Texto» busca contenido en todo el proyecto. `Alt+C` cambia de proyecto sin abrir otra ventana.
5. **Inspecciona y corrige.** El editor ofrece resaltado de sintaxis, números de línea, selección con ratón/teclado, copiar/pegar, sangría automática, pares de paréntesis y deshacer/rehacer. Cuando hay cambios aparecen «Guardar cambios» y «Revertir». Al navegar o cerrar puedes guardar, descartar o cancelar. Markdown dispone de «Vista previa / Editor»; los demás archivos ofrecen «Lectura / Editor».

«Tus proyectos» es una lista persistente, no una ruta ni un árbol de carpetas. El explorador separado muestra las subcarpetas del proyecto activo.

## Atajos

| Acción | Atajo |
| --- | --- |
| Abrir carpeta | `Ctrl+O` |
| Cambiar proyecto | `Alt+C` / `Ctrl+Shift+P` |
| Buscar archivo | `Ctrl+P` |
| Buscar texto en el proyecto | `Ctrl+Shift+F` |
| Buscar texto literal en el archivo | `Ctrl+F`, Enter / `F3` para siguiente, `Shift+F3` para anterior |
| Buscar y reemplazar | `Ctrl+H` |
| Ir a una línea | `Ctrl+G` |
| Editar el archivo | Escribir en «Editor» |
| Copiar la selección en el editor | `Ctrl+C` |
| Guardar | `Ctrl+S` |
| Deshacer / rehacer | `Ctrl+Z` / `Ctrl+Shift+Z` o `Ctrl+Y` |
| Sangrar / quitar sangría | `Tab` / `Shift+Tab` |
| Comentar / descomentar líneas | `Ctrl+/` (lenguajes compatibles) |
| Duplicar línea | `Ctrl+D` |
| Seleccionar todo / seleccionar palabra | `Ctrl+A` / doble clic |
| Desplazar horizontalmente el editor | `Shift+rueda` o rueda horizontal |
| Recargar archivo e índice | `F5` |
| Mostrar/ocultar explorador | `Ctrl+B` |
| Zoom del código | `Ctrl++` / `Ctrl+-` |
| Zoom de la interfaz | `Ctrl+Shift++` / `Ctrl+Shift+-` |
| Cerrar diálogo | `Esc` |

## Comportamiento y límites

- **Configuración centralizada:** tema claro/oscuro, zoom de interfaz, zoom de código, paneles visibles y proyecto inicial viven en «Configuración». El diálogo permite desplazamiento con zoom alto. En modo claro, la sintaxis conserva contraste calculado de al menos 7:1 y los iconos tienen una paleta específica.
- **Identidad de proyectos:** la lista usa `favicon.ico` o iconos PNG comunes del proyecto cuando existen; de lo contrario conserva el símbolo de carpeta. El menú `⋯` contiene «Eliminar de la lista» y admite futuras acciones sin ensanchar cada fila.
- Lecturas, indexación, búsqueda de contenido, resaltado y guardados se ejecutan fuera del hilo de la interfaz. Las búsquedas nuevas cancelan los resultados de generaciones anteriores; cada archivo aporta hasta tres coincidencias y al abrir una se navega directamente a su línea. El visor crea únicamente las líneas del área visible.
- El índice respeta `.gitignore` y conserva archivos ocultos de configuración útiles. Omite `.git`, `node_modules`, `target` y directorios de compilación/caché conocidos.
- El archivo activo se comprueba aproximadamente cada segundo de inactividad del trabajador. Los cambios externos se recargan automáticamente si no hay edición ni cambios locales pendientes. `F5` actualiza también el árbol y el índice.
- El guardado usa un temporal único, conserva permisos y comprueba el contenido original antes de reemplazar el archivo. Si otro programa lo cambió, rechaza el guardado. Esta comprobación no constituye un bloqueo exclusivo frente a escritores externos.
- Los saltos de línea, incluidos archivos mixtos LF/CRLF, y el BOM UTF-8 se conservan. Archivos binarios y otras codificaciones muestran un error sin vaciar el archivo que estaba abierto.
- Las pestañas, las carpetas desplegadas y la posición de lectura se restauran durante la sesión. En disco se guardan la lista de proyectos, el inicio, el último proyecto, el historial y los dos tamaños de fuente, no el contenido de los archivos.
- Las líneas se ajustan al ancho disponible en Lectura/Vista previa. El editor conserva las líneas físicas y desplaza horizontalmente el contenido al mover el cursor o usar Shift+rueda. Los controles «Interfaz» y «Código» están en «Configuración» y ajustan sus tamaños por separado.
- Las pestañas tienen × y cierre con botón central; los proyectos usan un menú `⋯`. Los diálogos tienen cierre visible además de Esc. Los iconos de extensiones usan fuentes incrustadas recortadas de 12,0 KiB, sin SVG.
- El editor dibuja únicamente las líneas y los tokens del área visible sobre el buffer Ropey. Ofrece 100 pasos de deshacer con rehacer, restauración de cursor/selección, edición Unicode, números de línea y resaltado también mientras escribes. Un control nativo pequeño recibe texto e IME; no almacena el documento completo.
- Buscar/reemplazar usa texto literal, permite distinguir mayúsculas y palabras completas, cuenta coincidencias y recorre varias coincidencias en la misma línea. Reemplazar todos constituye una sola operación de deshacer. La búsqueda se limita a una línea por coincidencia; no incluye expresiones regulares.
- El resaltado usa contexto acotado de hasta 64 líneas anteriores en gramáticas Syntect. Los bloques que empiezan antes pueden mostrar colores aproximados; todavía faltan checkpoints incrementales. Markdown conserva todos los marcadores en Editor y los presenta con formato en Vista previa.
- La sangría usa cuatro espacios y conserva los saltos de línea y BOM originales. Los comentarios de línea dependen del lenguaje; formatos como JSON y Markdown no ofrecen esta acción. La Vista previa sigue copiando una línea completa, sin selección de rangos.
- No incluye autocompletado semántico, LSP, terminal ni depurador. La entrada IME está conectada al control nativo, pero requiere validación manual con los métodos de entrada de cada sistema operativo.
- La auditoría nativa se documenta en [docs/EDITOR-UI-AUDIT.md](docs/EDITOR-UI-AUDIT.md). Lintapple necesita un DOM web y no mide esta aplicación Slint; la revisión utiliza capturas sin ventana del renderizador nativo y pruebas de geometría, teclado y contraste.
- Los objetivos de arranque inferior a 20 ms y memoria de 25–35 MB son metas, **no benchmarks verificados**.

## Comparación con VS Code

La comparación se mantiene en [docs/BENCHMARKS.md](docs/BENCHMARKS.md), con metodología reproducible y espacios para peso, apertura y RAM. El binario release local compilado el 14 de octubre de 2026 ocupa **17.629.696 bytes (16,8 MiB)**. Los datos de VS Code se publicarán únicamente después de medir ambas aplicaciones en la misma computadora y con el mismo proyecto.

## Auditoría de recursos

La [auditoría de RAM, CPU y arranque](docs/PERFORMANCE-AUDIT.md) registra mediciones del release y una lista priorizada para laptops con menos recursos. La configuración probada mantiene aproximadamente 95 MiB residentes y 102 MiB privados; las metas originales de memoria y primer fotograma siguen pendientes de validar. El informe detalla qué se midió y qué mejoras todavía son propuestas.

## Instalación desde el código fuente

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

## Clonar y contribuir

```powershell
git clone https://github.com/rafaeljara-dev/code-peek.git
cd code-peek
cargo check --workspace
cargo test --workspace
```

Las contribuciones son bienvenidas. Lee [CONTRIBUTING.md](CONTRIBUTING.md) para preparar Rust y las herramientas de Windows, ejecutar las verificaciones y abrir un pull request. El proyecto se distribuye bajo la [licencia MIT](LICENSE); Devicon y Codicons conservan sus licencias dentro de `crates/peek-ui/assets/`.
