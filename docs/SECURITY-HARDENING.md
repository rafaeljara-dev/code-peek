# Endurecimiento para distribución abierta

## Alcance

Esta etapa mejora el guardado Windows, los lectores de imágenes, la integración del sistema y la distribución. La auditoría de origen no confirmó vulnerabilidades explotables: dejó dos hipótesis pendientes sobre permisos y sustitución del temporal. Los cambios son medidas preventivas; las pruebas con el mismo usuario no demuestran un ataque entre identidades distintas.

## Cambios

- **Guardado Windows:** temporal creado con `CREATE_NEW`, aleatoriedad del sistema, acceso exclusivo y DACL del archivo original desde su creación. El reemplazo usa el identificador abierto y un nombre en el mismo directorio. Un fallo conserva el original y limpia el temporal. Los archivos nuevos reciben los permisos normales del directorio. La DACL se protege frente a herencia adicional; los cambios futuros en permisos heredados del directorio requieren revisión explícita.
- **Imágenes:** un solo decodificador raster con límites de dimensiones, memoria y tamaño físico. El directorio ICO se valida antes de decodificar, incluidos offsets y longitudes. SVG/SVGZ limita entrada expandida a 8 MiB, nodos a 50.000, profundidad a 64 y referencias `use` a 512; rechaza DTD y expansiones anidadas mediante `use`. Bloquea imágenes externas y SVG incrustados; permite imágenes raster incrustadas con presupuesto agregado de 32 MiB. Algunos SVG complejos válidos dejarán de mostrarse.
- **Windows:** un solo script de integración incrustado, rutas transmitidas como datos, PowerShell del sistema por ruta absoluta, instalación sin elevación, verificación de copia y accesos directos. Rechaza directorios de instalación con reparse points. La desinstalación retira primero el ejecutable; un bloqueo de renombrado deja disponibles los mecanismos de desinstalación. Una instancia abierta puede dejar un ejecutable retirado hasta la siguiente instalación. Los errores del proceso auxiliar llegan al llamador.
- **Licencias:** código propio MIT; Slint bajo Royalty-free 2.0 con `AboutSlint` accesible desde «Acerca de». Inventario generado con textos de dependencias y fuentes, incluido en la instalación. El inventario abarca todo el lockfile, no es un SBOM exacto del ejecutable ni una decisión automática sobre compatibilidad legal.
- **CI:** compilador fijado, lockfile obligatorio, acciones fijadas por commit, permisos de lectura por defecto, caché de release separada y verificación en Windows/Linux/macOS. Comprobación programada de avisos de RustSec y propuestas de Dependabot. Las publicaciones por etiqueta separan compilación y escritura de releases, verifican el hash del artefacto recibido y publican licencias y metadatos.

## Comprobaciones reproducibles

```powershell
cargo fmt --all -- --check
cargo check --locked --workspace
cargo test --locked --workspace --all-targets -- --test-threads=1
cargo clippy --locked --workspace --all-targets -- -D warnings
python scripts/generate-notices.py --check
powershell -NoProfile -File scripts/test-windows-integration.ps1
cargo audit --deny unsound
cargo build --locked --release --bin peek
```

Las pruebas usan archivos ficticios: permisos y exclusividad del temporal, sustitución fallida, rutas canónicas/largas, ICO truncado, SVG externo omitido, DTD/profundidad/expansión comprimida, y retirada del ejecutable bloqueado. Las pruebas de UI usan el backend de Slint sin abrir ventanas. La instalación local se verifica comparando hashes y destinos de accesos directos; no se desinstala una instalación real como prueba.

La CI Windows comprueba además un ciclo de instalación, desinstalación desde la propia imagen cargada, reinstalación y limpieza. `scripts/test-windows-install.ps1` exige un runner desechable y rechaza una instalación preexistente. El proceso auxiliar reinicializa `PSModulePath` para que Windows PowerShell utilice sus módulos estándar aunque se lance desde PowerShell 7.

## Dependencias sin mantenimiento

La consulta a RustSec del 14 de octubre de 2026 con cargo-audit 0.22.2 no reportó vulnerabilidades conocidas. Sí devolvió seis avisos de mantenimiento en cinco identificadores: `bincode` 1.3.3 y 2.0.1 ([RUSTSEC-2025-0141](https://rustsec.org/advisories/RUSTSEC-2025-0141)), `paste` 1.0.15 ([RUSTSEC-2024-0436](https://rustsec.org/advisories/RUSTSEC-2024-0436)), `rustybuzz` 0.20.1 ([RUSTSEC-2026-0206](https://rustsec.org/advisories/RUSTSEC-2026-0206)), `ttf-parser` 0.25.1 ([RUSTSEC-2026-0192](https://rustsec.org/advisories/RUSTSEC-2026-0192)) y `yaml-rust` 0.4.5 ([RUSTSEC-2024-0320](https://rustsec.org/advisories/RUSTSEC-2024-0320)).

`bincode` 1 y `yaml-rust` llegan mediante Syntect; `paste` mediante dependencias de compilación de Slint; `rustybuzz` y `ttf-parser` mediante usvg/resvg. `bincode` 2 pertenece al grafo de otras plataformas. No se agregan excepciones que oculten esos identificadores. `cargo audit --deny warnings` falla por estos seis avisos; la CI utiliza `cargo audit --deny unsound`, que bloquea vulnerabilidades y avisos de comportamiento indefinido, manteniendo visibles los de mantenimiento. Migrar estos componentes y volver a medir compatibilidad, renderizado y rendimiento queda pendiente. Dependabot propone actualizaciones, pero no resuelve automáticamente una migración entre bibliotecas.

Gitleaks 8.30.1 no encontró secretos en los ocho commits previos ni en los cambios preparados de esta etapa. La CI escanea el historial con esa versión y su archivo de distribución verificado por SHA-256. Un resultado limpio no demuestra ausencia absoluta de secretos.

## Trabajo pendiente

1. Validación de ACL entre dos usuarios y directorios deliberadamente compartidos. El reemplazo por identificador no elimina todos los cambios concurrentes del directorio o del archivo destino.
2. Semántica de propietario, ACL extendidas, enlaces y metadatos en Linux/macOS. La matriz de CI comprueba compilación y pruebas; no sustituye validar una aplicación empaquetada en cada escritorio.
3. Firma Authenticode y notarización de macOS cuando existan identidades de firma. Los metadatos JSON y checksums actuales no son una atestación criptográfica.
4. Instalador y paquete nativos para macOS/Linux; por ahora se compila desde fuente y la integración automática se limita a Windows.
5. Límites de tiempo o aislamiento de decodificadores para entradas patológicas. Los presupuestos de bytes no establecen una cota universal de CPU o memoria de toda la aplicación.
6. Evaluar protección de ramas/etiquetas, revisión de contribuciones y política de versiones según crezca el equipo. No se presenta la ausencia de avisos de dependencias como garantía de seguridad total.
