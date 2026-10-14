# Auditoría de interfaz y edición

Este informe conserva la revisión anterior. La auditoría y los cambios del 14 de octubre de 2026 están en [EDITOR-UI-AUDIT.md](EDITOR-UI-AUDIT.md).

## Causas verificadas

- El componente de botones no limitaba su ancho máximo: las acciones absorbían el espacio sobrante de sus filas. Ahora usan ancho de contenido; los botones de icono permanecen compactos.
- Los glifos `sync` y `ellipsis` no estaban incluidos en la fuente recortada. Se incorporaron desde la fuente existente, sin SVG ni descarga en ejecución.
- El menú de proyectos se mostraba al pie de la lista. Ahora es un popup anclado al botón de cada proyecto.
- Configuración tenía una altura fija insuficiente al aumentar la tipografía. Ahora su altura depende del espacio disponible y el contenido permite desplazamiento.
- Las pestañas mostraban rutas largas como botones separados. Ahora tienen ancho acotado, nombre corto, indicador de cambios y cierre integrado; la ruta permanece en la cabecera.
- El documento estaba compuesto por etiquetas sin selección. El modo Editor utiliza un control de texto nativo con selección, copiar, pegar y edición multilínea; los cambios llegan al buffer y conservan las protecciones de guardado existentes.
- Los iconos de archivo en modo claro se oscurecían genéricamente. Ahora usan una paleta específica; los favicons se presentan sobre una superficie blanca estable.

## Modos del documento

Markdown ofrece Vista previa y Editor. Los demás archivos ofrecen Lectura y Editor. La lectura mantiene el resaltado y la virtualización de filas; el editor mantiene una copia del texto en su control nativo y presenta texto sin resaltado. Hace falta medir su comportamiento con documentos muy grandes antes de atribuirle las metas de memoria y latencia del visor.

## Verificación

Las pruebas cubren escritura nativa sin activar el diálogo de línea, ancho de las acciones de icono y aplicación de cambios multilínea preservando Unicode, BOM y finales de línea no modificados. La comprobación visual automatizada no se pudo ejecutar porque el comando `orca` no está disponible en esta terminal.

## Pendientes para la siguiente iteración

### 1. Búsqueda de palabras clave (prioridad P0)

El usuario reporta que el buscador no encuentra palabras clave. El problema debe investigarse como un flujo completo, no solo como un ajuste visual:

- Confirmar que la consulta de contenido llega al controlador y que el índice usa la consulta más reciente.
- Comprobar cancelación y descarte de resultados obsoletos cuando se escribe rápido o se cambia de proyecto.
- Revisar filtros `.gitignore`, archivos ocultos, extensiones permitidas, límites de tamaño, mayúsculas/minúsculas y Unicode.
- Separar con claridad búsqueda de nombre de archivo y búsqueda dentro del texto, con estados de carga, cero coincidencias y error.
- Cada resultado debe incluir proyecto, ruta, número de línea y fragmento; al activarlo debe abrir el archivo y posicionarse en esa línea.

Aceptación: encontrar una palabra conocida en `.rs`, `.ts/.tsx`, `.md`, `.json` y `.env`, soportar coincidencias parciales y varias coincidencias por archivo, no congelar la UI y contar con pruebas para consultas rápidas y resultados cancelados.

### 2. Selección y copia en Vista previa (prioridad P1)

La Vista previa de Markdown debe ser legible y no editable, pero actualmente no ofrece una selección fiable. Implementar una superficie de texto seleccionable de solo lectura o una capa de selección equivalente:

- Arrastre con ratón y selección por teclado.
- `Ctrl+C` y menú contextual copian únicamente la selección, respetando saltos de línea y caracteres Unicode.
- La Vista previa nunca muta el buffer ni activa el estado de cambios pendientes.
- El foco, el contraste de selección y el recorrido por teclado deben funcionar en tema claro y oscuro.

Validar títulos, listas, enlaces, bloques de código y documentos largos sin perder el formato visual ni la posición del scroll.

### 3. Sidebar de proyectos (prioridad P1)

La lista de proyectos aún se percibe poco refinada: botones demasiado grandes, acciones que sobresalen, alineación irregular y controles difíciles de distinguir. Auditar:

- filas de altura constante, ancho del contenedor y separación entre nombre, favicón/icono y menú;
- truncado con tooltip para nombres largos y nombres alineados a la izquierda;
- estados activo, hover, foco de teclado y deshabilitado;
- contraste de favicones e iconos en blanco y oscuro, incluido el fallback sin favicón;
- menú `⋯` anclado a su fila, sin quedar fuera del viewport ni tapar otros controles;
- comportamiento al redimensionar y a 100/125/150/200 % de escala.

Aceptación: ninguna fila se estira para rellenar espacio, ningún botón sale del panel, las acciones siguen siendo alcanzables con ratón/teclado y el sidebar conserva una jerarquía visual clara en ambos temas.

### 4. Pestañas con varios archivos (prioridad P1)

Con más de un archivo abierto, las pestañas pierden legibilidad y sus cierres no se perciben como controles coherentes. Rediseñar la tira como una región horizontal desplazable:

- pestaña activa con indicador inequívoco y contraste suficiente;
- ancho mínimo/máximo, nombre truncado y tooltip con la ruta completa;
- indicador de cambios sin guardar y cierre compacto con foco visible;
- cerrar con botón, rueda central y `Ctrl+W`, sin cerrar accidentalmente otra pestaña;
- mantener la pestaña activa visible al abrir muchas y conservar el orden al cambiar de proyecto;
- estados de hover/foco consistentes con el resto de la aplicación.

### 5. Matriz de revisión visual (prioridad P2)

Repetir una auditoría nativa cuando la herramienta de automatización esté disponible. Capturar tema claro y oscuro, 960/1320 px, 100/125/150/200 %, y los estados de configuración, búsqueda, menú de proyecto, sidebar, pestañas y editor. Registrar cualquier desborde, dropdown mal anclado, icono ilegible o foco perdido antes de cerrar estos pendientes.
