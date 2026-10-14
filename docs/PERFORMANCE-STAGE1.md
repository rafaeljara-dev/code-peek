# Primera etapa de rendimiento — 14 de octubre de 2026

La primera etapa aplica las mejoras de imágenes, miniaturas y presentación de código identificadas en la [auditoría inicial](PERFORMANCE-AUDIT.md). El release final está compilado e instalado. Las ganancias se concentran en trabajo repetido y fluidez durante el uso; esta etapa no reduce la memoria base ni demuestra un arranque más rápido. [Datos completos sanitizados](performance/2026-10-14-stage1.json).

## Cambios aplicados

- **Píxeles compartidos:** `RawRgbaImage` conserva un `SharedPixelBuffer` preparado en el trabajador. Crear la imagen de Slint comparte ese buffer; no copia otra vez sus píxeles. El controlador publica la imagen abierta sólo cuando cambia o se cierra. La conversión inicial de `Vec` al buffer compartido sigue copiando una vez, fuera del hilo UI. En imágenes RGBA, `into_rgba8` reutiliza el buffer decodificado cuando corresponde.
- **Miniaturas y favicons:** un trabajador dedicado atiende una única solicitud pendiente reemplazable. La solicitud se envía después de publicar los modelos en el hilo UI, para evitar trabajo de disco simultáneo con la preparación inicial de la ventana. Al navegar, cancela la generación anterior entre archivos; una decodificación ya iniciada termina antes de comprobar la cancelación. Publica primero logos y después miniaturas por tandas. Ninguna consulta de metadatos, lectura o decodificación de estas imágenes se ejecuta en los constructores de modelos de la UI.
- **Presupuesto de caché:** LRU de hasta 4 MiB de píxeles y 600 entradas, incluidas respuestas negativas de decodificación. La clave distingue ruta y tamaño; comprueba fecha y longitud en el trabajador. El árbol solicita sólo su tramo visible y un margen de anticipación. Se conservan hasta 100 logos de proyectos; el exceso usa el icono de respaldo.
- **Límites de imagen:** rasters limitados a 16.384 px por dimensión y 128 MiB de salida decodificada/RGBA, comprobados antes de crear el buffer completo. `max_alloc` del decoder también se configura. SVG se rasteriza con un lado máximo de 4096 px, conservando proporción. Las miniaturas ICO se reducen al tamaño solicitado.
- **Sintaxis:** una caché del viewport activo conserva tokens y estilos del documento actual. Cursor, selección, paneles y resultados de navegación reutilizan esos tokens. Edición, historial, recarga y apertura invalidan correctamente; las claves incluyen revisión, ruta, rango y modo. Los colores de tema se aplican después y conservan el contraste existente.
- **Modo activo:** sólo se calculan Editor o Lectura/Vista previa, según el modo seleccionado. Markdown mantiene su presentación diferente en Vista previa.
- **Tandas sin reconstrucción:** las miniaturas y logos actualizan sólo las filas cuyo buffer cambia. Los modelos existentes, scroll, cursor, foco y documento se conservan. Una tanda sin cambios no publica modelos ni activa «ocupado»; durante una carga progresiva se mantienen miniaturas válidas y las eliminaciones se aplican al completar la solicitud.
- **Líneas minificadas:** el wrapping produce únicamente las filas solicitadas, conservando grafemas, colores, tabulaciones y la continuidad de filas. Contar filas ya no construye sus spans. El texto fuente y el guardado permanecen intactos.
- **Instalación:** una copia provisional se valida por hash y se intercambia con el ejecutable instalado. La versión abierta puede seguir ejecutándose; no se cierra ningún proceso ajeno al benchmark. Si falla el intercambio, se restaura el ejecutable anterior.

