# Iconos nativos

La UI incrusta dos fuentes recortadas, sin SVG ni solicitudes de red:

- `devicon-subset.ttf`: 8.880 bytes. Símbolos de lenguajes de [Devicon v2.16.0](https://github.com/devicons/devicon/tree/v2.16.0), licencia en `DEVICON-LICENSE`.
- `codicon-subset.ttf`: 3.380 bytes. Carpetas, archivos y acciones (incluidos recarga y menú) de [Codicons](https://github.com/microsoft/vscode-codicons), licencia en `CODICONS-LICENSE`.

Se conservaron solo los glifos usados mediante `fontTools.subset`. Total: 12.260 bytes. El mapa de extensiones vive en `peek-tree/src/icons.rs`; al ampliarlo hay que incluir el glifo en la fuente recortada.

El editor incorpora también los glifos incrustados `discard`, `redo`, `arrow-up`, `arrow-down` y `close`. Las acciones de deshacer, rehacer y búsqueda usan Codicons para no depender de caracteres que falten en la fuente del sistema.
