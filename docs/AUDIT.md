# Auditoría de intención y flujo de CodePeek

17 de septiembre de 2026.

## Intención

CodePeek debe reducir el tiempo entre «quiero revisar ese proyecto» y «estoy leyendo el archivo correcto». El caso principal es trabajar con código generado por agentes y consultar archivos de configuración, con correcciones pequeñas cuando hace falta. El flujo conserva la sencillez de un inspector; no se copió la implementación de Orca.

«Tus proyectos» es el historial persistente de proyectos abiertos, no una carpeta raíz. Admite proyectos repartidos en ubicaciones distintas. El explorador muestra las subcarpetas del proyecto activo; el inicio predeterminado es independiente.

## Hallazgos y resolución

| Hallazgo en el prototipo | Efecto | Cambio |
| --- | --- | --- |
| Proyectos inferidos desde padres, abuelos y carpetas del usuario | Lista impredecible, sin biblioteca elegida | Lista persistente de proyectos abiertos, sin depender de una carpeta común |
| Sin inicio configurable | Abrir la aplicación dependía del directorio de trabajo | Inicio persistente; último proyecto como alternativa; CLI prioritario |
| Se descartaban todos los nombres con punto | `.env`, `.github` y `.gitignore` no aparecían | Ocultos útiles visibles; acceso dedicado a entornos |
| Paletas sin gestión explícita del foco ni selección por teclado | Los atajos abrían un panel que no completaba el flujo | Entrada enfocada, flechas, Enter, Escape y estados sin resultados |
| Lectura, recorrido y guardado en callbacks de UI | Congelamientos al inspeccionar proyectos grandes | Controlador serializado en un trabajador; publicación por event loop |
| Botón de guardar sin edición en la UI | Funcionalidad anunciada pero inaccesible | Editor de línea, estado modificado y deshacer |
| Errores de lectura y guardado ignorados | Acciones sin respuesta visible | Errores de ruta, permisos, codificación y conflictos visibles |
| Reemplazar el workspace podía perder cambios | Riesgo de pérdida de correcciones locales | Guardar/descartar/cancelar antes de navegar, recargar o cerrar |
| Guardado sin comparar la versión de disco | Podía pisar cambios de un agente | Comparación contra la versión original antes de guardar |
| Temporal de nombre fijo y sin preservar permisos | Colisiones y cambios de permisos | Temporal único con limpieza automática y permisos conservados |
| Markdown transformado y líneas envueltas con scroll de altura fija | Código diferente del original y cálculo de scroll incorrecto | Fuente sin transformar, ajuste al ancho y navegación por filas visuales |
| Resaltado de todo el prefijo al desplazarse | El coste crecía con el número de línea | Resaltado local; colores multilínea aproximados documentados |
| `mmap.to_vec()` duplicaba archivos grandes | Memoria adicional y mapa inseguro frente a truncados externos | Lectura por bloques directamente al Rope |
| README presentaba prestaciones aspiracionales como medidas | Expectativas sin evidencia | Documentación del comportamiento real y límites explícitos |

## Dirección visual

La skill `frontend-design` orientó la jerarquía: proyectos, explorador, documento. Paneles regulables y plegables, archivos recientes accesibles y acciones de búsqueda visibles evitan depender de atajos memorizados.

Paleta: fondo `#131c26`, panel `#1b2836`, selección `#2b455d`, texto `#edf4fc`, secundario `#acbfd0`, error `#ffe0cf`. La selección y el foco tienen contraste explícito. La interfaz usa Segoe UI en Windows y el código Consolas, con fuente monoespaciada genérica en otros sistemas. No hay animaciones ornamentales ni superficies web.

## Verificación y límites

El modo claro para exteriores sustituye los colores fijos por una paleta compartida: blanco, superficies grises claras, texto oscuro y selección azul pálido. Incluye diálogos y controles nativos. El botón de tema guarda la preferencia sin modificar los zooms. Los colores del resaltado y del árbol conservan su tono, reduciendo su luminancia; una prueba recorre 4.096 colores y comprueba contraste mínimo de 7:1 sobre blanco y dos fondos de selección. Esto verifica contraste digital, no legibilidad física bajo reflejos solares.

Las pruebas cubren prioridad y persistencia del inicio, rutas Unicode, entornos ignorados, cambios de proyecto, restauración de posición, cancelar/guardar/descartar, navegación fallida, conflictos con cambios externos, deshacer, búsqueda circular, BOM y saltos mixtos. La prueba de Slint comprueba escritura inmediata en la paleta y apertura con flechas/Enter sin abrir ventanas.

El diseño mantiene solo el buffer activo, una versión original compartida mediante Rope y hasta 50 versiones de deshacer. Los proyectos inactivos guardan rutas y estado de navegación en memoria. Ni el contenido de archivos ni los valores de entornos se escriben en configuración.

Quedan fuera de esta iteración: mediciones de RAM/arranque, revisión visual nativa, validación en Linux/macOS, diffs Git, selección multilínea, redo y actualización incremental del árbol. La indexación usa un segundo trabajador, se cancela al cambiar de proyecto y descarta resultados de generaciones anteriores; abrir archivos y navegar no esperan a que termine el índice. No se prometen latencias sin medir.

Referencias técnicas consultadas: [foco y teclado de Slint](https://docs.slint.dev/latest/docs/slint/reference/keyboard-input/focusscope/), [publicación desde trabajadores](https://docs.slint.dev/latest/docs/rust/slint/struct.Weak), [selector nativo RFD](https://docs.rs/rfd/0.17.2/rfd/).
