# Contribuir a CodePeek

Gracias por ayudar a construir un inspector de código nativo, rápido y enfocado.

## Preparar el proyecto

Necesitas Git y Rust mediante rustup. `rust-toolchain.toml` fija Rust 1.94.1 y sus componentes. En Windows instala las herramientas de Visual Studio con «Desktop development with C++». En macOS instala las Command Line Tools de Xcode. En Ubuntu instala `libfontconfig1-dev libfreetype6-dev libxkbcommon-dev libwayland-dev libx11-dev libxcb-shape0-dev libxcb-xfixes0-dev libssl-dev pkg-config`.

```powershell
git clone https://github.com/rafaeljara-dev/code-peek.git
cd code-peek
cargo check --locked --workspace
cargo test --locked --workspace -- --test-threads=1
```

Para generar el ejecutable optimizado:

```powershell
cargo build --locked --release --bin peek
```

## Antes de enviar un cambio

```powershell
cargo fmt --all -- --check
cargo test --locked --workspace --all-targets -- --test-threads=1
cargo clippy --locked --workspace --all-targets -- -D warnings
python scripts/generate-notices.py --allow-network --check
```

Abre una rama descriptiva, mantén cada pull request enfocado y explica el comportamiento visible que cambia. No incluyas archivos `.env`, rutas locales, credenciales, contenido de proyectos inspeccionados ni binarios generados bajo `target/`.

Las decisiones centrales del producto están en [AGENTS.md](AGENTS.md), la arquitectura en [ARCHITECTURE.md](ARCHITECTURE.md) y la guía para agregar lenguajes en [EXTENSIONS.md](EXTENSIONS.md).

## Portabilidad y distribución

En Linux/macOS el ejecutable es `target/release/peek`; `--install`, `--uninstall` y el registro del menú contextual son exclusivos de Windows. La CI incluye los tres sistemas. Antes de distribuir un paquete nuevo, comprueba manualmente entrada de texto, portapapeles, selector de carpetas, guardado y permisos en ese sistema. Mantén el I/O pesado fuera del hilo de UI y no añadas motores web.

El código propio está bajo MIT. Las dependencias conservan sus condiciones: la distribución de escritorio usa Slint Royalty-free 2.0 y debe conservar `AboutSlint`, la atribución y los avisos. Al cambiar dependencias, ejecuta `python scripts/generate-notices.py --allow-network`, revisa las licencias y commitea el inventario actualizado. El generador incluye dependencias de desarrollo y otras plataformas.

Consulta [SECURITY.md](SECURITY.md) para reportar problemas de forma privada. No incluyas archivos privados ni pruebas destructivas. En Windows, `powershell -NoProfile -File scripts/test-windows-integration.ps1` verifica funciones del instalador con archivos temporales, sin modificar la instalación del usuario.
