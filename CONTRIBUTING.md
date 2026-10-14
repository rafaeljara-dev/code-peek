# Contribuir a CodePeek

Gracias por ayudar a construir un inspector de código nativo, rápido y enfocado.

## Preparar el proyecto

Necesitas Git, Rust estable y, en Windows, las herramientas de compilación de Visual Studio con el componente «Desktop development with C++».

```powershell
git clone https://github.com/rafaeljara-dev/code-peek.git
cd code-peek
cargo check --workspace
cargo test --workspace
```

Para generar el ejecutable optimizado:

```powershell
cargo build --release --bin peek
```

## Antes de enviar un cambio

```powershell
cargo fmt --all -- --check
cargo test --workspace -- --test-threads=1
cargo clippy --workspace --all-targets -- -D warnings
```

Abre una rama descriptiva, mantén cada pull request enfocado y explica el comportamiento visible que cambia. No incluyas archivos `.env`, rutas locales, credenciales, contenido de proyectos inspeccionados ni binarios generados bajo `target/`.

Las decisiones centrales del producto están en [AGENTS.md](AGENTS.md), la arquitectura en [ARCHITECTURE.md](ARCHITECTURE.md) y la guía para agregar lenguajes en [EXTENSIONS.md](EXTENSIONS.md).