Slint documenta que `Image` pertenece al hilo UI y que el buffer compartido se puede preparar en otro hilo: [Image](https://docs.slint.dev/latest/docs/rust/slint/struct.Image), [SharedPixelBuffer](https://docs.slint.dev/latest/docs/rust/slint/struct.SharedPixelBuffer). Se verificó también el código instalado de Slint 1.17.1.

## Comparación medida

Código final: `e45200d`, con las mejoras de `28abcad` y `4a602ed`. Ejecutable de 17.673.216 bytes (16,86 MiB), SHA-256 `21631dd1cf6f7d0b234a8cd82bbe01a20af18a69d49e4805eb006b917a498b06`. El instalado tiene el mismo hash. Creció 43.520 bytes respecto al release aprobado.

Se usó la misma laptop y configuración: Ryzen 5 7530U, seis núcleos/doce procesadores, 31,3 GiB de RAM, diez proyectos registrados, código de 23 px e interfaz de 17 px. No había documento seleccionado al iniciar. Los bytes de `settings.json` permanecieron idénticos. La comparación final corrió sin compilación ni otras ventanas de CodePeek preexistentes; sólo se cerraron procesos creados por el benchmark.

### Operaciones de interacción

Medianas de probes release aislados. La columna anterior reutiliza el ejecutable original del harness; las APIs medidas después corresponden al código de `28abcad` y siguen iguales en el release final.

| Operación | Referencia | Implementación actual | Qué significa |
| --- | ---: | ---: | --- |
| Entregar RGBA 4096×4096 a Slint | 12,03 ms | <0,01 ms | Comparte píxeles ya preparados; evita una copia adicional de 64 MiB |
| Obtener tokens de 40 líneas profundas de Rust | 8,93 ms sin reutilización | <0,01 ms con caché vigente | No vuelve a tokenizar al mover cursor/selección dentro del mismo viewport |
| Primer resaltado sin caché | 8,93 ms | 9,65 ms | No se aceleró el parser; reaparece el coste cuando cambia el texto/rango |
| Generar miniatura de 36 px de PNG 4096×4096 | 42,45 ms | 41,89 ms | Coste similar; ahora se ejecuta fuera del hilo UI y se reutiliza |
| Wrapping de línea ASCII de 4 MiB | 143,39 ms para todas las filas | 0,25 ms para 40 filas | Compara materialización completa y visible con el mismo algoritmo actual |
| Wrapping de línea ASCII de 16 MiB | 605,12 ms para todas las filas | 2,03 ms para 40 filas | La comparación no incluye lectura, resaltado ni un fotograma completo |

Los valores por debajo de 0,01 ms son tiempos de acceso/entrega, **no latencias de un fotograma** ni de abrir/decodificar una imagen. Preparar el buffer compartido conserva una copia inicial fuera de UI. Las mediciones con duración redondeada a cero no significan coste literalmente cero.

El wrapping completo produjo 41.944/167.773 filas; el visible produjo 40. El proceso del probe de wrapping completo alcanzó 87,20 MiB residentes, frente a 36,48 MiB en el visible; ambos preparan fixtures de 4 y 16 MiB. Es una comparación controlada de ese algoritmo, no el consumo total de CodePeek ni una medición del algoritmo histórico anterior. Los picos del probe de imagen fueron 132,35 y 129,21 MiB: incluyen preparar la imagen y no permiten atribuir una reducción equivalente al uso completo de la aplicación. La prueba de identidad de buffers sí verifica que Slint y el trabajador comparten los mismos píxeles.

### Inicio y reposo

Cinco ejecuciones por condición. Se detecta HWND/título con sondeo de 5 ms, se mantiene oculto durante la inicialización, se estabiliza tres segundos y se observa CPU durante otros tres. No se mide primer fotograma, interacción visible, indexación terminada, arranque frío, GPU ni batería. La afinidad se aplica después de crear el proceso.

| Mediana | Normal antes | Normal después | Dos procesadores antes | Dos procesadores después |
| --- | ---: | ---: | ---: | ---: |
| Ventana observada | 58,07 ms | 74,06 ms | 74,45 ms | 74,26 ms |
| Memoria residente | 89,30 MiB | 89,88 MiB | 89,27 MiB | 89,60 MiB |
| Memoria privada | 100,80 MiB | 101,30 MiB | 100,76 MiB | 101,07 MiB |
| Pico residente | 102,80 MiB | 103,29 MiB | 102,74 MiB | 103,21 MiB |
| CPU acumulada al estabilizar | 375,00 ms | 453,13 ms | 453,13 ms | 515,63 ms |
| CPU en reposo sobre los doce procesadores | 0 % observado | 0,042 % | 0 % observado | 0 % observado |
| Hilos | 17 | 18 | 17 | 18 |

La RAM base **no bajó**: hay un trabajador adicional y unos 0,3–0,5 MiB privados más en estas medianas. La serie normal mostró 16 ms adicionales de ventana observada; con dos procesadores la mediana quedó igual. Los rangos se superponen: normal 57,49–87,82 ms antes y 59,35–377,28 ms después; dos procesadores 58,31–80,92 y 61,46–101,72 ms. Se mantienen las cinco muestras, incluido el primer lanzamiento del release final, más largo. No se atribuye el rango a una causa no perfilada.

El trabajo acumulado de inicio creció 78,13 ms en normal y 62,50 ms con dos procesadores: no corresponde presentarlo como ahorro de CPU de arranque. La CPU de reposo sigue baja, con intervalos de 0–46,875 ms y resolución discreta del contador. Memoria privada y residente son contadores distintos y no se suman. Esta tabla usa un instrumento que mantiene ocultas las ventanas; no debe confundirse con una comparación idéntica al informe inicial, que las ocultaba una sola vez.

El beneficio de memoria más claro está al trabajar con imágenes: compartir el buffer evita mantener la segunda copia CPU para Slint y evita copiarla de nuevo en cambios de estado. El renderer y sus texturas pueden retener memoria adicional; falta medir fotos grandes en la aplicación completa y hardware realmente limitado.

### Ajustes durante la validación

La primera implementación añadió una regresión de ventana observada con dos procesadores: 124,89 ms frente a 70,17 ms en una serie de control con orden invertido. Se corrigió la competencia inicial de miniaturas al diferir la solicitud y se eliminaron publicaciones completas/«ocupado» de cada tanda. La comparación final con dos procesadores volvió a 74,26 frente a 74,45 ms. Los datos diagnósticos se conservan junto a los resultados finales.

Una serie quedó incompleta con el error real «La aplicación terminó durante el muestreo.»; su causa no quedó establecida y se excluyó. Se reforzó el instrumento para detectar salidas con código y mantener ocultas las ventanas durante la preparación y la muestra; la comparación final completó las veinte ejecuciones. No se cerraron procesos preexistentes mediante el instrumento.

La actualización se probó mientras un proceso de prueba seguía ejecutando el binario anterior instalado: el intercambio tuvo éxito, el proceso sobrevivió y el nuevo ejecutable quedó verificado por hash. La prueba se cerró después de completar su muestreo. La próxima apertura desde Inicio/escritorio ejecuta esta versión.

## Qué permanece pendiente

Esta etapa no implementa representación reducida de fotos en «Ajustar», búsqueda F3 cacheada/progresiva, top-k de búsqueda difusa, pool limitado de búsqueda de contenido, guardado por trozos, comparación de backends ni perfil de primera presentación.

Prioridades de la siguiente etapa: (1) perfilar el primer fotograma y el trabajo de inicio, incluyendo las publicaciones completas al cambiar el viewport del árbol; (2) evitar volver a comprobar encabezados/favicons ausentes en cada navegación; (3) cachear/cancelar búsquedas del documento y compartir el índice; (4) reducir fotos en «Ajustar» y evitar copias de la línea seleccionada completa cuando sólo está visible Editor. Estos costes siguen presentes; no se incluyen como ahorros de esta entrega.

Los límites descritos son de buffers y cachés concretos, **no un límite de RAM total**. La salida decodificada, conversiones temporales, estructuras internas del decoder, SVG con recursos incrustados, renderer y GPU pueden consumir memoria adicional. `max_alloc` es de mejor esfuerzo; por eso también se comprueban dimensiones, área RGBA y `decoder.total_bytes()`. [image 0.25.10: límites](https://docs.rs/image/0.25.10/image/struct.Limits.html).

El resaltado inicial aún examina las líneas completas y hasta 64 anteriores; una línea minificada enorme puede seguir costando CPU al abrirla o cambiar el viewport. Contar y alcanzar una continuación muy lejana también requiere recorrer texto. La caché acota el número de líneas, no los bytes de una línea enorme. No se certifican primer fotograma menor de 20 ms, RAM de 25–35 MB, ausencia de fugas en sesiones largas ni rendimiento en una laptop real de 4 GB.

## Verificación

`cargo test --workspace --all-targets`: 69 pruebas aprobadas; `cargo clippy --workspace --all-targets -- -D warnings`, type-check de todos los targets y formato aprobados. La matriz UI se renderiza con el backend de pruebas sin ventanas ni navegador.

Las pruebas cubren identidad de píxeles compartidos entre hilos y Slint, presupuesto LRU e invalidación por cambios de archivo, generaciones obsoletas, identidad de modelos y tandas parciales, Unicode y wrapping visible, selección/cursor, ramas de undo con revisión reutilizada, navegación fallida desde imagen y conservación de cambios sin guardar. Dos expectativas iniciales se corrigieron para respetar el comportamiento existente: las miniaturas pequeñas se amplían al tamaño solicitado y seleccionar el archivo ya abierto conserva su modo.

## Reproducir

```powershell
cargo build --release -p peek-ui --example performance_audit
cargo build --release --bin peek
.\scripts\measure-probes.ps1 -OutputPath .audit-ui/performance-stage1/probes-after.json -Cases viewport,image-copy,thumbnail,minified-visible,minified-full
.\scripts\measure-resources.ps1 -Executable .\target\release\peek.exe -OutputPath .audit-ui/performance-stage1/runtime-after.json
.\scripts\measure-resources.ps1 -Executable .\target\release\peek.exe -AffinityMask 3 -OutputPath .audit-ui/performance-stage1/runtime-after-two-logical.json
```

La carpeta de salida debe existir. Para repetir el antes/después hay que conservar el binario de `53534ac` y su harness original; comparar con la misma configuración, sin compilar durante la medición y sin cerrar ventanas del usuario.
