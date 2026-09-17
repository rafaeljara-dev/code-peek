# Estado y próximos pasos de CodePeek

Estado revisado el 17 de septiembre de 2026. La prioridad es completar el flujo de inspección de código entre proyectos manteniendo una aplicación nativa pequeña.

## Implementado

- [x] Configuración centralizada, edición de línea dentro del visor, Guardar/Revertir contextual, favicons de proyecto y menú extensible `⋯`.
- [x] Modo claro para exteriores, preferencia persistente y adaptación de sintaxis e iconos con pruebas de contraste.
- [x] Workspace Rust con buffer Ropey, árbol, buscador Nucleo, sintaxis y UI Slint.
- [x] Lista persistente de proyectos de cualquier ubicación, alineada a la izquierda y con cierre individual.
- [x] Proyecto predeterminado, último proyecto, recientes y persistencia de configuración.
- [x] Selector nativo de carpetas y entrada de rutas.
- [x] Buscador de archivos y proyectos con foco automático, flechas, Enter y Escape.
- [x] Archivos ocultos útiles y acceso a `.env*`, incluidos entornos ignorados por Git.
- [x] Código virtualizado, selección de línea y ajuste automático de líneas y desplazamiento por filas visuales.
- [x] Búsqueda literal dentro del archivo y copia de la línea seleccionada.
- [x] Edición puntual de líneas, deshacer, guardado atómico y protección de cambios pendientes.
- [x] Recarga automática del archivo activo sin modificaciones locales y recarga manual del índice.
- [x] Restauración del archivo, posición y árbol de cada proyecto durante la sesión.
- [x] Lectura directa a Rope, sin copia completa adicional ni mapa de memoria susceptible a truncados externos.
- [x] Indexación independiente y cancelable al cambiar de proyecto, con descarte de resultados antiguos.
- [x] Pruebas de buffers, entornos, configuración, navegación, conflictos y teclado de la paleta.

## Pendiente, con alcance acotado

- [ ] Medir arranque frío/caliente, RAM y latencia de interacción en release sobre proyectos reales y archivos de 500.000 líneas.
- [ ] Verificación visual nativa en distintas escalas de Windows y ejecución en Linux/macOS.
- [ ] Estados de sintaxis incrementales con checkpoints para colores exactos al saltar dentro de bloques multilínea.
- [ ] Actualización incremental del árbol mediante eventos del sistema de archivos, sin rescans completos.
- [ ] Selección de rangos entre líneas; edición multilínea y redo solo si el uso real los justifica.
- [ ] Vista de diferencias Git acotada a lectura, sin convertir CodePeek en un cliente Git completo.
- [x] Compilación release e instalación por usuario en Windows, accesos directos en Inicio y escritorio y hash del ejecutable verificados.
- [ ] Validar interactivamente el menú contextual de Windows.

No se proponen terminal integrada, marketplace, ejecución de agentes ni servidores de lenguaje.
