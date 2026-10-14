# Benchmarks: CodePeek y VS Code

CodePeek no busca reemplazar Visual Studio Code. VS Code es un editor e IDE extensible para desarrollar, depurar y ejecutar proyectos completos. CodePeek es una alternativa complementaria para abrir rápidamente un proyecto, revisar código y hacer una corrección puntual.

## Resultados

Mediciones del 17 de septiembre de 2026 sobre Windows 11. Los valores de tiempo y RAM se publicarán cuando se midan ambos programas bajo las mismas condiciones.

| Métrica | CodePeek 0.1.0 | Visual Studio Code | Diferencia |
| --- | ---: | ---: | ---: |
| Descarga | Pendiente de publicar release | Pendiente | Pendiente |
| Ejecutable principal | 15,0 MiB¹ | Pendiente | Pendiente |
| Instalación completa | Pendiente de medir release | Pendiente | Pendiente |
| Apertura en caliente | Pendiente | Pendiente | Pendiente |
| RAM en reposo con el mismo proyecto | Pendiente | Pendiente | Pendiente |

¹ Binario `target/release/peek.exe`: 16.087.040 bytes antes del empaquetado público. Se actualizará con el artefacto de cada release.

## Metodología

- Misma computadora, sesión de Windows, proyecto y estado de energía.
- Cinco ejecuciones por aplicación; publicar mediana y rango, no solo la mejor.
- «Apertura» termina cuando la ventana principal está disponible para interacción.
- RAM es la suma del working set de todos los procesos de la aplicación, tres segundos después de abrir el mismo proyecto.
- VS Code se mide sin extensiones (`--disable-extensions`) y también con la configuración habitual, como resultados separados.
- Para arranque frío, reiniciar Windows antes de cada medición. Las ejecuciones consecutivas se etiquetan como arranque en caliente.
- Cerrar ambas aplicaciones antes de cada ejecución y registrar versión de Windows, CPU, RAM y tipo de disco.

El script [`scripts/measure-startup.ps1`](../scripts/measure-startup.ps1) automatiza ejecuciones en caliente sin cerrar procesos que ya estaban abiertos. Abre ventanas reales; úsalo solo en una sesión preparada para benchmarking.

## Plantilla del entorno

```text
Windows:
CPU:
RAM:
Disco:
Proyecto probado:
Archivos del proyecto:
Versión de CodePeek:
Versión de VS Code:
Extensiones de VS Code:
```
