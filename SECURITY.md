# Seguridad de CodePeek

## Versiones y comunicación

Las correcciones se aplican a `main`. La versión 0.1.0 está en desarrollo; no hay un compromiso de mantenimiento de versiones antiguas.

Reporta vulnerabilidades de forma privada en [GitHub Security Advisories](https://github.com/rafaeljara-dev/code-peek/security/advisories/new). Incluye versión o commit, sistema operativo, pasos mínimos con archivos ficticios y el efecto observado. No publiques credenciales, archivos privados ni una prueba destructiva en un issue público.

## Modelo de confianza

CodePeek inspecciona archivos locales con los permisos del usuario que lo ejecuta. No ejecutes la aplicación como administrador para revisar proyectos desconocidos. Abrir código no ejecuta comandos del proyecto, tareas de compilación ni extensiones. La aplicación no constituye un sandbox del sistema operativo.

Los nombres, contenidos, imágenes y configuraciones de un proyecto se consideran datos no confiables. Las imágenes tienen límites de tamaño y dimensiones. Los SVG no pueden cargar imágenes externas del disco o de la red; los recursos incrustados permitidos tienen un presupuesto de decodificación. Estos límites reducen consumo accidental, pero no garantizan un tiempo máximo frente a cualquier archivo especialmente construido.

En Windows, el guardado crea un temporal exclusivo con la DACL del destino y lo sustituye mediante su identificador abierto. Comprueba cambios externos antes de guardar. Esa comprobación no es una transacción frente a otro proceso que modifica el archivo simultáneamente. No se garantiza conservar propietario, flujos alternativos ni todos los metadatos. En Linux/macOS se conserva la implementación de reemplazo portable; los permisos extendidos y el propietario necesitan validación específica.

## Privacidad

No hay telemetría propia ni subida de código. El listado de proyectos y la configuración se guardan localmente. Los archivos ocultos, incluidos `.env`, pueden verse y buscarse: evita compartir capturas o resultados con secretos. La atribución de Slint permite abrir su sitio mediante una acción explícita del usuario.

## Distribución

La instalación Windows es por usuario y no solicita elevación. El flujo de publicación genera SHA-256 y datos de compilación junto al instalador. Un checksum detecta cambios respecto al archivo publicado; no sustituye una firma de editor. Los ejecutables todavía no tienen firma Authenticode ni notarización de Apple.

Consulta [el endurecimiento y sus límites](docs/SECURITY-HARDENING.md) y [las condiciones de terceros](THIRD-PARTY-NOTICES.md) antes de redistribuir una modificación.
