# Auditoría de RAM, CPU y arranque — 14 de octubre de 2026

Este informe conserva el baseline de `53534ac`. Las mejoras aplicadas posteriormente y su comparación están en [PERFORMANCE-STAGE1.md](PERFORMANCE-STAGE1.md).

## Resultado

Las oportunidades con mayor impacto están en imágenes, búsqueda y trabajo repetido al publicar el viewport. La CPU en reposo es baja en las muestras recogidas. La memoria base medida sí supera la meta de 25–35 MB: el proceso mantiene aproximadamente **95 MiB residentes y 102 MiB privados** con el proyecto inicial abierto y sin documento seleccionado.

La aplicación y la instalación conservan el comportamiento de la versión aprobada. Se agregaron herramientas de medición y este informe. El código auditado está en el commit `53534ac`; el ejecutable release tiene SHA-256 `d4bc435da06b39bde75540570c61f7cea7df492a0e0820ea558bcb91a5b82c7b` y ocupa 17.629.696 bytes. Las optimizaciones siguientes son propuestas, no ganancias ya implementadas.

[Datos completos sanitizados](performance/2026-10-14.json). Los informes locales originales están en `.audit-ui/performance/`, excluidos de Git. No se publican rutas ni contenido de los proyectos del usuario.

## Entorno y método

- Windows 11 Home, versión 10.0.26200; AMD Ryzen 5 7530U, seis núcleos físicos y doce procesadores lógicos; 31,3 GiB de RAM física.
- Ejecutable de producción con Slint 1.17.1, Ropey 1.6.1, Syntect 5.3.0, Nucleo 0.5.0, Rayon 1.12.0 e image 0.25.10, según el lockfile.
- Configuración existente: diez proyectos registrados, interfaz de 17 px y código de 23 px. Se comprobó que `settings.json` conserva exactamente los mismos bytes después de las diez ejecuciones.
- Cinco arranques normales y cinco con afinidad restringida a los procesadores lógicos 0 y 1. La afinidad se aplica después de crear el proceso.
- Ventana detectada con un observador Win32 y sondeo de 5 ms. Después se oculta, se esperan tres segundos y se mide CPU durante otros tres segundos. Sólo se cierran procesos creados por la medición.
- La primera serie, realizada durante una compilación, se descartó del resumen. Las series publicadas se ejecutaron sin compilaciones concurrentes.
- Microbenchmarks release del código existente, con fixtures aislados. Cada caso corre en un proceso separado; sus picos de memoria incluyen la preparación del fixture. No se suman a la memoria de la aplicación como si fueran una medición de extremo a extremo.

La ventana observada no equivale al primer fotograma ni a un proyecto completamente indexado. No se midieron arranque frío tras reiniciar Windows, GPU, energía, paginación, disco lento, presión de memoria ni interacción con la ventana visible. Dos procesadores lógicos de esta laptop no reproducen una laptop antigua con 4 GB y otra GPU.

## Arranque y reposo

Valores medianos de cinco ejecuciones; rangos entre paréntesis.

| Métrica | Afinidad normal | Dos procesadores lógicos |
| --- | ---: | ---: |
| Ventana observada | 68,81 ms (56,74–72,06) | 107,99 ms (73,12–124,29) |
| Memoria residente al muestrear | 94,91 MiB (94,53–95,57) | 98,37 MiB (98,12–98,70) |
| Memoria privada al muestrear | 102,33 MiB (102,27–102,66) | 102,27 MiB (102,16–102,31) |
| Pico de memoria residente | 103,02 MiB | 106,77 MiB |
| CPU durante la muestra de reposo | 15,625 ms (0–93,75) | 0 ms observado |
| CPU en reposo, normalizada sobre los doce procesadores del equipo | 0,042 % (0–0,251) | 0 % observado |
| CPU acumulada durante lanzamiento y estabilización | 562,5 ms | 843,75 ms |
| Hilos del proceso | 17 | 17 |

