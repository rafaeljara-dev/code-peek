# Iconos nativos

La UI incrusta dos fuentes recortadas, sin SVG ni solicitudes de red:

- `devicon-subset.ttf`: 8.880 bytes. Símbolos de lenguajes de [Devicon v2.16.0](https://github.com/devicons/devicon/tree/v2.16.0), licencia en `DEVICON-LICENSE`.
- `codicon-subset.ttf`: 2.840 bytes. Carpetas y archivos genéricos de [Codicons](https://github.com/microsoft/vscode-codicons), licencia en `CODICONS-LICENSE`.

Se conservaron solo los glifos usados mediante `fontTools.subset --no-hinting`. Total: 11.720 bytes. El mapa de extensiones vive en `peek-tree/src/icons.rs`; al ampliarlo hay que incluir el glifo en la fuente recortada.
