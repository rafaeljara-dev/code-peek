# Estado y próximos pasos de CodePeek

Estado revisado el 14 de octubre de 2026. La prioridad es completar el flujo de inspección de código entre proyectos manteniendo una aplicación nativa pequeña.

## Implementado

- [x] Configuración centralizada, edición de línea dentro del visor, Guardar/Revertir contextual, favicons de proyecto y menú extensible `⋯`.
- [x] Modo claro para exteriores, preferencia persistente y adaptación de sintaxis e iconos con pruebas de contraste.
- [x] Workspace Rust con buffer Ropey, árbol, buscador Nucleo, sintaxis y UI Slint.
- [x] Lista persistente de proyectos de cualquier ubicación, alineada a la izquierda y con cierre individual.
- [x] Proyecto predeterminado, último proyecto, recientes y persistencia de configuración.
- [x] Selector nativo de carpetas y entrada de rutas.
- [x] Buscador flotante de archivos, proyectos y contenido con foco automático, flechas, Enter y Escape.
- [x] Archivos ocultos de configuración útiles visibles sin una sección especial.
- [x] Código virtualizado, selección de línea y ajuste automático de líneas y desplazamiento por filas visuales.
- [x] Búsqueda literal dentro del archivo y copia de la línea seleccionada.
- [x] Edición puntual de líneas, deshacer, guardado atómico y protección de cambios pendientes.
- [x] Recarga automática del archivo activo sin modificaciones locales y recarga manual del índice.
- [x] Restauración del archivo, posición y árbol de cada proyecto durante la sesión.
- [x] Lectura directa a Rope, sin copia completa adicional ni mapa de memoria susceptible a truncados externos.
- [x] Indexación independiente y cancelable al cambiar de proyecto, con descarte de resultados antiguos.
- [x] Pruebas de buffers, configuración, navegación, Markdown, conflictos y teclado de la paleta.

## Optimización para equipos modestos

La [auditoría inicial de recursos](docs/PERFORMANCE-AUDIT.md) prioriza trabajo medido. La [primera etapa de mejoras](docs/PERFORMANCE-STAGE1.md) aplica imágenes compartidas, miniaturas en segundo plano y cachés de viewport.

- [x] P0: miniaturas/favicons fuera del hilo UI, caché de píxeles de 4 MiB y conversión compartida de imágenes.
- [x] P1: publicar el modo activo, reutilizar resaltado y generar sólo las filas visibles de líneas minificadas.
- [ ] P0: representar fotos grandes a resolución reducida en «Ajustar» y cargar la resolución original al pedir «100 %».
- [ ] P1: reducir el coste de tokenizar y contar/desplazar líneas minificadas completas.
- [ ] P1: índice compartido, consultas cancelables, búsqueda progresiva y concurrencia limitada.
- [ ] P2: estado de cambios/guardado por trozos, perfiles de memoria base y primer fotograma, comparación de backends en equipos reales.

## Pendiente, con alcance acotado

- [ ] Medir arranque frío/caliente, RAM y latencia de interacción en release sobre proyectos reales y archivos de 500.000 líneas.
- [ ] Verificación visual nativa en distintas escalas de Windows y ejecución en Linux/macOS.
- [ ] Estados de sintaxis incrementales con checkpoints para colores exactos al saltar dentro de bloques multilínea.
- [ ] Actualización incremental del árbol mediante eventos del sistema de archivos, sin rescans completos.
- [x] Selección de rangos, edición multilínea con resaltado, deshacer/rehacer, búsqueda/reemplazo e ir a línea.
- [x] Auditoría de geometría y capturas nativas sin ventana a 960/1320 px y temas claro/oscuro.
- [ ] Validar IME, lectores de pantalla y escalas reales 125/150/200 % del sistema operativo.
- [ ] Vista de diferencias Git acotada a lectura, sin convertir CodePeek en un cliente Git completo.
- [x] Compilación release e instalación por usuario en Windows, accesos directos en Inicio y escritorio y hash del ejecutable verificados.
- [ ] Validar interactivamente el menú contextual de Windows.

### Pendientes de la siguiente iteración de UI y navegación

Estos puntos registran el trabajo anterior; la iteración de octubre incorpora el editor con resaltado y una auditoría nativa. Consulta [el informe actualizado](docs/EDITOR-UI-AUDIT.md) para las verificaciones y límites.

- [ ] **P0 — Resolver la búsqueda de palabras clave.** El modo de búsqueda de contenido puede no devolver coincidencias aunque el texto exista en el proyecto. Auditar el recorrido completo (evento de entrada, cancelación de consultas anteriores, índice, filtros de archivos ocultos/ignorados, sensibilidad a mayúsculas y Unicode, y modelo de resultados). Debe encontrar coincidencias exactas y parciales en `.rs`, `.ts`, `.tsx`, `.md`, `.json`, `.env` y archivos similares, mostrar archivo/línea/fragmento, abrir el archivo en la línea correcta y mantener estados claros de cargando, sin resultados y error. Añadir pruebas de regresión para una consulta dentro de un archivo, varias coincidencias, cambio rápido de consulta y resultados obsoletos.
- [ ] **P1 — Habilitar selección y copia en Vista previa.** La Vista previa de Markdown debe seguir siendo de solo lectura, pero permitir seleccionar texto con el ratón, copiar la selección con `Ctrl+C`/menú contextual y usar teclado para ampliar o reducir la selección. La copia debe conservar el texto visible (incluidos saltos de línea y Unicode), no editar el documento ni robar el foco al editor. Validar párrafos, títulos, listas, código y documentos largos; mantener una alternativa accesible por teclado.
- [ ] **P1 — Rehacer visualmente el sidebar de proyectos.** Los botones y controles actuales se perciben desproporcionados, con alineación y contraste inconsistentes. Revisar ancho, alto, espaciado, truncado del nombre, icono/favicón, estados activo/hover/foco y anclaje del menú `⋯` en tema claro y oscuro, escalas 100/125/150/200 % y ventanas estrechas. Criterios: filas estables sin estirarse, nombres alineados a la izquierda, acciones compactas sin sobresalir, menú contextual dentro del viewport y fallback de icono legible cuando no exista favicón.
- [ ] **P1 — Rediseñar la tira de pestañas.** Con varios archivos abiertos las pestañas pierden jerarquía y sus cierres/alineación se ven pobres. Diseñar una tira desplazable horizontalmente con pestaña activa claramente marcada, ancho mínimo/máximo, nombre truncado con tooltip de ruta completa, indicador de cambios sin guardar, cierre compacto y foco visible. Añadir y probar cerrar con ratón, rueda central y `Ctrl+W`, mantener visible la pestaña activa al abrir muchas y conservar el orden al cambiar de proyecto.
- [ ] **P2 — Auditoría visual de regresión.** Cuando haya automatización nativa disponible, revisar una matriz de pantallas (tema claro/oscuro, búsqueda, configuración, menú de proyecto, sidebar, pestañas y editor) en 960/1320 px y las cuatro escalas de Windows. Registrar capturas y corregir desbordamientos, dropdowns fuera de lugar, contraste de iconos y estados de foco antes de dar por cerrada la iteración.

No se proponen terminal integrada, marketplace, ejecución de agentes ni servidores de lenguaje.