Memoria residente y memoria privada representan contadores distintos y no deben sumarse. `PrivateMemorySize64` corresponde al contador Private Bytes; el working set describe páginas residentes e incluye páginas que pueden compartirse. [Microsoft: memoria privada](https://learn.microsoft.com/en-us/dotnet/api/system.diagnostics.process.privatememorysize64), [Microsoft: working set](https://learn.microsoft.com/en-us/dotnet/api/system.diagnostics.process.workingset64).

Los ceros de CPU reflejan el intervalo y la resolución del contador. No prueban consumo cero permanente. La CPU acumulada de inicio tampoco es una duración de arranque: suma el trabajo de los hilos durante los primeros segundos.

Estas mediciones no verifican la promesa de primer fotograma en menos de 20 ms ni la meta de 25–35 MB. Hace falta medir los hitos reales de presentación y atribuir la memoria base antes de cambiar el backend gráfico.

## Costes medidos de operaciones

El archivo grande contiene 500.000 líneas de contenido y una línea vacía final: 15.000.000 bytes, unos 14,3 MiB. Los tiempos son medianas de los microbenchmarks, no latencias completas del editor.

| Operación | Mediana | Muestras | Interpretación |
| --- | ---: | ---: | --- |
| Crear el registro de sintaxis | 1,11 ms | 7 | No aparece como el principal coste de arranque |
| Resaltar 40 líneas de Rust cerca del final | 8,09 ms | 50 | Incluye hasta 64 líneas anteriores de contexto |
| Calcular Editor y Lectura para ese viewport | 8,92 ms | 50 | En Rust, Lectura usa el tokenizador específico; no cuesta el doble |
| Clonar el buffer Ropey | 0,0001 ms | 1.000 | No copia los 15 MB completos |
| Comparar cambios de igual longitud al principio | 0,0002 ms | 100 | La diferencia se encuentra inmediatamente |
| Comparar cambios de igual longitud al final | 1,95 ms | 100 | Se recorre el prefijo compartido por contenido |
| Insertar al final y comprobar cambios | 0,0022 ms | 100 | La diferencia de longitud permite salir rápido |
| Reemplazar un carácter al final y comprobar cambios | 1,82 ms | 100 | Este caso conserva la longitud y activa el recorrido |
| Buscar todas las coincidencias en 500.000 líneas | 643,89 ms | 5 | Devuelve 500.000 rangos; vuelve a calcularlos en cada navegación |
| Búsqueda difusa en 100.000 rutas, consulta amplia | 50,34 ms | 7 | El resultado final contiene 60 rutas, pero se ordenan todas las coincidencias |
| Búsqueda difusa en 100.000 rutas, consulta específica | 11,96 ms | 7 | Resultado final limitado a 60 rutas |
| Convertir una imagen RGBA 4096×4096 a Slint | 10,71 ms | 7 | Copia 64 MiB; el proceso del probe alcanzó 132,35 MiB residentes |
| Generar miniatura de 36 px desde un PNG 4096×4096 | 39,25 ms | 5 | El PNG pesa 335.481 bytes, pero sus píxeles RGBA ocupan 64 MiB |
| Cargar el archivo UTF-8 grande | 96,34 ms | 5 | Lectura a Rope y comprobaciones; rango 82,19–396,28 ms |
| Comprobar conflicto y guardar atómicamente | 110,81 ms | 1 | Una única muestra, insuficiente para una distribución de latencia |

La primera prueba de guardado conservaba un handle de `NamedTempFile` sobre el archivo de destino y Windows rechazó reemplazarlo. Se corrigió el fixture para cerrar ese handle antes de medir. La ejecución publicada terminó correctamente.

## Mejoras priorizadas

### P0 — Miniaturas y favicons fuera del hilo de la interfaz

**Confirmado en código y respaldado por la prueba de miniatura.** `publish` llama a `tree_model` y `project_model` dentro de `upgrade_in_event_loop`. Esas funciones consultan el disco y decodifican imágenes. La caché de miniaturas conserva píxeles, pero también hace `metadata` en cada consulta; los favicons no tienen una caché equivalente.

Referencias: [controller.rs](../crates/peek-ui/src/controller.rs), funciones `tree_model`, `project_model` y `load_project_image`; [image_loader.rs](../crates/peek-ui/src/image_loader.rs), `get_or_create` y `load_raster_thumb_raw`.

**Propuesta:** mostrar primero los iconos de archivo, cargar miniaturas y favicons en trabajadores, deduplicar trabajos por ruta y fecha de modificación, y publicar sólo los píxeles listos. Usar una cola con concurrencia y memoria limitadas. Para una laptop modesta, probar uno o dos decodificadores simultáneos.

**Aceptación:** cero lecturas/decodificaciones en el hilo UI; una carpeta con muchas fotos mantiene ratón y teclado fluidos; resultados de proyectos anteriores se descartan; nombres, iconos y foco mantienen el comportamiento aprobado.

### P0 — Presupuesto de píxeles y reutilización de imágenes convertidas

**Confirmado.** `RawRgbaImage` comparte los datos mediante `Arc`, pero `to_slint_image` copia los píxeles. `publish` vuelve a convertir la imagen abierta en cada comando, incluidos cambios que no cambian la imagen. La carga inicial también publica una conversión antes de `publish`.

El límite de 40 MB de la caché se basa en el tamaño del archivo comprimido. El PNG del probe, de sólo 328 KiB aproximadamente, necesita 64 MiB de RGBA. La carga SVG limita el ancho a 4096 en una rama, pero calcula la altura por la proporción sin un límite equivalente; una imagen muy alta puede requerir un buffer enorme.

**Propuesta:** convertir una vez por versión de imagen y conservar el `slint::Image` mientras los píxeles no cambien; validar ancho, alto y producto de píxeles; fijar límites de decodificación y de caché por bytes; evitar copias intermedias con conversiones que consuman el buffer cuando sea posible. Para «Ajustar a ventana», investigar una representación reducida y cargar la resolución completa cuando se solicite «100 %».

Los límites de dimensiones de `image` son estrictos; `max_alloc` es de mejor esfuerzo y algunos decodificadores pueden ignorarlo. Por eso no basta configurar sólo ese campo. [image 0.25.10: Limits](https://docs.rs/image/0.25.10/image/struct.Limits.html).

**Aceptación:** un cambio de estado sin modificar la imagen no copia de nuevo sus píxeles; fotos grandes y SVG verticales respetan el presupuesto; «100 %» conserva su semántica; medir el pico de RAM en la aplicación completa.

### P1 — Publicar sólo el modo activo y reutilizar el resaltado

**Confirmado y parcialmente medido.** `publish` genera siempre los modelos de Editor y Lectura/Vista previa, aunque sólo uno sea visible. Además, recalcula tokens al mover el cursor, buscar archivos o recibir resultados del índice, incluso cuando el documento no cambió.

Referencia: [controller.rs](../crates/peek-ui/src/controller.rs), `publish`; [highlighter.rs](../crates/peek-syntax/src/highlighter.rs).

**Propuesta:** separar cambios de texto, selección, navegación y paneles; cachear tokens por identidad del documento, viewport, lenguaje y tema; calcular el modo solicitado; actualizar selección/cursor sin volver a tokenizar. Reutilizar modelos y evitar publicaciones de «ocupado» en operaciones rápidas. Mantener intactos los estilos distintos de Markdown en Editor y Vista previa.

El ahorro medido de eliminar la segunda fase en el caso Rust es aproximadamente 0,83 ms de 8,92 ms. La caché de la fase principal puede ser más relevante. El ahorro con otras gramáticas requiere otra medición.

**Aceptación:** una flecha o arrastre de selección sobre el mismo viewport no dispara otro resaltado; undo/redo, recarga externa, tema y cambio de archivo invalidan correctamente la caché.

### P1 — Líneas minificadas: recortar antes de construir filas

**Confirmado por inspección; no se midió un fixture minificado.** `wrapping::wrap` construye todas las filas de una línea y el controlador descarta las sobrantes después. Como también calcula el modelo de lectura con Editor visible, una línea minificada puede generar miles de filas innecesarias. `row_count` vuelve a construirlas para contar al desplazar.

Referencias: [wrapping.rs](../crates/peek-ui/src/wrapping.rs), `wrap`, `row_count` y `advance`; `publish` en el controlador.

**Propuesta:** iterar las filas visibles sin materializar el resto, separar cálculo de cuenta y generación, y limitar el texto de presentación horizontal. Añadir fixtures de JSON/CSS minificado de 4 y 16 MiB. Una línea ASCII de 4 MiB a 100 columnas puede producir unas 41.944 filas antes del recorte actual.

**Aceptación:** el número de elementos/modelos de presentación depende del viewport; cambiar de modo y desplazar una línea enorme mantiene memoria acotada y texto fuente intacto.

### P1 — Búsquedas reutilizables, cancelables y con menos asignaciones

**Confirmado y medido.** La búsqueda difusa construye rutas relativas por consulta, crea un matcher nuevo en el controlador, calcula índices de caracteres que la UI no usa y ordena todas las coincidencias antes de conservar 60. Las consultas de contenido clonan el vector completo de rutas. Buscar en el documento vuelve a recorrer todas las líneas y generar todos los rangos incluso al pulsar F3.

Referencias: [fuzzy.rs](../crates/peek-tree/src/fuzzy.rs); `search` y `find_advanced` en el controlador; [editor.rs](../crates/peek-ui/src/editor.rs), `matches`.

**Propuesta:** conservar un índice de rutas y un matcher; seleccionar sólo los mejores resultados; reutilizar buffers; compartir el índice con `Arc`; consolidar consultas obsoletas; cachear resultados por consulta/opciones/identidad del documento. Ejecutar búsqueda del documento en un trabajador con snapshot del Rope y entregar pronto el primer resultado y después el contador. Mantener un presupuesto para rangos: 500.000 `Range<usize>` requieren unos 7,63 MiB de datos en 64 bits, antes de capacidad sobrante.

Para reducir picos de CPU y lecturas simultáneas, probar un pool de búsqueda de uno o dos hilos. Rayon permite fijar explícitamente su tamaño; este límite sólo afecta sus trabajos, no todos los hilos de Slint y del proceso. [Rayon: ThreadPoolBuilder](https://docs.rs/rayon/1.12.0/rayon/struct.ThreadPoolBuilder.html).

**Aceptación:** consultas rápidas muestran sólo resultados vigentes; F3 reutiliza una búsqueda vigente; misma clasificación y comportamiento Unicode; guardar y editar no esperan una consulta obsoleta. Nunca consolidar o descartar comandos de edición, guardado o historial.

### P2 — Estado de cambios y guardado con menos recorridos/copias

**Medido.** Comparar reemplazos de igual longitud al final del archivo grande consume cerca de 2 ms; insertar se resuelve mucho más rápido por la diferencia de longitud. El guardado relee el archivo para detectar conflictos y materializa el texto completo para escribirlo.

**Propuesta:** investigar comparación de trozos divergentes o identidad de entradas del historial para el estado de cambios; evitar una comparación global innecesaria. Para guardar, investigar escritura por chunks a un temporal sin crear otro `String` completo, conservando comprobación de conflicto, BOM, finales de línea, permisos y reemplazo atómico.

No basta comparar contadores de revisión que puedan repetirse después de undo y una nueva rama de edición. La aceptación debe mantener el estado limpio al volver al contenido guardado y rechazar cambios externos como ahora.

### P2 — Arranque, memoria base y colores: perfilar antes de cambiar

La inicialización del registro de sintaxis tomó aproximadamente 1,1 ms y su probe separado alcanzó unos 3 MiB residentes. Esto no atribuye la memoria base de 95 MiB a una causa concreta. Faltan hitos de primera presentación, fuentes, backend gráfico, favicons, índice y proyecto listo, más perfiles de asignaciones.

El binario usa `renderer-femtovg`, cuya vía actual es OpenGL. Slint ofrece otras vías, incluido software y FemtoVG sobre WGPU; cambiar el backend requiere medir RAM, CPU, arranque, Unicode e imágenes en hardware real. La vía software tiene restricciones y no garantiza una mejora de energía o CPU. [Slint: backends y renderizadores](https://docs.slint.dev/latest/docs/slint/guide/backends-and-renderers/backends_and_renderers/).

El ajuste de contraste calcula luminancia con `powf` repetidamente al convertir spans dentro de la publicación UI. Una tabla pequeña por RGB y tema puede conservar exactamente los colores actuales; su beneficio aún no se midió. El PNG de icono de ventana es 1024×1024: probar una variante menor para el icono de ventana, manteniendo los recursos de instalación de alta resolución, es otra hipótesis por medir.

**Aceptación:** trazas que atribuyan el coste y comparación antes/después, incluyendo GPU antigua; mismos colores/contrastes, glifos y calidad visual. No cambiar el perfil de compilación por tamaño sin medir el impacto en CPU y arranque.

## Elementos que conviene conservar

- Buffer Ropey y snapshots de historial compartidos. El clone medido cuesta unos 0,1 microsegundos; no corresponde interpretar 100 snapshots como 100 copias completas del archivo. La memoria aumenta con los cambios retenidos. [Ropey 1.6.1: clonación](https://docs.rs/ropey/1.6.1/ropey/struct.Rope.html).
- Guardado explícito, atómico y con protección de conflicto.
- Renderizado de código nativo y filas visibles; corregir el trabajo previo a ese recorte, conservando el texto.
- Exclusión de `.git`, `node_modules`, `target` y cachés; cancelación de generaciones de indexado/búsqueda.
- El stack de 64 MiB de `build.rs` pertenece al compilador de la UI, no al proceso instalado. No es una explicación del consumo de RAM observado.

## Plan para laptops con menos recursos

1. **Imágenes y carga de iconos:** presupuestos por bytes/píxeles, decodificación fuera de UI, caché y conversión única. Medir picos con 50 fotos, cambios de proyecto y SVG altos.
2. **Interacción:** modo activo, caché de tokens/modelos y consolidación de eventos de navegación. Medir percentiles de escritura, selección, scroll y apertura, con archivos normales y minificados.
3. **Búsqueda:** índice compartido y estable, cancelación, resultados progresivos, top-k y uno/dos trabajadores. Comparar 10.000 y 100.000 rutas y documentos grandes.
4. **Memoria base y arranque:** separar hitos y probar backends en una máquina real. Medir visible/oculto, batería/cargador, apertura caliente y fría.

Presupuestos iniciales para experimentar, no límites ya implementados: uno/dos trabajos de fondo pesados simultáneos; caché de miniaturas/favicons de 4–8 MiB; representación de imagen ajustada a ventana de hasta 16 MiB; caché de resaltado acotada al viewport; historial con presupuesto de cambios retenidos, además del número de pasos. Estos presupuestos no limitan la memoria total del renderer o del sistema.

Matriz pendiente: equipo real de 4 GB y dos núcleos, equipo de 8 GB/cuatro núcleos y esta laptop; GPU integrada antigua, SSD y disco lento; documentos de 1/15/100 MiB; 10.000/100.000 archivos; sesión larga alternando proyectos e imágenes. Registrar working set, private bytes, VRAM, picos, latencia p50/p95/p99 y consumo energético. Esta auditoría no certifica ausencia de fugas en sesiones largas.

## Reproducir y verificar

```powershell
cargo build --release --bin peek
cargo build --release -p peek-ui --example performance_audit
New-Item -ItemType Directory -Path .audit-ui/performance -Force

.\scripts\measure-resources.ps1 -Executable .\target\release\peek.exe -OutputPath .audit-ui/performance/runtime-default.json
.\scripts\measure-resources.ps1 -Executable .\target\release\peek.exe -AffinityMask 3 -OutputPath .audit-ui/performance/runtime-two-logical.json
.\scripts\measure-probes.ps1 -OutputPath .audit-ui/performance/probes.json

cargo fmt --all -- --check
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
```

Resultados de validación: build release del harness aprobado; todos los casos publicados ejecutados correctamente; Clippy sin advertencias; **59 pruebas** de todos los targets aprobadas (52 del workspace y siete reutilizadas por el harness). Las mediciones conservan la configuración del usuario y el hash del ejecutable aprobado.
